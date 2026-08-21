//! FLAC metadata block headers: the four bytes that fix a block's
//! length.

use super::fault::AudioFault;

/// One parsed metadata-block header.
#[derive(Debug, Clone, Copy)]
pub(super) struct FlacBlock {
    /// Whether this is the last metadata block before audio.
    pub(super) last: bool,
    /// The block's payload length.
    pub(super) length: u64,
}

/// Parse a metadata-block header; `first` requires STREAMINFO.
pub(super) fn block(header: &[u8; 4], first: bool) -> Result<FlacBlock, AudioFault> {
    let kind = header[0] & 0x7F;
    if kind == 127 {
        return Err(AudioFault::BadMetadataBlock);
    }
    if first && kind != 0 {
        return Err(AudioFault::BadMetadataBlock);
    }
    let length = (u64::from(header[1]) << 16) | (u64::from(header[2]) << 8) | u64::from(header[3]);
    Ok(FlacBlock {
        last: header[0] & 0x80 != 0,
        length,
    })
}
