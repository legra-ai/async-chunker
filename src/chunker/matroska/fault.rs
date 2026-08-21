//! [`EbmlFault`] — why the walker rejected a stream.

use crate::ChunkError;

use crate::profile::ChunkingProfile;

/// The frozen name, for diagnostics.
const PROFILE: &str = ChunkingProfile::MatroskaV1.name();

/// A structural rejection with its frozen diagnostic text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EbmlFault {
    /// The stream does not begin with the EBML header element.
    NotMatroska,
    /// An element ID varint is invalid (a zero lead byte, or longer
    /// than four bytes).
    InvalidId,
    /// An element size varint is invalid (a zero lead byte — longer
    /// than eight bytes).
    InvalidSize,
    /// An unknown ("all ones") size on an element other than
    /// `Segment` or `Cluster`.
    UnknownSizeForbidden,
    /// An element (or merely its header) runs past the bytes its
    /// parent has left.
    ElementOverrunsParent,
    /// A top-level element other than the EBML header, `Segment`,
    /// or `Void`.
    TopLevelElement,
    /// A child of an unknown-size `Cluster` that is neither a
    /// cluster child nor a segment-level element closing it.
    UnexpectedClusterChild,
    /// The stream ended inside an element header or payload, inside
    /// a known-size container, or before any `Segment`.
    Truncated,
}

impl EbmlFault {
    /// The frozen diagnostic text.
    pub(super) const fn detail(self) -> &'static str {
        match self {
            Self::NotMatroska => "stream does not begin with the EBML header",
            Self::InvalidId => "invalid EBML element ID",
            Self::InvalidSize => "invalid EBML element size",
            Self::UnknownSizeForbidden => "unknown size outside Segment and Cluster",
            Self::ElementOverrunsParent => "element runs past its parent",
            Self::TopLevelElement => "unexpected top-level element",
            Self::UnexpectedClusterChild => "unexpected element inside an open cluster",
            Self::Truncated => "stream ends inside an element",
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
