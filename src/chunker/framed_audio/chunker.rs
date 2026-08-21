//! [`FramedAudioChunker`] — the `framed-audio-v1` streaming boundary
//! detector.

use crate::ChunkError;

use super::fault::{AudioFault, stream_rejected};
use super::walker::Walker;
use crate::chunker::gear::{self, GearHash};
use crate::chunker::profile_chunker::Chunker;
use crate::constants::{
    FRAMED_AUDIO_RELAXED_MASK, FRAMED_AUDIO_STRICT_MASK, GENERIC_CDC_CHUNK_MAX_BYTES,
    GENERIC_CDC_CHUNK_MIN_BYTES, GENERIC_CDC_CHUNK_TARGET_BYTES,
};

/// Forced seam-cut threshold: the largest legal frame (an ADTS frame,
/// 8191 bytes) always fits between this and the maximum, so a chunk
/// in a framed region never exceeds the envelope mid-frame.
const FORCED_SEAM_LIMIT: usize = GENERIC_CDC_CHUNK_MAX_BYTES - 8192;

/// Streaming `framed-audio-v1` chunker.
///
/// The structural invariant is the sniffed format's framing: MPEG
/// audio frames (lengths from the header tables), ADTS frames
/// (explicit lengths), or FLAC metadata blocks followed by the audio
/// region — plus leading `ID3v2` tags and the `ID3v1` trailer. Nothing is
/// decoded; CRC fields are framing-skipped, never verified (checking
/// them would be decoding, <ADR number="0014" />).
///
/// Two region kinds drive the cuts. **Framed** regions (frames,
/// headers, the trailer) cut only at unit seams — every chunk is a
/// whole number of frames — judged under masks scaled to per-frame
/// candidate spacing, with a forced seam cut before the envelope
/// could overflow. **Opaque** regions (tag bodies, FLAC metadata
/// payloads such as cover art, the FLAC audio region) take per-byte
/// content-defined cuts under the generic masks. Malformed streams
/// reject before any root is written.
pub struct FramedAudioChunker {
    walker: Walker,
    hash: GearHash,
    // bounded: capacity capped at the maximum chunk size.
    buffer: Vec<u8>,
    rejected: bool,
}

impl Default for FramedAudioChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl FramedAudioChunker {
    /// Start a chunker for the frozen `framed-audio-v1` parameters.
    #[must_use]
    pub fn new() -> Self {
        Self {
            walker: Walker::new(),
            hash: GearHash::new(gear::FRAMED_AUDIO_GEAR_SEED),
            buffer: Vec::with_capacity(GENERIC_CDC_CHUNK_MAX_BYTES),
            rejected: false,
        }
    }

    fn reject(&mut self, fault: AudioFault) -> ChunkError {
        self.rejected = true;
        self.buffer.clear();
        fault.into_error(self.walker.offset())
    }

    fn guard(&self) -> Result<(), ChunkError> {
        if self.rejected {
            return Err(stream_rejected());
        }
        Ok(())
    }

    /// Judge a cut at a unit seam, under the frame-spacing masks.
    fn cut_at_seam(&self) -> bool {
        let len = self.buffer.len();
        if len < GENERIC_CDC_CHUNK_MIN_BYTES {
            return false;
        }
        if len >= FORCED_SEAM_LIMIT {
            return true;
        }
        let mask = if len < GENERIC_CDC_CHUNK_TARGET_BYTES {
            FRAMED_AUDIO_STRICT_MASK
        } else {
            FRAMED_AUDIO_RELAXED_MASK
        };
        self.hash.cuts(mask)
    }

    /// Judge a per-byte cut inside an opaque region, under the
    /// generic masks.
    fn cut_in_opaque(&self) -> bool {
        let len = self.buffer.len();
        if len < GENERIC_CDC_CHUNK_MIN_BYTES {
            return false;
        }
        if len >= GENERIC_CDC_CHUNK_MAX_BYTES {
            return true;
        }
        let mask = if len < GENERIC_CDC_CHUNK_TARGET_BYTES {
            gear::generic_strict_mask()
        } else {
            gear::generic_relaxed_mask()
        };
        self.hash.cuts(mask)
    }

    fn emit_buffer(&mut self, emit: &mut dyn FnMut(&[u8])) {
        emit(&self.buffer);
        self.buffer.clear();
        self.hash.reset();
    }
}

impl Chunker for FramedAudioChunker {
    fn push(&mut self, window: &[u8], emit: &mut dyn FnMut(&[u8])) -> Result<(), ChunkError> {
        self.guard()?;
        for &byte in window {
            if self.walker.at_seam() && !self.buffer.is_empty() && self.cut_at_seam() {
                self.emit_buffer(emit);
            }
            if let Err(fault) = self.walker.consume(byte) {
                return Err(self.reject(fault));
            }
            self.buffer.push(byte);
            self.hash.update(byte);
            if self.walker.in_opaque() && self.cut_in_opaque() {
                self.emit_buffer(emit);
            }
        }
        Ok(())
    }

    fn finish(&mut self, emit: &mut dyn FnMut(&[u8])) -> Result<(), ChunkError> {
        self.guard()?;
        if let Err(fault) = self.walker.finish() {
            return Err(self.reject(fault));
        }
        if !self.buffer.is_empty() {
            self.emit_buffer(emit);
        }
        self.walker = Walker::new();
        Ok(())
    }
}
