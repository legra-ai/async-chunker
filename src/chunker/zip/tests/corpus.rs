//! Fixture corpora: OOXML-shaped archives, edited variants, and
//! archives sharing a member.

use super::writer::{Framing, Member, Options, archive};

/// Deterministic pseudo-random bytes (incompressible, like media).
pub(super) fn noise(seed: &str, len: usize) -> Vec<u8> {
    // bounded: fixture payloads are test constants.
    let mut bytes = vec![0u8; len];
    let mut hasher = blake3::Hasher::new();
    hasher.update(seed.as_bytes());
    hasher.finalize_xof().fill(&mut bytes);
    bytes
}

/// WordprocessingML-shaped XML of roughly `paragraphs` paragraphs,
/// with `marker` in the middle so an edit can target it.
pub(super) fn document_xml(seed: &str, paragraphs: usize, marker: &str) -> Vec<u8> {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>",
    );
    // Word stamps every paragraph with random revision identifiers;
    // they are what keeps a real document part from deflating to
    // almost nothing.
    let words = noise(seed, paragraphs * 12);
    for (index, chunk) in words.chunks(12).enumerate() {
        let rsid: String = chunk[4..]
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect();
        out.push_str(&format!(
            "<w:p w:rsidR=\"{}\" w14:paraId=\"{}\"><w:r><w:t>",
            &rsid[..8],
            &rsid[8..]
        ));
        for byte in &chunk[..4] {
            out.push_str(match byte % 6 {
                0 => "legra ",
                1 => "literal ",
                2 => "manifest ",
                3 => "chunk ",
                4 => "workspace ",
                _ => "canonical ",
            });
        }
        if index == paragraphs / 2 {
            out.push_str(marker);
        }
        out.push_str("</w:t></w:r></w:p>");
    }
    out.push_str("</w:body></w:document>");
    out.into_bytes()
}

const CONTENT_TYPES: &[u8] = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/></Types>";
const RELS: &[u8] = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"officeDocument\" Target=\"word/document.xml\"/></Relationships>";

/// The media parts every variant shares.
pub(super) struct Media {
    pub(super) image1: Vec<u8>,
    pub(super) image2: Vec<u8>,
    pub(super) styles: Vec<u8>,
}

impl Media {
    pub(super) fn new() -> Self {
        Self {
            image1: noise("els07/image1", 300 << 10),
            image2: noise("els07/image2", 150 << 10),
            styles: document_xml("els07/styles", 400, ""),
        }
    }
}

/// A Word-shaped document: content types, rels, the document part
/// (deflated), styles, and two media parts.
pub(super) fn docx(media: &Media, document: &[u8]) -> Vec<u8> {
    archive(
        &[
            Member::stored("[Content_Types].xml", CONTENT_TYPES),
            Member::stored("_rels/.rels", RELS),
            Member::deflated("word/document.xml", document),
            Member::deflated("word/styles.xml", &media.styles),
            Member::deflated("word/media/image1.png", &media.image1),
            Member::deflated("word/media/image2.png", &media.image2),
        ],
        Options::default(),
    )
}

/// The same document under every framing the walker must accept,
/// including ZIP64 sizes and a ZIP64 end record.
pub(super) fn docx_with_framings(media: &Media, document: &[u8], zip64_end: bool) -> Vec<u8> {
    archive(
        &[
            Member::stored("[Content_Types].xml", CONTENT_TYPES),
            Member::stored("_rels/.rels", RELS).framed(Framing::SignedDescriptor),
            Member::deflated("word/document.xml", document).framed(Framing::SignedDescriptor),
            Member::deflated("word/styles.xml", &media.styles)
                .framed(Framing::UnsignedDescriptorKnownSize),
            Member::deflated("word/media/image1.png", &media.image1)
                .framed(Framing::Zip64 { descriptor: false }),
            Member::deflated("word/media/image2.png", &media.image2)
                .framed(Framing::Zip64 { descriptor: true }),
        ],
        Options {
            zip64_end,
            comment: b"els07",
        },
    )
}

/// Offset of a member's central directory entry.
pub(super) fn central_entry(bytes: &[u8], name: &str) -> usize {
    (0..bytes.len() - 4)
        .find(|&at| {
            bytes[at..at + 4] == [0x50, 0x4B, 0x01, 0x02]
                && bytes[at + 46..].starts_with(name.as_bytes())
        })
        .expect("central entry present")
}

/// Byte range of a member's data-bearing region: from its local
/// header to the next local header (or the central directory).
pub(super) fn member_span(bytes: &[u8], name: &str) -> std::ops::Range<usize> {
    let header = [0x50, 0x4B, 0x03, 0x04];
    let starts: Vec<usize> = (0..bytes.len().saturating_sub(4))
        .filter(|&at| bytes[at..at + 4] == header)
        .collect();
    let start = starts
        .iter()
        .copied()
        .find(|&at| bytes[at + 30..].starts_with(name.as_bytes()))
        .expect("member present");
    let end = starts
        .iter()
        .copied()
        .find(|&at| at > start)
        .unwrap_or_else(|| {
            (0..bytes.len() - 4)
                .find(|&at| bytes[at..at + 4] == [0x50, 0x4B, 0x01, 0x02])
                .expect("central directory")
        });
    start..end
}
