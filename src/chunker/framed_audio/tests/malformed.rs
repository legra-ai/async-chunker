//! The fail-hard corpus per format: bad syncs, reserved header
//! fields, free-format bitrates, bad lengths, malformed tags and
//! metadata blocks, trailers, and truncation.

use crate::ChunkError;

use super::writer::{adts_frame, adts_frames, flac, flac_block, id3v1, id3v2, mp3_frames, noise};
use crate::chunker::framed_audio::fault::AudioFault;
use crate::chunker::{ChunkBoundaries, Chunker, FramedAudioChunker};
use crate::profile::ChunkingProfile;

fn fault_of(bytes: &[u8]) -> Option<&'static str> {
    match ChunkBoundaries::of(ChunkingProfile::FramedAudioV1, bytes) {
        Ok(_) => None,
        Err(ChunkError::MalformedProfileInput {
            profile, detail, ..
        }) => {
            assert_eq!(profile, "framed-audio-v1");
            Some(detail)
        }
        Err(other) => panic!("unexpected error: {other}"),
    }
}

fn expect_fault(name: &str, bytes: &[u8], fault: AudioFault) {
    assert_eq!(fault_of(bytes), Some(fault.detail()), "{name}");
}

#[test]
fn unknown_leading_bytes_fail() {
    for (name, bytes) in [
        ("riff", b"RIFF....WAVE".to_vec()),
        ("ogg", b"OggS\0\0\0\0".to_vec()),
        ("leading trailer", id3v1("els11/lead")),
        ("fLaX", b"fLaX\0\0\0\0".to_vec()),
    ] {
        expect_fault(name, &bytes, AudioFault::NotFramedAudio);
    }
    expect_fault("empty", &[], AudioFault::Truncated);
}

#[test]
fn mid_stream_sync_loss_fails() {
    let mut bytes = mp3_frames("els11/sync", 40);
    // Corrupt the second frame's sync byte.
    bytes[417] = 0x00;
    expect_fault("bad boundary byte", &bytes, AudioFault::BadFrameSync);

    let mut bytes = mp3_frames("els11/sync2", 40);
    bytes[418] = 0x1B; // second sync byte loses its high bits
    expect_fault("bad second sync byte", &bytes, AudioFault::BadFrameSync);
}

#[test]
fn reserved_mp3_header_fields_fail() {
    let mut bytes = mp3_frames("els11/reserved", 10);
    bytes[1] = 0xEB; // version bits 01 (reserved)
    expect_fault("reserved version", &bytes, AudioFault::BadFrameHeader);

    let mut bytes = mp3_frames("els11/reserved2", 10);
    bytes[2] = 0xF0; // bitrate index 15
    expect_fault("bitrate 15", &bytes, AudioFault::BadFrameHeader);

    let mut bytes = mp3_frames("els11/reserved3", 10);
    bytes[2] = 0x9C; // samplerate index 3
    expect_fault("samplerate 3", &bytes, AudioFault::BadFrameHeader);

    let mut bytes = mp3_frames("els11/free", 10);
    bytes[2] = 0x00; // bitrate index 0: free format
    expect_fault("free format", &bytes, AudioFault::FreeFormatBitrate);
}

#[test]
fn bad_adts_fields_fail() {
    // Frame length below the CRC-present minimum.
    let mut bytes = adts_frame("els11/adts", 0, 300);
    bytes[1] &= !1; // protection present (CRC)
    bytes[3] = 0x80;
    bytes[4] = 1; // length 8 < 9
    bytes[5] = 0x1F;
    expect_fault(
        "length under CRC minimum",
        &bytes,
        AudioFault::BadFrameLength,
    );

    let mut bytes = adts_frame("els11/adts2", 0, 300);
    bytes[2] = 0xFC; // sampling-frequency index 15
    expect_fault("frequency index 15", &bytes, AudioFault::BadFrameHeader);
}

#[test]
fn malformed_tags_fail() {
    // A syncsafe size byte with its high bit set.
    let mut bytes = id3v2("els11/tag", 100);
    bytes[6] = 0x80;
    bytes.extend_from_slice(&mp3_frames("els11/tag", 5));
    expect_fault("non-syncsafe size", &bytes, AudioFault::BadTag);

    // "ID" followed by the wrong byte.
    let mut bytes = b"ID2".to_vec();
    bytes.extend_from_slice(&[0u8; 20]);
    expect_fault("broken magic", &bytes, AudioFault::BadTag);

    // A trailer with a broken magic.
    let mut bytes = mp3_frames("els11/trailer", 5);
    bytes.extend_from_slice(b"TAX");
    bytes.extend_from_slice(&[0u8; 125]);
    expect_fault("broken trailer magic", &bytes, AudioFault::BadTag);

    // Bytes after a complete trailer.
    let mut bytes = mp3_frames("els11/trailer2", 5);
    bytes.extend_from_slice(&id3v1("els11/trailer2"));
    bytes.push(0x00);
    expect_fault("bytes after trailer", &bytes, AudioFault::TrailingBytes);
}

#[test]
fn malformed_flac_metadata_fails() {
    // First block is not STREAMINFO.
    let mut bytes = b"fLaC".to_vec();
    bytes.extend_from_slice(&flac_block(4, true, &noise("els11/flacbad", 40)));
    expect_fault(
        "first block not streaminfo",
        &bytes,
        AudioFault::BadMetadataBlock,
    );

    // Invalid block type 127.
    let mut bytes = b"fLaC".to_vec();
    bytes.extend_from_slice(&flac_block(0, false, &noise("els11/flacbad2", 34)));
    bytes.extend_from_slice(&flac_block(127, true, &noise("els11/flacbad3", 10)));
    expect_fault("type 127", &bytes, AudioFault::BadMetadataBlock);
}

#[test]
fn truncation_fails_at_finish() {
    let bytes = mp3_frames("els11/trunc", 40);
    for (name, cut) in [
        ("mid first header", 2usize),
        ("mid frame", 200),
        ("one byte short", bytes.len() - 1),
        ("dangling sync byte", 417 + 1),
    ] {
        expect_fault(name, &bytes[..cut], AudioFault::Truncated);
    }

    // Mid `ID3v2` header and mid tag body.
    let tagged = {
        let mut out = id3v2("els11/trunc/tag", 1000);
        out.extend_from_slice(&mp3_frames("els11/trunc", 5));
        out
    };
    expect_fault("mid tag header", &tagged[..6], AudioFault::Truncated);
    expect_fault("mid tag body", &tagged[..300], AudioFault::Truncated);

    // Mid trailer.
    let mut with_trailer = mp3_frames("els11/trunc2", 5);
    with_trailer.extend_from_slice(&id3v1("els11/trunc2"));
    expect_fault(
        "mid trailer",
        &with_trailer[..with_trailer.len() - 10],
        AudioFault::Truncated,
    );

    // Mid FLAC metadata header and payload; audio may end anywhere.
    let flac_bytes = flac("els11/trunc/flac", 1000, 5000);
    expect_fault("mid flac magic", &flac_bytes[..2], AudioFault::Truncated);
    expect_fault(
        "mid flac block header",
        &flac_bytes[..6],
        AudioFault::Truncated,
    );
    expect_fault(
        "mid flac block payload",
        &flac_bytes[..20],
        AudioFault::Truncated,
    );
    assert_eq!(
        fault_of(&flac_bytes[..flac_bytes.len() - 100]),
        None,
        "audio may end anywhere"
    );

    // A truncated ADTS run.
    let adts = adts_frames("els11/trunc/adts", 20);
    expect_fault(
        "mid adts frame",
        &adts[..adts.len() - 5],
        AudioFault::Truncated,
    );
}

/// Once rejected, the chunker stays rejected.
#[test]
fn a_rejected_stream_stays_rejected() {
    let mut chunker = FramedAudioChunker::new();
    let mut sink = |_: &[u8]| {};
    chunker.push(b"OggS", &mut sink).expect_err("rejects");
    assert!(matches!(
        chunker.push(&mp3_frames("els11/after", 2), &mut sink),
        Err(ChunkError::ProfileStreamRejected { .. })
    ));
    assert!(matches!(
        chunker.finish(&mut sink),
        Err(ChunkError::ProfileStreamRejected { .. })
    ));
}
