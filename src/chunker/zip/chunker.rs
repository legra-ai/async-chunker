//! [`ZipChunker`] — the `zip-v1` streaming boundary detector.

use crate::ChunkError;

use super::fault::{ZipFault, stream_rejected};
use super::walker::Walker;
use crate::chunker::assembler::BoundaryAssembler;
use crate::chunker::gear;
use crate::chunker::profile_chunker::Chunker;

/// Streaming `zip-v1` chunker.
///
/// The structural invariant is the ZIP container itself: a
/// forward-only walk over local file headers, member bytes, data
/// descriptors, the central directory, and the end records, with
/// every size claim reconciled and no member ever inflated. Cut
/// candidates are **member boundaries** — the start of every local
/// file header and of the central directory — so an unchanged member
/// (an OOXML `word/media/*` part, a shared library inside a JAR)
/// yields identical chunks wherever it appears. Inside a large member
/// the gear hash places ordinary content-defined cuts, so a changed
/// member still re-synchronises. Malformed archives reject the whole
/// stream before any root is written.
///
/// A large member always begins a chunk and small members attach
/// **backward** ([`BoundaryAssembler`]): an
/// OOXML document part followed by its relationship parts and then
/// media therefore keeps the media chunks identical across edits of
/// the document. Memory is two chunks at most.
pub struct ZipChunker {
    walker: Walker,
    assembler: BoundaryAssembler,
    rejected: bool,
}

impl Default for ZipChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl ZipChunker {
    /// Start a chunker for the frozen `zip-v1` parameters.
    #[must_use]
    pub fn new() -> Self {
        Self {
            walker: Walker::new(),
            assembler: BoundaryAssembler::new(gear::ZIP_GEAR_SEED),
            rejected: false,
        }
    }

    fn reject(&mut self, fault: ZipFault) -> ChunkError {
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

impl Chunker for ZipChunker {
    fn push(&mut self, window: &[u8], emit: &mut dyn FnMut(&[u8])) -> Result<(), ChunkError> {
        self.guard()?;
        for &byte in window {
            if self.walker.at_member_boundary() {
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
