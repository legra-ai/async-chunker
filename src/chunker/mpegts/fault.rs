//! [`TsFault`] — why the packet framer rejected a stream.

use crate::ChunkError;

use crate::profile::ChunkingProfile;

/// The frozen name, for diagnostics.
const PROFILE: &str = ChunkingProfile::MpegtsV1.name();

/// A structural rejection with its frozen diagnostic text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TsFault {
    /// A packet does not begin with the `0x47` sync byte. The
    /// profile never resynchronizes by scanning: the same bytes must
    /// always produce one representation.
    BadSync,
    /// The reserved adaptation-field control `00`.
    ReservedAdaptationControl,
    /// An adaptation-field length that leaves no room for the
    /// payload its control promises, or overruns the packet.
    MalformedAdaptationField,
    /// The stream ended inside a packet.
    PartialPacket,
    /// The stream contained no packets at all.
    Empty,
}

impl TsFault {
    /// The frozen diagnostic text.
    pub(super) const fn detail(self) -> &'static str {
        match self {
            Self::BadSync => "packet does not begin with the 0x47 sync byte",
            Self::ReservedAdaptationControl => "reserved adaptation-field control",
            Self::MalformedAdaptationField => "adaptation field overruns its packet",
            Self::PartialPacket => "stream ends inside a packet",
            Self::Empty => "stream holds no packets",
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
