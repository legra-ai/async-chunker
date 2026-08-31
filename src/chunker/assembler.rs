//! [`BoundaryAssembler`] — the chunk-assembly policy shared by the
//! container profiles (`zip-v1`, `isobmff-v1`): structural
//! boundaries close chunks, small units attach backward, and the
//! gear hash places ordinary content-defined cuts inside large
//! opaque regions.

use super::gear::{self, GearHash};
use crate::constants::{
    GENERIC_CDC_CHUNK_MAX_BYTES, GENERIC_CDC_CHUNK_MIN_BYTES, GENERIC_CDC_CHUNK_TARGET_BYTES,
    STRUCTURED_TEXT_RELAXED_MASK, STRUCTURED_TEXT_STRICT_MASK,
};

/// The candidate class a pushed byte ends (text-seam gating).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Seam {
    None,
    /// A line end or tag close: judged under the strict mask at any
    /// length past the minimum.
    Strict,
    /// Whitespace or a structural terminator: judged under the
    /// relaxed mask once the target length is reached.
    Soft,
}

/// How the assembler judges content-defined cuts inside the current
/// region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CutGate {
    /// Every byte is a candidate (binary payloads).
    PerByte,
    /// Textual candidates only, mirroring the structured-text
    /// profile's classes: a line end (or tag close) is a strict
    /// candidate, other whitespace and structural terminators are
    /// soft candidates judged once the target length is reached —
    /// under the structured-text masks, which are tuned to exactly
    /// this candidate density. Low-entropy periodic text starves the
    /// per-byte gear of distinct window states, degrading cuts to
    /// offset-aligned forced cuts that never re-synchronize after an
    /// insertion; seam gating keeps boundaries on content.
    TextSeam,
}

/// Assembles chunks from a byte stream whose structure a walker
/// reports as boundaries.
///
/// - A **boundary** closes the current chunk when it is at least the minimum
///   size. Otherwise the bytes gathered since the last cut (one or more small
///   structural units) attach to the previously closed chunk while that stays
///   within the maximum — *backward*, so a large unchanged unit that follows
///   starts a chunk of its own and keeps its chunks identical across edits of
///   what precedes it.
/// - A unit the walker knows to be **large** (at least the minimum chunk size,
///   learned from its header) always begins a chunk: the bytes before its
///   header attach backward if they can, else close as a chunk of their own
///   even below the minimum. A large unchanged unit therefore yields identical
///   chunks regardless of what precedes it — a re-muxed `mdat`, a shared ZIP
///   member.
/// - Between boundaries the gear hash (the profile's own seed, the
///   `generic-cdc-v1` mask rule, the shared 16 / 64 / 256 KiB envelope) places
///   content-defined cuts, so a changed large region still re-synchronises.
/// - A closed chunk is held for one step to allow the backward attachment;
///   memory is two chunks at most.
pub(super) struct BoundaryAssembler {
    hash: GearHash,
    strict_mask: u64,
    relaxed_mask: u64,
    gate: CutGate,
    /// The candidate class of the byte just pushed.
    seam: Seam,
    /// A structural boundary closes the chunk at this size; the
    /// container profiles use the shared minimum, the document
    /// profiles a lower unit-aligned one.
    unit_min: usize,
    // bounded: capacity capped at the maximum chunk size.
    buffer: Vec<u8>,
    // bounded: one closed chunk awaiting small trailing units.
    held: Vec<u8>,
}

impl BoundaryAssembler {
    /// An assembler hashing with the gear table derived from `seed`,
    /// closing at structural boundaries from the shared minimum.
    pub(super) fn new(seed: &str) -> Self {
        Self::with_unit_min(seed, GENERIC_CDC_CHUNK_MIN_BYTES)
    }

    /// An assembler whose structural boundaries close the chunk at
    /// `unit_min` instead of the shared minimum. Content-defined
    /// cuts between boundaries keep the shared envelope.
    pub(super) fn with_unit_min(seed: &str, unit_min: usize) -> Self {
        Self {
            hash: GearHash::new(seed),
            strict_mask: gear::generic_strict_mask(),
            relaxed_mask: gear::generic_relaxed_mask(),
            gate: CutGate::PerByte,
            seam: Seam::None,
            unit_min,
            buffer: Vec::with_capacity(GENERIC_CDC_CHUNK_MAX_BYTES),
            held: Vec::with_capacity(GENERIC_CDC_CHUNK_MAX_BYTES),
        }
    }

    /// Select how cuts are judged inside the region that follows.
    pub(super) fn set_gate(&mut self, gate: CutGate) {
        self.gate = gate;
    }

    /// The walker reports that the next byte begins a structural
    /// unit.
    pub(super) fn boundary(&mut self, emit: &mut dyn FnMut(&[u8])) {
        if self.buffer.len() >= self.unit_min {
            self.close_chunk(emit);
        } else if !self.held.is_empty()
            && !self.buffer.is_empty()
            && self.held.len() + self.buffer.len() <= GENERIC_CDC_CHUNK_MAX_BYTES
        {
            self.held.extend_from_slice(&self.buffer);
            self.buffer.clear();
            self.hash.reset();
        }
    }

    /// The walker learned, `header_len` bytes into a unit, that the
    /// unit is large: realign so the unit's header begins a chunk.
    /// The bytes before it attach backward when they fit, else close
    /// as their own (possibly sub-minimum) chunk. A no-op when the
    /// header already begins the chunk or a cut fell inside it.
    pub(super) fn large_unit_starts(&mut self, header_len: usize, emit: &mut dyn FnMut(&[u8])) {
        if self.buffer.len() <= header_len {
            return;
        }
        // bounded: a unit header (at most one ZIP local header).
        let header = self.buffer.split_off(self.buffer.len() - header_len);
        if !self.held.is_empty()
            && self.held.len() + self.buffer.len() <= GENERIC_CDC_CHUNK_MAX_BYTES
        {
            self.held.extend_from_slice(&self.buffer);
            self.buffer.clear();
        } else {
            self.close_chunk(emit);
        }
        self.hash.reset();
        for &byte in &header {
            self.hash.update(byte);
        }
        self.buffer.extend_from_slice(&header);
    }

    /// Append one accepted byte and place a content-defined cut if
    /// the hash says so.
    pub(super) fn push(&mut self, byte: u8, emit: &mut dyn FnMut(&[u8])) {
        self.buffer.push(byte);
        self.hash.update(byte);
        self.seam = match byte {
            b'\n' | b'>' => Seam::Strict,
            b'\t' | b'\r' | b' ' | b',' | b';' | b'.' | b'}' | b']' | b')' | b'"' => Seam::Soft,
            _ => Seam::None,
        };
        if self.cut_here() {
            self.close_chunk(emit);
        }
    }

    /// The stream ended well-formed: flush the held chunk, then the
    /// trailing one.
    pub(super) fn finish(&mut self, emit: &mut dyn FnMut(&[u8])) {
        if !self.held.is_empty() {
            emit(&self.held);
            self.held.clear();
        }
        if !self.buffer.is_empty() {
            emit(&self.buffer);
            self.buffer.clear();
        }
        self.hash.reset();
    }

    /// Drop everything (the stream was rejected).
    pub(super) fn clear(&mut self) {
        self.buffer.clear();
        self.held.clear();
        self.hash.reset();
    }

    fn cut_here(&self) -> bool {
        let len = self.buffer.len();
        if len < GENERIC_CDC_CHUNK_MIN_BYTES {
            return false;
        }
        if len >= GENERIC_CDC_CHUNK_MAX_BYTES {
            return true;
        }
        let before_target = len < GENERIC_CDC_CHUNK_TARGET_BYTES;
        match self.gate {
            CutGate::PerByte => {
                let mask = if before_target {
                    self.strict_mask
                } else {
                    self.relaxed_mask
                };
                self.hash.cuts(mask)
            }
            CutGate::TextSeam => match (self.seam, before_target) {
                (Seam::None, _) | (Seam::Soft, true) => false,
                (Seam::Strict, true) => self.hash.cuts(STRUCTURED_TEXT_STRICT_MASK),
                (Seam::Strict | Seam::Soft, false) => self.hash.cuts(STRUCTURED_TEXT_RELAXED_MASK),
            },
        }
    }

    /// Close the current chunk: the previously held chunk goes out,
    /// the new one is held.
    fn close_chunk(&mut self, emit: &mut dyn FnMut(&[u8])) {
        if !self.held.is_empty() {
            emit(&self.held);
            self.held.clear();
        }
        std::mem::swap(&mut self.held, &mut self.buffer);
        self.buffer.clear();
        self.hash.reset();
    }
}
