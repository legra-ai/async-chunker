//! [`IsobmffChunker`] — the `isobmff-v1` streaming boundary
//! detector.

use crate::ChunkError;

use super::fault::{BoxFault, stream_rejected};
use super::walker::Walker;
use crate::chunker::assembler::BoundaryAssembler;
use crate::chunker::gear;
use crate::chunker::profile_chunker::Chunker;

/// Streaming `isobmff-v1` chunker.
///
/// The structural invariant is the ISO Base Media box grammar: a
/// forward-only walk over box headers (compact, extended-size, and
/// `uuid`), descending into the frozen set of pure containers and
/// counting every other payload — `mdat` above all — without
/// decoding it. Cut candidates are **box boundaries**: the start of
/// every top-level box and of every child of a descended container.
/// A re-mux or metadata edit rewrites `ftyp`/`moov`/`udta` while the
/// media bytes in `mdat` stay identical, so `mdat` chunks are reused;
/// fragmented files (`moof` + `mdat` pairs) and HEIF items behave the
/// same way. A large box always begins a chunk and small boxes attach
/// backward (`BoundaryAssembler`).
/// Malformed streams reject before any root is written.
pub struct IsobmffChunker {
    walker: Walker,
    assembler: BoundaryAssembler,
    rejected: bool,
}

impl Default for IsobmffChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl IsobmffChunker {
    /// Start a chunker for the frozen `isobmff-v1` parameters.
    #[must_use]
    pub fn new() -> Self {
        Self {
            walker: Walker::new(),
            assembler: BoundaryAssembler::new(gear::ISOBMFF_GEAR_SEED),
            rejected: false,
        }
    }

    fn reject(&mut self, fault: BoxFault) -> ChunkError {
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

impl Chunker for IsobmffChunker {
    fn push(&mut self, window: &[u8], emit: &mut dyn FnMut(&[u8])) -> Result<(), ChunkError> {
        self.guard()?;
        for &byte in window {
            if self.walker.at_box_boundary() {
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
