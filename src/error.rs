//! Errors reported by chunk profiles and the probing entry points.

use crate::media_type::MediaTypeError;
use crate::probe::ProfileSet;

/// A chunk profile rejected the input stream.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChunkError {
    /// The asynchronous source could not provide the next input window.
    #[error("input stream failed: {0}")]
    Io(String),

    /// The profile is registered but does not have an implementation.
    #[error("chunking profile {profile} is registered but not implemented")]
    ProfileUnimplemented {
        /// The registered profile name.
        profile: &'static str,
    },
    /// The input is malformed for the selected structured profile.
    #[error("chunking profile {profile} rejects the input at byte {offset}: {detail}")]
    MalformedProfileInput {
        /// The profile name.
        profile: &'static str,
        /// The byte offset of the first rejected byte.
        offset: u64,
        /// The parser's rejection detail.
        detail: &'static str,
    },
    /// The profile rejected the stream and cannot accept more input.
    #[error("chunking profile {profile} already rejected this stream")]
    ProfileStreamRejected {
        /// The profile name.
        profile: &'static str,
    },
    /// A declared media type failed to parse.
    #[error("malformed media type: {0}")]
    MalformedMediaType(#[from] MediaTypeError),
    /// The byte prefix matched more than one specialist and no media
    /// type was declared to pick one.
    #[error("byte prefix matches several chunking profiles ({candidates}); declare a media type")]
    AmbiguousDetection {
        /// Every specialist whose probe matched.
        candidates: ProfileSet,
    },
    /// A specialist was declared but the byte prefix was recognized
    /// only as other specialists.
    #[error("declared chunking profile {declared} but the byte prefix is recognized as {detected}")]
    DeclaredDetectedMismatch {
        /// The profile the declared media type selected.
        declared: &'static str,
        /// Every specialist whose probe matched.
        detected: ProfileSet,
    },
}
