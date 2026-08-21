//! One transport packet's frozen framing checks.

use super::fault::TsFault;

/// Every MPEG transport packet is exactly this long.
pub(super) const PACKET_LEN: usize = 188;

/// The sync byte beginning every packet.
pub(super) const SYNC: u8 = 0x47;

/// What one packet's header says about chunk placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PacketSeam {
    /// The payload-unit-start indicator: a new PES packet or PSI
    /// section begins here — MPEG-TS's natural seam.
    pub(super) unit_start: bool,
    /// The adaptation field's discontinuity indicator.
    pub(super) discontinuity: bool,
}

impl PacketSeam {
    /// Whether the packet is a cut candidate.
    pub(super) fn is_candidate(self) -> bool {
        self.unit_start || self.discontinuity
    }
}

/// Validate one complete packet and report its seam flags.
///
/// Checks are framing-only (ADR 0014): the sync byte,
/// the reserved adaptation-field control, and the adaptation-field
/// length against the packet budget. PSI/PES payloads are never
/// decoded.
pub(super) fn inspect(packet: &[u8; PACKET_LEN]) -> Result<PacketSeam, TsFault> {
    if packet[0] != SYNC {
        return Err(TsFault::BadSync);
    }
    let unit_start = packet[1] & 0x40 != 0;
    let control = (packet[3] >> 4) & 0b11;
    let mut discontinuity = false;
    match control {
        0b00 => return Err(TsFault::ReservedAdaptationControl),
        0b01 => {}
        adaptation => {
            let length = usize::from(packet[4]);
            // Control 11 promises payload after the field: the field
            // and its length byte must leave at least one byte.
            // Control 10 (adaptation only) may fill the remainder.
            let budget = if adaptation == 0b11 {
                PACKET_LEN - 4 - 1 - 1
            } else {
                PACKET_LEN - 4 - 1
            };
            if length > budget {
                return Err(TsFault::MalformedAdaptationField);
            }
            if length > 0 {
                discontinuity = packet[5] & 0x80 != 0;
            }
        }
    }
    Ok(PacketSeam {
        unit_start,
        discontinuity,
    })
}
