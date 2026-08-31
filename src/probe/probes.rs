//! The frozen version-1 probes: each mirrors what its engine accepts
//! at the start of a stream.

use super::text::is_structured_text_prefix;
use crate::profile::ChunkingProfile;

/// One probe: a specialist profile and its prefix predicate.
pub(super) struct Probe {
    pub(super) profile: ChunkingProfile,
    pub(super) matches: fn(&[u8]) -> bool,
}

/// The version-2 probes: version 1 plus the OOXML-package and PDF
/// signatures, with the ZIP probe deferring to the OOXML one so
/// detections stay disjoint.
pub(super) const V2: &[Probe] = &[
    Probe {
        profile: ChunkingProfile::StructuredTextV1,
        matches: is_structured_text_prefix,
    },
    Probe {
        profile: ChunkingProfile::OoxmlV1,
        matches: is_ooxml,
    },
    Probe {
        profile: ChunkingProfile::PdfV1,
        matches: is_pdf,
    },
    Probe {
        profile: ChunkingProfile::ZipV1,
        matches: is_zip_not_ooxml,
    },
    Probe {
        profile: ChunkingProfile::IsobmffV1,
        matches: is_isobmff,
    },
    Probe {
        profile: ChunkingProfile::MatroskaV1,
        matches: is_matroska,
    },
    Probe {
        profile: ChunkingProfile::MpegtsV1,
        matches: is_mpegts,
    },
    Probe {
        profile: ChunkingProfile::FramedAudioV1,
        matches: is_framed_audio,
    },
];

/// The version-1 probes, in registry order.
pub(super) const V1: &[Probe] = &[
    Probe {
        profile: ChunkingProfile::StructuredTextV1,
        matches: is_structured_text_prefix,
    },
    Probe {
        profile: ChunkingProfile::ZipV1,
        matches: is_zip,
    },
    Probe {
        profile: ChunkingProfile::IsobmffV1,
        matches: is_isobmff,
    },
    Probe {
        profile: ChunkingProfile::MatroskaV1,
        matches: is_matroska,
    },
    Probe {
        profile: ChunkingProfile::MpegtsV1,
        matches: is_mpegts,
    },
    Probe {
        profile: ChunkingProfile::FramedAudioV1,
        matches: is_framed_audio,
    },
];

/// A local file header (a member) or an end record (an empty
/// archive) — the two records `zip-v1` accepts first.
fn is_zip(prefix: &[u8]) -> bool {
    prefix.starts_with(&[0x50, 0x4B, 0x03, 0x04]) || prefix.starts_with(&[0x50, 0x4B, 0x05, 0x06])
}

/// Types an ISO BMFF stream may begin with (ISO 14496-12 §4.3 and
/// the segment/`QuickTime` variants) — the same vocabulary as the
/// engine's first-box check.
const ISOBMFF_FIRST_BOXES: [[u8; 4]; 9] = [
    *b"ftyp", *b"styp", *b"sidx", *b"moov", *b"moof", *b"free", *b"skip", *b"wide", *b"mdat",
];

/// A box header whose type may begin a stream and whose compact size
/// is a plausible header length (`0` open-ended, `1` large, or at
/// least the eight header bytes).
fn is_isobmff(prefix: &[u8]) -> bool {
    let Some(header) = prefix.get(..8) else {
        return false;
    };
    let size = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
    let kind = [header[4], header[5], header[6], header[7]];
    (size == 0 || size == 1 || size >= 8) && ISOBMFF_FIRST_BOXES.contains(&kind)
}

/// The EBML header element ID that begins every Matroska/`WebM`
/// stream.
fn is_matroska(prefix: &[u8]) -> bool {
    prefix.starts_with(&[0x1A, 0x45, 0xDF, 0xA3])
}

const TS_PACKET_LEN: usize = 188;
const TS_SYNC: u8 = 0x47;

/// A sync byte at the start of the first two packets — or of a
/// single, complete packet when the stream is exactly one packet
/// long. One sync byte alone (`G`) is too common to be a signature.
fn is_mpegts(prefix: &[u8]) -> bool {
    if prefix.first() != Some(&TS_SYNC) {
        return false;
    }
    match prefix.get(TS_PACKET_LEN) {
        Some(second) => *second == TS_SYNC,
        None => prefix.len() == TS_PACKET_LEN,
    }
}

/// What `framed-audio-v1` locks onto: an `ID3v2` tag header with
/// sync-safe size bytes, a `fLaC` marker followed by its mandatory
/// `STREAMINFO` block header, or an MPEG/ADTS frame sync.
fn is_framed_audio(prefix: &[u8]) -> bool {
    if prefix.starts_with(b"ID3") {
        return prefix
            .get(6..10)
            .is_some_and(|sizes| sizes.iter().all(|byte| byte & 0x80 == 0));
    }
    if prefix.starts_with(b"fLaC") {
        return prefix.get(4).is_some_and(|flags| flags & 0x7F == 0);
    }
    matches!(prefix, [0xFF, second, ..] if second & 0xE0 == 0xE0)
}

/// An Office Open XML package: a ZIP whose first member is named
/// `[Content_Types].xml` (Office writers emit it first; the
/// 19-byte name and its length field sit inside any prefix that
/// reaches 49 bytes).
fn is_ooxml(prefix: &[u8]) -> bool {
    const NAME: &[u8] = b"[Content_Types].xml";
    if !prefix.starts_with(&[0x50, 0x4B, 0x03, 0x04]) {
        return false;
    }
    let Some(len_bytes) = prefix.get(26..28) else {
        return false;
    };
    if u16::from_le_bytes([len_bytes[0], len_bytes[1]]) != NAME.len() as u16 {
        return false;
    }
    prefix.get(30..30 + NAME.len()) == Some(NAME)
}

/// A ZIP container that is not an OOXML package.
fn is_zip_not_ooxml(prefix: &[u8]) -> bool {
    is_zip(prefix) && !is_ooxml(prefix)
}

/// The `%PDF-` header.
fn is_pdf(prefix: &[u8]) -> bool {
    prefix.starts_with(b"%PDF-")
}
