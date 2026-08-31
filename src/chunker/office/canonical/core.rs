//! [`CanonCore`] — the event bridge that turns the walked source
//! package into the canonical byte stream.

use std::collections::VecDeque;

use super::super::super::zip::records::MemberSizes;
use super::super::super::zip::walker::ZipEvents;
use super::super::ber::PackageCheck;
use super::super::fault::OfficeFault;
use super::super::kind::OfficeKind;
use super::super::observer::PackageObserver;
use super::decoder::MemberDecoder;
use super::writer::{self, CentralEntry};
use crate::constants::DOCUMENT_UNIT_CHUNK_MIN_BYTES;

/// Most members a package may hold.
const MAX_MEMBERS: usize = 100_000;
/// Most member-name bytes retained for the canonical central
/// directory.
const MAX_NAME_BYTES: usize = 8 << 20;

/// One canonical-stream step for the assembler.
pub(super) enum CanonStep {
    /// A structural boundary precedes the next bytes.
    Boundary,
    /// The bytes just enqueued began a large unit whose canonical
    /// header is this long — realign so it starts a chunk.
    LargeUnit(usize),
    /// Canonical bytes, in stream order.
    Bytes(Vec<u8>),
}

/// The canonicalizing bridge. It receives walker events for the
/// *source* bytes and enqueues the canonical bytes with their
/// boundaries; the chunker drains the queue into the assembler.
pub(super) struct CanonCore {
    // bounded: drained after every consumed byte; holds at most one
    // member header plus one decoder output window.
    steps: VecDeque<CanonStep>,
    check: PackageCheck,
    decoder: Option<MemberDecoder>,
    member: Option<Member>,
    // bounded: MAX_MEMBERS entries / MAX_NAME_BYTES name bytes.
    entries: Vec<CentralEntry>,
    names_total: usize,
    canonical_offset: u64,
    tail_written: bool,
    pub(super) fault: Option<OfficeFault>,
    observer: Option<Box<dyn PackageObserver>>,
}

/// The member currently streaming.
struct Member {
    name: Box<[u8]>,
    utf8: bool,
    header_offset: u64,
    claimed: Option<(MemberSizes, u32)>,
    canonical_len_seen: u64,
}

impl CanonCore {
    pub(super) fn new(expected: Option<OfficeKind>) -> Self {
        Self {
            steps: VecDeque::new(),
            check: PackageCheck::new(expected),
            decoder: None,
            member: None,
            entries: Vec::new(),
            names_total: 0,
            canonical_offset: 0,
            tail_written: false,
            fault: None,
            observer: None,
        }
    }

    pub(super) fn set_observer(&mut self, observer: Box<dyn PackageObserver>) {
        self.observer = Some(observer);
    }

    pub(super) fn next_step(&mut self) -> Option<CanonStep> {
        self.steps.pop_front()
    }

    /// The package must have closed completely.
    pub(super) fn close(&mut self) -> Result<(), OfficeFault> {
        self.check.close()?;
        if !self.tail_written {
            return Err(OfficeFault::Zip(
                super::super::super::zip::fault::ZipFault::Truncated,
            ));
        }
        Ok(())
    }

    fn enqueue_bytes(&mut self, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        self.canonical_offset += bytes.len() as u64;
        self.steps.push_back(CanonStep::Bytes(bytes));
    }

    fn drain_decoder(&mut self) {
        let Some(decoder) = self.decoder.as_mut() else {
            return;
        };
        let produced = decoder.take_pending();
        if produced.is_empty() {
            return;
        }
        if let Some(member) = self.member.as_mut() {
            member.canonical_len_seen += produced.len() as u64;
        }
        if let Some(observer) = self.observer.as_mut() {
            observer.member_bytes(&produced);
        }
        self.enqueue_bytes(produced);
    }

    fn fail(&mut self, fault: OfficeFault) {
        if self.fault.is_none() {
            self.fault = Some(fault);
        }
    }
}

impl ZipEvents for CanonCore {
    fn local_header(
        &mut self,
        name: &[u8],
        method: u16,
        utf8_flag: bool,
        encrypted: bool,
        sizes: Option<MemberSizes>,
        crc: u32,
    ) {
        if self.fault.is_some() {
            return;
        }
        if encrypted {
            return self.fail(OfficeFault::UnsupportedMethod);
        }
        if let Err(fault) = self.check.member(name) {
            return self.fail(fault);
        }
        if self.check.is_signed() {
            return self.fail(OfficeFault::SignedPackage);
        }
        if self.entries.len() >= MAX_MEMBERS || self.names_total + name.len() > MAX_NAME_BYTES {
            return self.fail(OfficeFault::MetadataOverBound);
        }
        self.names_total += name.len();
        let decoder = match MemberDecoder::new(method) {
            Ok(decoder) => decoder,
            Err(fault) => return self.fail(fault),
        };
        self.decoder = Some(decoder);
        let header_offset = self.canonical_offset;
        let header = match sizes {
            Some(sizes) => writer::local_header_known(name, utf8_flag, crc, sizes.uncompressed),
            None => writer::local_header_unknown(name, utf8_flag),
        };
        let header_len = header.len();
        let large =
            sizes.is_none_or(|sizes| sizes.uncompressed >= DOCUMENT_UNIT_CHUNK_MIN_BYTES as u64);
        if let Some(observer) = self.observer.as_mut() {
            observer.member_start(name, header_offset);
        }
        self.steps.push_back(CanonStep::Boundary);
        self.enqueue_bytes(header);
        if large {
            self.steps.push_back(CanonStep::LargeUnit(header_len));
        }
        self.member = Some(Member {
            name: name.into(),
            utf8: utf8_flag,
            header_offset,
            claimed: sizes.map(|sizes| (sizes, crc)),
            canonical_len_seen: 0,
        });
    }

    fn member_data(&mut self, byte: u8) {
        if self.fault.is_some() {
            return;
        }
        let Some(decoder) = self.decoder.as_mut() else {
            return;
        };
        if let Err(fault) = decoder.push(byte) {
            return self.fail(fault);
        }
        self.drain_decoder();
    }

    fn member_end(&mut self, sizes: MemberSizes, crc: u32) {
        if self.fault.is_some() {
            return;
        }
        let Some(mut decoder) = self.decoder.take() else {
            return;
        };
        let (computed_crc, computed_len) = match decoder.close() {
            Ok(result) => result,
            Err(fault) => return self.fail(fault),
        };
        self.decoder = Some(decoder);
        self.drain_decoder();
        self.decoder = None;
        let Some(member) = self.member.take() else {
            return;
        };
        // The source's claims, from its header or its descriptor,
        // must agree with what the bytes decoded to.
        if computed_len != sizes.uncompressed {
            return self.fail(OfficeFault::InflatedSizeMismatch);
        }
        let claimed_crc = member.claimed.map_or(crc, |(_, header_crc)| header_crc);
        if computed_crc != claimed_crc || computed_crc != crc {
            return self.fail(OfficeFault::CrcMismatch);
        }
        let descriptor = member.claimed.is_none();
        if descriptor {
            let Ok(len32) = u32::try_from(computed_len) else {
                return self.fail(OfficeFault::UnknownSizeMemberTooLarge);
            };
            self.enqueue_bytes(writer::descriptor(computed_crc, len32));
        }
        if let Some(observer) = self.observer.as_mut() {
            observer.member_end(computed_len);
        }
        self.entries.push(CentralEntry {
            name: member.name,
            utf8: member.utf8,
            crc: computed_crc,
            len: computed_len,
            offset: member.header_offset,
            descriptor,
        });
    }

    fn central_begun(&mut self) {
        if self.fault.is_some() || self.tail_written {
            return;
        }
        self.tail_written = true;
        if let Some(observer) = self.observer.as_mut() {
            observer.package_end(self.check.members_seen());
        }
        self.steps.push_back(CanonStep::Boundary);
        let tail = writer::tail(&self.entries, self.canonical_offset);
        self.enqueue_bytes(tail);
    }
}
