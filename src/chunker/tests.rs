//! `generic-cdc-v1` regression tests: frozen boundaries, structural
//! bounds, feed-order independence, and the reuse property the
//! parameters were chosen for.

use super::*;
use crate::constants::{GENERIC_CDC_CHUNK_MAX_BYTES, GENERIC_CDC_CHUNK_MIN_BYTES};
use crate::profile::ChunkingProfile;

/// Generic boundaries of an in-memory payload.
fn generic(bytes: &[u8]) -> ChunkBoundaries {
    ChunkBoundaries::of(ChunkingProfile::GenericCdcV1, bytes).expect("generic chunking never fails")
}

/// Deterministic pseudo-random fixture bytes.
fn payload(seed: &str, len: usize) -> Vec<u8> {
    // bounded: fixture payloads are test constants (≤ 4 MiB).
    let mut bytes = vec![0u8; len];
    let mut hasher = blake3::Hasher::new();
    hasher.update(seed.as_bytes());
    hasher.finalize_xof().fill(&mut bytes);
    bytes
}

/// BLAKE3 hex over the boundary offsets, for compact goldens.
fn boundary_digest(boundaries: &ChunkBoundaries) -> String {
    let mut hasher = blake3::Hasher::new();
    for range in boundaries.ranges() {
        hasher.update(&(range.end as u64).to_le_bytes());
    }
    hasher.finalize().to_hex()[..16].to_owned()
}

/// Frozen boundaries for a 4 MiB payload. A change here is a change
/// to `generic-cdc-v1` itself — a format cutover, never a tweak.
#[test]
fn golden_boundaries() {
    let boundaries = generic(&payload("els05/golden", 4 << 20));
    assert_eq!(boundaries.len(), 59, "frozen chunk count");
    assert_eq!(boundary_digest(&boundaries), "805772eed894132f");
}

/// Chunks cover the payload exactly, in order, within the frozen
/// bounds. Only the final chunk may fall below the minimum.
#[test]
fn bounds_and_coverage() {
    for (name, bytes) in [
        ("entropy", payload("els05/bounds", 4 << 20)),
        ("short", payload("els05/short", 100)),
        (
            "exact-min",
            payload("els05/min", GENERIC_CDC_CHUNK_MIN_BYTES),
        ),
        ("zeros", vec![0u8; 1 << 20]),
    ] {
        let boundaries = generic(&bytes);
        let ranges: Vec<_> = boundaries.ranges().collect();
        let mut expected_start = 0usize;
        for (position, range) in ranges.iter().enumerate() {
            assert_eq!(range.start, expected_start, "{name}: gap or overlap");
            assert!(
                range.len() <= GENERIC_CDC_CHUNK_MAX_BYTES,
                "{name}: chunk above the frozen maximum"
            );
            if position + 1 < ranges.len() {
                assert!(
                    range.len() >= GENERIC_CDC_CHUNK_MIN_BYTES,
                    "{name}: non-final chunk below the frozen minimum"
                );
            }
            expected_start = range.end;
        }
        assert_eq!(expected_start, bytes.len(), "{name}: incomplete coverage");
    }
}

/// An empty payload produces no chunks.
#[test]
fn empty_payload_produces_no_chunks() {
    let boundaries = generic(&[]);
    assert!(boundaries.is_empty());
    assert_eq!(boundaries.len(), 0);
}

/// Incompressible input still closes chunks at the maximum.
#[test]
fn maximum_forces_a_cut() {
    // A constant byte stream never matches the mask, so only the
    // hard maximum can close a chunk.
    let boundaries = generic(&vec![0xab; GENERIC_CDC_CHUNK_MAX_BYTES * 3]);
    let ranges: Vec<_> = boundaries.ranges().collect();
    assert_eq!(ranges.len(), 3);
    for range in &ranges {
        assert_eq!(range.len(), GENERIC_CDC_CHUNK_MAX_BYTES);
    }
}

/// Boundaries depend on content alone — not on how the bytes were
/// fed in.
#[test]
fn feed_order_does_not_change_boundaries() {
    let bytes = payload("els05/windows", 1 << 20);
    let whole = generic(&bytes);

    for window in [1usize, 7, 4096, 1 << 17] {
        let mut ends = Vec::new();
        let mut end = 0usize;
        let mut chunker = GenericCdcChunker::new();
        let mut record = |chunk: &[u8]| {
            end += chunk.len();
            ends.push(end);
        };
        for slice in bytes.chunks(window) {
            chunker
                .push(slice, &mut record)
                .expect("generic never fails");
        }
        chunker.finish(&mut record).expect("generic never fails");
        let windowed: Vec<usize> = whole.ranges().map(|range| range.end).collect();
        assert_eq!(ends, windowed, "window size {window} changed boundaries");
    }
}

/// The reuse property the frozen parameters exist for: shifting the
/// whole payload leaves almost every chunk intact, because the
/// rolling hash re-synchronizes after the inserted prefix.
#[test]
fn boundaries_resynchronize_after_a_shift() {
    let shared = payload("els05/shared", 4 << 20);
    let base: Vec<blake3::Hash> = generic(&shared)
        .ranges()
        .map(|range| blake3::hash(&shared[range]))
        .collect();

    for prefix_len in [1usize, 64, 4096, 65_536] {
        let mut shifted = payload("els05/prefix", prefix_len);
        shifted.extend_from_slice(&shared);
        let shifted_chunks: Vec<blake3::Hash> = generic(&shifted)
            .ranges()
            .map(|range| blake3::hash(&shifted[range]))
            .collect();
        let reused = shifted_chunks
            .iter()
            .filter(|hash| base.contains(hash))
            .count();
        let ratio = reused as f64 / shifted_chunks.len() as f64;
        assert!(
            ratio > 0.90,
            "a {prefix_len}-byte shift reused only {ratio:.3} of chunks"
        );
    }
}
