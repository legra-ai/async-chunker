//! [`Walker`] — the struct, its public surface, and the per-byte
//! step that routes into the record, variable-part, and descriptor
//! handlers.

use super::super::fault::ZipFault;
use super::super::records::{MemberSizes, Zip64EndRecord};
use super::events::ZipEvents;
use super::state::{Phase, State, fixed_len};

/// Largest fixed record part the walker collects at once.
pub(super) const FIXED_MAX: usize = Zip64EndRecord::FIXED_LEN;
/// Longest data descriptor: signature + CRC + two 64-bit sizes.
const DESCRIPTOR_MAX: usize = 4 + 4 + 16;
const _: () = assert!(DESCRIPTOR_MAX <= FIXED_MAX);

/// The walker. Holds one small fixed buffer, one bounded variable
/// buffer (name + extra + comment, each at most 65 535 bytes), a few
/// counters — never the archive.
pub(crate) struct Walker {
    pub(super) state: State,
    pub(super) phase: Phase,
    /// Bytes consumed so far (the diagnostic offset).
    pub(super) offset: u64,
    pub(super) fixed: [u8; FIXED_MAX],
    // bounded: at most name + extra + comment of one header (≤ 3 × 65 535).
    pub(super) variable: Vec<u8>,
    pub(super) local_count: u64,
    pub(super) central_count: u64,
    pub(super) central_start: Option<u64>,
    pub(super) central_end: u64,
    pub(super) zip64_end: Option<Zip64EndRecord>,
}

impl Walker {
    pub(crate) fn new() -> Self {
        Self {
            state: State::Signature { len: 0 },
            phase: Phase::Members,
            offset: 0,
            fixed: [0; FIXED_MAX],
            variable: Vec::new(),
            local_count: 0,
            central_count: 0,
            central_start: None,
            central_end: 0,
            zip64_end: None,
        }
    }

    /// Bytes consumed so far.
    pub(crate) const fn offset(&self) -> u64 {
        self.offset
    }

    /// Whether the next byte begins a member (or the central
    /// directory) — the profile's cut candidate.
    pub(crate) fn at_member_boundary(&self) -> bool {
        matches!(self.state, State::Signature { len: 0 }) && self.phase == Phase::Members
    }

    /// Consume one byte, reporting structure to `events`. Returns
    /// the local header's length (fixed part, name, and extra) when
    /// this byte completes the header of a **large** member —
    /// compressed size at least the minimum chunk size — so the
    /// assembler can realign the chunk to the member.
    pub(crate) fn consume(
        &mut self,
        byte: u8,
        events: &mut dyn ZipEvents,
    ) -> Result<Option<usize>, ZipFault> {
        let result = self.step(byte, events);
        self.offset += 1;
        result
    }

    /// The stream ended: the archive must be complete.
    pub(crate) fn finish(&self) -> Result<(), ZipFault> {
        if self.phase == Phase::Complete && matches!(self.state, State::Signature { len: 0 }) {
            Ok(())
        } else {
            Err(ZipFault::Truncated)
        }
    }

    fn step(&mut self, byte: u8, events: &mut dyn ZipEvents) -> Result<Option<usize>, ZipFault> {
        match self.state {
            State::Signature { len } => {
                if self.phase == Phase::Complete {
                    return Err(ZipFault::TrailingBytes);
                }
                self.fixed[len] = byte;
                if len + 1 == 4 {
                    self.dispatch_signature(events)?;
                } else {
                    self.state = State::Signature { len: len + 1 };
                }
                Ok(None)
            }
            State::Fixed { kind, len } => {
                self.fixed[len] = byte;
                if len + 1 == fixed_len(kind) {
                    self.dispatch_fixed(kind, events)
                } else {
                    self.state = State::Fixed { kind, len: len + 1 };
                    Ok(None)
                }
            }
            State::Variable { kind, total } => {
                self.variable.push(byte);
                if self.variable.len() == total {
                    self.dispatch_variable(kind, events)
                } else {
                    Ok(None)
                }
            }
            State::Data {
                remaining,
                total,
                uncompressed,
                crc,
                method,
                descriptor,
            } => {
                events.member_data(byte);
                let remaining = remaining - 1;
                self.state = if remaining > 0 {
                    State::Data {
                        remaining,
                        total,
                        uncompressed,
                        crc,
                        method,
                        descriptor,
                    }
                } else {
                    match descriptor {
                        Some(shape) => State::Descriptor {
                            shape,
                            data_len: total,
                            method,
                            len: 0,
                        },
                        None => {
                            events.member_end(
                                MemberSizes {
                                    compressed: total,
                                    uncompressed,
                                },
                                crc,
                            );
                            State::Signature { len: 0 }
                        }
                    }
                };
                Ok(None)
            }
            State::DataScan {
                consumed,
                method,
                zip64,
                pending,
            } => self
                .scan(byte, consumed, method, zip64, pending, events)
                .map(|()| None),
            State::Descriptor {
                shape,
                data_len,
                method,
                len,
            } => {
                self.fixed[len] = byte;
                self.state = State::Descriptor {
                    shape,
                    data_len,
                    method,
                    len: len + 1,
                };
                self.try_close_descriptor(shape, data_len, method, len + 1, events)
                    .map(|()| None)
            }
            State::Skip { remaining, then } => {
                let remaining = remaining - 1;
                if remaining == 0 {
                    self.phase = then;
                    self.state = State::Signature { len: 0 };
                } else {
                    self.state = State::Skip { remaining, then };
                }
                Ok(None)
            }
        }
    }
}
