//! [`MpegtsChunker`] — the `mpegts-v1` streaming boundary detector.

use crate::ChunkError;

use super::fault::{TsFault, stream_rejected};
use super::packet::{PACKET_LEN, inspect};
use crate::chunker::gear::{self, GearHash};
use crate::chunker::profile_chunker::Chunker;
use crate::constants::{
    GENERIC_CDC_CHUNK_MAX_BYTES, GENERIC_CDC_CHUNK_MIN_BYTES, GENERIC_CDC_CHUNK_TARGET_BYTES,
    MPEGTS_RELAXED_MASK, MPEGTS_STRICT_MASK,
};

/// Whole packets per chunk at the frozen maximum (the largest
/// packet-aligned length not exceeding the shared maximum).
const MAX_PACKETS: usize = GENERIC_CDC_CHUNK_MAX_BYTES / PACKET_LEN;

/// Streaming `mpegts-v1` chunker.
///
/// The structural invariant is transport-packet framing: a flat
/// sequence of 188-byte packets, each beginning with the `0x47` sync
/// byte, with sane adaptation-field lengths — validated per packet,
/// never decoded, and never resynchronized by scanning. Every chunk
/// is a whole number of packets. Cut candidates are packets whose
/// header marks a seam — the payload-unit-start indicator or an
/// adaptation-field discontinuity — with the gear hash consulted only
/// there, so identical packet runs re-converge at the next seam after
/// an edit and re-segmented streams reuse their chunks. A forced cut
/// at the packet-aligned maximum keeps candidate-free streams
/// bounded. Malformed streams reject before any root is written.
pub struct MpegtsChunker {
    hash: GearHash,
    strict_mask: u64,
    relaxed_mask: u64,
    // bounded: one 188-byte packet.
    packet: Vec<u8>,
    // bounded: capacity capped at MAX_PACKETS whole packets.
    buffer: Vec<u8>,
    offset: u64,
    rejected: bool,
    seen_packet: bool,
}

impl Default for MpegtsChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl MpegtsChunker {
    /// Start a chunker for the frozen `mpegts-v1` parameters.
    #[must_use]
    pub fn new() -> Self {
        Self {
            hash: GearHash::new(gear::MPEGTS_GEAR_SEED),
            strict_mask: MPEGTS_STRICT_MASK,
            relaxed_mask: MPEGTS_RELAXED_MASK,
            packet: Vec::with_capacity(PACKET_LEN),
            buffer: Vec::with_capacity(MAX_PACKETS * PACKET_LEN),
            offset: 0,
            rejected: false,
            seen_packet: false,
        }
    }

    fn reject(&mut self, fault: TsFault) -> ChunkError {
        self.rejected = true;
        self.packet.clear();
        self.buffer.clear();
        fault.into_error(self.offset)
    }

    fn guard(&self) -> Result<(), ChunkError> {
        if self.rejected {
            return Err(stream_rejected());
        }
        Ok(())
    }

    /// Judge a cut with the packet that begins the *next* chunk
    /// already inspected: the candidate rule looks at the seam the
    /// upcoming packet opens.
    fn cut_before_candidate(&self) -> bool {
        let len = self.buffer.len();
        if len < GENERIC_CDC_CHUNK_MIN_BYTES {
            return false;
        }
        let mask = if len < GENERIC_CDC_CHUNK_TARGET_BYTES {
            self.strict_mask
        } else {
            self.relaxed_mask
        };
        self.hash.cuts(mask)
    }

    fn emit_buffer(&mut self, emit: &mut dyn FnMut(&[u8])) {
        emit(&self.buffer);
        self.buffer.clear();
        self.hash.reset();
    }

    /// A complete, validated packet: place it, cutting first when it
    /// opens a seam that qualifies, or when the buffer is at the
    /// packet-aligned maximum.
    fn place_packet(&mut self, candidate: bool, emit: &mut dyn FnMut(&[u8])) {
        if !self.buffer.is_empty()
            && ((candidate && self.cut_before_candidate())
                || self.buffer.len() >= MAX_PACKETS * PACKET_LEN)
        {
            self.emit_buffer(emit);
        }
        for &byte in &self.packet {
            self.hash.update(byte);
        }
        self.buffer.extend_from_slice(&self.packet);
        self.packet.clear();
    }
}

impl Chunker for MpegtsChunker {
    fn push(&mut self, window: &[u8], emit: &mut dyn FnMut(&[u8])) -> Result<(), ChunkError> {
        self.guard()?;
        for &byte in window {
            self.packet.push(byte);
            self.offset += 1;
            if self.packet.len() == PACKET_LEN {
                let packet: &[u8; PACKET_LEN] =
                    self.packet.as_slice().try_into().expect("sized above");
                let seam = match inspect(packet) {
                    Ok(seam) => seam,
                    Err(fault) => return Err(self.reject(fault)),
                };
                self.seen_packet = true;
                self.place_packet(seam.is_candidate(), emit);
            }
        }
        Ok(())
    }

    fn finish(&mut self, emit: &mut dyn FnMut(&[u8])) -> Result<(), ChunkError> {
        self.guard()?;
        if !self.packet.is_empty() {
            return Err(self.reject(TsFault::PartialPacket));
        }
        if !self.seen_packet {
            return Err(self.reject(TsFault::Empty));
        }
        if !self.buffer.is_empty() {
            self.emit_buffer(emit);
        }
        self.offset = 0;
        self.seen_packet = false;
        Ok(())
    }
}
