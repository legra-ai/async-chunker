//! Canonical boundaries, box-boundary cuts, header forms, reuse.

use super::writer::{
    bx, bx_large, bx_open, bx_uuid, container, fragmented_mp4, ftyp, heif, moov, mp4, noise,
};
use crate::chunker::{ChunkBoundaries, Chunker, IsobmffChunker};
use crate::constants::{GENERIC_CDC_CHUNK_MAX_BYTES, GENERIC_CDC_CHUNK_MIN_BYTES};
use crate::profile::ChunkingProfile;

fn isobmff(bytes: &[u8]) -> ChunkBoundaries {
    ChunkBoundaries::of(ChunkingProfile::IsobmffV1, bytes).expect("well-formed stream")
}

fn boundary_digest(boundaries: &ChunkBoundaries) -> String {
    let mut hasher = blake3::Hasher::new();
    for end in boundaries.ends() {
        hasher.update(&(end as u64).to_le_bytes());
    }
    hasher.finalize().to_hex()[..16].to_owned()
}

fn chunk_hashes(bytes: &[u8]) -> Vec<(std::ops::Range<usize>, blake3::Hash)> {
    isobmff(bytes)
        .ranges()
        .map(|range| (range.clone(), blake3::hash(&bytes[range])))
        .collect()
}

/// Offset of the first top-level box of `kind`.
fn top_level_box(bytes: &[u8], kind: &[u8; 4]) -> std::ops::Range<usize> {
    let mut at = 0usize;
    while at + 8 <= bytes.len() {
        let size =
            u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize;
        let end = if size == 0 { bytes.len() } else { at + size };
        if &bytes[at + 4..at + 8] == kind {
            return at..end;
        }
        at = end;
    }
    panic!("box not found");
}

/// Frozen boundaries for the MP4, fragmented MP4, and HEIF corpora.
/// A change here is a change to `isobmff-v1` itself — a format
/// cutover.
#[test]
fn golden_boundaries() {
    let fragments: Vec<Vec<u8>> = (0..6)
        .map(|index| noise(&format!("els08/golden/frag{index}"), 300 << 10))
        .collect();
    let fragment_refs: Vec<&[u8]> = fragments.iter().map(Vec::as_slice).collect();
    for (name, bytes, count, digest) in [
        (
            "mp4",
            mp4("els08/golden", &noise("els08/golden/mdat", 4 << 20)),
            57usize,
            "091f2af912cc26de",
        ),
        (
            "fmp4",
            fragmented_mp4("els08/golden", &fragment_refs),
            28,
            "e5f9471e459dd763",
        ),
        (
            "heif",
            heif("els08/golden", &noise("els08/golden/pic", 1 << 20)),
            16,
            "aeec3468ab8ecb7f",
        ),
    ] {
        let boundaries = isobmff(&bytes);
        assert_eq!(boundaries.len(), count, "{name}: frozen chunk count");
        assert_eq!(
            boundary_digest(&boundaries),
            digest,
            "{name}: frozen boundaries"
        );
    }
}

/// Chunks cover the stream exactly within the envelope, and every
/// large top-level box begins a chunk.
#[test]
fn bounds_coverage_and_box_boundaries() {
    let bytes = mp4("els08/bounds", &noise("els08/bounds/mdat", 2 << 20));
    let ranges: Vec<_> = isobmff(&bytes).ranges().collect();
    let box_starts: Vec<usize> = [b"ftyp", b"free", b"moov", b"mdat"]
        .iter()
        .map(|kind| top_level_box(&bytes, kind).start)
        .collect();
    let mut expected_start = 0usize;
    for (position, range) in ranges.iter().enumerate() {
        assert_eq!(range.start, expected_start, "gap or overlap");
        assert!(range.len() <= GENERIC_CDC_CHUNK_MAX_BYTES, "above maximum");
        // A chunk below the minimum is only ever the final one or the
        // small lead-in closed because a large box begins.
        if position + 1 < ranges.len() && range.len() < GENERIC_CDC_CHUNK_MIN_BYTES {
            assert!(
                box_starts.contains(&range.end),
                "sub-minimum chunk {position} does not end at a box start"
            );
        }
        expected_start = range.end;
    }
    assert_eq!(expected_start, bytes.len(), "incomplete coverage");
    let mdat = top_level_box(&bytes, b"mdat");
    assert!(
        ranges.iter().any(|range| range.start == mdat.start),
        "mdat must begin a chunk"
    );
}

/// Every header form — compact, extended-size, open-ended at top
/// level, `uuid`, unknown types, empty boxes, nested containers —
/// parses, and feeding window size changes nothing.
#[test]
fn all_header_forms_parse_and_feed_order_is_irrelevant() {
    let bytes = [
        ftyp(b"isom"),
        bx(b"free", &[]),
        bx_uuid(b"0123456789abcdef", &noise("els08/uuid", 300)),
        bx(b"zzzz", &noise("els08/unknown", 20 << 10)),
        moov("els08/forms", 40),
        container(
            b"moov",
            &[container(b"trak", &[]), bx(b"mvhd", &[0u8; 100])],
        ),
        bx_large(b"mdat", &noise("els08/forms/mdat1", 200 << 10)),
        bx_open(b"mdat", &noise("els08/forms/mdat2", 150 << 10)),
    ]
    .concat();
    let whole: Vec<usize> = isobmff(&bytes).ends().collect();
    assert!(whole.len() > 3);
    for window in [1usize, 7, 4096, 1 << 17] {
        let mut ends = Vec::new();
        let mut end = 0usize;
        let mut chunker = IsobmffChunker::new();
        let mut record = |chunk: &[u8]| {
            end += chunk.len();
            ends.push(end);
        };
        for slice in bytes.chunks(window) {
            chunker.push(slice, &mut record).expect("well-formed");
        }
        chunker.finish(&mut record).expect("well-formed");
        assert_eq!(ends, whole, "window size {window} changed boundaries");
    }
}

/// Re-muxing — a rewritten `moov` over identical media — leaves every
/// `mdat` chunk intact.
#[test]
fn remuxing_reuses_every_mdat_chunk() {
    let media = noise("els08/remux/mdat", 4 << 20);
    let base = mp4("els08/remux/a", &media);
    let remuxed = mp4("els08/remux/b", &media);
    let base_hashes: Vec<_> = chunk_hashes(&base)
        .into_iter()
        .map(|(_, hash)| hash)
        .collect();
    let mdat = top_level_box(&remuxed, b"mdat");
    let remuxed_chunks = chunk_hashes(&remuxed);
    let mdat_chunks: Vec<_> = remuxed_chunks
        .iter()
        .filter(|(range, _)| range.start >= mdat.start)
        .collect();
    assert!(mdat_chunks.len() >= 8, "mdat must span several chunks");
    assert!(
        mdat_chunks
            .iter()
            .all(|(_, hash)| base_hashes.contains(hash)),
        "every mdat chunk must be reused"
    );
    let changed = remuxed_chunks
        .iter()
        .filter(|(_, hash)| !base_hashes.contains(hash))
        .count();
    assert_eq!(
        changed, 1,
        "only the chunk holding the rewritten moov changes"
    );
}

/// Fragment reuse: a `moof`/`mdat` pair shared by two fragmented
/// files yields the same chunks in both.
#[test]
fn shared_fragments_yield_the_same_chunks() {
    let shared: Vec<Vec<u8>> = (0..3)
        .map(|index| noise(&format!("els08/shared/{index}"), 200 << 10))
        .collect();
    let own_a = noise("els08/own/a", 250 << 10);
    let own_b = noise("els08/own/b", 120 << 10);
    let a = fragmented_mp4(
        "els08/frag/a",
        &[&own_a, &shared[0], &shared[1], &shared[2]],
    );
    let b = fragmented_mp4(
        "els08/frag/b",
        &[&own_b, &shared[0], &shared[1], &shared[2]],
    );
    let a_hashes: Vec<_> = chunk_hashes(&a).into_iter().map(|(_, hash)| hash).collect();
    let b_chunks = chunk_hashes(&b);
    let shared_start = top_level_box(&b, b"moof").end;
    let after_first = b_chunks
        .iter()
        .filter(|(range, _)| range.start > shared_start + own_b.len())
        .collect::<Vec<_>>();
    assert!(after_first.len() >= 6);
    assert!(
        after_first.iter().all(|(_, hash)| a_hashes.contains(hash)),
        "every shared-fragment chunk must be reused"
    );
}
