//! [`MemberDecoder`] — bounded streaming decode of one member's
//! bytes (stored passthrough or raw-deflate inflate), with CRC and
//! length accounting, over the crate's shared [`RawInflater`].

use super::super::fault::OfficeFault;
use crate::inflate::{InflateFault, RawInflater};

/// ZIP compression methods canonicalization accepts.
const METHOD_STORED: u16 = 0;
const METHOD_DEFLATE: u16 = 8;

/// Decodes one member; produced bytes are the member's canonical
/// bytes.
pub(super) struct MemberDecoder {
    kind: Kind,
    crc: crc32fast::Hasher,
    /// Stored-member bytes not yet drained.
    // bounded: drained after every consumed byte.
    stored_pending: Vec<u8>,
    produced: u64,
}

enum Kind {
    Stored,
    Deflate(RawInflater),
}

impl MemberDecoder {
    /// A decoder for one member of `method`.
    pub(super) fn new(method: u16) -> Result<Self, OfficeFault> {
        let kind = match method {
            METHOD_STORED => Kind::Stored,
            METHOD_DEFLATE => Kind::Deflate(RawInflater::new()),
            _ => return Err(OfficeFault::UnsupportedMethod),
        };
        Ok(Self {
            kind,
            crc: crc32fast::Hasher::new(),
            stored_pending: Vec::new(),
            produced: 0,
        })
    }

    /// Feed one stored/compressed byte.
    pub(super) fn push(&mut self, byte: u8) -> Result<(), OfficeFault> {
        match &mut self.kind {
            Kind::Stored => {
                self.crc.update(&[byte]);
                self.stored_pending.push(byte);
                self.produced += 1;
                Ok(())
            }
            Kind::Deflate(inflater) => {
                inflater.push(byte).map_err(map_fault)?;
                let out = inflater.take_pending();
                if !out.is_empty() {
                    self.crc.update(&out);
                    self.stored_pending.extend_from_slice(&out);
                }
                Ok(())
            }
        }
    }

    /// The member's compressed bytes ended: flush and return the
    /// decoded `(crc, length)`.
    pub(super) fn close(&mut self) -> Result<(u32, u64), OfficeFault> {
        if let Kind::Deflate(inflater) = &mut self.kind {
            inflater.close().map_err(map_fault)?;
            let out = inflater.take_pending();
            if !out.is_empty() {
                self.crc.update(&out);
                self.stored_pending.extend_from_slice(&out);
            }
        }
        Ok((self.crc.clone().finalize(), self.total()))
    }

    /// Canonical bytes produced since the last take.
    pub(super) fn take_pending(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.stored_pending)
    }

    fn total(&self) -> u64 {
        match &self.kind {
            Kind::Stored => self.produced,
            Kind::Deflate(inflater) => inflater.produced(),
        }
    }
}

const fn map_fault(fault: InflateFault) -> OfficeFault {
    match fault {
        InflateFault::Malformed => OfficeFault::MalformedDeflate,
        InflateFault::Geometry => OfficeFault::DeflateGeometry,
    }
}
