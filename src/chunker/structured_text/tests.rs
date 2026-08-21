//! `structured-text-v1` regression tests: frozen boundaries on
//! Markdown/JSON/XML corpora, candidate-only cuts, scalar safety at
//! forced cuts, fail-hard malformed input, feed-order independence,
//! and shifted-region reuse.

use crate::ChunkError;

use super::*;
use crate::chunker::{ChunkBoundaries, Chunker};
use crate::constants::{
    GENERIC_CDC_CHUNK_MAX_BYTES, GENERIC_CDC_CHUNK_MIN_BYTES, GENERIC_CDC_CHUNK_TARGET_BYTES,
};
use crate::profile::ChunkingProfile;

/// Deterministic word stream: seeded pseudo-random picks from a
/// small vocabulary, so corpora are text-shaped but unique.
struct Words {
    reader: blake3::OutputReader,
}

impl Words {
    const VOCABULARY: [&'static str; 24] = [
        "literal",
        "manifest",
        "chunk",
        "workspace",
        "canonical",
        "boundary",
        "profile",
        "stream",
        "résumé",
        "naïve",
        "日本語",
        "Ελληνικά",
        "emoji😀",
        "block",
        "index",
        "commit",
        "branch",
        "merge",
        "custody",
        "encrypt",
        "deterministic",
        "frozen",
        "registry",
        "datatype",
    ];

    fn new(seed: &str) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(seed.as_bytes());
        Self {
            reader: hasher.finalize_xof(),
        }
    }

    fn byte(&mut self) -> u8 {
        let mut one = [0u8; 1];
        self.reader.fill(&mut one);
        one[0]
    }

    fn word(&mut self) -> &'static str {
        Self::VOCABULARY[usize::from(self.byte()) % Self::VOCABULARY.len()]
    }

    /// A line of `n` words.
    fn line(&mut self, n: usize) -> String {
        (0..n).map(|_| self.word()).collect::<Vec<_>>().join(" ")
    }
}

/// Markdown-shaped text of at least `len` bytes: headings,
/// paragraphs, lists.
fn markdown(seed: &str, len: usize) -> Vec<u8> {
    let mut words = Words::new(seed);
    let mut out = String::new();
    let mut paragraph = 0usize;
    while out.len() < len {
        if paragraph.is_multiple_of(5) {
            out.push_str("## ");
            out.push_str(&words.line(3));
            out.push_str("\n\n");
        }
        if paragraph % 3 == 2 {
            for _ in 0..4 {
                out.push_str("- ");
                out.push_str(&words.line(6));
                out.push('\n');
            }
            out.push('\n');
        } else {
            let sentences = 2 + usize::from(words.byte() % 4);
            for _ in 0..sentences {
                let words_in_sentence = 8 + usize::from(words.byte() % 10);
                out.push_str(&words.line(words_in_sentence));
                out.push_str(". ");
            }
            out.push_str("\n\n");
        }
        paragraph += 1;
    }
    out.into_bytes()
}

/// Minified JSON of at least `len` bytes: one line, no whitespace.
fn minified_json(seed: &str, len: usize) -> Vec<u8> {
    let mut words = Words::new(seed);
    let mut out = String::from("[");
    let mut first = true;
    while out.len() < len {
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str(&format!(
            "{{\"id\":{},\"name\":\"{}\",\"tags\":[\"{}\",\"{}\"]}}",
            u32::from(words.byte()) * 7919,
            words.word(),
            words.word(),
            words.word()
        ));
    }
    out.push(']');
    out.into_bytes()
}

/// Pretty-printed XML of at least `len` bytes.
fn xml(seed: &str, len: usize) -> Vec<u8> {
    let mut words = Words::new(seed);
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<corpus>\n");
    while out.len() < len {
        out.push_str(&format!(
            "  <entry id=\"{}\">\n    <title>{}</title>\n    <body>{}</body>\n  </entry>\n",
            u32::from(words.byte()),
            words.line(3),
            words.line(12)
        ));
    }
    out.push_str("</corpus>\n");
    out.into_bytes()
}

fn structured(bytes: &[u8]) -> ChunkBoundaries {
    ChunkBoundaries::of(ChunkingProfile::StructuredTextV1, bytes).expect("well-formed text")
}

/// BLAKE3 hex over the boundary offsets, for compact goldens.
fn boundary_digest(boundaries: &ChunkBoundaries) -> String {
    let mut hasher = blake3::Hasher::new();
    for end in boundaries.ends() {
        hasher.update(&(end as u64).to_le_bytes());
    }
    hasher.finalize().to_hex()[..16].to_owned()
}

/// Whether a chunk's final byte is a frozen cut candidate.
fn ends_at_candidate(chunk: &[u8]) -> bool {
    matches!(
        chunk.last(),
        Some(b'\n' | b'\t' | b'\r' | b' ' | b',' | b';' | b'.' | b'}' | b']' | b'>' | b')')
    )
}

/// Frozen boundaries for the three corpus shapes. A change here is a
/// change to `structured-text-v1` itself — a format cutover.
#[test]
fn golden_boundaries() {
    for (name, bytes, count, digest) in [
        (
            "markdown",
            markdown("els06/golden", 4 << 20),
            64usize,
            "8bb01e8519dba671",
        ),
        (
            "json",
            minified_json("els06/golden", 4 << 20),
            50,
            "58fb93728ae7c51b",
        ),
        ("xml", xml("els06/golden", 4 << 20), 81, "72521c0a749656ee"),
    ] {
        let boundaries = structured(&bytes);
        assert_eq!(boundaries.len(), count, "{name}: frozen chunk count");
        assert_eq!(
            boundary_digest(&boundaries),
            digest,
            "{name}: frozen boundaries"
        );
    }
}

/// Chunks cover the payload exactly within the frozen envelope, every
/// chunk is whole UTF-8, and every non-forced cut lands after a
/// candidate byte.
#[test]
fn bounds_coverage_and_candidate_cuts() {
    for (name, bytes) in [
        ("markdown", markdown("els06/bounds", 2 << 20)),
        ("json", minified_json("els06/bounds", 2 << 20)),
        ("xml", xml("els06/bounds", 2 << 20)),
    ] {
        let boundaries = structured(&bytes);
        let ranges: Vec<_> = boundaries.ranges().collect();
        assert!(
            ranges.len() > 8,
            "{name}: corpus too small to exercise cuts"
        );
        let mut expected_start = 0usize;
        for (position, range) in ranges.iter().enumerate() {
            assert_eq!(range.start, expected_start, "{name}: gap or overlap");
            let chunk = &bytes[range.clone()];
            assert!(
                chunk.len() <= GENERIC_CDC_CHUNK_MAX_BYTES,
                "{name}: above maximum"
            );
            assert!(
                std::str::from_utf8(chunk).is_ok(),
                "{name}: chunk splits a scalar"
            );
            let last = position + 1 == ranges.len();
            if !last {
                assert!(
                    chunk.len() >= GENERIC_CDC_CHUNK_MIN_BYTES,
                    "{name}: non-final chunk below minimum"
                );
                assert!(
                    ends_at_candidate(chunk) || chunk.len() >= GENERIC_CDC_CHUNK_MAX_BYTES - 3,
                    "{name}: chunk {position} ends at a non-candidate byte"
                );
            }
            expected_start = range.end;
        }
        assert_eq!(expected_start, bytes.len(), "{name}: incomplete coverage");
    }
}

/// Candidate density matters, not line structure: single-line JSON
/// still closes chunks near the target through soft breaks instead
/// of always running to the maximum.
#[test]
fn long_lines_still_cut_near_target() {
    let bytes = minified_json("els06/long-lines", 4 << 20);
    let boundaries = structured(&bytes);
    let forced = boundaries
        .ranges()
        .filter(|range| range.len() >= GENERIC_CDC_CHUNK_MAX_BYTES - 3)
        .count();
    assert!(
        forced * 4 < boundaries.len(),
        "{forced} of {} chunks were forced at the maximum",
        boundaries.len()
    );
    let mean = bytes.len() / boundaries.len();
    assert!(
        mean < GENERIC_CDC_CHUNK_TARGET_BYTES * 2,
        "mean chunk {mean} bytes drifted far above target"
    );
}

/// Candidate-free text (no whitespace, no punctuation) can only cut
/// at the maximum, and a forced cut inside a multi-byte scalar backs
/// off to the last scalar boundary.
#[test]
fn forced_cuts_never_split_a_scalar() {
    // 4-byte scalars; 256 KiB is divisible by 4 so a plain maximum
    // cut would be safe — shift by one 3-byte scalar so it is not.
    let mut bytes = "€".as_bytes().to_vec();
    bytes.extend("😀".repeat(GENERIC_CDC_CHUNK_MAX_BYTES * 2).as_bytes());
    let boundaries = structured(&bytes);
    let ranges: Vec<_> = boundaries.ranges().collect();
    assert!(ranges.len() >= 8);
    for (position, range) in ranges.iter().enumerate() {
        let chunk = &bytes[range.clone()];
        assert!(
            std::str::from_utf8(chunk).is_ok(),
            "chunk {position} splits a scalar"
        );
        if position + 1 < ranges.len() {
            assert!(
                chunk.len() <= GENERIC_CDC_CHUNK_MAX_BYTES
                    && chunk.len() >= GENERIC_CDC_CHUNK_MAX_BYTES - 3,
                "chunk {position} is {} bytes, not a maximum cut",
                chunk.len()
            );
        }
    }
}

/// Empty input produces no chunks.
#[test]
fn empty_payload_produces_no_chunks() {
    assert!(structured(&[]).is_empty());
}

/// Every way UTF-8 can be malformed rejects the stream with the
/// offending offset, and the chunker accepts nothing afterwards.
#[test]
fn malformed_input_fails_hard() {
    let mut prefix = markdown("els06/malformed", 40 << 10);
    prefix.truncate(40 << 10);
    while !std::str::from_utf8(&prefix).is_ok() {
        prefix.pop();
    }
    let at = prefix.len() as u64;
    for (name, tail) in [
        ("stray continuation", vec![0x80u8]),
        ("overlong two-byte", vec![0xC0, 0x80]),
        ("overlong three-byte", vec![0xE0, 0x80, 0x80]),
        ("surrogate", vec![0xED, 0xA0, 0x80]),
        ("above U+10FFFF", vec![0xF4, 0x90, 0x80, 0x80]),
        ("F5 lead", vec![0xF5]),
        ("broken continuation", vec![0xE2, 0x28, 0xA1]),
    ] {
        let mut bytes = prefix.clone();
        bytes.extend_from_slice(&tail);
        let error = ChunkBoundaries::of(ChunkingProfile::StructuredTextV1, &bytes).expect_err(name);
        match error {
            ChunkError::MalformedProfileInput {
                profile, offset, ..
            } => {
                assert_eq!(profile, "structured-text-v1", "{name}");
                assert!(
                    offset >= at && offset < at + tail.len() as u64,
                    "{name}: fault at {offset}, tail starts at {at}"
                );
            }
            other => panic!("{name}: expected malformed-input error, got {other}"),
        }
    }

    // A stream ending inside a scalar is rejected at finish.
    let mut bytes = prefix.clone();
    bytes.extend_from_slice(&[0xE2, 0x82]);
    let error = ChunkBoundaries::of(ChunkingProfile::StructuredTextV1, &bytes)
        .expect_err("truncated scalar");
    assert!(
        matches!(error, ChunkError::MalformedProfileInput { .. }),
        "truncated scalar: {error}"
    );

    // Once rejected, the chunker stays rejected.
    let mut chunker = StructuredTextChunker::new();
    let mut sink = |_: &[u8]| {};
    chunker.push(&[0xFF], &mut sink).expect_err("rejects");
    assert!(matches!(
        chunker.push(b"fine", &mut sink),
        Err(ChunkError::ProfileStreamRejected { .. })
    ));
    assert!(matches!(
        chunker.finish(&mut sink),
        Err(ChunkError::ProfileStreamRejected { .. })
    ));
}

/// Boundaries depend on content alone — not on how the bytes were
/// fed in, even when a window splits a scalar.
#[test]
fn feed_order_does_not_change_boundaries() {
    let bytes = markdown("els06/windows", 1 << 20);
    let whole: Vec<usize> = structured(&bytes).ends().collect();

    for window in [1usize, 3, 7, 4096, 1 << 17] {
        let mut ends = Vec::new();
        let mut end = 0usize;
        let mut chunker = StructuredTextChunker::new();
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

fn chunk_hashes(bytes: &[u8]) -> Vec<blake3::Hash> {
    structured(bytes)
        .ranges()
        .map(|range| blake3::hash(&bytes[range]))
        .collect()
}

fn reuse_ratio(base: &[blake3::Hash], edited: &[blake3::Hash]) -> f64 {
    let reused = edited.iter().filter(|hash| base.contains(hash)).count();
    reused as f64 / edited.len() as f64
}

/// The reuse property the profile exists for: a prefix shift and a
/// mid-document paragraph insertion leave almost every chunk intact,
/// because cuts re-synchronize at the next textual seam.
#[test]
fn boundaries_resynchronize_after_edits() {
    let shared = markdown("els06/shared", 4 << 20);
    let base = chunk_hashes(&shared);

    for prefix_len in [1usize, 64, 4096, 65_536] {
        let mut shifted = markdown("els06/prefix", prefix_len);
        shifted.truncate(prefix_len);
        while !std::str::from_utf8(&shifted).is_ok() {
            shifted.pop();
        }
        shifted.extend_from_slice(&shared);
        let ratio = reuse_ratio(&base, &chunk_hashes(&shifted));
        assert!(
            ratio > 0.90,
            "a {prefix_len}-byte shift reused only {ratio:.3} of chunks"
        );
    }

    let middle = shared.len() / 2;
    let split = (middle..shared.len())
        .find(|&offset| shared[offset] == b'\n')
        .expect("a line end")
        + 1;
    let mut edited = shared[..split].to_vec();
    edited.extend_from_slice(markdown("els06/insert", 2 << 10).as_slice());
    edited.extend_from_slice(&shared[split..]);
    let ratio = reuse_ratio(&base, &chunk_hashes(&edited));
    assert!(
        ratio > 0.90,
        "a mid-document insertion reused only {ratio:.3} of chunks"
    );
}
