//! Member media-type inference: the frozen member-name → media-type
//! table, reconciled with the byte-prefix detector.

use crate::media_type::MediaType;
use crate::probe::{Detection, Detector};
use crate::profile::ChunkingProfile;
use crate::registry::ProfileRegistry;

use super::sink::ContainerKind;

/// The frozen extension → media-type table (lower-case extensions,
/// final component only; `.tar.gz`/`.tgz` map to the wrapped-tar
/// container). Membership is a format decision: moving an entry is
/// an explicit cutover.
const EXTENSIONS: &[(&str, &str)] = &[
    // documents
    ("pdf", "application/pdf"),
    (
        "docx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ),
    (
        "xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    ),
    (
        "pptx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ),
    ("odt", "application/vnd.oasis.opendocument.text"),
    ("ods", "application/vnd.oasis.opendocument.spreadsheet"),
    ("odp", "application/vnd.oasis.opendocument.presentation"),
    // text and data
    ("txt", "text/plain"),
    ("md", "text/markdown"),
    ("html", "text/html"),
    ("htm", "text/html"),
    ("css", "text/css"),
    ("csv", "text/csv"),
    ("tsv", "text/tab-separated-values"),
    ("xml", "application/xml"),
    ("json", "application/json"),
    ("yaml", "application/yaml"),
    ("yml", "application/yaml"),
    ("toml", "application/toml"),
    ("js", "text/javascript"),
    ("svg", "image/svg+xml"),
    ("ttl", "text/turtle"),
    ("n3", "text/n3"),
    ("nt", "application/n-triples"),
    ("nq", "application/n-quads"),
    ("trig", "application/trig"),
    // images
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("heic", "image/heic"),
    ("heif", "image/heif"),
    ("avif", "image/avif"),
    // audio / video
    ("mp3", "audio/mpeg"),
    ("aac", "audio/aac"),
    ("flac", "audio/flac"),
    ("mp4", "video/mp4"),
    ("m4a", "audio/mp4"),
    ("mov", "video/quicktime"),
    ("mkv", "video/x-matroska"),
    ("webm", "video/webm"),
    ("ts", "video/mp2t"),
    // containers
    ("zip", "application/zip"),
    ("jar", "application/java-archive"),
    ("epub", "application/epub+zip"),
    ("tar", "application/x-tar"),
    ("gz", "application/gzip"),
    ("tgz", "application/gzip"),
];

/// What inference concluded about one member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferredMember {
    /// The media type to declare for the member's own literal, when
    /// the table and the detector resolve one without contradiction.
    pub media_type: Option<MediaType>,
    /// The nested container to recurse into, when the name and the
    /// bytes agree the member is one.
    pub container: Option<ContainerKind>,
}

/// The final extension of `path` (bytes after the last `.` of the
/// last component), ASCII-lowercased.
fn extension(path: &[u8]) -> Option<String> {
    let name = path.rsplit(|byte| *byte == b'/').next()?;
    let dot = name.iter().rposition(|byte| *byte == b'.')?;
    let ext = &name[dot + 1..];
    if ext.is_empty() || ext.len() > 8 {
        return None;
    }
    std::str::from_utf8(ext)
        .ok()
        .map(|ext| ext.to_ascii_lowercase())
}

/// Whether `path` names a wrapped tar (`.tar.gz` or `.tgz`).
fn is_wrapped_tar_name(path: &[u8]) -> bool {
    let lower: Vec<u8> = path.iter().map(u8::to_ascii_lowercase).collect();
    lower.ends_with(b".tar.gz") || lower.ends_with(b".tgz")
}

/// The container kind a byte prefix positively identifies, if any.
pub(super) fn container_by_prefix(prefix: &[u8]) -> Option<ContainerKind> {
    if prefix.starts_with(&[0x1F, 0x8B]) {
        // Gzip; whether it wraps a TAR is decided after inflating.
        return Some(ContainerKind::Gzip);
    }
    if prefix.starts_with(&[0x50, 0x4B, 0x03, 0x04])
        || prefix.starts_with(&[0x50, 0x4B, 0x05, 0x06])
    {
        return Some(ContainerKind::Zip);
    }
    if is_tar_prefix(prefix) {
        return Some(ContainerKind::Tar);
    }
    None
}

/// Whether `prefix` begins with a plausible TAR header: the `ustar`
/// magic, or (for pre-POSIX writers) a verified header checksum.
pub(super) fn is_tar_prefix(prefix: &[u8]) -> bool {
    if prefix.len() < 512 {
        return false;
    }
    if &prefix[257..262] == b"ustar" {
        return true;
    }
    super::tar::checksum_matches(&prefix[..512])
}

/// Infer a member's media type and nested-container status from its
/// `path` and its first bytes.
///
/// The frozen rules: the name table plays the declared side and the
/// byte prefix the detected side. Recursion happens only when the
/// bytes positively identify a container **and** the name does not
/// contradict it (a `.zip` name with non-ZIP bytes, or ZIP bytes
/// under a `.png` name, make an ordinary member). The declared media
/// type survives unless the detector positively contradicts it, in
/// which case the member is typed by nothing (`None` — the caller
/// stores it under its generic datatype).
#[must_use]
pub fn infer_member_media(path: &[u8], prefix: &[u8]) -> InferredMember {
    let named: Option<MediaType> = extension(path)
        .and_then(|ext| EXTENSIONS.iter().find(|(entry, _)| *entry == ext))
        .and_then(|(_, essence)| MediaType::parse(essence).ok());
    let by_bytes = container_by_prefix(prefix);
    let named_container = named.as_ref().is_some_and(|media| {
        ProfileRegistry::V2.select(media) == ChunkingProfile::ZipV1
            || ProfileRegistry::V2.select(media) == ChunkingProfile::OoxmlV1
            || media.essence() == "application/x-tar"
            || media.essence() == "application/gzip"
    }) || is_wrapped_tar_name(path);

    // Recursion: bytes must positively identify a container and the
    // name must not contradict.
    let container = match by_bytes {
        Some(kind) if named_container || named.is_none() => Some(kind),
        _ => None,
    };
    if container.is_some() {
        return InferredMember {
            media_type: named,
            container,
        };
    }

    // Ordinary member: reconcile the named type with the detector.
    let media_type = match &named {
        Some(media) => {
            let declared = ProfileRegistry::V2.select(media);
            match Detector::V2.detect(prefix) {
                Detection::Unrecognized => named.clone(),
                detection => detection.reconcile(declared).ok().and(named.clone()),
            }
        }
        None => None,
    };
    InferredMember {
        media_type,
        container: None,
    }
}
