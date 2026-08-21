//! [`BoxFault`] — why the walker rejected a stream.

use crate::ChunkError;

use crate::profile::ChunkingProfile;

/// The frozen name, for diagnostics.
const PROFILE: &str = ChunkingProfile::IsobmffV1.name();

/// A structural rejection with its frozen diagnostic text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BoxFault {
    /// The first box is not one an ISO BMFF stream may begin with.
    NotAnIsoBmffStream,
    /// A box declares a size smaller than its own header.
    SizeBelowHeader,
    /// A box declares "to end of stream" (`size == 0`) below top
    /// level, where a parent bounds it.
    OpenSizeNested,
    /// A child box's size (or its header alone) runs past the bytes
    /// its parent has left.
    ChildOverrunsParent,
    /// Containers nest deeper than the frozen bound.
    DepthExceeded,
    /// The stream ended inside a box header or payload.
    Truncated,
}

impl BoxFault {
    /// The frozen diagnostic text.
    pub(super) const fn detail(self) -> &'static str {
        match self {
            Self::NotAnIsoBmffStream => "stream does not begin with an ISO BMFF box",
            Self::SizeBelowHeader => "box size is smaller than its header",
            Self::OpenSizeNested => "open-ended box size below top level",
            Self::ChildOverrunsParent => "child box runs past its parent",
            Self::DepthExceeded => "box nesting exceeds the frozen depth bound",
            Self::Truncated => "stream ends inside a box",
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
