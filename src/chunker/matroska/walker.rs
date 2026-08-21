//! [`Walker`] — the forward-only, bounded EBML/Matroska walker.
//!
//! It reads the stream exactly once: the EBML header, then one or
//! more `Segment`s, descending only into `Segment` itself and into
//! **unknown-size** `Cluster`s (whose children must be valid cluster
//! elements, and which close at the next segment-level element).
//! Every other element — known-size clusters above all — is opaque
//! payload that is counted, never decoded.

use super::elements::{CLUSTER, EBML_HEADER, SEGMENT, VOID, is_cluster_child, is_segment_level};
use super::fault::EbmlFault;
use super::varint;
use crate::constants::GENERIC_CDC_CHUNK_MIN_BYTES;

/// Longest element header: a four-byte ID and an eight-byte size.
const HEADER_MAX: usize = 4 + 8;

/// What kind of container is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Container {
    Segment,
    Cluster,
}

/// One open container and what it has left.
#[derive(Debug, Clone, Copy)]
struct Open {
    kind: Container,
    remaining: Option<u64>,
}

/// What the walker is collecting.
#[derive(Debug, Clone, Copy)]
enum State {
    /// Collecting an element ID; `len` bytes so far.
    Id { len: usize },
    /// Collecting the size varint; the ID sits in the header buffer.
    Size { len: usize },
    /// Counting an opaque payload.
    Payload { remaining: u64 },
}

/// The walker. State is one header buffer, the open-container stack
/// (at most a `Segment` and a `Cluster`), and the offset.
pub(super) struct Walker {
    state: State,
    header: [u8; HEADER_MAX],
    id_len: usize,
    size_len: usize,
    // bounded: at most one Segment and one Cluster.
    open: Vec<Open>,
    offset: u64,
    seen_header: bool,
    seen_segment: bool,
}

impl Walker {
    pub(super) fn new() -> Self {
        Self {
            state: State::Id { len: 0 },
            header: [0; HEADER_MAX],
            id_len: 0,
            size_len: 0,
            open: Vec::with_capacity(2),
            offset: 0,
            seen_header: false,
            seen_segment: false,
        }
    }

    /// Bytes consumed so far.
    pub(super) const fn offset(&self) -> u64 {
        self.offset
    }

    /// Whether the next byte begins a unit — a top-level element or
    /// a direct child of a `Segment`.
    pub(super) fn at_unit_boundary(&self) -> bool {
        matches!(self.state, State::Id { len: 0 })
            && self
                .open
                .last()
                .is_none_or(|open| open.kind == Container::Segment)
    }

    /// Consume one byte. Returns the header length when this byte
    /// completes the header of a **large** unit — a segment child
    /// whose payload is at least the minimum chunk size, or an
    /// unknown-size cluster — so the assembler can realign.
    pub(super) fn consume(&mut self, byte: u8) -> Result<Option<usize>, EbmlFault> {
        let result = self.step(byte);
        self.offset += 1;
        result
    }

    /// The stream ended: nothing but unknown-size containers may
    /// remain open, and a `Segment` must have been seen.
    pub(super) fn finish(&self) -> Result<(), EbmlFault> {
        let at_boundary = matches!(self.state, State::Id { len: 0 });
        let unbounded_only = self.open.iter().all(|open| open.remaining.is_none());
        if at_boundary && unbounded_only && self.seen_segment {
            Ok(())
        } else {
            Err(EbmlFault::Truncated)
        }
    }

    fn step(&mut self, byte: u8) -> Result<Option<usize>, EbmlFault> {
        match self.state {
            State::Id { len } => {
                if len == 0 {
                    self.id_len = varint::id_len(byte).ok_or(EbmlFault::InvalidId)?;
                }
                self.header[len] = byte;
                self.debit(1)?;
                if len + 1 == self.id_len {
                    self.state = State::Size { len: 0 };
                } else {
                    self.state = State::Id { len: len + 1 };
                }
                Ok(None)
            }
            State::Size { len } => {
                if len == 0 {
                    self.size_len = varint::size_len(byte).ok_or(EbmlFault::InvalidSize)?;
                }
                self.header[self.id_len + len] = byte;
                self.debit(1)?;
                if len + 1 == self.size_len {
                    self.open_element()
                } else {
                    self.state = State::Size { len: len + 1 };
                    Ok(None)
                }
            }
            State::Payload { remaining } => {
                self.debit(1)?;
                let remaining = remaining - 1;
                self.state = if remaining == 0 {
                    State::Id { len: 0 }
                } else {
                    State::Payload { remaining }
                };
                self.close_exhausted();
                Ok(None)
            }
        }
    }

    /// Charge `bytes` against every known-size open container.
    fn debit(&mut self, bytes: u64) -> Result<(), EbmlFault> {
        for open in &mut self.open {
            if let Some(remaining) = &mut open.remaining {
                if *remaining < bytes {
                    return Err(EbmlFault::ElementOverrunsParent);
                }
                *remaining -= bytes;
            }
        }
        Ok(())
    }

    /// Pop exhausted known-size containers, and unknown-size ones
    /// force-closed by an exhausted ancestor. Only at a boundary.
    fn close_exhausted(&mut self) {
        if !matches!(self.state, State::Id { len: 0 }) {
            return;
        }
        loop {
            let depth = self.open.len();
            if depth == 0 {
                return;
            }
            match self.open[depth - 1].remaining {
                Some(0) => {
                    self.open.pop();
                }
                None => {
                    let ancestor_exhausted = self.open[..depth - 1]
                        .iter()
                        .rev()
                        .find_map(|open| open.remaining)
                        .is_some_and(|remaining| remaining == 0);
                    if ancestor_exhausted {
                        self.open.pop();
                    } else {
                        return;
                    }
                }
                _ => return,
            }
        }
    }

    /// A complete element header: validate and dispatch.
    fn open_element(&mut self) -> Result<Option<usize>, EbmlFault> {
        let id = varint::id_value(&self.header[..self.id_len]);
        let (size, unknown) =
            varint::size_value(&self.header[self.id_len..self.id_len + self.size_len]);
        let header_len = self.id_len + self.size_len;
        let payload = (!unknown).then_some(size);
        if let Some(payload) = payload
            && let Some(remaining) = self.open.iter().rev().find_map(|open| open.remaining)
            && payload > remaining
        {
            return Err(EbmlFault::ElementOverrunsParent);
        }
        let context = self.open.last().map(|open| open.kind);
        match context {
            Some(Container::Cluster) => self.cluster_child(id, payload, header_len),
            Some(Container::Segment) => self.segment_child(id, payload, header_len),
            None => self.top_level(id, payload),
        }
    }

    fn top_level(&mut self, id: u32, payload: Option<u64>) -> Result<Option<usize>, EbmlFault> {
        if !self.seen_header {
            if id != EBML_HEADER {
                return Err(EbmlFault::NotMatroska);
            }
            let payload = payload.ok_or(EbmlFault::UnknownSizeForbidden)?;
            self.seen_header = true;
            self.begin_payload(payload);
            return Ok(None);
        }
        match id {
            SEGMENT => {
                self.seen_segment = true;
                self.open.push(Open {
                    kind: Container::Segment,
                    remaining: payload,
                });
                self.state = State::Id { len: 0 };
                self.close_exhausted();
                Ok(None)
            }
            VOID => {
                let payload = payload.ok_or(EbmlFault::UnknownSizeForbidden)?;
                self.begin_payload(payload);
                Ok(None)
            }
            _ => Err(EbmlFault::TopLevelElement),
        }
    }

    fn segment_child(
        &mut self,
        id: u32,
        payload: Option<u64>,
        header_len: usize,
    ) -> Result<Option<usize>, EbmlFault> {
        match payload {
            None => {
                if id != CLUSTER {
                    return Err(EbmlFault::UnknownSizeForbidden);
                }
                self.open.push(Open {
                    kind: Container::Cluster,
                    remaining: None,
                });
                self.state = State::Id { len: 0 };
                Ok(Some(header_len))
            }
            Some(payload) => {
                self.begin_payload(payload);
                Ok((payload >= GENERIC_CDC_CHUNK_MIN_BYTES as u64).then_some(header_len))
            }
        }
    }

    fn cluster_child(
        &mut self,
        id: u32,
        payload: Option<u64>,
        header_len: usize,
    ) -> Result<Option<usize>, EbmlFault> {
        if is_cluster_child(id) {
            let payload = payload.ok_or(EbmlFault::UnknownSizeForbidden)?;
            self.begin_payload(payload);
            return Ok(None);
        }
        if is_segment_level(id) {
            // The element closes the open cluster and belongs to the
            // segment.
            self.open.pop();
            return self.segment_child(id, payload, header_len);
        }
        Err(EbmlFault::UnexpectedClusterChild)
    }

    fn begin_payload(&mut self, payload: u64) {
        self.state = if payload == 0 {
            State::Id { len: 0 }
        } else {
            State::Payload { remaining: payload }
        };
        self.close_exhausted();
    }
}
