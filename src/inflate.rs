//! [`RawInflater`] — the crate's one streaming raw-deflate decoder,
//! shared by the Office member decoder and the gzip wrapper.

use miniz_oxide::inflate::stream::{InflateState, inflate};
use miniz_oxide::{DataFormat, MZError, MZFlush, MZStatus};

/// Compressed bytes staged before each bulk inflate call.
const STAGE_BYTES: usize = 4 << 10;
/// Inflate output window per call.
const OUT_BYTES: usize = 16 << 10;

/// Why the inflater rejected the stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InflateFault {
    /// The deflate bit stream is malformed.
    Malformed,
    /// The stream ended out of step with its bytes: compressed bytes
    /// after the deflate stream ended, or an end-of-input inside it.
    Geometry,
}

/// Streaming raw-deflate decoder: push compressed bytes, drain
/// decompressed windows. Bounded: one input stage and one output
/// window.
pub(crate) struct RawInflater {
    state: Box<InflateState>,
    // bounded: STAGE_BYTES.
    staged: Vec<u8>,
    /// Decompressed bytes produced and not yet taken.
    // bounded: at most one output window per drain cycle plus the
    // consumer's takes between pushes.
    pending: Vec<u8>,
    produced: u64,
    done: bool,
    /// Compressed bytes consumed by the deflate stream itself.
    consumed: u64,
}

impl RawInflater {
    pub(crate) fn new() -> Self {
        Self {
            state: InflateState::new_boxed(DataFormat::Raw),
            staged: Vec::with_capacity(STAGE_BYTES),
            pending: Vec::new(),
            produced: 0,
            done: false,
            consumed: 0,
        }
    }

    /// Whether the deflate stream reached its end marker.
    pub(crate) const fn is_done(&self) -> bool {
        self.done
    }

    /// Decompressed length so far.
    pub(crate) const fn produced(&self) -> u64 {
        self.produced
    }

    /// Push one compressed byte. After the stream has ended
    /// ([`Self::is_done`]) pushing is a fault — the caller owns any
    /// trailing bytes.
    pub(crate) fn push(&mut self, byte: u8) -> Result<(), InflateFault> {
        if self.done {
            return Err(InflateFault::Geometry);
        }
        self.staged.push(byte);
        if self.staged.len() >= STAGE_BYTES {
            self.drain(MZFlush::None)?;
        }
        Ok(())
    }

    /// The compressed input ended: flush; the stream must have
    /// reached its end marker.
    pub(crate) fn close(&mut self) -> Result<(), InflateFault> {
        if !self.done {
            self.drain(MZFlush::Finish)?;
        }
        if !self.done {
            return Err(InflateFault::Geometry);
        }
        if !self.staged.is_empty() {
            return Err(InflateFault::Geometry);
        }
        Ok(())
    }

    /// Force a drain so `take_pending` sees everything decodable and
    /// `is_done` reflects an end marker inside the staged bytes.
    /// Returns the number of staged bytes NOT consumed by the
    /// stream (trailing bytes after its end).
    pub(crate) fn drain_now(&mut self) -> Result<usize, InflateFault> {
        if !self.done {
            self.drain(MZFlush::None)?;
        }
        Ok(self.staged.len())
    }

    /// Take the staged bytes the ended stream did not consume.
    pub(crate) fn take_trailing(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.staged)
    }

    /// Decompressed bytes produced since the last take.
    pub(crate) fn take_pending(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.pending)
    }

    fn drain(&mut self, flush: MZFlush) -> Result<(), InflateFault> {
        let mut out = [0u8; OUT_BYTES];
        let mut consumed = 0usize;
        loop {
            if self.done {
                break;
            }
            let result = inflate(&mut self.state, &self.staged[consumed..], &mut out, flush);
            consumed += result.bytes_consumed;
            if result.bytes_written > 0 {
                self.pending.extend_from_slice(&out[..result.bytes_written]);
                self.produced += result.bytes_written as u64;
            }
            match result.status {
                Ok(MZStatus::StreamEnd) => self.done = true,
                Ok(_) => {
                    let stalled = result.bytes_consumed == 0 && result.bytes_written == 0;
                    if consumed >= self.staged.len() && (flush == MZFlush::None || stalled) {
                        if flush == MZFlush::Finish && stalled {
                            return Err(InflateFault::Geometry);
                        }
                        break;
                    }
                }
                Err(MZError::Buf) => {
                    if flush == MZFlush::Finish {
                        return Err(InflateFault::Geometry);
                    }
                    break;
                }
                Err(_) => return Err(InflateFault::Malformed),
            }
        }
        self.consumed += consumed as u64;
        self.staged.drain(..consumed);
        Ok(())
    }
}
