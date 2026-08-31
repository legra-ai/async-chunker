//! [`PdfChunker`] — the `pdf-v1` streaming boundary detector.

use super::super::assembler::BoundaryAssembler;
use super::super::gear;
use super::super::profile_chunker::Chunker;
use super::fault::{PdfFault, stream_rejected};
use super::walker::Walker;
use crate::ChunkError;
use crate::constants::DOCUMENT_UNIT_CHUNK_MIN_BYTES;

/// Streaming `pdf-v1` chunker.
///
/// Byte-exact: chunks concatenate to the input. The structural
/// invariant is the PDF body grammar — the `%PDF-` header, then
/// indirect objects (stream payloads skipped by their direct
/// `/Length`, or by scanning for `endstream` when the length is
/// indirect), comments, classic `xref` tables, `trailer`
/// dictionaries, and `startxref`, through any number of
/// incremental-update sections, ending at a final `%%EOF`.
///
/// Cut candidates are **object boundaries** (each body item start),
/// closing from the document unit minimum so small object runs
/// coalesce while an edited object invalidates only its own chunk;
/// the gear hash places content-defined cuts inside large stream
/// payloads. Verbatim image, font, and content streams therefore
/// reproduce identical chunks across rewrites, and an
/// incrementally-updated document reuses every chunk of its
/// original bytes. Malformed documents reject the whole stream.
pub struct PdfChunker {
    walker: Walker,
    assembler: BoundaryAssembler,
    rejected: bool,
}

impl Default for PdfChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl PdfChunker {
    /// Start a chunker for the frozen `pdf-v1` parameters.
    #[must_use]
    pub fn new() -> Self {
        Self {
            walker: Walker::new(),
            assembler: BoundaryAssembler::with_unit_min(
                gear::PDF_GEAR_SEED,
                DOCUMENT_UNIT_CHUNK_MIN_BYTES,
            ),
            rejected: false,
        }
    }

    fn reject(&mut self, fault: PdfFault) -> ChunkError {
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

impl Chunker for PdfChunker {
    fn push(&mut self, window: &[u8], emit: &mut dyn FnMut(&[u8])) -> Result<(), ChunkError> {
        self.guard()?;
        for &byte in window {
            if self.walker.at_item_boundary() && !byte.is_ascii_whitespace() {
                self.assembler.boundary(emit);
            }
            if let Err(fault) = self.walker.consume(byte) {
                return Err(self.reject(fault));
            }
            self.assembler.push(byte, emit);
        }
        Ok(())
    }

    fn finish(&mut self, emit: &mut dyn FnMut(&[u8])) -> Result<(), ChunkError> {
        self.guard()?;
        if let Err(fault) = self.walker.finish() {
            return Err(self.reject(fault));
        }
        self.assembler.finish(emit);
        *self = Self::new();
        Ok(())
    }
}
