use super::{Detection, Detector, ProfileSet};
use crate::constants::PROBE_PREFIX_MAX_BYTES;
use crate::error::ChunkError;
use crate::profile::ChunkingProfile;

fn detect(prefix: &[u8]) -> Detection {
    Detector::V1.detect(prefix)
}

fn ts_packets(count: usize) -> Vec<u8> {
    let mut packet = [0xFF_u8; 188];
    packet[0] = 0x47;
    packet[1] = 0x40;
    packet[2] = 0x11;
    packet[3] = 0x10;
    packet.repeat(count)
}

fn id3v2(body_len: u32) -> Vec<u8> {
    let mut header = b"ID3\x04\x00\x00".to_vec();
    for shift in [21, 14, 7, 0] {
        #[allow(clippy::cast_possible_truncation)]
        header.push(((body_len >> shift) & 0x7F) as u8);
    }
    header
}

#[test]
fn recognizes_each_specialist_signature() {
    let mut ftyp = 0x20_u32.to_be_bytes().to_vec();
    ftyp.extend_from_slice(b"ftypisom");
    let mut open_mdat = 0_u32.to_be_bytes().to_vec();
    open_mdat.extend_from_slice(b"mdat");
    let cases: [(&[u8], ChunkingProfile); 12] = [
        (b"PK\x03\x04\x14\x00", ChunkingProfile::ZipV1),
        (b"PK\x05\x06\x00\x00", ChunkingProfile::ZipV1),
        (&ftyp, ChunkingProfile::IsobmffV1),
        (&open_mdat, ChunkingProfile::IsobmffV1),
        (&[0x1A, 0x45, 0xDF, 0xA3, 0x9F], ChunkingProfile::MatroskaV1),
        (&ts_packets(2), ChunkingProfile::MpegtsV1),
        (&ts_packets(1), ChunkingProfile::MpegtsV1),
        (&id3v2(1000), ChunkingProfile::FramedAudioV1),
        (b"fLaC\x00\x00\x00\x22", ChunkingProfile::FramedAudioV1),
        (b"fLaC\x80\x00\x00\x22", ChunkingProfile::FramedAudioV1),
        (&[0xFF, 0xFB, 0x90, 0x00], ChunkingProfile::FramedAudioV1),
        (&[0xFF, 0xF1, 0x50, 0x80], ChunkingProfile::FramedAudioV1),
    ];
    for (prefix, expected) in cases {
        assert_eq!(
            detect(prefix),
            Detection::Recognized(expected),
            "{prefix:02X?}"
        );
    }
}

#[test]
fn recognizes_structured_text() {
    for text in [
        "hello\n",
        "{\"a\": 1}\r\n",
        "\u{FEFF}<?xml version=\"1.0\"?>",
        "tab\tform\x0Cfeed",
        "ünïcödé",
    ] {
        assert_eq!(
            detect(text.as_bytes()),
            Detection::Recognized(ChunkingProfile::StructuredTextV1),
            "{text:?}"
        );
    }
    let mut cut = "é".repeat(PROBE_PREFIX_MAX_BYTES).into_bytes();
    cut.truncate(PROBE_PREFIX_MAX_BYTES + 1);
    assert_eq!(
        detect(&cut),
        Detection::Recognized(ChunkingProfile::StructuredTextV1),
        "a scalar cut at the prefix bound is tolerated"
    );
}

#[test]
fn leaves_unsigned_bytes_unrecognized() {
    let cases: [&[u8]; 9] = [
        b"",
        b"\x89PNG\r\n\x1A\n",
        b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n",
        b"PK\x01\x02",
        b"text with \x00 nul",
        b"esc\x1B[0m",
        b"\xC0\xC1invalid utf-8",
        b"\x47\x00\x01\x02 one sync byte",
        &[0xFF, 0x1F, 0x00],
    ];
    for prefix in cases {
        assert_eq!(detect(prefix), Detection::Unrecognized, "{prefix:02X?}");
    }
    let mut short_cut = "é".repeat(4).into_bytes();
    short_cut.truncate(7);
    assert_eq!(
        detect(&short_cut),
        Detection::Unrecognized,
        "a scalar cut before the bound is a truncated stream, not text"
    );
    let mut single_and_more = ts_packets(1);
    single_and_more.push(0x00);
    assert_eq!(detect(&single_and_more), Detection::Unrecognized);
}

#[test]
fn reports_ambiguity_as_a_set() {
    let prefix = b"ID3 tag text";
    let mut expected = ProfileSet::single(ChunkingProfile::StructuredTextV1);
    expected.insert(ChunkingProfile::FramedAudioV1);
    assert_eq!(detect(prefix), Detection::Ambiguous(expected));
    assert_eq!(expected.len(), 2);
    assert_eq!(expected.to_string(), "structured-text-v1, framed-audio-v1");
    assert_eq!(
        detect(prefix).resolve(),
        Err(ChunkError::AmbiguousDetection {
            candidates: expected
        })
    );
}

#[test]
fn ignores_bytes_beyond_the_prefix_bound() {
    let mut prefix = b"plain text ".repeat(PROBE_PREFIX_MAX_BYTES);
    prefix.push(0x00);
    assert_eq!(
        detect(&prefix),
        Detection::Recognized(ChunkingProfile::StructuredTextV1)
    );
}

#[test]
fn resolves_and_reconciles() {
    assert_eq!(
        Detection::Unrecognized.resolve(),
        Ok(ChunkingProfile::GenericCdcV1)
    );
    assert_eq!(
        Detection::Recognized(ChunkingProfile::ZipV1).resolve(),
        Ok(ChunkingProfile::ZipV1)
    );

    let zip = Detection::Recognized(ChunkingProfile::ZipV1);
    assert_eq!(
        zip.reconcile(ChunkingProfile::ZipV1),
        Ok(ChunkingProfile::ZipV1)
    );
    assert_eq!(
        zip.reconcile(ChunkingProfile::GenericCdcV1),
        Ok(ChunkingProfile::GenericCdcV1),
        "a generic declaration makes no structural claim"
    );
    assert_eq!(
        zip.reconcile(ChunkingProfile::MatroskaV1),
        Err(ChunkError::DeclaredDetectedMismatch {
            declared: "matroska-v1",
            detected: ProfileSet::single(ChunkingProfile::ZipV1),
        })
    );
    assert_eq!(
        Detection::Unrecognized.reconcile(ChunkingProfile::MatroskaV1),
        Ok(ChunkingProfile::MatroskaV1),
        "the engine remains the authority on malformed input"
    );

    let mut both = ProfileSet::single(ChunkingProfile::StructuredTextV1);
    both.insert(ChunkingProfile::FramedAudioV1);
    let ambiguous = Detection::Ambiguous(both);
    assert_eq!(
        ambiguous.reconcile(ChunkingProfile::FramedAudioV1),
        Ok(ChunkingProfile::FramedAudioV1)
    );
    assert_eq!(
        ambiguous.reconcile(ChunkingProfile::ZipV1),
        Err(ChunkError::DeclaredDetectedMismatch {
            declared: "zip-v1",
            detected: both,
        })
    );
}

#[test]
fn profile_set_iterates_in_registry_order() {
    let mut set = ProfileSet::EMPTY;
    assert!(set.is_empty());
    set.insert(ChunkingProfile::FramedAudioV1);
    set.insert(ChunkingProfile::GenericCdcV1);
    set.insert(ChunkingProfile::IsobmffV1);
    assert_eq!(
        set.iter().collect::<Vec<_>>(),
        [
            ChunkingProfile::GenericCdcV1,
            ChunkingProfile::IsobmffV1,
            ChunkingProfile::FramedAudioV1
        ]
    );
    assert!(!set.contains(ChunkingProfile::ZipV1));
}

#[test]
fn v2_separates_ooxml_pdf_and_plain_zip() {
    let detector = Detector::V2;
    let mut ooxml = b"PK\x03\x04\x14\x00\x00\x00\x08\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x13\x00\x00\x00".to_vec();
    ooxml.extend_from_slice(b"[Content_Types].xml");
    assert_eq!(
        detector.detect(&ooxml),
        Detection::Recognized(ChunkingProfile::OoxmlV1)
    );
    let mut plain = ooxml.clone();
    plain[30] = b'x';
    assert_eq!(
        detector.detect(&plain),
        Detection::Recognized(ChunkingProfile::ZipV1)
    );
    assert_eq!(
        detector.detect(b"PK\x05\x06\x00\x00"),
        Detection::Recognized(ChunkingProfile::ZipV1)
    );
    assert_eq!(
        detector.detect(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n"),
        Detection::Recognized(ChunkingProfile::PdfV1)
    );
    assert_eq!(
        Detector::V1.detect(&ooxml),
        Detection::Recognized(ChunkingProfile::ZipV1),
        "v1 stays frozen"
    );
    assert_eq!(Detector::default().detect(&ooxml), detector.detect(&ooxml));
}
