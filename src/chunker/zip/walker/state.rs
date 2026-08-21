//! The walker's state vocabulary: phases, per-byte states, record
//! kinds, and the data-descriptor shape.

use super::super::records::{
    CentralHeader, EndRecord, LocalHeader, ZIP64_LOCATOR_FIXED_LEN, Zip64EndRecord, u64_at,
};

/// Where the walk stands in the end-of-archive sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    /// Local file headers and member data.
    Members,
    /// Central directory headers.
    Central,
    /// The ZIP64 end record has been read; the locator must follow.
    Zip64EndSeen,
    /// The ZIP64 locator has been read; the end record must follow.
    LocatorSeen,
    /// The end record and its comment have been read.
    Complete,
}

/// What the walker is collecting right now. Copy, so a step can
/// inspect the state by value and assign the next one.
#[derive(Clone, Copy)]
pub(super) enum State {
    /// Collecting a four-byte record signature.
    Signature { len: usize },
    /// Collecting the fixed part of a record of `kind`.
    Fixed { kind: Record, len: usize },
    /// Collecting the variable part (name, extra, comment) of a
    /// header.
    Variable { kind: Variable, total: usize },
    /// Counting member bytes of a known compressed size.
    Data {
        remaining: u64,
        total: u64,
        method: u16,
        descriptor: Option<DescriptorShape>,
    },
    /// Scanning member bytes of unknown size for the data
    /// descriptor that closes them.
    DataScan {
        consumed: u64,
        method: u16,
        zip64: bool,
        pending: usize,
    },
    /// Collecting the descriptor after a known-size member.
    Descriptor {
        shape: DescriptorShape,
        data_len: u64,
        method: u16,
        len: usize,
    },
    /// Skipping a known number of bytes (ZIP64 extensible data, the
    /// archive comment).
    Skip { remaining: u64, then: Phase },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Record {
    Local,
    Central,
    End,
    Zip64End,
    Zip64Locator,
}

#[derive(Clone, Copy)]
pub(super) enum Variable {
    Local(LocalHeader),
    Central(CentralHeader),
}

/// The descriptor that closes a known-size member: 64-bit sizes
/// when the header deferred to ZIP64.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DescriptorShape {
    pub(super) zip64: bool,
}

impl DescriptorShape {
    /// Bytes after the optional signature: CRC plus two sizes.
    pub(super) const fn body_len(self) -> usize {
        if self.zip64 { 4 + 16 } else { 4 + 8 }
    }

    /// The compressed size inside a descriptor body.
    pub(super) fn compressed(self, body: &[u8]) -> u64 {
        if self.zip64 {
            u64_at(body, 4)
        } else {
            u64::from(u32::from_le_bytes([body[4], body[5], body[6], body[7]]))
        }
    }

    pub(super) fn uncompressed(self, body: &[u8]) -> u64 {
        if self.zip64 {
            u64_at(body, 12)
        } else {
            u64::from(u32::from_le_bytes([body[8], body[9], body[10], body[11]]))
        }
    }
}

pub(super) const fn fixed_len(kind: Record) -> usize {
    match kind {
        Record::Local => LocalHeader::FIXED_LEN,
        Record::Central => CentralHeader::FIXED_LEN,
        Record::End => EndRecord::FIXED_LEN,
        Record::Zip64End => Zip64EndRecord::FIXED_LEN,
        Record::Zip64Locator => ZIP64_LOCATOR_FIXED_LEN,
    }
}
