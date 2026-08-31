//! [`Detector`] — the versioned probe table.

use super::detection::Detection;
use super::probes::{self, Probe};
use super::set::ProfileSet;
use crate::constants::PROBE_PREFIX_MAX_BYTES;

/// A versioned table of prefix probes, one per specialist profile.
///
/// Every probe is a pure function of at most
/// [`PROBE_PREFIX_MAX_BYTES`] leading bytes, so detection is
/// deterministic and never depends on how the stream was windowed.
/// Probes are deliberately conservative: each accepts only prefixes
/// its engine's opening parse accepts, and the structured-text probe
/// additionally refuses control characters so binary containers
/// with printable signatures never read as text.
#[derive(Clone, Copy)]
pub struct Detector {
    probes: &'static [Probe],
}

impl std::fmt::Debug for Detector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Detector")
            .field("probes", &self.probes.len())
            .finish()
    }
}

impl Detector {
    /// Detector version 1: the probes for registry version 1.
    pub const V1: Self = Self { probes: probes::V1 };

    /// Detector version 2: the probes for registry version 2 —
    /// version 1 plus OOXML-package and PDF signatures, with ZIP
    /// deferring to OOXML so detections stay disjoint.
    pub const V2: Self = Self { probes: probes::V2 };

    /// Probe `prefix` — the stream's first bytes, at most
    /// [`PROBE_PREFIX_MAX_BYTES`] of them (a longer slice is
    /// truncated so the result matches a bounded read).
    #[must_use]
    pub fn detect(&self, prefix: &[u8]) -> Detection {
        let prefix = &prefix[..prefix.len().min(PROBE_PREFIX_MAX_BYTES)];
        let mut matched = ProfileSet::EMPTY;
        for probe in self.probes {
            if (probe.matches)(prefix) {
                matched.insert(probe.profile);
            }
        }
        match matched.len() {
            0 => Detection::Unrecognized,
            1 => Detection::Recognized(matched.iter().next().expect("one member")),
            _ => Detection::Ambiguous(matched),
        }
    }
}

impl Default for Detector {
    fn default() -> Self {
        Self::V2
    }
}
