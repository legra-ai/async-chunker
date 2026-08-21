//! Chunking-profile identity: the frozen registry of versioned
//! profiles and their stable wire IDs.

use std::fmt;

/// The stable wire identifier of one chunking profile, as recorded
/// in every literal root manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChunkingProfileId(u16);

impl ChunkingProfileId {
    /// A profile ID from its wire value. An unregistered value is
    /// representable so a decoder can report it precisely; resolve
    /// through [`ChunkingProfile::from_id`] to learn whether it
    /// names a known profile.
    #[must_use]
    pub const fn from_value(value: u16) -> Self {
        Self(value)
    }

    /// The raw wire value.
    #[must_use]
    pub const fn value(self) -> u16 {
        self.0
    }
}

impl fmt::Display for ChunkingProfileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match ChunkingProfile::from_id(*self) {
            Some(profile) => f.write_str(profile.name()),
            None => write!(f, "unknown-profile-{}", self.0),
        }
    }
}

/// The frozen initial chunking-profile registry (ELS-02;
/// `docs-internal/blocks/content.mdx`). A datatype mapped to a
/// profile that is not yet implemented fails hard at ingest — it
/// never silently falls back to [`Self::GenericCdcV1`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChunkingProfile {
    /// Canonical content-defined chunking for every MIME datatype
    /// without a specialist profile (gear hash, 16/64/256 KiB).
    GenericCdcV1,
    /// Markdown, JSON, XML/HTML text media types, and the RDF/XSD
    /// textual datatypes (ELS-06).
    StructuredTextV1,
    /// ZIP containers and the OOXML/ODF families (ELS-07).
    ZipV1,
    /// ISO Base Media File Format: MP4/MOV/HEIF (ELS-08).
    IsobmffV1,
    /// Matroska/WebM (ELS-09).
    MatroskaV1,
    /// MPEG transport streams (ELS-10).
    MpegtsV1,
    /// Framed audio: MP3/AAC/FLAC-style streams (ELS-11).
    FramedAudioV1,
}

impl ChunkingProfile {
    /// Every registry entry, in stable wire-ID order.
    pub const ALL: [Self; 7] = [
        Self::GenericCdcV1,
        Self::StructuredTextV1,
        Self::ZipV1,
        Self::IsobmffV1,
        Self::MatroskaV1,
        Self::MpegtsV1,
        Self::FramedAudioV1,
    ];

    /// The profile's frozen wire ID.
    #[must_use]
    pub const fn id(self) -> ChunkingProfileId {
        ChunkingProfileId(match self {
            Self::GenericCdcV1 => 1,
            Self::StructuredTextV1 => 2,
            Self::ZipV1 => 3,
            Self::IsobmffV1 => 4,
            Self::MatroskaV1 => 5,
            Self::MpegtsV1 => 6,
            Self::FramedAudioV1 => 7,
        })
    }

    /// Resolve a wire ID back to its registry entry.
    #[must_use]
    pub fn from_id(id: ChunkingProfileId) -> Option<Self> {
        Self::ALL.into_iter().find(|profile| profile.id() == id)
    }

    /// The profile's frozen versioned name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::GenericCdcV1 => "generic-cdc-v1",
            Self::StructuredTextV1 => "structured-text-v1",
            Self::ZipV1 => "zip-v1",
            Self::IsobmffV1 => "isobmff-v1",
            Self::MatroskaV1 => "matroska-v1",
            Self::MpegtsV1 => "mpegts-v1",
            Self::FramedAudioV1 => "framed-audio-v1",
        }
    }

    /// Whether the profile's chunker is implemented. Since ELS-11
    /// every registry profile is; the gate remains for the frozen
    /// registry contract (a future profile fails hard at ingest
    /// until its chunker lands).
    #[must_use]
    pub const fn is_implemented(self) -> bool {
        true
    }
}
