//! `ID3v2` tag headers: the ten bytes that fix a tag's length.

use super::fault::AudioFault;

/// The body length (everything after the ten-byte header, footer
/// included) of the `ID3v2` tag `header` begins.
pub(super) fn body_len(header: &[u8; 10]) -> Result<u64, AudioFault> {
    if &header[..3] != b"ID3" {
        return Err(AudioFault::BadTag);
    }
    let mut size = 0u64;
    for &byte in &header[6..10] {
        if byte & 0x80 != 0 {
            return Err(AudioFault::BadTag);
        }
        size = (size << 7) | u64::from(byte);
    }
    let footer = header[5] & 0x10 != 0;
    Ok(size + if footer { 10 } else { 0 })
}
