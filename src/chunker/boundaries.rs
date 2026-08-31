//! [`ChunkBoundaries`] — the boundaries a profile places in an
//! in-memory payload.

use crate::ChunkError;

use super::profile_chunker::{Chunker, ProfileChunker};
use crate::profile::ChunkingProfile;

/// Chunk boundaries of an in-memory payload, as end offsets.
///
/// The offsets address the profile's **canonical form** of the
/// payload (the payload itself for every profile except the
/// canonicalizing `ooxml-v1`).
///
/// Convenience over the streaming chunkers for callers that already
/// hold the whole value (tests, measurement harnesses); streaming
/// ingest drives a [`ProfileChunker`] directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkBoundaries {
    // bounded: one offset per chunk of the caller's own payload.
    ends: Vec<usize>,
}

impl ChunkBoundaries {
    /// Compute the boundaries `profile` places in `bytes`.
    ///
    /// # Errors
    ///
    /// Propagates the profile's fail-hard rejection of malformed
    /// input, and [`ChunkError::ProfileUnimplemented`] for
    /// a profile without an implementation.
    pub fn of(profile: ChunkingProfile, bytes: &[u8]) -> Result<Self, ChunkError> {
        let mut ends = Vec::new();
        let mut end = 0usize;
        let mut chunker = ProfileChunker::open(profile)?;
        let mut record = |chunk: &[u8]| {
            end += chunk.len();
            ends.push(end);
        };
        chunker.push(bytes, &mut record)?;
        chunker.finish(&mut record)?;
        Ok(Self { ends })
    }

    /// The chunk count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.ends.len()
    }

    /// Whether the payload produced no chunks (it was empty).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ends.is_empty()
    }

    /// The chunk ranges, in payload order.
    pub fn ranges(&self) -> impl Iterator<Item = core::ops::Range<usize>> + '_ {
        let mut start = 0usize;
        self.ends.iter().map(move |&end| {
            let range = start..end;
            start = end;
            range
        })
    }

    /// The chunk end offsets, in payload order.
    pub fn ends(&self) -> impl Iterator<Item = usize> + '_ {
        self.ends.iter().copied()
    }
}
