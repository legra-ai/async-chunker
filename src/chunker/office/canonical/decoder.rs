//! [`MemberDecoder`] — bounded streaming decode of one member's
//! bytes (stored passthrough or raw-deflate inflate), with CRC and
//! length accounting.

use miniz_oxide::inflate::stream::{InflateState, inflate};
use miniz_oxide::{DataFormat, MZError, MZFlush, MZStatus};

use super::super::fault::OfficeFault;

/// Compressed bytes staged before each bulk inflate call.
const STAGE_BYTES: usize = 4 << 10;
/// Inflate output window per call.
const OUT_BYTES: usize = 16 << 10;

/// ZIP compression methods canonicalization accepts.
const METHOD_STORED: u16 = 0;
const METHOD_DEFLATE: u16 = 8;

/// Decodes one member; produced bytes are the member's canonical
/// bytes.
pub(super) struct MemberDecoder {
    kind: Kind,
    crc: crc32fast::Hasher,
    /// Canonical bytes produced and not yet drained.
    // bounded: at most one inflate output window per drain cycle.
    pending: Vec<u8>,
    produced: u64,
}

enum Kind {
    Stored,
    Deflate {
        state: Box<InflateState>,
        // bounded: STAGE_BYTES.
        staged: Vec<u8>,
        done: bool,
    },
}

impl MemberDecoder {
    /// A decoder for one member of `method`.
    pub(super) fn new(method: u16) -> Result<Self, OfficeFault> {
        let kind = match method {
            METHOD_STORED => Kind::Stored,
            METHOD_DEFLATE => Kind::Deflate {
                state: InflateState::new_boxed(DataFormat::Raw),
                staged: Vec::with_capacity(STAGE_BYTES),
                done: false,
            },
            _ => return Err(OfficeFault::UnsupportedMethod),
        };
        Ok(Self {
            kind,
            crc: crc32fast::Hasher::new(),
            pending: Vec::new(),
            produced: 0,
        })
    }

    /// Feed one stored/compressed byte.
    pub(super) fn push(&mut self, byte: u8) -> Result<(), OfficeFault> {
        match &mut self.kind {
            Kind::Stored => {
                self.crc.update(&[byte]);
                self.pending.push(byte);
                self.produced += 1;
                Ok(())
            }
            Kind::Deflate { staged, .. } => {
                staged.push(byte);
                if staged.len() >= STAGE_BYTES {
                    self.drain(MZFlush::None)?;
                }
                Ok(())
            }
        }
    }

    /// The member's compressed bytes ended: flush and return the
    /// decoded `(crc, length)`.
    pub(super) fn close(&mut self) -> Result<(u32, u64), OfficeFault> {
        if let Kind::Deflate { done, .. } = &self.kind {
            let finished = *done;
            if !finished {
                self.drain(MZFlush::Finish)?;
            }
            if let Kind::Deflate { done: false, .. } = &self.kind {
                return Err(OfficeFault::DeflateGeometry);
            }
        }
        Ok((self.crc.clone().finalize(), self.produced))
    }

    /// Canonical bytes produced since the last take.
    pub(super) fn take_pending(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.pending)
    }

    fn drain(&mut self, flush: MZFlush) -> Result<(), OfficeFault> {
        let Kind::Deflate {
            state,
            staged,
            done,
        } = &mut self.kind
        else {
            return Ok(());
        };
        let mut out = [0u8; OUT_BYTES];
        let mut consumed = 0usize;
        loop {
            if *done {
                // Compressed bytes after the deflate stream ended.
                if consumed < staged.len() {
                    return Err(OfficeFault::DeflateGeometry);
                }
                break;
            }
            let result = inflate(state, &staged[consumed..], &mut out, flush);
            consumed += result.bytes_consumed;
            if result.bytes_written > 0 {
                let bytes = &out[..result.bytes_written];
                self.crc.update(bytes);
                self.pending.extend_from_slice(bytes);
                self.produced += result.bytes_written as u64;
            }
            match result.status {
                Ok(MZStatus::StreamEnd) => *done = true,
                Ok(_) => {
                    let stalled = result.bytes_consumed == 0 && result.bytes_written == 0;
                    if consumed >= staged.len() && (flush == MZFlush::None || stalled) {
                        if flush == MZFlush::Finish && stalled {
                            return Err(OfficeFault::DeflateGeometry);
                        }
                        break;
                    }
                }
                Err(MZError::Buf) => {
                    if flush == MZFlush::Finish {
                        return Err(OfficeFault::DeflateGeometry);
                    }
                    break;
                }
                Err(_) => return Err(OfficeFault::MalformedDeflate),
            }
        }
        staged.clear();
        Ok(())
    }
}
