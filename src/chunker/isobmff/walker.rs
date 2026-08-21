//! [`Walker`] — the forward-only, bounded-depth ISO BMFF box walker.

use super::boxes::{BoxType, UUID, is_container, may_begin_stream};
use super::fault::BoxFault;
use crate::constants::GENERIC_CDC_CHUNK_MIN_BYTES;

/// Deepest container nesting the walker follows; real files nest
/// six or seven levels (`moov/trak/mdia/minf/stbl`).
const MAX_DEPTH: usize = 16;
/// Longest box header: size, type, largesize, usertype.
const HEADER_MAX: usize = 4 + 4 + 8 + 16;
/// The compact header: size and type.
const HEADER_MIN: usize = 8;

/// What the walker is collecting.
#[derive(Debug, Clone, Copy)]
enum State {
    /// Collecting a box header; `len` bytes so far.
    Header { len: usize },
    /// Counting an opaque payload.
    Payload { remaining: u64 },
    /// Counting an open-ended (`size == 0`) top-level payload to the
    /// end of the stream.
    PayloadToEnd,
}

/// The walker. State is the header buffer, one remaining-bytes
/// counter per open container, and the depth — never the stream.
pub(super) struct Walker {
    state: State,
    header: [u8; HEADER_MAX],
    /// Bytes left in each open container, innermost last. `None`
    /// marks an open-ended top-level container.
    // bounded: MAX_DEPTH entries.
    open: Vec<Option<u64>>,
    offset: u64,
    seen_first: bool,
}

impl Walker {
    pub(super) fn new() -> Self {
        Self {
            state: State::Header { len: 0 },
            header: [0; HEADER_MAX],
            open: Vec::with_capacity(MAX_DEPTH),
            offset: 0,
            seen_first: false,
        }
    }

    /// Bytes consumed so far.
    pub(super) const fn offset(&self) -> u64 {
        self.offset
    }

    /// Whether the next byte begins a box — the profile's cut
    /// candidate.
    pub(super) fn at_box_boundary(&self) -> bool {
        matches!(self.state, State::Header { len: 0 })
    }

    /// Consume one byte. Returns the header length when this byte
    /// completes the header of a **large** box — one whose payload
    /// is at least the minimum chunk size, or open-ended — so the
    /// assembler can realign the chunk to the box.
    pub(super) fn consume(&mut self, byte: u8) -> Result<Option<usize>, BoxFault> {
        let result = self.step(byte);
        self.offset += 1;
        result
    }

    /// The stream ended: no box may be open unless it was declared
    /// open-ended.
    pub(super) fn finish(&self) -> Result<(), BoxFault> {
        let at_boundary = matches!(self.state, State::Header { len: 0 });
        let closed = self.open.iter().all(Option::is_none);
        if (at_boundary && closed && self.seen_first) || matches!(self.state, State::PayloadToEnd) {
            Ok(())
        } else {
            Err(BoxFault::Truncated)
        }
    }

    fn step(&mut self, byte: u8) -> Result<Option<usize>, BoxFault> {
        match self.state {
            State::Header { len } => {
                self.header[len] = byte;
                let len = len + 1;
                self.state = State::Header { len };
                self.debit(1)?;
                if len >= HEADER_MIN && len == self.header_len() {
                    return self.open_box();
                }
                Ok(None)
            }
            State::Payload { remaining } => {
                self.debit(1)?;
                let remaining = remaining - 1;
                self.state = if remaining == 0 {
                    State::Header { len: 0 }
                } else {
                    State::Payload { remaining }
                };
                self.close_exhausted();
                Ok(None)
            }
            State::PayloadToEnd => Ok(None),
        }
    }

    /// The header length implied by the bytes collected so far.
    fn header_len(&self) -> usize {
        let size32 = u32::from_be_bytes([
            self.header[0],
            self.header[1],
            self.header[2],
            self.header[3],
        ]);
        let kind: BoxType = [
            self.header[4],
            self.header[5],
            self.header[6],
            self.header[7],
        ];
        let mut len = HEADER_MIN;
        if size32 == 1 {
            len += 8;
        }
        if kind == UUID {
            len += 16;
        }
        len
    }

    /// Charge `bytes` against every open container; a container that
    /// runs dry inside a header or before a child closes is an
    /// overrun.
    fn debit(&mut self, bytes: u64) -> Result<(), BoxFault> {
        for remaining in self.open.iter_mut().flatten() {
            if *remaining < bytes {
                return Err(BoxFault::ChildOverrunsParent);
            }
            *remaining -= bytes;
        }
        Ok(())
    }

    /// Pop containers whose payload is fully consumed, innermost
    /// first. Only meaningful at a box boundary.
    fn close_exhausted(&mut self) {
        if !matches!(self.state, State::Header { len: 0 }) {
            return;
        }
        while matches!(self.open.last(), Some(Some(0))) {
            self.open.pop();
        }
    }

    /// A complete header: validate the size against the header and
    /// the parent, then descend, count, or run to the end. Reports
    /// the header length of a large opaque box.
    fn open_box(&mut self) -> Result<Option<usize>, BoxFault> {
        let header_len = self.header_len() as u64;
        let size32 = u32::from_be_bytes([
            self.header[0],
            self.header[1],
            self.header[2],
            self.header[3],
        ]);
        let kind: BoxType = [
            self.header[4],
            self.header[5],
            self.header[6],
            self.header[7],
        ];
        if !self.seen_first {
            if !may_begin_stream(kind) {
                return Err(BoxFault::NotAnIsoBmffStream);
            }
            self.seen_first = true;
        }
        let size = match size32 {
            0 => None,
            1 => {
                let mut raw = [0u8; 8];
                raw.copy_from_slice(&self.header[8..16]);
                Some(u64::from_be_bytes(raw))
            }
            compact => Some(u64::from(compact)),
        };
        let payload = match size {
            None => {
                if !self.open.is_empty() {
                    return Err(BoxFault::OpenSizeNested);
                }
                None
            }
            Some(size) => {
                if size < header_len {
                    return Err(BoxFault::SizeBelowHeader);
                }
                let payload = size - header_len;
                if let Some(Some(remaining)) = self.open.last()
                    && payload > *remaining
                {
                    return Err(BoxFault::ChildOverrunsParent);
                }
                Some(payload)
            }
        };
        if is_container(kind) {
            if self.open.len() == MAX_DEPTH {
                return Err(BoxFault::DepthExceeded);
            }
            self.open.push(payload);
            self.state = State::Header { len: 0 };
            self.close_exhausted();
            return Ok(None);
        }
        let large = payload.is_none_or(|payload| payload >= GENERIC_CDC_CHUNK_MIN_BYTES as u64);
        self.state = match payload {
            None => State::PayloadToEnd,
            Some(0) => State::Header { len: 0 },
            Some(remaining) => State::Payload { remaining },
        };
        self.close_exhausted();
        Ok(large.then_some(header_len as usize))
    }
}
