//! [`OfficeFault`] — why an Office profile rejected a package.

use super::super::zip::fault::ZipFault;
use crate::ChunkError;
use crate::profile::ChunkingProfile;

/// A structural rejection by an Office profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OfficeFault {
    /// The underlying ZIP container was rejected.
    Zip(ZipFault),
    /// The first member is not `[Content_Types].xml`.
    NotOoxmlPackage,
    /// The declared kind's main part never appeared.
    MissingMainPart,
    /// No recognized main part appeared at all.
    UnrecognizedPackage,
    /// The package carries digital signatures; canonicalization
    /// would invalidate them.
    SignedPackage,
    /// A member uses a compression method canonicalization cannot
    /// inflate (only stored and deflate are supported).
    UnsupportedMethod,
    /// A member's deflate stream is malformed.
    MalformedDeflate,
    /// A member's inflated bytes disagree with its declared
    /// uncompressed size.
    InflatedSizeMismatch,
    /// A member's inflated bytes disagree with its declared CRC-32.
    CrcMismatch,
    /// A member's deflate stream ended before its compressed bytes
    /// did, or needed more bytes than it had.
    DeflateGeometry,
    /// The package holds more members, or more member-name bytes,
    /// than the frozen canonicalization bound.
    MetadataOverBound,
    /// An unknown-size member grew past what a canonical data
    /// descriptor can express.
    UnknownSizeMemberTooLarge,
}

impl From<ZipFault> for OfficeFault {
    fn from(fault: ZipFault) -> Self {
        Self::Zip(fault)
    }
}

impl OfficeFault {
    /// The frozen diagnostic text.
    pub(super) const fn detail(self) -> &'static str {
        match self {
            Self::Zip(fault) => fault.detail(),
            Self::NotOoxmlPackage => "first member is not [Content_Types].xml",
            Self::MissingMainPart => "declared Office kind's main part is missing",
            Self::UnrecognizedPackage => "no Word, Excel, or PowerPoint main part",
            Self::SignedPackage => {
                "package is digitally signed; canonicalization would invalidate the signature — \
                 store it byte-exact under ooxml-ber-v1"
            }
            Self::UnsupportedMethod => "member compression method is not stored or deflate",
            Self::MalformedDeflate => "member deflate stream is malformed",
            Self::InflatedSizeMismatch => "member inflates to a size other than declared",
            Self::CrcMismatch => "member inflates to bytes with a different CRC-32 than declared",
            Self::DeflateGeometry => "member deflate stream ends out of step with its bytes",
            Self::MetadataOverBound => "package exceeds the canonicalization metadata bound",
            Self::UnknownSizeMemberTooLarge => {
                "unknown-size member exceeds what a canonical descriptor can express"
            }
        }
    }

    /// The typed error for a fault at input `offset`, attributed to
    /// `profile`.
    pub(super) const fn into_error(self, profile: ChunkingProfile, offset: u64) -> ChunkError {
        ChunkError::MalformedProfileInput {
            profile: profile.name(),
            offset,
            detail: self.detail(),
        }
    }
}

/// The stream was already rejected.
pub(super) const fn stream_rejected(profile: ChunkingProfile) -> ChunkError {
    ChunkError::ProfileStreamRejected {
        profile: profile.name(),
    }
}
