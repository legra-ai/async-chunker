//! The fail-hard corpus: bad sync, partial packets, reserved and
//! malformed adaptation fields.

use crate::ChunkError;

use super::writer::{Packet, noise, stream};
use crate::chunker::mpegts::fault::TsFault;
use crate::chunker::mpegts::packet::PACKET_LEN;
use crate::chunker::{ChunkBoundaries, Chunker, MpegtsChunker};
use crate::profile::ChunkingProfile;

fn fault_of(bytes: &[u8]) -> Option<&'static str> {
    match ChunkBoundaries::of(ChunkingProfile::MpegtsV1, bytes) {
        Ok(_) => None,
        Err(ChunkError::MalformedProfileInput {
            profile, detail, ..
        }) => {
            assert_eq!(profile, "mpegts-v1");
            Some(detail)
        }
        Err(other) => panic!("unexpected error: {other}"),
    }
}

fn expect_fault(name: &str, bytes: &[u8], fault: TsFault) {
    assert_eq!(fault_of(bytes), Some(fault.detail()), "{name}");
}

#[test]
fn bad_sync_fails_first_and_mid_stream() {
    let mut bytes = stream("els10/sync", 200, 20);
    bytes[0] = 0x48;
    expect_fault("first packet", &bytes, TsFault::BadSync);

    let mut bytes = stream("els10/sync2", 200, 20);
    bytes[100 * PACKET_LEN] = 0x00;
    expect_fault("mid-stream", &bytes, TsFault::BadSync);

    // The profile never resynchronizes: one shifted byte fails.
    let mut shifted = vec![0xFFu8];
    shifted.extend_from_slice(&stream("els10/sync3", 50, 10));
    expect_fault("shifted stream", &shifted, TsFault::BadSync);
}

#[test]
fn reserved_adaptation_control_fails() {
    let mut bytes = stream("els10/reserved", 50, 10);
    bytes[3] &= 0b1100_1111; // control 00
    expect_fault("control 00", &bytes, TsFault::ReservedAdaptationControl);
}

#[test]
fn malformed_adaptation_fields_fail() {
    // Control 11 with a length leaving no payload byte.
    let packet = Packet {
        pid: 0x100,
        unit_start: false,
        counter: 0,
        adaptation: Some(183),
        discontinuity: false,
    };
    let mut bytes = packet.render(&noise("els10/af", 184)).to_vec();
    bytes[4] = 183;
    expect_fault(
        "length 183 under control 11",
        &bytes,
        TsFault::MalformedAdaptationField,
    );

    // Control 10 (adaptation only) may fill the remainder (183)…
    let mut ok = packet.render(&noise("els10/af2", 184)).to_vec();
    ok[3] = (0b10 << 4) | (ok[3] & 0x0F);
    ok[4] = 183;
    for slot in ok[5..].iter_mut() {
        *slot = 0;
    }
    assert_eq!(fault_of(&ok), None, "length 183 under control 10 is legal");
    // …but not more.
    let mut bytes = ok;
    bytes[4] = 184;
    expect_fault(
        "length 184 under control 10",
        &bytes,
        TsFault::MalformedAdaptationField,
    );
}

#[test]
fn partial_and_empty_streams_fail() {
    let bytes = stream("els10/partial", 40, 10);
    expect_fault(
        "trailing partial packet",
        &bytes[..bytes.len() - 1],
        TsFault::PartialPacket,
    );
    expect_fault("one byte", &bytes[..1], TsFault::PartialPacket);
    expect_fault("empty", &[], TsFault::Empty);
}

/// Once rejected, the chunker stays rejected.
#[test]
fn a_rejected_stream_stays_rejected() {
    let mut chunker = MpegtsChunker::new();
    let mut sink = |_: &[u8]| {};
    chunker
        .push(&[0u8; PACKET_LEN], &mut sink)
        .expect_err("rejects");
    assert!(matches!(
        chunker.push(&stream("els10/after", 2, 1), &mut sink),
        Err(ChunkError::ProfileStreamRejected { .. })
    ));
    assert!(matches!(
        chunker.finish(&mut sink),
        Err(ChunkError::ProfileStreamRejected { .. })
    ));
}
