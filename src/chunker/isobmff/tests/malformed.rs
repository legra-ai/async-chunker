//! The fail-hard corpus: bad first box, sizes below the header,
//! open-ended sizes below top level, children overrunning parents,
//! depth bombs, and truncation at every layer.

use crate::ChunkError;

use super::writer::{bx, bx_large, bx_open, container, ftyp, moov, mp4, noise};
use crate::chunker::isobmff::fault::BoxFault;
use crate::chunker::{ChunkBoundaries, Chunker, IsobmffChunker};
use crate::profile::ChunkingProfile;

fn fault_of(bytes: &[u8]) -> Option<&'static str> {
    match ChunkBoundaries::of(ChunkingProfile::IsobmffV1, bytes) {
        Ok(_) => None,
        Err(ChunkError::MalformedProfileInput {
            profile, detail, ..
        }) => {
            assert_eq!(profile, "isobmff-v1");
            Some(detail)
        }
        Err(other) => panic!("unexpected error: {other}"),
    }
}

fn expect_fault(name: &str, bytes: &[u8], fault: BoxFault) {
    assert_eq!(fault_of(bytes), Some(fault.detail()), "{name}");
}

fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
}

fn sample() -> Vec<u8> {
    mp4("els08/malformed", &noise("els08/malformed/mdat", 300 << 10))
}

#[test]
fn streams_must_begin_with_a_known_box() {
    for (name, bytes) in [
        ("png", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec()),
        ("unknown first box", bx(b"zzzz", &[0u8; 16])),
        ("moov child first", bx(b"mvhd", &[0u8; 100])),
    ] {
        expect_fault(name, &bytes, BoxFault::NotAnIsoBmffStream);
    }
    // Empty input never saw a first box.
    expect_fault("empty", &[], BoxFault::Truncated);
}

#[test]
fn sizes_below_the_header_fail() {
    let mut bytes = sample();
    // The `free` box claims 4 bytes: smaller than its own header.
    let free = 24; // after ftyp
    assert_eq!(&bytes[free + 4..free + 8], b"free");
    put32(&mut bytes, free, 4);
    expect_fault("compact", &bytes, BoxFault::SizeBelowHeader);

    // An extended-size box whose largesize is below 16.
    let mut bytes = [ftyp(b"isom"), bx_large(b"mdat", &noise("els08/large", 64))].concat();
    bytes[24 + 8..24 + 16].copy_from_slice(&12u64.to_be_bytes());
    expect_fault("largesize", &bytes, BoxFault::SizeBelowHeader);
}

#[test]
fn open_ended_sizes_below_top_level_fail() {
    let bytes = [
        ftyp(b"isom"),
        container(b"moov", &[bx_open(b"mvhd", &[0u8; 100])]),
    ]
    .concat();
    expect_fault("nested open size", &bytes, BoxFault::OpenSizeNested);
}

#[test]
fn children_overrunning_their_parent_fail() {
    // A child claiming more than its container holds.
    let mut bytes = [ftyp(b"isom"), moov("els08/overrun", 40)].concat();
    let moov_start = 24;
    assert_eq!(&bytes[moov_start + 4..moov_start + 8], b"moov");
    let moov_size = u32::from_be_bytes([
        bytes[moov_start],
        bytes[moov_start + 1],
        bytes[moov_start + 2],
        bytes[moov_start + 3],
    ]);
    // First child is mvhd at moov_start + 8.
    put32(&mut bytes, moov_start + 8, moov_size);
    expect_fault("child too large", &bytes, BoxFault::ChildOverrunsParent);

    // A container whose declared size ends in the middle of a child's
    // header.
    let mut bytes = [
        ftyp(b"isom"),
        container(b"moov", &[bx(b"mvhd", &[0u8; 100]), bx(b"free", &[0u8; 4])]),
    ]
    .concat();
    put32(&mut bytes, 24, 8 + 108 + 3);
    expect_fault(
        "parent ends mid-header",
        &bytes,
        BoxFault::ChildOverrunsParent,
    );
}

#[test]
fn depth_bombs_fail() {
    let mut bytes = bx(b"free", &[]);
    for _ in 0..40 {
        bytes = container(b"moov", &[bytes]);
    }
    let bytes = [ftyp(b"isom"), bytes].concat();
    expect_fault("depth", &bytes, BoxFault::DepthExceeded);
}

#[test]
fn truncation_fails_at_finish() {
    let bytes = sample();
    let mdat = bytes.len() - (300 << 10) - 8;
    for (name, cut) in [
        ("inside first header", 5usize),
        ("inside ftyp", 12),
        ("inside moov", 24 + 8 + 40),
        ("inside mdat header", mdat + 3),
        ("inside mdat", mdat + 2000),
        ("one byte short", bytes.len() - 1),
    ] {
        expect_fault(name, &bytes[..cut], BoxFault::Truncated);
    }
    // An open-ended mdat is the one box that may end with the stream,
    // but its container parents may not stay open.
    let ok = [ftyp(b"isom"), bx_open(b"mdat", &noise("els08/open", 5000))].concat();
    assert_eq!(fault_of(&ok), None);
}

/// Once rejected, the chunker stays rejected.
#[test]
fn a_rejected_stream_stays_rejected() {
    let mut chunker = IsobmffChunker::new();
    let mut sink = |_: &[u8]| {};
    chunker
        .push(b"\0\0\0\x10zzzz", &mut sink)
        .expect_err("rejects");
    assert!(matches!(
        chunker.push(&ftyp(b"isom"), &mut sink),
        Err(ChunkError::ProfileStreamRejected { .. })
    ));
    assert!(matches!(
        chunker.finish(&mut sink),
        Err(ChunkError::ProfileStreamRejected { .. })
    ));
}
