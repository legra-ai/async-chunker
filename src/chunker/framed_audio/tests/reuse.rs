//! Canonical boundaries, frame alignment, and reuse.

use super::writer::{adts_frames, flac, id3v1, id3v2, mp3_frames};
use crate::chunker::{ChunkBoundaries, Chunker, FramedAudioChunker};
use crate::constants::{GENERIC_CDC_CHUNK_MAX_BYTES, GENERIC_CDC_CHUNK_MIN_BYTES};
use crate::profile::ChunkingProfile;

fn framed(bytes: &[u8]) -> ChunkBoundaries {
    ChunkBoundaries::of(ChunkingProfile::FramedAudioV1, bytes).expect("well-formed stream")
}

fn boundary_digest(boundaries: &ChunkBoundaries) -> String {
    let mut hasher = blake3::Hasher::new();
    for end in boundaries.ends() {
        hasher.update(&(end as u64).to_le_bytes());
    }
    hasher.finalize().to_hex()[..16].to_owned()
}

fn chunk_hashes(bytes: &[u8]) -> Vec<blake3::Hash> {
    framed(bytes)
        .ranges()
        .map(|range| blake3::hash(&bytes[range]))
        .collect()
}

/// A tagged MP3 file: leading `ID3v2`, frames, `ID3v1` trailer.
fn tagged_mp3(tag_seed: &str, frame_seed: &str, frames: usize) -> Vec<u8> {
    let mut out = id3v2(tag_seed, 2 << 10);
    out.extend_from_slice(&mp3_frames(frame_seed, frames));
    out.extend_from_slice(&id3v1(tag_seed));
    out
}

/// Frozen boundaries per format. A change here is a change to
/// `framed-audio-v1` itself — a format cutover.
#[test]
fn golden_boundaries() {
    for (name, bytes, count, digest) in [
        (
            "mp3",
            tagged_mp3("els11/golden/tag", "els11/golden", 4000),
            28usize,
            "23f3a3ab1c28450f",
        ),
        (
            "adts",
            adts_frames("els11/golden", 3000),
            27,
            "2f48b1924f436110",
        ),
        (
            "flac",
            flac("els11/golden", 60 << 10, 2 << 20),
            30,
            "fa9ea9387cb542f5",
        ),
    ] {
        let boundaries = framed(&bytes);
        assert_eq!(boundaries.len(), count, "{name}: frozen chunk count");
        assert_eq!(
            boundary_digest(&boundaries),
            digest,
            "{name}: frozen boundaries"
        );
    }
}

/// Chunks cover the stream exactly within the envelope, and in
/// framed regions every cut lands on a frame boundary.
#[test]
fn bounds_coverage_and_frame_alignment() {
    let frames = 3000usize;
    let bytes = mp3_frames("els11/bounds", frames);
    // Frame start offsets, from the writer's own layout.
    let mut frame_starts = std::collections::HashSet::new();
    let mut at = 0usize;
    for index in 0..frames {
        frame_starts.insert(at);
        at += if index % 3 == 2 { 418 } else { 417 };
    }
    let ranges: Vec<_> = framed(&bytes).ranges().collect();
    assert!(ranges.len() > 4, "corpus too small");
    let mut expected_start = 0usize;
    for (position, range) in ranges.iter().enumerate() {
        assert_eq!(range.start, expected_start, "gap or overlap");
        assert!(range.len() <= GENERIC_CDC_CHUNK_MAX_BYTES, "above maximum");
        assert!(
            frame_starts.contains(&range.start),
            "cut {position} not on a frame boundary"
        );
        if position + 1 < ranges.len() {
            assert!(
                range.len() >= GENERIC_CDC_CHUNK_MIN_BYTES,
                "non-final chunk below minimum"
            );
        }
        expected_start = range.end;
    }
    assert_eq!(expected_start, bytes.len(), "incomplete coverage");
}

/// All three formats parse and feeding window size changes nothing.
#[test]
fn feed_order_does_not_change_boundaries() {
    for (name, bytes) in [
        (
            "mp3",
            tagged_mp3("els11/windows/tag", "els11/windows", 1500),
        ),
        ("adts", adts_frames("els11/windows", 1200)),
        ("flac", flac("els11/windows", 20 << 10, 1 << 20)),
    ] {
        let whole: Vec<usize> = framed(&bytes).ends().collect();
        assert!(whole.len() > 2, "{name}: corpus too small");
        for window in [1usize, 7, 417, 4096, 1 << 17] {
            let mut ends = Vec::new();
            let mut end = 0usize;
            let mut chunker = FramedAudioChunker::new();
            let mut record = |chunk: &[u8]| {
                end += chunk.len();
                ends.push(end);
            };
            for slice in bytes.chunks(window) {
                chunker.push(slice, &mut record).expect("well-formed");
            }
            chunker.finish(&mut record).expect("well-formed");
            assert_eq!(ends, whole, "{name}: window {window} changed boundaries");
        }
    }
}

/// Retagging — a different `ID3v2` tag (different length), the same
/// frames — reuses every full frame chunk.
#[test]
fn retagging_reuses_frame_chunks() {
    let frames = mp3_frames("els11/retag", 4000);
    let mut a = id3v2("els11/retag/tag-a", 2 << 10);
    a.extend_from_slice(&frames);
    let mut b = id3v2("els11/retag/tag-b", 9 << 10);
    b.extend_from_slice(&frames);
    let a_hashes = chunk_hashes(&a);
    let b_hashes = chunk_hashes(&b);
    let missing = b_hashes
        .iter()
        .filter(|hash| !a_hashes.contains(hash))
        .count();
    // The chunk carrying the tag changes, and cut positions may
    // drift for a couple of seams while the strict/relaxed regime
    // difference (the tags differ in length) washes out.
    assert!(
        missing <= 4,
        "{missing} of {} chunks changed on retagging",
        b_hashes.len()
    );
}

/// Multi-stream: concatenating two frame runs (a stitched podcast)
/// reuses both halves' interior chunks.
#[test]
fn concatenated_streams_reuse_both_halves() {
    let first = mp3_frames("els11/concat/a", 3000);
    let second = mp3_frames("els11/concat/b", 3000);
    let mut joined = first.clone();
    joined.extend_from_slice(&second);
    let first_hashes = chunk_hashes(&first);
    let second_hashes = chunk_hashes(&second);
    let joined_hashes = chunk_hashes(&joined);
    let new = joined_hashes
        .iter()
        .filter(|hash| !first_hashes.contains(hash) && !second_hashes.contains(hash))
        .count();
    assert!(
        new <= 3,
        "{new} of {} joined chunks were new",
        joined_hashes.len()
    );
}

/// A FLAC re-tag — a different `VORBIS_COMMENT`, the same audio —
/// reuses the audio chunks (per-byte cuts re-synchronize).
#[test]
fn flac_retag_reuses_audio_chunks() {
    let a = flac("els11/flac", 4 << 10, 4 << 20);
    let b = {
        // Same seed for streaminfo/audio, different comment length.
        let mut out = b"fLaC".to_vec();
        let streaminfo = super::writer::noise("els11/flac/streaminfo", 34);
        out.extend_from_slice(&super::writer::flac_block(0, false, &streaminfo));
        out.extend_from_slice(&super::writer::flac_block(
            4,
            true,
            &super::writer::noise("els11/flac/other-comment", 11 << 10),
        ));
        out.extend_from_slice(&super::writer::noise("els11/flac/audio", 4 << 20));
        out
    };
    let a_hashes = chunk_hashes(&a);
    let b_hashes = chunk_hashes(&b);
    let reused = b_hashes
        .iter()
        .filter(|hash| a_hashes.contains(hash))
        .count();
    let ratio = reused as f64 / b_hashes.len() as f64;
    assert!(ratio > 0.85, "retag reused only {ratio:.3} of chunks");
}
