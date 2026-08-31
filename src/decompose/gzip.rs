//! The gzip wrapper: header and trailer framing over the shared
//! raw-deflate inflater. Concatenated gzip members decompress to one
//! logical stream, per the format's semantics.

use std::collections::VecDeque;

use crate::inflate::{InflateFault, RawInflater};

use super::fault::OpaqueReason;

/// Longest FNAME field retained.
const NAME_MAX: usize = 4 << 10;

const FLAG_FHCRC: u8 = 1 << 1;
const FLAG_FEXTRA: u8 = 1 << 2;
const FLAG_FNAME: u8 = 1 << 3;
const FLAG_FCOMMENT: u8 = 1 << 4;

/// What the reader is collecting.
#[derive(Debug, Clone, Copy)]
enum State {
    /// Fixed 10-byte header; `len` collected.
    Header {
        len: usize,
    },
    /// FEXTRA: two length bytes then the payload.
    ExtraLen {
        len: usize,
    },
    Extra {
        remaining: usize,
    },
    /// NUL-terminated FNAME.
    Name,
    /// NUL-terminated FCOMMENT.
    Comment,
    /// FHCRC: two bytes.
    HeaderCrc {
        len: usize,
    },
    /// The deflate body.
    Body,
    /// The 8-byte CRC32 + ISIZE trailer.
    Trailer {
        len: usize,
    },
    /// After a trailer: end of input, or another concatenated
    /// member's header.
    Between,
}

/// The optional header parts, in wire order after the fixed header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Part {
    Fixed,
    Extra,
    Name,
    Comment,
}

/// Streaming gzip reader: push compressed bytes, drain decompressed
/// windows.
pub(super) struct GzipReader {
    state: State,
    header: [u8; 10],
    flags: u8,
    /// The first member's FNAME, if present.
    // bounded: NAME_MAX.
    name: Vec<u8>,
    first_member: bool,
    inflater: RawInflater,
    crc: crc32fast::Hasher,
    isize_acc: u64,
    scratch: [u8; 8],
    /// Decompressed output not yet taken.
    // bounded: drained by the caller after every push.
    pending: Vec<u8>,
    /// Bytes the inflater returned as trailing, awaiting re-entry.
    // bounded: at most one inflater stage.
    replay: VecDeque<u8>,
    offset: u64,
}

impl GzipReader {
    pub(super) fn new() -> Self {
        Self {
            state: State::Header { len: 0 },
            header: [0; 10],
            flags: 0,
            name: Vec::new(),
            first_member: true,
            inflater: RawInflater::new(),
            crc: crc32fast::Hasher::new(),
            isize_acc: 0,
            scratch: [0; 8],
            pending: Vec::new(),
            replay: VecDeque::new(),
            offset: 0,
        }
    }

    /// The first member's stored original name, if any.
    pub(super) fn stored_name(&self) -> Option<&[u8]> {
        if self.name.is_empty() {
            None
        } else {
            Some(&self.name)
        }
    }

    /// Decompressed bytes produced since the last take.
    pub(super) fn take_pending(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.pending)
    }

    /// Push one compressed byte.
    pub(super) fn push(&mut self, byte: u8) -> Result<(), OpaqueReason> {
        self.step(byte)?;
        self.offset += 1;
        while let Some(byte) = self.replay.pop_front() {
            self.step(byte)?;
        }
        Ok(())
    }

    /// The compressed input ended.
    pub(super) fn finish(&mut self) -> Result<(), OpaqueReason> {
        match self.state {
            State::Between => Ok(()),
            _ => Err(self.malformed("gzip stream is truncated")),
        }
    }

    fn malformed(&self, detail: &'static str) -> OpaqueReason {
        OpaqueReason::Malformed {
            detail,
            offset: self.offset,
        }
    }

    fn step(&mut self, byte: u8) -> Result<(), OpaqueReason> {
        match self.state {
            State::Header { len } => {
                self.header[len] = byte;
                if len + 1 < 10 {
                    self.state = State::Header { len: len + 1 };
                    return Ok(());
                }
                if self.header[0] != 0x1F || self.header[1] != 0x8B {
                    return Err(self.malformed("not a gzip header"));
                }
                if self.header[2] != 8 {
                    return Err(OpaqueReason::UnsupportedCompression);
                }
                self.flags = self.header[3];
                if self.flags & 0xE0 != 0 {
                    return Err(self.malformed("reserved gzip flag bits set"));
                }
                self.state = self.next_part(Part::Fixed);
                Ok(())
            }
            State::ExtraLen { len } => {
                self.scratch[len] = byte;
                if len + 1 < 2 {
                    self.state = State::ExtraLen { len: len + 1 };
                    return Ok(());
                }
                let total = usize::from(u16::from_le_bytes([self.scratch[0], self.scratch[1]]));
                self.state = if total == 0 {
                    self.next_part(Part::Extra)
                } else {
                    State::Extra { remaining: total }
                };
                Ok(())
            }
            State::Extra { remaining } => {
                self.state = if remaining == 1 {
                    self.next_part(Part::Extra)
                } else {
                    State::Extra {
                        remaining: remaining - 1,
                    }
                };
                Ok(())
            }
            State::Name => {
                if byte == 0 {
                    self.state = self.next_part(Part::Name);
                    return Ok(());
                }
                if self.first_member {
                    if self.name.len() >= NAME_MAX {
                        return Err(OpaqueReason::MetadataOverBound);
                    }
                    self.name.push(byte);
                }
                Ok(())
            }
            State::Comment => {
                if byte == 0 {
                    self.state = self.next_part(Part::Comment);
                }
                Ok(())
            }
            State::HeaderCrc { len } => {
                self.state = if len + 1 < 2 {
                    State::HeaderCrc { len: len + 1 }
                } else {
                    State::Body
                };
                Ok(())
            }
            State::Body => {
                if let Err(fault) = self.inflater.push(byte) {
                    return Err(self.inflate_fault(fault));
                }
                self.drain_inflater()?;
                if self.inflater.is_done() {
                    self.state = State::Trailer { len: 0 };
                    for byte in self.inflater.take_trailing() {
                        self.replay.push_back(byte);
                    }
                }
                Ok(())
            }
            State::Trailer { len } => {
                self.scratch[len] = byte;
                if len + 1 < 8 {
                    self.state = State::Trailer { len: len + 1 };
                    return Ok(());
                }
                let crc = u32::from_le_bytes([
                    self.scratch[0],
                    self.scratch[1],
                    self.scratch[2],
                    self.scratch[3],
                ]);
                let declared_isize = u32::from_le_bytes([
                    self.scratch[4],
                    self.scratch[5],
                    self.scratch[6],
                    self.scratch[7],
                ]);
                if crc != self.crc.clone().finalize() {
                    return Err(self.malformed("gzip CRC mismatch"));
                }
                #[allow(clippy::cast_possible_truncation)]
                let actual = self.isize_acc as u32;
                if declared_isize != actual {
                    return Err(self.malformed("gzip ISIZE mismatch"));
                }
                self.state = State::Between;
                Ok(())
            }
            State::Between => {
                // A concatenated member: reset per-member state and
                // re-enter the header with this byte.
                self.inflater = RawInflater::new();
                self.crc = crc32fast::Hasher::new();
                self.isize_acc = 0;
                self.first_member = false;
                self.state = State::Header { len: 0 };
                self.step(byte)
            }
        }
    }

    fn drain_inflater(&mut self) -> Result<(), OpaqueReason> {
        if let Err(fault) = self.inflater.drain_now() {
            return Err(self.inflate_fault(fault));
        }
        let out = self.inflater.take_pending();
        if !out.is_empty() {
            self.crc.update(&out);
            self.isize_acc += out.len() as u64;
            self.pending.extend_from_slice(&out);
        }
        Ok(())
    }

    fn inflate_fault(&self, fault: InflateFault) -> OpaqueReason {
        match fault {
            InflateFault::Malformed => self.malformed("gzip deflate body is malformed"),
            InflateFault::Geometry => self.malformed("gzip deflate body ends out of step"),
        }
    }

    /// The state after header part `done`: the first flagged
    /// optional part strictly after it, else the body.
    fn next_part(&self, done: Part) -> State {
        if done < Part::Extra && self.flags & FLAG_FEXTRA != 0 {
            return State::ExtraLen { len: 0 };
        }
        if done < Part::Name && self.flags & FLAG_FNAME != 0 {
            return State::Name;
        }
        if done < Part::Comment && self.flags & FLAG_FCOMMENT != 0 {
            return State::Comment;
        }
        if self.flags & FLAG_FHCRC != 0 {
            return State::HeaderCrc { len: 0 };
        }
        State::Body
    }
}
