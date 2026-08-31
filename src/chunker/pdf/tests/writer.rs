//! A deliberately small PDF writer for fixtures: enough of the COS
//! grammar to produce objects, direct- and indirect-length streams,
//! classic xref tables, trailers, and incremental updates.

/// Deterministic pseudo-random bytes.
pub(super) fn noise(seed: &str, len: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; len];
    let mut hasher = blake3::Hasher::new();
    hasher.update(seed.as_bytes());
    hasher.finalize_xof().fill(&mut bytes);
    bytes
}

/// One indirect object.
pub(super) struct Object {
    pub(super) number: u32,
    pub(super) body: Vec<u8>,
}

impl Object {
    /// A plain dictionary/value object.
    pub(super) fn plain(number: u32, body: &str) -> Self {
        Self {
            number,
            body: body.as_bytes().to_vec(),
        }
    }

    /// A stream object with a **direct** `/Length`.
    pub(super) fn stream(number: u32, payload: &[u8]) -> Self {
        let mut body = format!("<< /Length {} >>\nstream\n", payload.len()).into_bytes();
        body.extend_from_slice(payload);
        body.extend_from_slice(b"\nendstream");
        Self { number, body }
    }

    /// A stream object whose `/Length` is an **indirect** reference
    /// (the walker must scan for `endstream`).
    pub(super) fn stream_indirect_length(number: u32, length_obj: u32, payload: &[u8]) -> Self {
        let mut body = format!("<< /Length {length_obj} 0 R >>\nstream\n").into_bytes();
        body.extend_from_slice(payload);
        body.extend_from_slice(b"\nendstream");
        Self { number, body }
    }
}

/// One body section: objects, xref, trailer, startxref, `%%EOF`.
fn section(out: &mut Vec<u8>, objects: &[Object], root: u32, size: u32) {
    let mut offsets = Vec::new();
    for object in objects {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", object.number).as_bytes());
        out.extend_from_slice(&object.body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = out.len();
    out.extend_from_slice(b"xref\n");
    out.extend_from_slice(b"0 1\n");
    out.extend_from_slice(b"0000000000 65535 f \n");
    for (object, offset) in objects.iter().zip(&offsets) {
        out.extend_from_slice(format!("{} 1\n", object.number).as_bytes());
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root {root} 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
}

/// A complete single-section document.
pub(super) fn document(objects: &[Object]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    section(&mut out, objects, 1, objects.len() as u32 + 1);
    out
}

/// Append an incremental update to an existing document.
pub(super) fn incremental_update(base: &[u8], objects: &[Object]) -> Vec<u8> {
    let mut out = base.to_vec();
    section(&mut out, objects, 1, 100);
    out
}

/// A typical fixture: a catalog, pages, a content stream, and one
/// large binary image stream.
pub(super) fn typical(content_seed: &str, image_seed: &str) -> Vec<u8> {
    document(&[
        Object::plain(1, "<< /Type /Catalog /Pages 2 0 R >>"),
        Object::plain(2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        Object::plain(3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>"),
        Object::stream(4, &text_stream(content_seed, 20 << 10)),
        Object::stream(5, &noise(image_seed, 200 << 10)),
    ])
}

/// Text-shaped stream payload.
pub(super) fn text_stream(seed: &str, len: usize) -> Vec<u8> {
    let raw = noise(seed, len);
    let mut out = Vec::with_capacity(len + len / 16);
    for (index, byte) in raw.iter().enumerate() {
        out.push(b'A' + byte % 26);
        if index % 40 == 39 {
            out.extend_from_slice(b" Tj\n");
        }
    }
    out
}
