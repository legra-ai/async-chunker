//! [`ZipFault`] — why the walker rejected an archive.

use crate::ChunkError;

use crate::profile::ChunkingProfile;

/// The frozen name, for diagnostics.
const PROFILE: &str = ChunkingProfile::ZipV1.name();

/// A structural rejection. Every variant carries the frozen
/// diagnostic text that reaches the typed error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ZipFault {
    /// Four bytes where a record must begin are no known signature.
    UnknownSignature,
    /// A local file header after the central directory began.
    MemberAfterCentralDirectory,
    /// A record out of its place in the end-of-archive sequence.
    RecordOutOfSequence,
    /// A size field says `0xFFFFFFFF` but no ZIP64 extra field
    /// supplies the real value.
    MissingZip64Sizes,
    /// An extra field runs past the extra area it sits in.
    MalformedExtraField,
    /// A stored member declares different compressed and
    /// uncompressed sizes.
    StoredSizesDisagree,
    /// A deflated member claims a ratio no deflate stream can reach.
    ImplausibleExpansion,
    /// A data-descriptor member's descriptor declares a compressed
    /// size other than the bytes it actually covered.
    DescriptorSizeMismatch,
    /// A central-directory entry addresses a local header at or
    /// after the central directory itself.
    CentralOffsetOutOfRange,
    /// The end-of-central-directory record's entry count does not
    /// match the members and entries that streamed past.
    EntryCountMismatch,
    /// The end-of-central-directory record's central-directory
    /// offset or size does not match what streamed past.
    CentralDirectoryGeometry,
    /// Bytes followed the archive comment.
    TrailingBytes,
    /// The stream ended inside a record, a member, or before the
    /// end-of-central-directory record.
    Truncated,
}

impl ZipFault {
    /// The frozen diagnostic text.
    pub(super) const fn detail(self) -> &'static str {
        match self {
            Self::UnknownSignature => "unknown ZIP record signature",
            Self::MemberAfterCentralDirectory => "local file header after the central directory",
            Self::RecordOutOfSequence => "end-of-archive record out of sequence",
            Self::MissingZip64Sizes => "size field is 0xFFFFFFFF without a ZIP64 extra field",
            Self::MalformedExtraField => "extra field runs past its area",
            Self::StoredSizesDisagree => "stored member declares unequal sizes",
            Self::ImplausibleExpansion => "deflated member declares an impossible expansion ratio",
            Self::DescriptorSizeMismatch => "data descriptor disagrees with the member bytes",
            Self::CentralOffsetOutOfRange => "central directory entry offset is out of range",
            Self::EntryCountMismatch => "end-of-central-directory entry count mismatch",
            Self::CentralDirectoryGeometry => "central directory offset or size mismatch",
            Self::TrailingBytes => "bytes after the archive comment",
            Self::Truncated => "archive ends inside a record or before its end record",
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
