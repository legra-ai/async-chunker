//! Why decomposition stopped: the typed opaque outcome.

use std::fmt;

/// Why a container could not be decomposed. The caller stores the
/// exact source bytes as one explicitly-flagged opaque literal —
/// never a silent pretend-decomposition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpaqueReason {
    /// The container (or a member) is encrypted and no key was
    /// supplied.
    EncryptedWithoutKey,
    /// A member uses a compression method the adapter cannot decode.
    UnsupportedCompression,
    /// The container uses a feature the adapter does not walk
    /// (sparse TAR members, multi-disk ZIP).
    UnsupportedFeature {
        /// What was encountered.
        detail: &'static str,
    },
    /// The container's structure is malformed.
    Malformed {
        /// The walker's frozen diagnostic text.
        detail: &'static str,
        /// Input byte offset of the rejection.
        offset: u64,
    },
    /// A member path is absolute or escapes the container root.
    UnsafePath,
    /// Nesting exceeded the frozen depth cap.
    DepthExceeded,
    /// Retained metadata (paths, pax records) exceeded the frozen
    /// bound.
    MetadataOverBound,
}

impl fmt::Display for OpaqueReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EncryptedWithoutKey => f.write_str("encrypted without a supplied key"),
            Self::UnsupportedCompression => f.write_str("unsupported compression method"),
            Self::UnsupportedFeature { detail } => write!(f, "unsupported feature: {detail}"),
            Self::Malformed { detail, offset } => {
                write!(f, "malformed at byte {offset}: {detail}")
            }
            Self::UnsafePath => f.write_str("member path is absolute or escapes the root"),
            Self::DepthExceeded => f.write_str("container nesting exceeds the depth cap"),
            Self::MetadataOverBound => f.write_str("container metadata exceeds the bound"),
        }
    }
}

/// The decomposition failed; the stream is rejected for good.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecomposeError {
    /// The container cannot be decomposed; store the source bytes
    /// opaquely instead.
    #[error("container is opaque: {0}")]
    Opaque(OpaqueReason),
    /// The stream was already rejected.
    #[error("decomposition already rejected this stream")]
    StreamRejected,
    /// The input does not begin like any recognized container.
    #[error("input is not a recognized container")]
    NotAContainer,
}
