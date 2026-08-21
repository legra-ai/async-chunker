//! Canonical boundaries, member reuse, framings, and bounds.

use super::corpus::{Media, document_xml, docx, docx_with_framings, member_span, noise};
use super::writer::{Member, Options, archive};
use crate::chunker::{ChunkBoundaries, Chunker, ZipChunker};
use crate::constants::{GENERIC_CDC_CHUNK_MAX_BYTES, GENERIC_CDC_CHUNK_MIN_BYTES};
use crate::profile::ChunkingProfile;

fn zip(bytes: &[u8]) -> ChunkBoundaries {
    ChunkBoundaries::of(ChunkingProfile::ZipV1, bytes).expect("well-formed archive")
}

fn boundary_digest(boundaries: &ChunkBoundaries) -> String {
    let mut hasher = blake3::Hasher::new();
    for end in boundaries.ends() {
        hasher.update(&(end as u64).to_le_bytes());
    }
    hasher.finalize().to_hex()[..16].to_owned()
}

fn chunk_hashes(bytes: &[u8]) -> Vec<(std::ops::Range<usize>, blake3::Hash)> {
    zip(bytes)
        .ranges()
        .map(|range| (range.clone(), blake3::hash(&bytes[range])))
        .collect()
}

/// Frozen boundaries for the Word-shaped corpus. A change here is a
/// change to `zip-v1` itself — a format cutover.
#[test]
fn golden_boundaries() {
    let media = Media::new();
    let bytes = docx(&media, &document_xml("els07/golden", 3000, "golden"));
    let boundaries = zip(&bytes);
    assert_eq!(boundaries.len(), 9, "frozen chunk count");
    assert_eq!(
        boundary_digest(&boundaries),
        "8d25affaa059f0ea",
        "frozen boundaries"
    );
}

/// Chunks cover the archive exactly within the envelope, and every
/// member of at least the minimum size begins a chunk.
#[test]
fn bounds_coverage_and_member_boundaries() {
    let media = Media::new();
    let bytes = docx(&media, &document_xml("els07/bounds", 3000, "bounds"));
    let ranges: Vec<_> = zip(&bytes).ranges().collect();
    let header = [0x50, 0x4B, 0x03, 0x04];
    let mut expected_start = 0usize;
    for (position, range) in ranges.iter().enumerate() {
        assert_eq!(range.start, expected_start, "gap or overlap");
        assert!(range.len() <= GENERIC_CDC_CHUNK_MAX_BYTES, "above maximum");
        // A chunk below the minimum is only ever the final one or
        // the small lead-in closed because a large member begins.
        if position + 1 < ranges.len() && range.len() < GENERIC_CDC_CHUNK_MIN_BYTES {
            assert_eq!(
                &bytes[range.end..range.end + 4],
                &header,
                "sub-minimum chunk {position} does not end at a member start"
            );
        }
        expected_start = range.end;
    }
    assert_eq!(expected_start, bytes.len(), "incomplete coverage");

    let starts: Vec<usize> = ranges.iter().map(|range| range.start).collect();
    for name in [
        "word/document.xml",
        "word/media/image1.png",
        "word/media/image2.png",
    ] {
        let span = member_span(&bytes, name);
        assert!(starts.contains(&span.start), "{name} must begin a chunk");
    }
}

/// Every framing the walker accepts — descriptors signed and
/// unsigned, ZIP64 sizes, ZIP64 end records, comments — parses, and
/// feeding window size changes nothing.
#[test]
fn all_framings_parse_and_feed_order_is_irrelevant() {
    let media = Media::new();
    let document = document_xml("els07/framings", 2000, "framings");
    for zip64_end in [false, true] {
        let bytes = docx_with_framings(&media, &document, zip64_end);
        let whole: Vec<usize> = zip(&bytes).ends().collect();
        assert!(whole.len() > 4, "zip64_end={zip64_end}: corpus too small");
        for window in [1usize, 5, 4096, 1 << 17] {
            let mut ends = Vec::new();
            let mut end = 0usize;
            let mut chunker = ZipChunker::new();
            let mut record = |chunk: &[u8]| {
                end += chunk.len();
                ends.push(end);
            };
            for slice in bytes.chunks(window) {
                chunker.push(slice, &mut record).expect("well-formed");
            }
            chunker.finish(&mut record).expect("well-formed");
            assert_eq!(ends, whole, "window {window} changed boundaries");
        }
    }
}

/// An empty archive is a valid, single-chunk stream.
#[test]
fn empty_archive_parses() {
    let bytes = archive(&[], Options::default());
    assert_eq!(zip(&bytes).len(), 1);
}

/// Word-document variation: editing the document part leaves every
/// media chunk intact and most chunks overall reused.
#[test]
fn editing_the_document_part_reuses_media_chunks() {
    let media = Media::new();
    let base = docx(
        &media,
        &document_xml("els07/variation", 3000, "before the edit"),
    );
    let edited = docx(
        &media,
        &document_xml("els07/variation", 3000, "after the edit, longer"),
    );
    let base_chunks = chunk_hashes(&base);
    let edited_chunks = chunk_hashes(&edited);
    let base_hashes: Vec<_> = base_chunks.iter().map(|(_, hash)| *hash).collect();

    let reused = edited_chunks
        .iter()
        .filter(|(_, hash)| base_hashes.contains(hash))
        .count();
    // Exactly two chunks legitimately change: the one holding the
    // document part (and the small parts attached to it) and the
    // central directory, whose entry for that part changed.
    assert_eq!(
        edited_chunks.len() - reused,
        2,
        "edit reused only {reused} of {} chunks",
        edited_chunks.len()
    );

    // image2 follows an unchanged member, so every one of its chunks
    // comes back byte-identical.
    let span = member_span(&edited, "word/media/image2.png");
    let image2: Vec<_> = edited_chunks
        .iter()
        .filter(|(range, _)| range.start >= span.start && range.end <= span.end)
        .collect();
    assert!(image2.len() >= 2, "image2 must span several chunks");
    assert!(
        image2.iter().all(|(_, hash)| base_hashes.contains(hash)),
        "every image2 chunk must be reused"
    );
}

/// Shared-member reuse: the same member inside two different archives
/// yields the same chunks, except possibly the tail that coalesces
/// with whatever follows.
#[test]
fn a_shared_member_yields_the_same_chunks_in_different_archives() {
    let shared = noise("els07/shared-lib", 700 << 10);
    let lead_a = noise("els07/lead-a", 40 << 10);
    let lead_b = noise("els07/lead-b", 90 << 10);
    let a = archive(
        &[
            Member::stored("lead.bin", &lead_a),
            Member::deflated("lib/shared.bin", &shared),
            Member::stored("tail-a.txt", b"tail a"),
        ],
        Options::default(),
    );
    let b = archive(
        &[
            Member::stored("other/lead.bin", &lead_b),
            Member::deflated("lib/shared.bin", &shared),
            Member::stored("different/tail-b.txt", b"tail b, longer"),
        ],
        Options::default(),
    );
    let span_a = member_span(&a, "lib/shared.bin");
    let span_b = member_span(&b, "lib/shared.bin");
    let in_span = |chunks: Vec<(std::ops::Range<usize>, blake3::Hash)>,
                   span: &std::ops::Range<usize>| {
        chunks
            .into_iter()
            .filter(|(range, _)| range.start >= span.start && range.start < span.end)
            .map(|(_, hash)| hash)
            .collect::<Vec<_>>()
    };
    let chunks_a = in_span(chunk_hashes(&a), &span_a);
    let chunks_b = in_span(chunk_hashes(&b), &span_b);
    assert!(
        chunks_a.len() >= 3,
        "shared member must span several chunks"
    );
    let shared_count = chunks_b
        .iter()
        .filter(|hash| chunks_a.contains(hash))
        .count();
    assert!(
        shared_count + 1 >= chunks_a.len().min(chunks_b.len()),
        "only {shared_count} of {} shared-member chunks were reused",
        chunks_a.len()
    );
}
