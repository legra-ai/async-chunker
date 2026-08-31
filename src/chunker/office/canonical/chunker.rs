//! [`OoxmlChunker`] — the `ooxml-v1` canonicalizing streaming
//! chunker.

use super::super::super::assembler::BoundaryAssembler;
use super::super::super::gear;
use super::super::super::profile_chunker::Chunker;
use super::super::super::zip::walker::Walker;
use super::super::fault::{OfficeFault, stream_rejected};
use super::super::kind::OfficeKind;
use super::super::observer::PackageObserver;
use super::core::{CanonCore, CanonStep};
use crate::ChunkError;
use crate::constants::DOCUMENT_UNIT_CHUNK_MIN_BYTES;
use crate::profile::ChunkingProfile;

/// Streaming `ooxml-v1` chunker.
///
/// **Canonicalizing**: chunks concatenate to the package's frozen
/// canonical form, not to the input bytes. Members are walked with
/// the ZIP walker, inflated (stored and deflate only), and re-emitted
/// deterministically (STORE, zeroed metadata, original order and
/// names); the canonical stream is cut at **part boundaries** from
/// the document unit minimum, with content-defined cuts inside large
/// parts. Raw XML is what gets chunked, so shared headers, footers,
/// tables, and media reproduce identical chunks across document
/// variants, and two uploads differing only in compressor converge
/// to the same canonical bytes.
///
/// A digitally signed package fails hard — canonicalization would
/// invalidate the signature; store such a package byte-exact under
/// `ooxml-ber-v1` instead. Malformed containers, non-Office
/// packages, and members whose bytes contradict their declared size
/// or CRC also reject the stream.
pub struct OoxmlChunker {
    walker: Walker,
    core: CanonCore,
    assembler: BoundaryAssembler,
    rejected: bool,
}

impl Default for OoxmlChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl OoxmlChunker {
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
            core: CanonCore::new(kind),
            assembler: BoundaryAssembler::with_unit_min(
                gear::OOXML_GEAR_SEED,
                DOCUMENT_UNIT_CHUNK_MIN_BYTES,
            ),
            rejected: false,
        }
    }

    /// Attach an event tap observing member names, canonical bytes,
    /// and canonical offsets.
    pub fn set_observer(&mut self, observer: Box<dyn PackageObserver>) {
        self.core.set_observer(observer);
    }

    fn reject(&mut self, fault: OfficeFault) -> ChunkError {
        self.rejected = true;
        self.assembler.clear();
        fault.into_error(ChunkingProfile::OoxmlV1, self.walker.offset())
    }

    fn guard(&self) -> Result<(), ChunkError> {
        if self.rejected {
            return Err(stream_rejected(ChunkingProfile::OoxmlV1));
        }
        Ok(())
    }

    fn drain(&mut self, emit: &mut dyn FnMut(&[u8])) {
        while let Some(step) = self.core.next_step() {
            match step {
                CanonStep::Boundary => self.assembler.boundary(emit),
                CanonStep::LargeUnit(header_len) => {
                    self.assembler.large_unit_starts(header_len, emit);
                }
                CanonStep::Bytes(bytes) => {
                    for &byte in &bytes {
                        self.assembler.push(byte, emit);
                    }
                }
            }
        }
    }
}

impl Chunker for OoxmlChunker {
    fn push(&mut self, window: &[u8], emit: &mut dyn FnMut(&[u8])) -> Result<(), ChunkError> {
        self.guard()?;
        for &byte in window {
            if let Err(fault) = self.walker.consume(byte, &mut self.core) {
                return Err(self.reject(fault.into()));
            }
            if let Some(fault) = self.core.fault.take() {
                return Err(self.reject(fault));
            }
            self.drain(emit);
        }
        Ok(())
    }

    fn finish(&mut self, emit: &mut dyn FnMut(&[u8])) -> Result<(), ChunkError> {
        self.guard()?;
        if let Err(fault) = self.walker.finish() {
            return Err(self.reject(fault.into()));
        }
        if let Err(fault) = self.core.close() {
            return Err(self.reject(fault));
        }
        self.drain(emit);
        self.assembler.finish(emit);
        *self = Self::new();
        Ok(())
    }
}
