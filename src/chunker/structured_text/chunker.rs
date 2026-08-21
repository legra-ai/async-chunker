//! [`StructuredTextChunker`] — the `structured-text-v1` streaming
//! boundary detector.

use crate::ChunkError;

use super::utf8::Utf8Scanner;
use crate::chunker::gear::{self, GearHash};
use crate::chunker::profile_chunker::Chunker;
use crate::constants::{
    GENERIC_CDC_CHUNK_MAX_BYTES, GENERIC_CDC_CHUNK_MIN_BYTES, GENERIC_CDC_CHUNK_TARGET_BYTES,
    STRUCTURED_TEXT_RELAXED_MASK, STRUCTURED_TEXT_STRICT_MASK,
};
use crate::profile::ChunkingProfile;

/// The frozen name, for diagnostics.
const PROFILE: &str = ChunkingProfile::StructuredTextV1.name();

/// What kind of cut candidate a byte ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Candidate {
    /// Not a candidate: a cut never lands here except when forced.
    None,
    /// A line end (`\n`, which also closes `\r\n`).
    LineEnd,
    /// A soft break: other ASCII whitespace or a structural
    /// terminator, so long-line JSON/XML still offers candidates.
    SoftBreak,
}

impl Candidate {
    const fn of(byte: u8) -> Self {
        match byte {
            b'\n' => Self::LineEnd,
            b'\t' | b'\r' | b' ' | b',' | b';' | b'.' | b'}' | b']' | b'>' | b')' => {
                Self::SoftBreak
            }
            _ => Self::None,
        }
    }
}

/// Streaming `structured-text-v1` chunker.
///
/// The structural invariant is UTF-8 well-formedness: the bytes of
/// Markdown, JSON, XML/HTML, and the RDF/XSD textual datatypes must
/// be valid UTF-8, and a chunk never splits a scalar. Cut candidates
/// are positions after a line end or a soft break; the gear hash is
/// consulted only there, so boundaries fall on textual seams and
/// re-synchronize at line granularity after an edit. Malformed input
/// rejects the whole stream — there is no fallback to generic
/// chunking.
///
/// The chunk-size envelope (16 KiB / 64 KiB / 256 KiB) is the one
/// shared by every profile; the masks are the frozen
/// `STRUCTURED_TEXT_*` constants.
pub struct StructuredTextChunker {
    hash: GearHash,
    utf8: Utf8Scanner,
    /// Bytes consumed over the whole stream, for diagnostics.
    offset: u64,
    /// End of the last complete scalar within `buffer`.
    last_boundary: usize,
    /// Set once the stream was rejected: nothing further is accepted.
    rejected: bool,
    // bounded: capacity capped at the maximum chunk size.
    buffer: Vec<u8>,
}

impl Default for StructuredTextChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl StructuredTextChunker {
    /// Start a chunker for the frozen `structured-text-v1`
    /// parameters.
    #[must_use]
    pub fn new() -> Self {
        Self {
            hash: GearHash::new(gear::STRUCTURED_TEXT_GEAR_SEED),
            utf8: Utf8Scanner::default(),
            offset: 0,
            last_boundary: 0,
            rejected: false,
            buffer: Vec::with_capacity(GENERIC_CDC_CHUNK_MAX_BYTES),
        }
    }

    fn reject(&mut self, detail: &'static str) -> ChunkError {
        self.rejected = true;
        self.buffer.clear();
        ChunkError::MalformedProfileInput {
            profile: PROFILE,
            offset: self.offset,
            detail,
        }
    }

    fn guard(&self) -> Result<(), ChunkError> {
        if self.rejected {
            return Err(ChunkError::ProfileStreamRejected { profile: PROFILE });
        }
        Ok(())
    }

    /// Judge a boundary at the current buffer length, which ends a
    /// complete scalar whose last byte is `byte`.
    fn cut_here(&self, byte: u8) -> bool {
        let len = self.buffer.len();
        if len < GENERIC_CDC_CHUNK_MIN_BYTES {
            return false;
        }
        if len >= GENERIC_CDC_CHUNK_MAX_BYTES {
            return true;
        }
        match (Candidate::of(byte), len < GENERIC_CDC_CHUNK_TARGET_BYTES) {
            (Candidate::None, _) => false,
            (Candidate::LineEnd, true) => self.hash.cuts(STRUCTURED_TEXT_STRICT_MASK),
            (Candidate::SoftBreak, true) => false,
            (_, false) => self.hash.cuts(STRUCTURED_TEXT_RELAXED_MASK),
        }
    }

    fn emit_whole(&mut self, emit: &mut dyn FnMut(&[u8])) {
        emit(&self.buffer);
        self.buffer.clear();
        self.last_boundary = 0;
        self.hash.reset();
    }

    /// Forced cut reached inside a multi-byte scalar: close the chunk
    /// at the last scalar boundary and carry the open scalar's bytes
    /// (at most three) into the next chunk.
    fn emit_to_last_boundary(&mut self, emit: &mut dyn FnMut(&[u8])) {
        // bounded: an open scalar is at most three bytes.
        let tail = self.buffer.split_off(self.last_boundary);
        emit(&self.buffer);
        self.buffer.clear();
        self.buffer.extend_from_slice(&tail);
        self.last_boundary = 0;
        self.hash.reset();
        for &byte in &tail {
            self.hash.update(byte);
        }
    }
}

impl Chunker for StructuredTextChunker {
    fn push(&mut self, window: &[u8], emit: &mut dyn FnMut(&[u8])) -> Result<(), ChunkError> {
        self.guard()?;
        for &byte in window {
            let completed = match self.utf8.push(byte) {
                Ok(completed) => completed,
                Err(fault) => return Err(self.reject(fault.detail())),
            };
            self.offset += 1;
            self.buffer.push(byte);
            self.hash.update(byte);
            if completed {
                self.last_boundary = self.buffer.len();
                if self.cut_here(byte) {
                    self.emit_whole(emit);
                }
            } else if self.buffer.len() >= GENERIC_CDC_CHUNK_MAX_BYTES {
                self.emit_to_last_boundary(emit);
            }
        }
        Ok(())
    }

    fn finish(&mut self, emit: &mut dyn FnMut(&[u8])) -> Result<(), ChunkError> {
        self.guard()?;
        if !self.utf8.at_boundary() {
            return Err(self.reject("stream ends inside a UTF-8 scalar"));
        }
        if !self.buffer.is_empty() {
            self.emit_whole(emit);
        }
        self.utf8 = Utf8Scanner::default();
        self.offset = 0;
        Ok(())
    }
}
