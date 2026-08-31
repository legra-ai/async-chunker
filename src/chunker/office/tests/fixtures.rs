//! Synthetic Office packages built on the ZIP fixture writer.

use super::super::super::zip::tests::writer::{Member, Options, archive};

/// Deterministic pseudo-random bytes.
pub(super) fn noise(seed: &str, len: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; len];
    let mut hasher = blake3::Hasher::new();
    hasher.update(seed.as_bytes());
    hasher.finalize_xof().fill(&mut bytes);
    bytes
}

/// XML-shaped text: tags and prose lines.
pub(super) fn xml(seed: &str, len: usize) -> Vec<u8> {
    let raw = noise(seed, len);
    let mut out = Vec::with_capacity(len * 2);
    out.extend_from_slice(b"<?xml version=\"1.0\"?>\n<root>\n");
    for chunk in raw.chunks(24) {
        out.extend_from_slice(b"<p>");
        for byte in chunk {
            out.push(b'a' + byte % 26);
        }
        out.extend_from_slice(b"</p>\n");
    }
    out.extend_from_slice(b"</root>\n");
    out.truncate(len.max(64));
    out
}

/// One part for a synthetic package.
pub(super) struct Part {
    pub(super) name: &'static str,
    pub(super) bytes: Vec<u8>,
    pub(super) deflate: bool,
}

/// A minimal `.docx`-shaped package: content types first, rels,
/// document, styles, and one media part.
pub(super) fn docx_parts(doc_seed: &str, media_seed: &str) -> Vec<Part> {
    vec![
        Part {
            name: "[Content_Types].xml",
            bytes: xml("fixtures/content-types", 600),
            deflate: true,
        },
        Part {
            name: "_rels/.rels",
            bytes: xml("fixtures/rels", 400),
            deflate: true,
        },
        Part {
            name: "word/document.xml",
            bytes: xml(doc_seed, 120 << 10),
            deflate: true,
        },
        Part {
            name: "word/styles.xml",
            bytes: xml("fixtures/styles", 6 << 10),
            deflate: true,
        },
        Part {
            name: "word/media/image1.png",
            bytes: noise(media_seed, 300 << 10),
            deflate: false,
        },
    ]
}

/// Assemble parts into an archive; `deflate` per part.
pub(super) fn package(parts: &[Part]) -> Vec<u8> {
    let members: Vec<Member<'_>> = parts
        .iter()
        .map(|part| {
            if part.deflate {
                Member::deflated(part.name, &part.bytes)
            } else {
                Member::stored(part.name, &part.bytes)
            }
        })
        .collect();
    archive(&members, Options::default())
}

/// The same parts, all stored (a different "compressor").
pub(super) fn package_stored(parts: &[Part]) -> Vec<u8> {
    let members: Vec<Member<'_>> = parts
        .iter()
        .map(|part| Member::stored(part.name, &part.bytes))
        .collect();
    archive(&members, Options::default())
}
