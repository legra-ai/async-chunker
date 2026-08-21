//! [`Walker`] — the forward-only framed-audio walker: sniff the
//! format, walk frame/tag/block headers, count payloads, never
//! decode.

use super::fault::AudioFault;
use super::flac::{self, FlacBlock};
use super::{adts, id3, mp3};

/// Longest fixed piece the walker collects: an `ID3v2` header.
const HEADER_MAX: usize = 10;
/// The `ID3v1` trailer's fixed length.
const TRAILER_LEN: usize = 128;

/// The frame family the stream locked into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Mp3,
    Adts,
    Flac,
}

/// What follows a bounded opaque region.
#[derive(Debug, Clone, Copy)]
enum After {
    /// Re-sniff (after a leading tag) or the next frame boundary.
    Boundary,
    /// The next FLAC metadata-block header.
    FlacMeta,
    /// The FLAC audio region.
    FlacAudio,
}

/// What the walker is collecting.
#[derive(Debug, Clone, Copy)]
enum State {
    /// At a frame boundary (or the very start), deciding what the
    /// next bytes are; `len` bytes collected.
    Sniff { len: usize },
    /// Collecting a ten-byte `ID3v2` header.
    Id3Header { len: usize },
    /// Collecting an MPEG (4-byte) or ADTS (7-byte) frame header.
    FrameHeader { len: usize },
    /// Counting a frame's payload.
    FramePayload { remaining: u64 },
    /// Collecting the 128-byte `ID3v1` trailer.
    Trailer { len: usize },
    /// Nothing may follow the trailer.
    Complete,
    /// Collecting a four-byte FLAC metadata-block header.
    FlacMetaHeader { len: usize },
    /// Counting a bounded opaque region (a tag body, a metadata
    /// payload).
    Opaque { remaining: u64, then: After },
    /// The FLAC audio region, running to the end of the stream.
    FlacAudio,
}

/// The walker. State is one small header buffer, the family, and the
/// offset.
pub(super) struct Walker {
    state: State,
    header: [u8; HEADER_MAX],
    family: Option<Family>,
    offset: u64,
    seen_frame: bool,
}

impl Walker {
    pub(super) fn new() -> Self {
        Self {
            state: State::Sniff { len: 0 },
            header: [0; HEADER_MAX],
            family: None,
            offset: 0,
            seen_frame: false,
        }
    }

    /// Bytes consumed so far.
    pub(super) const fn offset(&self) -> u64 {
        self.offset
    }

    /// Whether the next byte begins a unit — a frame, tag, trailer,
    /// or metadata block.
    pub(super) fn at_seam(&self) -> bool {
        matches!(
            self.state,
            State::Sniff { len: 0 } | State::FlacMetaHeader { len: 0 }
        )
    }

    /// Whether the walker is inside an opaque byte region (a tag
    /// body, a FLAC metadata payload, or the FLAC audio region),
    /// where per-byte content-defined cuts apply.
    pub(super) fn in_opaque(&self) -> bool {
        matches!(self.state, State::Opaque { .. } | State::FlacAudio)
    }

    /// Consume one byte.
    pub(super) fn consume(&mut self, byte: u8) -> Result<(), AudioFault> {
        let result = self.step(byte);
        self.offset += 1;
        result
    }

    /// The stream ended: a frame boundary of a stream that held
    /// frames, a completed trailer, or the FLAC audio region.
    pub(super) fn finish(&self) -> Result<(), AudioFault> {
        match self.state {
            State::Sniff { len: 0 } if self.seen_frame => Ok(()),
            State::Complete | State::FlacAudio => Ok(()),
            _ => Err(AudioFault::Truncated),
        }
    }

    fn step(&mut self, byte: u8) -> Result<(), AudioFault> {
        match self.state {
            State::Sniff { len } => self.sniff(byte, len),
            State::Id3Header { len } => {
                self.header[len] = byte;
                if len + 1 < HEADER_MAX {
                    self.state = State::Id3Header { len: len + 1 };
                    return Ok(());
                }
                let body = id3::body_len(&self.header)?;
                self.begin_opaque(body, After::Boundary);
                Ok(())
            }
            State::FrameHeader { len } => {
                self.header[len] = byte;
                let needed = match self.family {
                    Some(Family::Adts) => 7,
                    _ => 4,
                };
                if len + 1 < needed {
                    self.state = State::FrameHeader { len: len + 1 };
                    return Ok(());
                }
                let frame_len = match self.family {
                    Some(Family::Adts) => {
                        adts::frame_len(self.header[..7].try_into().expect("sized"))?
                    }
                    _ => mp3::frame_len(self.header[..4].try_into().expect("sized"))?,
                };
                self.seen_frame = true;
                let remaining = (frame_len - needed) as u64;
                self.state = if remaining == 0 {
                    State::Sniff { len: 0 }
                } else {
                    State::FramePayload { remaining }
                };
                Ok(())
            }
            State::FramePayload { remaining } => {
                let remaining = remaining - 1;
                self.state = if remaining == 0 {
                    State::Sniff { len: 0 }
                } else {
                    State::FramePayload { remaining }
                };
                Ok(())
            }
            State::Trailer { len } => {
                let expected = *b"TAG";
                if len < 3 && byte != expected[len] {
                    return Err(AudioFault::BadTag);
                }
                if len + 1 == TRAILER_LEN {
                    self.state = State::Complete;
                } else {
                    self.state = State::Trailer { len: len + 1 };
                }
                Ok(())
            }
            State::Complete => Err(AudioFault::TrailingBytes),
            State::FlacMetaHeader { len } => {
                self.header[len] = byte;
                if len + 1 < 4 {
                    self.state = State::FlacMetaHeader { len: len + 1 };
                    return Ok(());
                }
                let first = !self.seen_frame;
                let FlacBlock { last, length } =
                    flac::block(self.header[..4].try_into().expect("sized"), first)?;
                self.seen_frame = true;
                let then = if last {
                    After::FlacAudio
                } else {
                    After::FlacMeta
                };
                self.begin_opaque(length, then);
                Ok(())
            }
            State::Opaque { remaining, then } => {
                let remaining = remaining - 1;
                if remaining == 0 {
                    self.arrive(then);
                } else {
                    self.state = State::Opaque { remaining, then };
                }
                Ok(())
            }
            State::FlacAudio => Ok(()),
        }
    }

    /// Decide what the bytes at a boundary begin. Before any family
    /// is locked (`fLaC`, `ID3`, or a frame sync); afterwards a
    /// frame sync, another tag, or the `ID3v1` trailer.
    fn sniff(&mut self, byte: u8, len: usize) -> Result<(), AudioFault> {
        self.header[len] = byte;
        let bad = if self.family.is_none() {
            AudioFault::NotFramedAudio
        } else {
            AudioFault::BadFrameSync
        };
        match self.header[0] {
            0xFF => {
                if len == 0 {
                    self.state = State::Sniff { len: 1 };
                    return Ok(());
                }
                if byte & 0xE0 != 0xE0 {
                    return Err(AudioFault::BadFrameSync);
                }
                if self.family.is_none() {
                    let layer = (byte >> 1) & 0b11;
                    self.family = Some(if layer == 0 {
                        Family::Adts
                    } else {
                        Family::Mp3
                    });
                }
                // Both sync bytes already sit in the header buffer.
                self.state = State::FrameHeader { len: 2 };
                Ok(())
            }
            b'I' => {
                // "ID3" — the magic's remaining bytes are validated
                // by the header parser once all ten are in.
                self.state = if len + 1 < 3 {
                    State::Sniff { len: len + 1 }
                } else {
                    State::Id3Header { len: 3 }
                };
                Ok(())
            }
            b'T' if matches!(self.family, Some(Family::Mp3 | Family::Adts)) => {
                self.state = State::Trailer { len: 1 };
                Ok(())
            }
            b'f' if self.family.is_none() => {
                if len + 1 < 4 {
                    self.state = State::Sniff { len: len + 1 };
                    return Ok(());
                }
                if &self.header[..4] != b"fLaC" {
                    return Err(AudioFault::NotFramedAudio);
                }
                self.family = Some(Family::Flac);
                self.state = State::FlacMetaHeader { len: 0 };
                Ok(())
            }
            _ => Err(bad),
        }
    }

    fn begin_opaque(&mut self, remaining: u64, then: After) {
        if remaining == 0 {
            self.arrive(then);
        } else {
            self.state = State::Opaque { remaining, then };
        }
    }

    fn arrive(&mut self, after: After) {
        self.state = match after {
            After::Boundary => State::Sniff { len: 0 },
            After::FlacMeta => State::FlacMetaHeader { len: 0 },
            After::FlacAudio => State::FlacAudio,
        };
    }
}
