//! A small transport-stream writer for fixtures: 188-byte packets
//! with PIDs, continuity counters, payload-unit starts, and
//! adaptation fields.

use crate::chunker::mpegts::packet::PACKET_LEN;

/// Deterministic pseudo-random bytes.
pub(super) fn noise(seed: &str, len: usize) -> Vec<u8> {
    // bounded: fixture payloads are test constants.
    let mut bytes = vec![0u8; len];
    let mut hasher = blake3::Hasher::new();
    hasher.update(seed.as_bytes());
    hasher.finalize_xof().fill(&mut bytes);
    bytes
}

/// One packet.
pub(super) struct Packet {
    pub(super) pid: u16,
    pub(super) unit_start: bool,
    pub(super) counter: u8,
    /// `Some(length)` adds an adaptation field of that length (the
    /// length byte itself excluded); the discontinuity flag is set
    /// when `discontinuity` is.
    pub(super) adaptation: Option<u8>,
    pub(super) discontinuity: bool,
}

impl Packet {
    pub(super) fn payload(pid: u16, counter: u8, unit_start: bool) -> Self {
        Self {
            pid,
            unit_start,
            counter,
            adaptation: None,
            discontinuity: false,
        }
    }

    /// Render the packet, filling the payload from `fill`.
    pub(super) fn render(&self, fill: &[u8]) -> [u8; PACKET_LEN] {
        let mut out = [0u8; PACKET_LEN];
        out[0] = 0x47;
        out[1] = (u8::from(self.unit_start) << 6) | ((self.pid >> 8) as u8 & 0x1F);
        out[2] = (self.pid & 0xFF) as u8;
        let control: u8 = if self.adaptation.is_some() {
            0b11
        } else {
            0b01
        };
        out[3] = (control << 4) | (self.counter & 0x0F);
        let mut at = 4usize;
        if let Some(length) = self.adaptation {
            out[at] = length;
            at += 1;
            if length > 0 {
                out[at] = u8::from(self.discontinuity) << 7;
            }
            at += usize::from(length);
        }
        for slot in out[at..].iter_mut() {
            *slot = fill[0];
        }
        let take = (PACKET_LEN - at).min(fill.len());
        out[at..at + take].copy_from_slice(&fill[..take]);
        out
    }
}

/// A stream of `packets` payload packets on one PID: a payload-unit
/// start every `unit_every` packets, deterministic payloads from
/// `seed`.
pub(super) fn stream(seed: &str, packets: usize, unit_every: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(packets * PACKET_LEN);
    let fill = noise(seed, packets * 184);
    for index in 0..packets {
        let packet = Packet::payload(0x100, (index % 16) as u8, index % unit_every == 0);
        out.extend_from_slice(&packet.render(&fill[index * 184..(index + 1) * 184]));
    }
    out
}

/// A packet carrying a discontinuity marker.
pub(super) fn discontinuity_packet(counter: u8) -> Vec<u8> {
    let packet = Packet {
        pid: 0x100,
        unit_start: false,
        counter,
        adaptation: Some(7),
        discontinuity: true,
    };
    packet.render(&noise("els10/disc", 184)).to_vec()
}
