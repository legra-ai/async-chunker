//! The ZIP container reader for decomposition: the shared walker
//! plus a per-member decoder, emitting reader events.

use crate::chunker::zip::records::MemberSizes;
use crate::chunker::zip::walker::{Walker, ZipEvents};
use crate::inflate::{InflateFault, RawInflater};

use super::decomposer::{ReaderEvent, ReaderOut};
use super::fault::OpaqueReason;
use super::sink::{EntryKind, MemberMeta};

const METHOD_STORED: u16 = 0;
const METHOD_DEFLATE: u16 = 8;

/// Decodes one member's stored or deflated bytes.
enum MemberDecoder {
    Stored,
    Deflate(RawInflater),
}

/// The bridge receiving walker events for one pushed byte.
struct Bridge<'o> {
    out: &'o mut ReaderOut,
    decoder: &'o mut Option<MemberDecoder>,
    member_open: &'o mut bool,
    produced: &'o mut u64,
    ordinal: &'o mut u64,
    office_first: &'o mut bool,
    fault: Option<OpaqueReason>,
}

impl Bridge<'_> {
    fn fail(&mut self, reason: OpaqueReason) {
        if self.fault.is_none() {
            self.fault = Some(reason);
        }
    }
}

impl ZipEvents for Bridge<'_> {
    fn local_header(
        &mut self,
        name: &[u8],
        method: u16,
        _utf8_flag: bool,
        encrypted: bool,
        sizes: Option<MemberSizes>,
        _crc: u32,
    ) {
        if self.fault.is_some() {
            return;
        }
        if encrypted {
            return self.fail(OpaqueReason::EncryptedWithoutKey);
        }
        if *self.ordinal == 0 && name == b"[Content_Types].xml" {
            *self.office_first = true;
        }
        let meta = MemberMeta {
            path: name.into(),
            ordinal: *self.ordinal,
            size: sizes.map(|sizes| sizes.uncompressed),
            mode: None,
            mtime: None,
        };
        *self.ordinal += 1;
        // A trailing '/' names an explicit directory entry.
        if name.ends_with(b"/") {
            if sizes.is_some_and(|sizes| sizes.uncompressed > 0) {
                return self.fail(OpaqueReason::Malformed {
                    detail: "zip directory entry declares a payload",
                    offset: 0,
                });
            }
            self.out
                .push(ReaderEvent::Entry(EntryKind::Directory, meta));
            *self.decoder = None;
            return;
        }
        let decoder = match method {
            METHOD_STORED => MemberDecoder::Stored,
            METHOD_DEFLATE => MemberDecoder::Deflate(RawInflater::new()),
            _ => return self.fail(OpaqueReason::UnsupportedCompression),
        };
        *self.decoder = Some(decoder);
        *self.member_open = true;
        *self.produced = 0;
        self.out.push(ReaderEvent::MemberStart(meta));
    }

    fn member_data(&mut self, byte: u8) {
        if self.fault.is_some() {
            return;
        }
        match self.decoder.as_mut() {
            Some(MemberDecoder::Stored) => {
                *self.produced += 1;
                self.out.push_bytes(&[byte]);
            }
            Some(MemberDecoder::Deflate(inflater)) => {
                if let Err(fault) = inflater.push(byte) {
                    return self.fail(map_inflate(fault));
                }
                let out = inflater.take_pending();
                if !out.is_empty() {
                    *self.produced += out.len() as u64;
                    self.out.push_bytes(&out);
                }
            }
            None => {}
        }
    }

    fn member_end(&mut self, sizes: MemberSizes, _crc: u32) {
        if self.fault.is_some() || !*self.member_open {
            *self.decoder = None;
            return;
        }
        if let Some(MemberDecoder::Deflate(inflater)) = self.decoder.as_mut() {
            if let Err(fault) = inflater.close() {
                return self.fail(map_inflate(fault));
            }
            let out = inflater.take_pending();
            if !out.is_empty() {
                *self.produced += out.len() as u64;
                self.out.push_bytes(&out);
            }
        }
        if *self.produced != sizes.uncompressed {
            return self.fail(OpaqueReason::Malformed {
                detail: "zip member inflates to a size other than declared",
                offset: 0,
            });
        }
        *self.member_open = false;
        *self.decoder = None;
        self.out.push(ReaderEvent::MemberEnd(*self.produced));
    }
}

const fn map_inflate(fault: InflateFault) -> OpaqueReason {
    match fault {
        InflateFault::Malformed => OpaqueReason::Malformed {
            detail: "zip member deflate stream is malformed",
            offset: 0,
        },
        InflateFault::Geometry => OpaqueReason::Malformed {
            detail: "zip member deflate stream ends out of step",
            offset: 0,
        },
    }
}

/// The ZIP reader.
pub(super) struct ZipReader {
    walker: Walker,
    decoder: Option<MemberDecoder>,
    member_open: bool,
    produced: u64,
    ordinal: u64,
    /// First member is `[Content_Types].xml` — an Office package
    /// heuristic reported in the container facts.
    office_first: bool,
}

impl ZipReader {
    pub(super) fn new() -> Self {
        Self {
            walker: Walker::new(),
            decoder: None,
            member_open: false,
            produced: 0,
            ordinal: 0,
            office_first: false,
        }
    }

    pub(super) const fn office_package(&self) -> bool {
        self.office_first
    }

    pub(super) fn push(&mut self, byte: u8, out: &mut ReaderOut) -> Result<(), OpaqueReason> {
        let mut bridge = Bridge {
            out,
            decoder: &mut self.decoder,
            member_open: &mut self.member_open,
            produced: &mut self.produced,
            ordinal: &mut self.ordinal,
            office_first: &mut self.office_first,
            fault: None,
        };
        let walked = self.walker.consume(byte, &mut bridge);
        let bridge_fault = bridge.fault;
        if let Err(fault) = walked {
            return Err(OpaqueReason::Malformed {
                detail: fault.detail(),
                offset: self.walker.offset(),
            });
        }
        if let Some(reason) = bridge_fault {
            return Err(reason);
        }
        Ok(())
    }

    pub(super) fn finish(&mut self, _out: &mut ReaderOut) -> Result<(), OpaqueReason> {
        self.walker
            .finish()
            .map_err(|fault| OpaqueReason::Malformed {
                detail: fault.detail(),
                offset: self.walker.offset(),
            })
    }
}
