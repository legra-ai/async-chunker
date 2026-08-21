//! MPEG audio frame headers: the four bytes that fix a frame's
//! length.

use super::fault::AudioFault;

/// V1 bitrates in kbit/s by layer (I, II, III), indexes 1..=14.
const V1_KBPS: [[u32; 14]; 3] = [
    [
        32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448,
    ],
    [
        32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
    ],
    [
        32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
    ],
];

/// V2/V2.5 bitrates in kbit/s: layer I, then layers II and III
/// (which share a table).
const V2_KBPS: [[u32; 14]; 2] = [
    [
        32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256,
    ],
    [8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160],
];

/// Sampling rates in Hz by version (V1, V2, V2.5), indexes 0..=2.
const HZ: [[u32; 3]; 3] = [
    [44_100, 48_000, 32_000],
    [22_050, 24_000, 16_000],
    [11_025, 12_000, 8_000],
];

/// The length in bytes of the MPEG audio frame `header` begins,
/// header included.
pub(super) fn frame_len(header: &[u8; 4]) -> Result<usize, AudioFault> {
    let version = match (header[1] >> 3) & 0b11 {
        0b11 => 0usize, // V1
        0b10 => 1,      // V2
        0b00 => 2,      // V2.5
        _ => return Err(AudioFault::BadFrameHeader),
    };
    let layer = match (header[1] >> 1) & 0b11 {
        0b11 => 0usize, // Layer I
        0b10 => 1,      // Layer II
        0b01 => 2,      // Layer III
        _ => return Err(AudioFault::BadFrameHeader),
    };
    let bitrate_index = usize::from(header[2] >> 4);
    if bitrate_index == 15 {
        return Err(AudioFault::BadFrameHeader);
    }
    if bitrate_index == 0 {
        return Err(AudioFault::FreeFormatBitrate);
    }
    let samplerate_index = usize::from((header[2] >> 2) & 0b11);
    if samplerate_index == 3 {
        return Err(AudioFault::BadFrameHeader);
    }
    let kbps = if version == 0 {
        V1_KBPS[layer][bitrate_index - 1]
    } else {
        V2_KBPS[usize::from(layer != 0)][bitrate_index - 1]
    };
    let hz = HZ[version][samplerate_index];
    let padding = u32::from((header[2] >> 1) & 1);
    let bits = kbps * 1000;
    let length = match layer {
        // Layer I: slots of four bytes.
        0 => (12 * bits / hz + padding) * 4,
        // Layer III on V2/V2.5 halves the samples per frame.
        2 if version > 0 => 72 * bits / hz + padding,
        // Layer II everywhere; Layer III on V1.
        _ => 144 * bits / hz + padding,
    };
    Ok(length as usize)
}
