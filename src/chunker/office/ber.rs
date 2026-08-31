//! [`OoxmlBerChunker`] — the `ooxml-ber-v1` streaming boundary
//! detector: part-aware cuts over the original package bytes.

use super::super::assembler::BoundaryAssembler;
use super::super::gear;
use super::super::profile_chunker::Chunker;
use super::super::zip::records::MemberSizes;
use super::super::zip::walker::{Walker, ZipEvents};
use super::fault::{OfficeFault, stream_rejected};
use super::kind::{CONTENT_TYPES, OfficeKind};
use crate::ChunkError;
use crate::constants::DOCUMENT_UNIT_CHUNK_MIN_BYTES;
use crate::profile::ChunkingProfile;

/// Streaming `ooxml-ber-v1` chunker.
///
/// Byte-exact-reversible: chunks concatenate to the input. The
/// structural walk is the ZIP walker's; on top of it the profile
/// validates that the container is an Office Open XML package
/// (`[Content_Types].xml` first, a Word/Excel/PowerPoint main part
/// present) and aligns chunks to **parts** at the document unit
/// minimum, so an edited slide or sheet invalidates only its own
/// chunk while every untouched part — media above all — reproduces
/// identical chunks. Digital signatures survive: nothing is
/// re-encoded.
pub struct OoxmlBerChunker {
    walker: Walker,
    assembler: BoundaryAssembler,
    check: PackageCheck,
    rejected: bool,
}

/// The package-level validation shared by both Office profiles.
pub(super) struct PackageCheck {
    expected: Option<OfficeKind>,
    members_seen: u64,
    first_ok: bool,
    // bounded: at most one entry per package kind.
    main_parts: Vec<OfficeKind>,
    signed: bool,
}

impl PackageCheck {
    pub(super) fn new(expected: Option<OfficeKind>) -> Self {
        Self {
            expected,
            members_seen: 0,
            first_ok: false,
            main_parts: Vec::new(),
            signed: false,
        }
    }

    /// Record one member name.
    pub(super) fn member(&mut self, name: &[u8]) -> Result<(), OfficeFault> {
        if self.members_seen == 0 {
            if name != CONTENT_TYPES {
                return Err(OfficeFault::NotOoxmlPackage);
            }
            self.first_ok = true;
        }
        self.members_seen += 1;
        if let Some(kind) = OfficeKind::of_main_part(name) {
            if !self.main_parts.contains(&kind) {
                self.main_parts.push(kind);
            }
        }
        if name.starts_with(super::kind::SIGNATURE_DIR) {
            self.signed = true;
        }
        Ok(())
    }

    /// Whether a signature member was seen.
    pub(super) const fn is_signed(&self) -> bool {
        self.signed
    }

    /// How many members were seen.
    pub(super) const fn members_seen(&self) -> u64 {
        self.members_seen
    }

    /// The package is complete: the kind claims must hold.
    pub(super) fn close(&self) -> Result<(), OfficeFault> {
        if !self.first_ok {
            return Err(OfficeFault::NotOoxmlPackage);
        }
        match self.expected {
            Some(kind) if !self.main_parts.contains(&kind) => Err(OfficeFault::MissingMainPart),
            Some(_) => Ok(()),
            None if self.main_parts.is_empty() => Err(OfficeFault::UnrecognizedPackage),
            None => Ok(()),
        }
    }
}

/// The event bridge: names into the check, nothing else observed.
struct BerEvents<'c> {
    check: &'c mut PackageCheck,
    fault: Option<OfficeFault>,
}

impl ZipEvents for BerEvents<'_> {
    fn local_header(
        &mut self,
        name: &[u8],
        _method: u16,
        _utf8_flag: bool,
        _sizes: Option<MemberSizes>,
        _crc: u32,
    ) {
        if self.fault.is_some() {
            return;
        }
        if let Err(fault) = self.check.member(name) {
            self.fault = Some(fault);
        }
    }
}

impl Default for OoxmlBerChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl OoxmlBerChunker {
    /// Start a chunker accepting any Office package kind.
    #[must_use]
    pub fn new() -> Self {
        Self::expecting(None)
    }

    /// Start a chunker that requires the package to be of `kind`.
    #[must_use]
    pub fn expecting(kind: Option<OfficeKind>) -> Self {
        Self {
            walker: Walker::new(),
            assembler: BoundaryAssembler::with_unit_min(
                gear::OOXML_BER_GEAR_SEED,
                DOCUMENT_UNIT_CHUNK_MIN_BYTES,
            ),
            check: PackageCheck::new(kind),
            rejected: false,
        }
    }

    fn reject(&mut self, fault: OfficeFault) -> ChunkError {
        self.rejected = true;
        self.assembler.clear();
        fault.into_error(ChunkingProfile::OoxmlBerV1, self.walker.offset())
    }

    fn guard(&self) -> Result<(), ChunkError> {
        if self.rejected {
            return Err(stream_rejected(ChunkingProfile::OoxmlBerV1));
        }
        Ok(())
    }
}

impl Chunker for OoxmlBerChunker {
    fn push(&mut self, window: &[u8], emit: &mut dyn FnMut(&[u8])) -> Result<(), ChunkError> {
        self.guard()?;
        for &byte in window {
            if self.walker.at_member_boundary() {
                self.assembler.boundary(emit);
            }
            let mut events = BerEvents {
                check: &mut self.check,
                fault: None,
            };
            let large = match self.walker.consume(byte, &mut events) {
                Ok(large) => large,
                Err(fault) => return Err(self.reject(fault.into())),
            };
            if let Some(fault) = events.fault {
                return Err(self.reject(fault));
            }
            self.assembler.push(byte, emit);
            if let Some(header_len) = large {
                self.assembler.large_unit_starts(header_len, emit);
            }
        }
        Ok(())
    }

    fn finish(&mut self, emit: &mut dyn FnMut(&[u8])) -> Result<(), ChunkError> {
        self.guard()?;
        if let Err(fault) = self.walker.finish() {
            return Err(self.reject(fault.into()));
        }
        if let Err(fault) = self.check.close() {
            return Err(self.reject(fault));
        }
        self.assembler.finish(emit);
        *self = Self::expecting(None);
        Ok(())
    }
}
