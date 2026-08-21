//! [`GenericCdcChunker`] — the `generic-cdc-v1` streaming boundary
//! detector.

use crate::ChunkError;

use super::gear::{self, GearHash};
use super::profile_chunker::Chunker;
use crate::constants::{
    GENERIC_CDC_CHUNK_MAX_BYTES, GENERIC_CDC_CHUNK_MIN_BYTES, GENERIC_CDC_CHUNK_TARGET_BYTES,
};

/// Streaming `generic-cdc-v1` chunker.
///
/// Feed input windows of any size; complete chunks come back through
/// a callback as their boundaries are reached. The chunker buffers
/// at most one maximum-size chunk, so memory is independent of
/// payload size.
///
/// Boundaries are a pure function of content: the same bytes always
/// split the same way, on any node, regardless of how they were fed
/// in. That is what makes chunk reuse survive insertions — after an
/// edit the rolling hash re-synchronizes and later boundaries return
/// to where they were.
pub struct GenericCdcChunker {
    hash: GearHash,
    strict_mask: u64,
    relaxed_mask: u64,
    // bounded: capacity capped at GENERIC_CDC_CHUNK_MAX_BYTES.
    buffer: Vec<u8>,
}

impl Default for GenericCdcChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl GenericCdcChunker {
    /// Start a chunker for the frozen `generic-cdc-v1` parameters.
    #[must_use]
    pub fn new() -> Self {
        Self {
            hash: GearHash::new(gear::GENERIC_CDC_GEAR_SEED),
            strict_mask: gear::generic_strict_mask(),
            relaxed_mask: gear::generic_relaxed_mask(),
            buffer: Vec::with_capacity(GENERIC_CDC_CHUNK_MAX_BYTES),
        }
    }

    /// Judge a boundary at the current buffer length.
    fn cut_here(&self) -> bool {
        let len = self.buffer.len();
        if len < GENERIC_CDC_CHUNK_MIN_BYTES {
            return false;
        }
        if len >= GENERIC_CDC_CHUNK_MAX_BYTES {
            return true;
        }
        let mask = if len < GENERIC_CDC_CHUNK_TARGET_BYTES {
            self.strict_mask
        } else {
            self.relaxed_mask
        };
        self.hash.cuts(mask)
    }
}

impl Chunker for GenericCdcChunker {
    /// Feed one input window; `emit` receives every chunk completed
    /// within it. Generic chunking accepts any bytes, so this never
    /// fails.
    fn push(&mut self, window: &[u8], emit: &mut dyn FnMut(&[u8])) -> Result<(), ChunkError> {
        for &byte in window {
            self.buffer.push(byte);
            self.hash.update(byte);
            if self.cut_here() {
                emit(&self.buffer);
                self.buffer.clear();
                self.hash.reset();
            }
        }
        Ok(())
    }

    /// Flush the trailing partial chunk and reset to a fresh-stream
    /// state. The final chunk of a payload is the only one permitted
    /// below the minimum size; an empty payload emits nothing.
    fn finish(&mut self, emit: &mut dyn FnMut(&[u8])) -> Result<(), ChunkError> {
        if !self.buffer.is_empty() {
            emit(&self.buffer);
            self.buffer.clear();
        }
        self.hash.reset();
        Ok(())
    }
}
