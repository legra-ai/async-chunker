//! [`AudioFault`] — why the frame walker rejected a stream.

use crate::ChunkError;

use crate::profile::ChunkingProfile;

/// The frozen name, for diagnostics.
const PROFILE: &str = ChunkingProfile::FramedAudioV1.name();

/// A structural rejection with its frozen diagnostic text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AudioFault {
    /// The leading bytes are no framed-audio format the profile
    /// knows: not a frame sync, not an `ID3v2` tag, not `fLaC`.
    NotFramedAudio,
    /// A frame boundary holds neither a frame sync, an `ID3v2` tag,
    /// nor an `ID3v1` trailer.
    BadFrameSync,
    /// A frame header carries a reserved or invalid field (version,
    /// layer, bitrate index 15, sampling-rate index).
    BadFrameHeader,
    /// A free-format MPEG bitrate (index 0): its frame length is
    /// only discoverable by scanning, which the profile forbids.
    FreeFormatBitrate,
    /// An ADTS frame length smaller than its own header.
    BadFrameLength,
    /// A malformed ID3 tag (a syncsafe size byte with its high bit
    /// set, or a broken `TAG`/`ID3` magic).
    BadTag,
    /// A malformed FLAC metadata block (invalid type 127, or a first
    /// block that is not STREAMINFO).
    BadMetadataBlock,
    /// Bytes after the `ID3v1` trailer, which must end the stream.
    TrailingBytes,
    /// The stream ended inside a header, frame, tag, trailer, or
    /// metadata block — or held no audio at all.
    Truncated,
}

impl AudioFault {
    /// The frozen diagnostic text.
    pub(super) const fn detail(self) -> &'static str {
        match self {
            Self::NotFramedAudio => "stream is not a known framed-audio format",
            Self::BadFrameSync => "frame boundary holds no frame sync, tag, or trailer",
            Self::BadFrameHeader => "frame header carries a reserved field",
            Self::FreeFormatBitrate => "free-format MPEG bitrate",
            Self::BadFrameLength => "ADTS frame length is smaller than its header",
            Self::BadTag => "malformed ID3 tag",
            Self::BadMetadataBlock => "malformed FLAC metadata block",
            Self::TrailingBytes => "bytes after the `ID3v1` trailer",
            Self::Truncated => "stream ends inside a frame, tag, or block",
        }
    }

    /// The typed error for a fault at `offset`.
    pub(super) const fn into_error(self, offset: u64) -> ChunkError {
        ChunkError::MalformedProfileInput {
            profile: PROFILE,
            offset,
            detail: self.detail(),
        }
    }
}

/// The typed error for a stream the profile already rejected.
pub(super) const fn stream_rejected() -> ChunkError {
    ChunkError::ProfileStreamRejected { profile: PROFILE }
}
