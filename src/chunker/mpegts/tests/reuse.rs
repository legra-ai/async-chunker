//! Canonical boundaries, packet alignment, and reuse.

use super::writer::{discontinuity_packet, noise, stream};
use crate::chunker::mpegts::packet::PACKET_LEN;
use crate::chunker::{ChunkBoundaries, Chunker, MpegtsChunker};
use crate::constants::{GENERIC_CDC_CHUNK_MAX_BYTES, GENERIC_CDC_CHUNK_MIN_BYTES};
use crate::profile::ChunkingProfile;

fn mpegts(bytes: &[u8]) -> ChunkBoundaries {
    ChunkBoundaries::of(ChunkingProfile::MpegtsV1, bytes).expect("well-formed stream")
}

fn boundary_digest(boundaries: &ChunkBoundaries) -> String {
    let mut hasher = blake3::Hasher::new();
    for end in boundaries.ends() {
        hasher.update(&(end as u64).to_le_bytes());
    }
    hasher.finalize().to_hex()[..16].to_owned()
}

fn chunk_hashes(bytes: &[u8]) -> Vec<blake3::Hash> {
    mpegts(bytes)
        .ranges()
        .map(|range| blake3::hash(&bytes[range]))
        .collect()
}

/// Frozen boundaries for a 20k-packet stream with a payload-unit
/// start every 40 packets. A change here is a change to `mpegts-v1`
/// itself — a format cutover.
#[test]
fn golden_boundaries() {
    let bytes = stream("els10/golden", 20_000, 40);
    let boundaries = mpegts(&bytes);
    assert_eq!(boundaries.len(), 51, "frozen chunk count");
    assert_eq!(
        boundary_digest(&boundaries),
        "aa45efe720954891",
        "frozen boundaries"
    );
}

/// Chunks cover the stream exactly, every chunk is a whole number of
/// packets within the envelope, and every non-forced cut lands at a
/// seam packet.
#[test]
fn packet_aligned_bounds_and_seam_cuts() {
    let unit_every = 40usize;
    let bytes = stream("els10/bounds", 15_000, unit_every);
    let ranges: Vec<_> = mpegts(&bytes).ranges().collect();
    let max_aligned = (GENERIC_CDC_CHUNK_MAX_BYTES / PACKET_LEN) * PACKET_LEN;
    let mut expected_start = 0usize;
    for (position, range) in ranges.iter().enumerate() {
        assert_eq!(range.start, expected_start, "gap or overlap");
        assert_eq!(range.len() % PACKET_LEN, 0, "chunk not packet-aligned");
        assert!(
            range.len() <= max_aligned,
            "above the packet-aligned maximum"
        );
        if position + 1 < ranges.len() {
            assert!(
                range.len() >= GENERIC_CDC_CHUNK_MIN_BYTES,
                "non-final chunk below minimum"
            );
            let next_packet = range.end / PACKET_LEN;
            assert!(
                next_packet.is_multiple_of(unit_every) || range.len() == max_aligned,
                "cut {position} lands at packet {next_packet}: neither a seam nor forced"
            );
        }
        expected_start = range.end;
    }
    assert_eq!(expected_start, bytes.len(), "incomplete coverage");
}

/// A candidate-free stream (no payload-unit starts after the first
/// packet) still closes chunks at the packet-aligned maximum.
#[test]
fn candidate_free_streams_force_aligned_cuts() {
    let bytes = stream("els10/forced", 8_000, usize::MAX);
    let ranges: Vec<_> = mpegts(&bytes).ranges().collect();
    let max_aligned = (GENERIC_CDC_CHUNK_MAX_BYTES / PACKET_LEN) * PACKET_LEN;
    assert!(ranges.len() >= 5);
    for range in &ranges[..ranges.len() - 1] {
        assert_eq!(
            range.len(),
            max_aligned,
            "forced cut not at the aligned maximum"
        );
    }
}

/// Feeding window size changes nothing.
#[test]
fn feed_order_does_not_change_boundaries() {
    let bytes = stream("els10/windows", 6_000, 25);
    let whole: Vec<usize> = mpegts(&bytes).ends().collect();
    assert!(whole.len() > 3);
    for window in [1usize, 7, 187, 188, 4096, 1 << 17] {
        let mut ends = Vec::new();
        let mut end = 0usize;
        let mut chunker = MpegtsChunker::new();
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

/// Re-segmentation reuse: the same packets behind a different
/// lead-in re-converge at the next seam.
#[test]
fn boundaries_resynchronize_after_a_new_lead_in() {
    let shared = stream("els10/shared", 20_000, 40);
    let base = chunk_hashes(&shared);
    for lead_packets in [1usize, 39, 173] {
        let mut shifted = stream("els10/lead", lead_packets, 13);
        shifted.extend_from_slice(&shared);
        let shifted_chunks = chunk_hashes(&shifted);
        let reused = shifted_chunks
            .iter()
            .filter(|hash| base.contains(hash))
            .count();
        let ratio = reused as f64 / shifted_chunks.len() as f64;
        assert!(
            ratio > 0.85,
            "a {lead_packets}-packet lead-in reused only {ratio:.3} of chunks"
        );
    }
}

/// A discontinuity packet is a candidate: splicing two streams at a
/// discontinuity reuses both halves' interior chunks.
#[test]
fn splices_at_discontinuities_reuse_both_halves() {
    let first = stream("els10/splice/a", 12_000, 40);
    let second = stream("els10/splice/b", 12_000, 40);
    let mut spliced = first.clone();
    spliced.extend_from_slice(&discontinuity_packet(3));
    spliced.extend_from_slice(&second);
    let first_hashes = chunk_hashes(&first);
    let second_hashes = chunk_hashes(&second);
    let spliced_hashes = chunk_hashes(&spliced);
    let reused = spliced_hashes
        .iter()
        .filter(|hash| first_hashes.contains(hash) || second_hashes.contains(hash))
        .count();
    assert!(
        spliced_hashes.len() - reused <= 3,
        "{} of {} spliced chunks were new",
        spliced_hashes.len() - reused,
        spliced_hashes.len()
    );
    let _ = noise("els10/unused", 1);
}
