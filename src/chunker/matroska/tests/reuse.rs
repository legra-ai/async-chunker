//! Canonical boundaries, cluster-boundary cuts, and reuse.

use super::writer::{ATTACHMENTS, TAGS, cluster, el, mkv, noise, streamed_webm};
use crate::chunker::{ChunkBoundaries, Chunker, MatroskaChunker};
use crate::constants::{GENERIC_CDC_CHUNK_MAX_BYTES, GENERIC_CDC_CHUNK_MIN_BYTES};
use crate::profile::ChunkingProfile;

fn matroska(bytes: &[u8]) -> ChunkBoundaries {
    ChunkBoundaries::of(ChunkingProfile::MatroskaV1, bytes).expect("well-formed stream")
}

fn boundary_digest(boundaries: &ChunkBoundaries) -> String {
    let mut hasher = blake3::Hasher::new();
    for end in boundaries.ends() {
        hasher.update(&(end as u64).to_le_bytes());
    }
    hasher.finalize().to_hex()[..16].to_owned()
}

fn chunk_hashes(bytes: &[u8]) -> Vec<(std::ops::Range<usize>, blake3::Hash)> {
    matroska(bytes)
        .ranges()
        .map(|range| (range.clone(), blake3::hash(&bytes[range])))
        .collect()
}

fn six_clusters(seed: &str) -> Vec<Vec<u8>> {
    (0..6)
        .map(|index| cluster(&format!("{seed}/cluster{index}"), 400 << 10))
        .collect()
}

/// Frozen boundaries for the Matroska and streamed-WebM corpora. A
/// change here is a change to `matroska-v1` itself — a format
/// cutover.
#[test]
fn golden_boundaries() {
    for (name, bytes, count, digest) in [
        (
            "mkv",
            mkv(
                "els09/golden",
                &six_clusters("els09/golden"),
                &[el(TAGS, &noise("els09/golden/tags", 500))],
            ),
            36usize,
            "eb8d0bd1ad053967",
        ),
        (
            "webm",
            streamed_webm("els09/golden", 5),
            25,
            "7668bea86707a34e",
        ),
    ] {
        let boundaries = matroska(&bytes);
        assert_eq!(boundaries.len(), count, "{name}: frozen chunk count");
        assert_eq!(
            boundary_digest(&boundaries),
            digest,
            "{name}: frozen boundaries"
        );
    }
}

/// Chunks cover the stream exactly within the envelope; sub-minimum
/// chunks end only where a unit starts.
#[test]
fn bounds_coverage_and_cluster_boundaries() {
    let clusters = six_clusters("els09/bounds");
    let bytes = mkv("els09/bounds", &clusters, &[]);
    let ranges: Vec<_> = matroska(&bytes).ranges().collect();
    let mut cluster_starts = Vec::new();
    let mut at = bytes.len() - clusters.iter().map(Vec::len).sum::<usize>();
    for cluster in &clusters {
        cluster_starts.push(at);
        at += cluster.len();
    }
    let mut expected_start = 0usize;
    for (position, range) in ranges.iter().enumerate() {
        assert_eq!(range.start, expected_start, "gap or overlap");
        assert!(range.len() <= GENERIC_CDC_CHUNK_MAX_BYTES, "above maximum");
        if position + 1 < ranges.len() && range.len() < GENERIC_CDC_CHUNK_MIN_BYTES {
            assert!(
                cluster_starts.contains(&range.end),
                "sub-minimum chunk {position} does not end at a cluster start"
            );
        }
        expected_start = range.end;
    }
    assert_eq!(expected_start, bytes.len(), "incomplete coverage");
    let starts: Vec<usize> = ranges.iter().map(|range| range.start).collect();
    for start in &cluster_starts {
        assert!(starts.contains(start), "a cluster must begin a chunk");
    }
}

/// Both file shapes parse and feeding window size changes nothing.
#[test]
fn feed_order_does_not_change_boundaries() {
    for (name, bytes) in [
        (
            "mkv",
            mkv("els09/windows", &six_clusters("els09/windows"), &[]),
        ),
        ("webm", streamed_webm("els09/windows", 4)),
    ] {
        let whole: Vec<usize> = matroska(&bytes).ends().collect();
        assert!(whole.len() > 3, "{name}: corpus too small");
        for window in [1usize, 7, 4096, 1 << 17] {
            let mut ends = Vec::new();
            let mut end = 0usize;
            let mut chunker = MatroskaChunker::new();
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

/// A metadata edit (different `Info`/`Tracks`/`Tags`) leaves every
/// cluster chunk intact.
#[test]
fn metadata_edits_reuse_every_cluster_chunk() {
    let clusters = six_clusters("els09/edit");
    let base = mkv(
        "els09/edit/a",
        &clusters,
        &[el(TAGS, &noise("els09/tags/a", 400))],
    );
    let edited = mkv(
        "els09/edit/b",
        &clusters,
        &[el(TAGS, &noise("els09/tags/b", 700))],
    );
    let base_hashes: Vec<_> = chunk_hashes(&base)
        .into_iter()
        .map(|(_, hash)| hash)
        .collect();
    let edited_chunks = chunk_hashes(&edited);
    let changed = edited_chunks
        .iter()
        .filter(|(_, hash)| !base_hashes.contains(hash))
        .count();
    // The lead-in (header + segment header + info + tracks) chunk and
    // the trailing tags chunk change; every cluster chunk is reused.
    assert!(changed <= 2, "{changed} chunks changed on a metadata edit");
    assert!(edited_chunks.len() >= 12, "corpus must span many chunks");
}

/// A shared attachment yields the same chunks in two different files,
/// and appending clusters to a stream reuses every earlier cluster
/// chunk.
#[test]
fn shared_elements_and_appended_streams_reuse_chunks() {
    let attachment = el(ATTACHMENTS, &noise("els09/shared/font", 700 << 10));
    let a = mkv(
        "els09/share/a",
        &six_clusters("els09/share/a"),
        std::slice::from_ref(&attachment),
    );
    let b = mkv(
        "els09/share/b",
        &[cluster("els09/share/b0", 100 << 10)],
        &[attachment],
    );
    let a_hashes: Vec<_> = chunk_hashes(&a).into_iter().map(|(_, hash)| hash).collect();
    let b_chunks = chunk_hashes(&b);
    let reused = b_chunks
        .iter()
        .filter(|(_, hash)| a_hashes.contains(hash))
        .count();
    assert!(reused >= 8, "only {reused} shared-attachment chunks reused");

    let short = streamed_webm("els09/append", 3);
    let long = streamed_webm("els09/append", 5);
    assert_eq!(&long[..short.len()], &short[..], "append-only fixture");
    let short_hashes: Vec<_> = chunk_hashes(&short)
        .into_iter()
        .map(|(_, hash)| hash)
        .collect();
    let long_chunks = chunk_hashes(&long);
    let missing = short_hashes
        .iter()
        .filter(|hash| !long_chunks.iter().any(|(_, long_hash)| long_hash == *hash))
        .count();
    // Only the seam may differ: the short stream's trailing partial
    // chunk (and the held chunk it merges into once a new cluster
    // follows); every settled chunk is reused.
    assert!(missing <= 2, "{missing} earlier chunks lost by appending");
}
