//! `pdf-v1` behavior tests.

use std::collections::HashSet;

use super::super::PdfChunker;
use super::writer::{Object, document, incremental_update, noise, typical};
use crate::ChunkError;
use crate::chunker::profile_chunker::Chunker;

fn chunks_of(bytes: &[u8], window: usize) -> Result<Vec<Vec<u8>>, ChunkError> {
    let mut chunker = PdfChunker::new();
    let mut chunks = Vec::new();
    let mut record = |chunk: &[u8]| chunks.push(chunk.to_vec());
    for slice in bytes.chunks(window) {
        chunker.push(slice, &mut record)?;
    }
    chunker.finish(&mut record)?;
    Ok(chunks)
}

#[test]
fn chunks_concatenate_to_the_input_and_are_window_independent() {
    let input = typical("pdf/base", "pdf/base-image");
    let whole = chunks_of(&input, 4096).expect("valid document");
    assert_eq!(whole.concat(), input);
    for window in [1usize, 13, 1 << 20] {
        assert_eq!(chunks_of(&input, window).expect("valid"), whole, "{window}");
    }
    assert!(whole.len() >= 3, "the image stream must split off");
}

#[test]
fn an_incremental_update_reuses_every_chunk_but_the_last() {
    let base = typical("pdf/incremental", "pdf/incremental-image");
    let updated = incremental_update(
        &base,
        &[Object::plain(
            6,
            "<< /Type /Annot /Contents (added later) >>",
        )],
    );
    let base_chunks = chunks_of(&base, 4096).expect("valid");
    let updated_chunks = chunks_of(&updated, 4096).expect("valid");
    let shared: HashSet<&[u8]> = base_chunks.iter().map(Vec::as_slice).collect();
    let reused = updated_chunks
        .iter()
        .filter(|chunk| shared.contains(chunk.as_slice()))
        .count();
    assert!(
        reused >= base_chunks.len() - 1,
        "reused only {reused} of {} original chunks",
        base_chunks.len()
    );
}

#[test]
fn an_edited_object_leaves_the_image_streams_chunks_identical() {
    let original = chunks_of(&typical("pdf/edit-a", "pdf/shared-image"), 4096).expect("valid");
    let edited = chunks_of(&typical("pdf/edit-b", "pdf/shared-image"), 4096).expect("valid");
    let original_set: HashSet<&[u8]> = original.iter().map(Vec::as_slice).collect();
    let shared: usize = edited
        .iter()
        .filter(|chunk| original_set.contains(chunk.as_slice()))
        .map(Vec::len)
        .sum();
    assert!(shared >= 190 << 10, "only {shared} shared bytes");
}

#[test]
fn indirect_length_streams_scan_to_endstream() {
    let payload = noise("pdf/indirect", 30 << 10);
    let input = document(&[
        Object::plain(1, "<< /Type /Catalog >>"),
        Object::stream_indirect_length(2, 3, &payload),
        Object::plain(3, "30720"),
    ]);
    let chunks = chunks_of(&input, 4096).expect("indirect length is legal");
    assert_eq!(chunks.concat(), input);
}

#[test]
fn a_lying_direct_length_falls_back_to_scanning_deterministically() {
    let payload = noise("pdf/lying", 12 << 10);
    let mut body = format!("<< /Length {} >>\nstream\n", payload.len() - 500).into_bytes();
    body.extend_from_slice(&payload);
    body.extend_from_slice(b"\nendstream");
    let input = document(&[
        Object::plain(1, "<< /Type /Catalog >>"),
        Object { number: 2, body },
    ]);
    let whole = chunks_of(&input, 4096).expect("scan fallback recovers");
    assert_eq!(whole.concat(), input);
    assert_eq!(chunks_of(&input, 7).expect("valid"), whole);
}

#[test]
fn binary_payloads_with_pdf_delimiters_do_not_confuse_the_walker() {
    let mut payload = noise("pdf/tricky", 8 << 10);
    payload.extend_from_slice(b")>>] endobj (");
    payload.extend_from_slice(&noise("pdf/tricky2", 8 << 10));
    let input = document(&[
        Object::plain(1, "<< /Type /Catalog >>"),
        Object::stream(2, &payload),
    ]);
    let chunks = chunks_of(&input, 4096).expect("length skips the payload");
    assert_eq!(chunks.concat(), input);
}

#[test]
fn malformed_documents_reject() {
    chunks_of(b"not a pdf at all", 4096).expect_err("no magic");
    let mut truncated = typical("pdf/truncated", "pdf/truncated-image");
    truncated.truncate(truncated.len() - 40);
    chunks_of(&truncated, 4096).expect_err("no %%EOF");
    let input = document(&[Object::plain(1, "<< /K (unterminated")]);
    chunks_of(&input, 4096).expect_err("unterminated string swallows endobj");
}

#[test]
fn the_stream_stays_poisoned_after_rejection() {
    let mut chunker = PdfChunker::new();
    let mut sink = |_chunk: &[u8]| {};
    chunker.push(b"garbage", &mut sink).expect_err("not a pdf");
    let error = chunker.push(b"more", &mut sink).expect_err("poisoned");
    assert!(matches!(error, ChunkError::ProfileStreamRejected { .. }));
}
