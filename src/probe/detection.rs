//! [`Detection`] — what the probes concluded, and how it reconciles
//! with a declared profile.

use super::set::ProfileSet;
use crate::error::ChunkError;
use crate::profile::ChunkingProfile;

/// The outcome of probing a byte prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detection {
    /// No specialist probe matched.
    Unrecognized,
    /// Exactly one specialist probe matched.
    Recognized(ChunkingProfile),
    /// More than one specialist probe matched.
    Ambiguous(ProfileSet),
}

impl Detection {
    /// The matching specialists as a set (empty when unrecognized).
    #[must_use]
    pub fn candidates(self) -> ProfileSet {
        match self {
            Self::Unrecognized => ProfileSet::EMPTY,
            Self::Recognized(profile) => ProfileSet::single(profile),
            Self::Ambiguous(set) => set,
        }
    }

    /// The profile to chunk with when nothing was declared: the
    /// recognized specialist, or the explicit generic profile when
    /// no probe matched.
    ///
    /// # Errors
    ///
    /// Returns [`ChunkError::AmbiguousDetection`] when more than one
    /// specialist matched — the caller must declare a media type.
    pub fn resolve(self) -> Result<ChunkingProfile, ChunkError> {
        match self {
            Self::Unrecognized => Ok(ChunkingProfile::GenericCdcV1),
            Self::Recognized(profile) => Ok(profile),
            Self::Ambiguous(candidates) => Err(ChunkError::AmbiguousDetection { candidates }),
        }
    }

    /// Reconcile the detection with the profile a declared media
    /// type selected. The declaration wins whenever the bytes do not
    /// positively contradict it:
    ///
    /// - a generic declaration makes no structural claim, so any
    ///   detection is compatible with it;
    /// - an unrecognized prefix contradicts nothing — the specialist
    ///   engine remains the authority on malformed input;
    /// - a recognized or ambiguous detection that includes the
    ///   declared specialist confirms it.
    ///
    /// # Errors
    ///
    /// Returns [`ChunkError::DeclaredDetectedMismatch`] when a
    /// specialist was declared and the probes recognized only other
    /// specialists.
    pub fn reconcile(self, declared: ChunkingProfile) -> Result<ChunkingProfile, ChunkError> {
        let detected = self.candidates();
        if declared == ChunkingProfile::GenericCdcV1
            || detected.is_empty()
            || detected.contains(declared)
        {
            return Ok(declared);
        }
        Err(ChunkError::DeclaredDetectedMismatch {
            declared: declared.name(),
            detected,
        })
    }
}
