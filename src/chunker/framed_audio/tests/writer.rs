//! Fixture writers: MP3 frames from the header tables, ADTS frames,
//! FLAC files, and ID3 tags.

/// Deterministic pseudo-random bytes.
pub(super) fn noise(seed: &str, len: usize) -> Vec<u8> {
    // bounded: fixture payloads are test constants.
    let mut bytes = vec![0u8; len];
    let mut hasher = blake3::Hasher::new();
    hasher.update(seed.as_bytes());
    hasher.finalize_xof().fill(&mut bytes);
    bytes
}

/// One MPEG-1 Layer III frame at 128 kbit/s, 44.1 kHz: 417 bytes
/// (418 with padding).
pub(super) fn mp3_frame(seed: &str, index: usize, padding: bool) -> Vec<u8> {
    let length = if padding { 418 } else { 417 };
    let mut out = vec![0xFF, 0xFB, 0x90 | (u8::from(padding) << 1), 0x00];
    out.extend_from_slice(&noise(&format!("{seed}/frame{index}"), length - 4));
    out
}

/// A run of MP3 frames, padding on every third frame.
pub(super) fn mp3_frames(seed: &str, frames: usize) -> Vec<u8> {
    (0..frames)
        .flat_map(|index| mp3_frame(seed, index, index % 3 == 2))
        .collect()
}

/// One ADTS frame of `length` total bytes (7-byte header, no CRC).
pub(super) fn adts_frame(seed: &str, index: usize, length: usize) -> Vec<u8> {
    let mut out = vec![
        0xFF,
        0xF1,
        0x50,
        0x80 | ((length >> 11) & 0b11) as u8,
        ((length >> 3) & 0xFF) as u8,
        (((length & 0b111) << 5) | 0x1F) as u8,
        0xFC,
    ];
    out.extend_from_slice(&noise(&format!("{seed}/adts{index}"), length - 7));
    out
}

/// A run of ADTS frames with varying lengths.
pub(super) fn adts_frames(seed: &str, frames: usize) -> Vec<u8> {
    (0..frames)
        .flat_map(|index| adts_frame(seed, index, 300 + (index * 53) % 500))
        .collect()
}

/// An ID3v2.4 tag with a body of `body_len` noise bytes.
pub(super) fn id3v2(seed: &str, body_len: usize) -> Vec<u8> {
    let mut out = b"ID3\x04\x00\x00".to_vec();
    let size = body_len as u32;
    out.push(((size >> 21) & 0x7F) as u8);
    out.push(((size >> 14) & 0x7F) as u8);
    out.push(((size >> 7) & 0x7F) as u8);
    out.push((size & 0x7F) as u8);
    out.extend_from_slice(&noise(seed, body_len));
    out
}

/// The 128-byte `ID3v1` trailer.
pub(super) fn id3v1(seed: &str) -> Vec<u8> {
    let mut out = b"TAG".to_vec();
    out.extend_from_slice(&noise(seed, 125));
    out
}

/// One FLAC metadata block.
pub(super) fn flac_block(kind: u8, last: bool, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![(u8::from(last) << 7) | kind];
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes()[1..]);
    out.extend_from_slice(payload);
    out
}

/// A FLAC file: magic, STREAMINFO, an optional `VORBIS_COMMENT` of
/// `comment_len` bytes, and `audio_len` bytes of audio.
pub(super) fn flac(seed: &str, comment_len: usize, audio_len: usize) -> Vec<u8> {
    let mut out = b"fLaC".to_vec();
    let streaminfo = noise(&format!("{seed}/streaminfo"), 34);
    if comment_len == 0 {
        out.extend_from_slice(&flac_block(0, true, &streaminfo));
    } else {
        out.extend_from_slice(&flac_block(0, false, &streaminfo));
        out.extend_from_slice(&flac_block(
            4,
            true,
            &noise(&format!("{seed}/comment"), comment_len),
        ));
    }
    out.extend_from_slice(&noise(&format!("{seed}/audio"), audio_len));
    out
}
