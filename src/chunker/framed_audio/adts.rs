//! ADTS frame headers: the seven bytes that fix a frame's length.

use super::fault::AudioFault;

/// The length in bytes of the ADTS frame `header` begins, header
/// (and any CRC) included.
pub(super) fn frame_len(header: &[u8; 7]) -> Result<usize, AudioFault> {
    // The sampling-frequency index 15 is forbidden.
    if (header[2] >> 2) & 0x0F == 15 {
        return Err(AudioFault::BadFrameHeader);
    }
    let length = (usize::from(header[3] & 0b11) << 11)
        | (usize::from(header[4]) << 3)
        | usize::from(header[5] >> 5);
    let protection_absent = header[1] & 1 == 1;
    let minimum = if protection_absent { 7 } else { 9 };
    if length < minimum {
        return Err(AudioFault::BadFrameLength);
    }
    Ok(length)
}
