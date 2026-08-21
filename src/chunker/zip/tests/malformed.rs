//! The fail-hard corpus: truncation, overlap, invalid directories,
//! expansion claims, bad framing — every one rejects before a root
//! could exist, with the frozen diagnostic.

use crate::ChunkError;

use super::corpus::{
    Media, central_entry, document_xml, docx, docx_with_framings, member_span, noise,
};
use super::writer::{Framing, Member, Options, archive};
use crate::chunker::zip::fault::ZipFault;
use crate::chunker::{ChunkBoundaries, Chunker, ZipChunker};
use crate::profile::ChunkingProfile;

/// The fault an archive is rejected with.
fn fault_of(bytes: &[u8]) -> Option<&'static str> {
    match ChunkBoundaries::of(ChunkingProfile::ZipV1, bytes) {
        Ok(_) => None,
        Err(ChunkError::MalformedProfileInput {
            profile, detail, ..
        }) => {
            assert_eq!(profile, "zip-v1");
            Some(detail)
        }
        Err(other) => panic!("unexpected error: {other}"),
    }
}

fn expect_fault(name: &str, bytes: &[u8], fault: ZipFault) {
    assert_eq!(fault_of(bytes), Some(fault.detail()), "{name}");
}

fn find(bytes: &[u8], needle: &[u8]) -> usize {
    (0..=bytes.len() - needle.len())
        .find(|&at| &bytes[at..at + needle.len()] == needle)
        .expect("needle present")
}

fn rfind(bytes: &[u8], needle: &[u8]) -> usize {
    (0..=bytes.len() - needle.len())
        .rev()
        .find(|&at| &bytes[at..at + needle.len()] == needle)
        .expect("needle present")
}

const LOCAL: [u8; 4] = [0x50, 0x4B, 0x03, 0x04];
const CENTRAL: [u8; 4] = [0x50, 0x4B, 0x01, 0x02];
const END: [u8; 4] = [0x50, 0x4B, 0x05, 0x06];

fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn put16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn sample() -> Vec<u8> {
    docx(
        &Media::new(),
        &document_xml("els07/malformed", 1500, "malformed"),
    )
}

/// Truncation anywhere — inside a header, inside member data, before
/// the central directory, inside the end record, inside the comment
/// — fails at finish.
#[test]
fn truncation_fails_at_finish() {
    let bytes = docx_with_framings(&Media::new(), &document_xml("els07/trunc", 1500, "t"), true);
    let end = rfind(&bytes, &END);
    let image1 = member_span(&bytes, "word/media/image1.png");
    for (name, cut) in [
        ("inside first header", 10usize),
        ("inside member data", image1.start + 2000),
        ("before central directory", find(&bytes, &CENTRAL)),
        ("inside central directory", find(&bytes, &CENTRAL) + 50),
        ("inside end record", end + 10),
        ("inside comment", bytes.len() - 2),
    ] {
        expect_fault(name, &bytes[..cut], ZipFault::Truncated);
    }
}

#[test]
fn unknown_signatures_fail() {
    let mut bytes = sample();
    bytes[0] = b'Q';
    expect_fault("first signature", &bytes, ZipFault::UnknownSignature);

    let mut bytes = sample();
    let second = member_span(&bytes, "_rels/.rels").start;
    bytes[second + 2] = 0x09;
    expect_fault("second signature", &bytes, ZipFault::UnknownSignature);

    // A spanning marker or self-extractor stub is not a ZIP stream.
    let mut prefixed = b"MZ stub".to_vec();
    prefixed.extend_from_slice(&sample());
    expect_fault("prefixed", &prefixed, ZipFault::UnknownSignature);
}

#[test]
fn trailing_bytes_after_the_comment_fail() {
    let mut bytes = sample();
    bytes.extend_from_slice(b"junk");
    expect_fault("trailing", &bytes, ZipFault::TrailingBytes);
}

#[test]
fn size_claims_are_checked() {
    // Stored member whose sizes disagree.
    let mut bytes = sample();
    let at = member_span(&bytes, "[Content_Types].xml").start;
    put32(&mut bytes, at + 22, 5);
    expect_fault("stored sizes", &bytes, ZipFault::StoredSizesDisagree);

    // Deflated member claiming an impossible expansion.
    let mut bytes = sample();
    let at = member_span(&bytes, "word/document.xml").start;
    put32(&mut bytes, at + 22, u32::MAX - 1);
    expect_fault("expansion", &bytes, ZipFault::ImplausibleExpansion);

    // The same claim in the central directory is caught too.
    let mut bytes = sample();
    let central = central_entry(&bytes, "word/document.xml");
    put32(&mut bytes, central + 24, u32::MAX - 1);
    expect_fault("central expansion", &bytes, ZipFault::ImplausibleExpansion);

    // Sizes marked ZIP64 without the extra field.
    let mut bytes = sample();
    let at = member_span(&bytes, "word/styles.xml").start;
    put32(&mut bytes, at + 18, u32::MAX);
    put32(&mut bytes, at + 22, u32::MAX);
    expect_fault("missing zip64", &bytes, ZipFault::MissingZip64Sizes);
}

#[test]
fn descriptor_mismatches_fail() {
    let data = noise("els07/descriptor", 30 << 10);
    let bytes = archive(
        &[Member::stored("a.bin", &data).framed(Framing::UnsignedDescriptorKnownSize)],
        Options::default(),
    );
    let mut wrong = bytes.clone();
    let descriptor = find(&bytes, &LOCAL) + 30 + "a.bin".len() + data.len();
    put32(&mut wrong, descriptor + 4, data.len() as u32 + 1);
    expect_fault(
        "unsigned descriptor",
        &wrong,
        ZipFault::DescriptorSizeMismatch,
    );

    // A signed-descriptor member whose descriptor never matches the
    // bytes it covers cannot be closed: the walk runs into the end
    // of the stream.
    let bytes = archive(
        &[Member::stored("a.bin", &data).framed(Framing::SignedDescriptor)],
        Options::default(),
    );
    let mut wrong = bytes.clone();
    let descriptor = find(&bytes, &LOCAL) + 30 + "a.bin".len() + data.len();
    put32(&mut wrong, descriptor + 8, data.len() as u32 + 1);
    expect_fault("signed descriptor", &wrong, ZipFault::Truncated);
}

#[test]
fn false_descriptor_signatures_inside_data_are_skipped() {
    // Member data that contains the descriptor signature followed by
    // a size that does not match must not end the member early.
    let mut data = noise("els07/false-sig", 40 << 10);
    data[100..104].copy_from_slice(&[0x50, 0x4B, 0x07, 0x08]);
    data[104..108].copy_from_slice(&0u32.to_le_bytes());
    data[108..112].copy_from_slice(&77u32.to_le_bytes());
    data[112..116].copy_from_slice(&77u32.to_le_bytes());
    let bytes = archive(
        &[
            Member::stored("a.bin", &data).framed(Framing::SignedDescriptor),
            Member::stored("b.txt", b"after"),
        ],
        Options::default(),
    );
    assert_eq!(
        fault_of(&bytes),
        None,
        "a real descriptor closes the member"
    );
}

#[test]
fn invalid_directories_fail() {
    // Entry count disagrees with what streamed past.
    let mut bytes = sample();
    let end = rfind(&bytes, &END);
    put16(&mut bytes, end + 10, 7);
    expect_fault("entry count", &bytes, ZipFault::EntryCountMismatch);

    // Central directory offset wrong.
    let mut bytes = sample();
    let end = rfind(&bytes, &END);
    put32(&mut bytes, end + 16, 1);
    expect_fault("central offset", &bytes, ZipFault::CentralDirectoryGeometry);

    // Central directory size wrong.
    let mut bytes = sample();
    let end = rfind(&bytes, &END);
    put32(&mut bytes, end + 12, 1);
    expect_fault("central size", &bytes, ZipFault::CentralDirectoryGeometry);

    // An entry pointing at or beyond the central directory.
    let mut bytes = sample();
    let central = find(&bytes, &CENTRAL);
    put32(&mut bytes, central + 42, central as u32);
    expect_fault("entry offset", &bytes, ZipFault::CentralOffsetOutOfRange);
}

/// Overlapping entries (the zip-bomb construction where many central
/// entries share one local member) are caught by the count
/// reconciliation: more entries than members walked.
#[test]
fn overlapping_entries_fail() {
    let data = noise("els07/overlap", 20 << 10);
    let base = archive(&[Member::stored("a.bin", &data)], Options::default());
    let central = find(&base, &CENTRAL);
    let end = rfind(&base, &END);
    let entry = base[central..end].to_vec();
    let mut bytes = base[..end].to_vec();
    bytes.extend_from_slice(&entry);
    bytes.extend_from_slice(&base[end..]);
    let end = rfind(&bytes, &END);
    put16(&mut bytes, end + 8, 2);
    put16(&mut bytes, end + 10, 2);
    put32(&mut bytes, end + 12, (entry.len() * 2) as u32);
    expect_fault("overlap", &bytes, ZipFault::EntryCountMismatch);
}

#[test]
fn records_out_of_sequence_fail() {
    // A local header after the central directory.
    let base = sample();
    let extra_member = archive(&[Member::stored("late.txt", b"late")], Options::default());
    let late = &extra_member[..find(&extra_member, &CENTRAL)];
    let end = rfind(&base, &END);
    let mut bytes = base[..end].to_vec();
    bytes.extend_from_slice(late);
    bytes.extend_from_slice(&base[end..]);
    expect_fault("late member", &bytes, ZipFault::MemberAfterCentralDirectory);

    // A ZIP64 locator without the ZIP64 end record before it.
    let base = sample();
    let end = rfind(&base, &END);
    let mut bytes = base[..end].to_vec();
    bytes.extend_from_slice(&[0x50, 0x4B, 0x06, 0x07]);
    bytes.extend_from_slice(&[0u8; 16]);
    bytes.extend_from_slice(&base[end..]);
    expect_fault("stray locator", &bytes, ZipFault::RecordOutOfSequence);
}

#[test]
fn malformed_extra_fields_fail() {
    let data = noise("els07/extra", 20 << 10);
    let mut bytes = archive(
        &[Member::stored("a.bin", &data).framed(Framing::Zip64 { descriptor: false })],
        Options::default(),
    );
    // The ZIP64 extra field claims more bytes than the extra area holds.
    let at = find(&bytes, &LOCAL) + 30 + "a.bin".len();
    put16(&mut bytes, at + 2, 200);
    expect_fault("extra overflow", &bytes, ZipFault::MalformedExtraField);
}

/// Once rejected, the chunker stays rejected.
#[test]
fn a_rejected_stream_stays_rejected() {
    let mut chunker = ZipChunker::new();
    let mut sink = |_: &[u8]| {};
    chunker.push(b"nope", &mut sink).expect_err("rejects");
    assert!(matches!(
        chunker.push(&LOCAL, &mut sink),
        Err(ChunkError::ProfileStreamRejected { .. })
    ));
    assert!(matches!(
        chunker.finish(&mut sink),
        Err(ChunkError::ProfileStreamRejected { .. })
    ));
}
