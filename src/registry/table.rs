//! The frozen registry versions and their media-type families.

use std::fmt;

use crate::media_type::MediaType;
use crate::profile::ChunkingProfile;

/// The version of a [`ProfileRegistry`]. Membership is a format
/// decision: a media type moving between profiles changes every
/// boundary it produces, so it changes the registry version rather
/// than an existing table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RegistryVersion(u16);

impl RegistryVersion {
    /// The raw version number.
    #[must_use]
    pub const fn value(self) -> u16 {
        self.0
    }
}

impl fmt::Display for RegistryVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "registry-v{}", self.0)
    }
}

/// One family: a specialist profile and the media-type essences it
/// serves.
struct Family {
    profile: ChunkingProfile,
    essences: &'static [&'static str],
}

/// A versioned media-type → profile registry.
///
/// Lookups key on the media type's [essence](MediaType::essence) —
/// parameters never change the profile. Any media type outside every
/// specialist family selects [`ChunkingProfile::GenericCdcV1`] by
/// the frozen rule; that is an explicit selection, not a fallback,
/// and the registry never guesses from the bytes (see
/// [`Detector`](crate::Detector) for that).
#[derive(Clone, Copy)]
pub struct ProfileRegistry {
    version: RegistryVersion,
    families: &'static [Family],
}

impl fmt::Debug for ProfileRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProfileRegistry")
            .field("version", &self.version)
            .field("specialist_count", &self.specialist_count())
            .finish()
    }
}

impl ProfileRegistry {
    /// Registry version 1: the initial families.
    pub const V1: Self = Self {
        version: RegistryVersion(1),
        families: V1_FAMILIES,
    };

    /// Registry version 2: the Office Open XML media types move from
    /// `zip-v1` to the canonicalizing `ooxml-v1` (the byte-exact
    /// `ooxml-ber-v1` is a policy selection, never a registry
    /// mapping), and `application/pdf` gains `pdf-v1`.
    pub const V2: Self = Self {
        version: RegistryVersion(2),
        families: V2_FAMILIES,
    };

    /// The registry's version.
    #[must_use]
    pub const fn version(&self) -> RegistryVersion {
        self.version
    }

    /// The profile that chunks `media_type`: its specialist, or
    /// [`ChunkingProfile::GenericCdcV1`] when no family lists it.
    #[must_use]
    pub fn select(&self, media_type: &MediaType) -> ChunkingProfile {
        self.specialist(media_type)
            .unwrap_or(ChunkingProfile::GenericCdcV1)
    }

    /// The specialist profile listed for `media_type`, if any.
    #[must_use]
    pub fn specialist(&self, media_type: &MediaType) -> Option<ChunkingProfile> {
        let essence = media_type.essence();
        self.families
            .iter()
            .find(|family| family.essences.contains(&essence))
            .map(|family| family.profile)
    }

    /// Every media-type essence a profile serves (none for the
    /// generic profile, which serves everything unlisted).
    pub fn essences(&self, profile: ChunkingProfile) -> impl Iterator<Item = &'static str> + use<> {
        let families = self.families;
        families
            .iter()
            .filter(move |family| family.profile == profile)
            .flat_map(|family| family.essences.iter().copied())
    }

    /// How many media-type essences the specialist families cover.
    #[must_use]
    pub fn specialist_count(&self) -> usize {
        self.families
            .iter()
            .map(|family| family.essences.len())
            .sum()
    }
}

impl Default for ProfileRegistry {
    fn default() -> Self {
        Self::V2
    }
}

/// `structured-text-v1`: text, JSON, and XML/HTML media types.
const STRUCTURED_TEXT: &[&str] = &[
    "text/plain",
    "text/markdown",
    "text/html",
    "text/css",
    "text/csv",
    "text/tab-separated-values",
    "text/xml",
    "text/calendar",
    "text/vcard",
    "text/javascript",
    "text/turtle",
    "text/n3",
    "application/json",
    "application/ld+json",
    "application/xml",
    "application/xhtml+xml",
    "application/rdf+xml",
    "application/n-triples",
    "application/n-quads",
    "application/trig",
    "application/sparql-query",
    "application/sparql-update",
    "application/sparql-results+json",
    "application/sparql-results+xml",
    "application/yaml",
    "application/toml",
    "application/javascript",
    "image/svg+xml",
];

/// `zip-v1` in registry v1: ZIP containers and the OOXML/ODF
/// families.
const ZIP: &[&str] = &[
    "application/zip",
    "application/java-archive",
    "application/epub+zip",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    "application/vnd.oasis.opendocument.text",
    "application/vnd.oasis.opendocument.spreadsheet",
    "application/vnd.oasis.opendocument.presentation",
];

/// `zip-v1` in registry v2: ZIP containers and the ODF family (ODF
/// packages begin with a `mimetype` member, not
/// `[Content_Types].xml`, so the OOXML profiles do not apply).
const ZIP_V2: &[&str] = &[
    "application/zip",
    "application/java-archive",
    "application/epub+zip",
    "application/vnd.oasis.opendocument.text",
    "application/vnd.oasis.opendocument.spreadsheet",
    "application/vnd.oasis.opendocument.presentation",
];

/// `ooxml-v1`: the Office Open XML package kinds.
const OOXML: &[&str] = &[
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation",
];

/// `pdf-v1`: PDF documents.
const PDF: &[&str] = &["application/pdf"];

/// `isobmff-v1`: the ISO Base Media File Format family.
const ISOBMFF: &[&str] = &[
    "video/mp4",
    "audio/mp4",
    "application/mp4",
    "video/quicktime",
    "image/heif",
    "image/heic",
    "image/avif",
];

/// `matroska-v1`: Matroska and `WebM`.
const MATROSKA: &[&str] = &[
    "video/x-matroska",
    "audio/x-matroska",
    "video/webm",
    "audio/webm",
];

/// `mpegts-v1`: MPEG transport streams.
const MPEGTS: &[&str] = &["video/mp2t"];

/// `framed-audio-v1`: frame-synchronized audio streams.
const FRAMED_AUDIO: &[&str] = &["audio/mpeg", "audio/aac", "audio/flac"];

const V1_FAMILIES: &[Family] = &[
    Family {
        profile: ChunkingProfile::StructuredTextV1,
        essences: STRUCTURED_TEXT,
    },
    Family {
        profile: ChunkingProfile::ZipV1,
        essences: ZIP,
    },
    Family {
        profile: ChunkingProfile::IsobmffV1,
        essences: ISOBMFF,
    },
    Family {
        profile: ChunkingProfile::MatroskaV1,
        essences: MATROSKA,
    },
    Family {
        profile: ChunkingProfile::MpegtsV1,
        essences: MPEGTS,
    },
    Family {
        profile: ChunkingProfile::FramedAudioV1,
        essences: FRAMED_AUDIO,
    },
];

const V2_FAMILIES: &[Family] = &[
    Family {
        profile: ChunkingProfile::StructuredTextV1,
        essences: STRUCTURED_TEXT,
    },
    Family {
        profile: ChunkingProfile::ZipV1,
        essences: ZIP_V2,
    },
    Family {
        profile: ChunkingProfile::OoxmlV1,
        essences: OOXML,
    },
    Family {
        profile: ChunkingProfile::PdfV1,
        essences: PDF,
    },
    Family {
        profile: ChunkingProfile::IsobmffV1,
        essences: ISOBMFF,
    },
    Family {
        profile: ChunkingProfile::MatroskaV1,
        essences: MATROSKA,
    },
    Family {
        profile: ChunkingProfile::MpegtsV1,
        essences: MPEGTS,
    },
    Family {
        profile: ChunkingProfile::FramedAudioV1,
        essences: FRAMED_AUDIO,
    },
];
