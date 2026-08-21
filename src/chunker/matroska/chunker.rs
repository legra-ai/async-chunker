//! [`MatroskaChunker`] — the `matroska-v1` streaming boundary
//! detector.

use crate::ChunkError;

use super::fault::{EbmlFault, stream_rejected};
use super::walker::Walker;
use crate::chunker::assembler::BoundaryAssembler;
use crate::chunker::gear;
use crate::chunker::profile_chunker::Chunker;

/// Streaming `matroska-v1` chunker.
///
/// The structural invariant is the EBML/Matroska grammar: the EBML
/// header, then `Segment`s whose direct children — `Cluster`s above
/// all — are the profile's units. Known-size elements are opaque
/// payload, never decoded; only `Segment` and unknown-size
/// `Cluster`s are descended, the latter closing at the next
/// segment-level element. Cut candidates are **unit boundaries**
/// under the shared container-profile assembly rule
/// (`BoundaryAssembler`): a large unit always begins a chunk,
/// small elements attach backward, and the gear hash places
/// content-defined cuts inside large clusters. A metadata edit
/// (`Info`, `Tags`) or an appended stream leaves every untouched
/// cluster chunk identical. Malformed streams reject before any
/// root is written.
pub struct MatroskaChunker {
    walker: Walker,
    assembler: BoundaryAssembler,
    rejected: bool,
}

impl Default for MatroskaChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl MatroskaChunker {
    /// Start a chunker for the frozen `matroska-v1` parameters.
    #[must_use]
    pub fn new() -> Self {
        Self {
            walker: Walker::new(),
            assembler: BoundaryAssembler::new(gear::MATROSKA_GEAR_SEED),
            rejected: false,
        }
    }

    fn reject(&mut self, fault: EbmlFault) -> ChunkError {
        self.rejected = true;
        self.assembler.clear();
        fault.into_error(self.walker.offset())
    }

    fn guard(&self) -> Result<(), ChunkError> {
        if self.rejected {
            return Err(stream_rejected());
        }
        Ok(())
    }
}

impl Chunker for MatroskaChunker {
    fn push(&mut self, window: &[u8], emit: &mut dyn FnMut(&[u8])) -> Result<(), ChunkError> {
        self.guard()?;
        for &byte in window {
            if self.walker.at_unit_boundary() {
                self.assembler.boundary(emit);
            }
            let large = match self.walker.consume(byte) {
                Ok(large) => large,
                Err(fault) => return Err(self.reject(fault)),
            };
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
            return Err(self.reject(fault));
        }
        self.assembler.finish(emit);
        self.walker = Walker::new();
        Ok(())
    }
}
