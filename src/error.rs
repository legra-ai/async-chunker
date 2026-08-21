//! Errors reported by chunk profiles.

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
}
