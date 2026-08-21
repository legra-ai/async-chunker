//! The fail-hard corpus: invalid varints, size overruns, forbidden
//! unknown sizes, bad nesting, and truncation everywhere.

use crate::ChunkError;

use super::writer::{
    CLUSTER, CUES, INFO, SEGMENT, SIMPLE_BLOCK, TIMESTAMP, cluster, ebml_header, el, el_unknown,
    mkv, noise, open_cluster, streamed_webm, vid, vsize,
};
use crate::chunker::matroska::fault::EbmlFault;
use crate::chunker::{ChunkBoundaries, Chunker, MatroskaChunker};
use crate::profile::ChunkingProfile;

fn fault_of(bytes: &[u8]) -> Option<&'static str> {
    match ChunkBoundaries::of(ChunkingProfile::MatroskaV1, bytes) {
        Ok(_) => None,
        Err(ChunkError::MalformedProfileInput {
            profile, detail, ..
        }) => {
            assert_eq!(profile, "matroska-v1");
            Some(detail)
        }
        Err(other) => panic!("unexpected error: {other}"),
    }
}

fn expect_fault(name: &str, bytes: &[u8], fault: EbmlFault) {
    assert_eq!(fault_of(bytes), Some(fault.detail()), "{name}");
}

fn sample() -> Vec<u8> {
    mkv(
        "els09/malformed",
        &[cluster("els09/malformed/c0", 200 << 10)],
        &[],
    )
}

#[test]
fn streams_must_begin_with_the_ebml_header() {
    for (name, bytes) in [
        ("riff", b"RIFF\x00\x00\x00\x00WEBP".to_vec()),
        ("segment first", el(SEGMENT, &[0u8; 8])),
        ("info first", el(INFO, &[0u8; 8])),
    ] {
        expect_fault(name, &bytes, EbmlFault::NotMatroska);
    }
    expect_fault("empty", &[], EbmlFault::Truncated);
    // An unknown-size EBML header is forbidden.
    let mut bytes = vid(0x1A45_DFA3);
    bytes.push(0xFF);
    bytes.extend_from_slice(&noise("els09/hdr", 20));
    expect_fault(
        "unknown-size header",
        &bytes,
        EbmlFault::UnknownSizeForbidden,
    );
}

#[test]
fn invalid_varints_fail() {
    // A zero ID lead byte.
    let mut bytes = sample();
    let header_len = ebml_header("matroska").len();
    bytes[header_len] = 0x00;
    expect_fault("zero id lead", &bytes, EbmlFault::InvalidId);

    // A zero size lead byte (a nine-byte size varint).
    let mut bytes = ebml_header("matroska");
    bytes.extend_from_slice(&vid(SEGMENT));
    bytes.push(0x00);
    bytes.extend_from_slice(&[0xFF; 16]);
    expect_fault("zero size lead", &bytes, EbmlFault::InvalidSize);
}

#[test]
fn elements_overrunning_their_parent_fail() {
    // A segment child claiming more than the segment holds.
    let mut bytes = ebml_header("matroska");
    // Claim the info payload is far larger than the segment.
    let body = [vid(INFO), vsize(10_000), noise("els09/overrun", 50)].concat();
    let mut segment = vid(SEGMENT);
    segment.extend_from_slice(&vsize(body.len() as u64));
    segment.extend_from_slice(&body);
    bytes.extend_from_slice(&segment);
    expect_fault("child too large", &bytes, EbmlFault::ElementOverrunsParent);

    // A segment whose declared size ends inside a child's header.
    let mut bytes = ebml_header("matroska");
    let body = el(INFO, &noise("els09/overrun2", 60));
    let mut segment = vid(SEGMENT);
    segment.extend_from_slice(&vsize(2));
    segment.extend_from_slice(&body);
    bytes.extend_from_slice(&segment);
    expect_fault(
        "parent ends mid-header",
        &bytes,
        EbmlFault::ElementOverrunsParent,
    );
}

#[test]
fn unknown_sizes_outside_segment_and_cluster_fail() {
    let mut bytes = ebml_header("matroska");
    let body = el_unknown(INFO, &noise("els09/unknown", 40));
    bytes.extend_from_slice(&el_unknown(SEGMENT, &body));
    expect_fault("unknown-size info", &bytes, EbmlFault::UnknownSizeForbidden);

    // Inside an open cluster, an unknown-size block is forbidden.
    let mut bytes = ebml_header("matroska");
    let mut body = el(TIMESTAMP, &[0, 0]);
    body.extend_from_slice(&el_unknown(SIMPLE_BLOCK, &noise("els09/blk", 40)));
    let mut segment_body = vid(CLUSTER);
    segment_body.push(0xFF);
    segment_body.extend_from_slice(&body);
    bytes.extend_from_slice(&el_unknown(SEGMENT, &segment_body));
    expect_fault(
        "unknown-size block",
        &bytes,
        EbmlFault::UnknownSizeForbidden,
    );
}

#[test]
fn bad_nesting_fails() {
    // A cluster-only element at top level.
    let mut bytes = ebml_header("matroska");
    bytes.extend_from_slice(&el(TIMESTAMP, &[0, 0]));
    expect_fault(
        "cluster child at top level",
        &bytes,
        EbmlFault::TopLevelElement,
    );

    // A garbage element inside an open cluster: neither a cluster
    // child nor a segment-level element.
    let mut bytes = ebml_header("matroska");
    let mut cluster_body = el(TIMESTAMP, &[0, 0]);
    cluster_body.extend_from_slice(&el(0x4286, &[1])); // EBMLVersion id
    let mut segment_body = vid(CLUSTER);
    segment_body.push(0xFF);
    segment_body.extend_from_slice(&cluster_body);
    bytes.extend_from_slice(&el_unknown(SEGMENT, &segment_body));
    expect_fault(
        "garbage in open cluster",
        &bytes,
        EbmlFault::UnexpectedClusterChild,
    );
}

#[test]
fn truncation_fails_at_finish() {
    let bytes = sample();
    for (name, cut) in [
        ("inside header id", 2usize),
        ("inside ebml header", 20),
        ("inside segment header", ebml_header("matroska").len() + 5),
        ("inside info", ebml_header("matroska").len() + 12),
        ("inside cluster", bytes.len() - 1000),
        ("one byte short", bytes.len() - 1),
    ] {
        expect_fault(name, &bytes[..cut], EbmlFault::Truncated);
    }
    // A header with no segment at all is truncated, not complete.
    expect_fault(
        "header only",
        &ebml_header("matroska"),
        EbmlFault::Truncated,
    );
    // Streamed files may end at any cluster-element boundary…
    assert_eq!(fault_of(&streamed_webm("els09/stream", 2)), None);
    // …but not inside a block.
    let stream = streamed_webm("els09/stream", 2);
    expect_fault(
        "inside streamed block",
        &stream[..stream.len() - 7],
        EbmlFault::Truncated,
    );

    // An unknown-size cluster closed by Cues, then truncated inside
    // the Cues payload, is still truncation.
    let mut bytes = ebml_header("matroska");
    let mut segment_body = open_cluster("els09/open", 4, 8 << 10);
    segment_body.extend_from_slice(&el(CUES, &noise("els09/cues", 90)));
    bytes.extend_from_slice(&el_unknown(SEGMENT, &segment_body));
    assert_eq!(fault_of(&bytes), None, "well-formed base");
    expect_fault(
        "inside trailing cues",
        &bytes[..bytes.len() - 10],
        EbmlFault::Truncated,
    );
}

/// Once rejected, the chunker stays rejected.
#[test]
fn a_rejected_stream_stays_rejected() {
    let mut chunker = MatroskaChunker::new();
    let mut sink = |_: &[u8]| {};
    chunker.push(b"RIFF....", &mut sink).expect_err("rejects");
    assert!(matches!(
        chunker.push(&ebml_header("matroska"), &mut sink),
        Err(ChunkError::ProfileStreamRejected { .. })
    ));
    assert!(matches!(
        chunker.finish(&mut sink),
        Err(ChunkError::ProfileStreamRejected { .. })
    ));
}
