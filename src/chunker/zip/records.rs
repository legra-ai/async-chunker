//! Fixed-layout ZIP records, parsed from their bytes after the
//! four-byte signature (APPNOTE 4.3).

use super::fault::ZipFault;

/// Record signatures, little-endian on the wire.
pub(super) const LOCAL_HEADER: [u8; 4] = [0x50, 0x4B, 0x03, 0x04];
pub(super) const CENTRAL_HEADER: [u8; 4] = [0x50, 0x4B, 0x01, 0x02];
pub(super) const END_OF_CENTRAL_DIRECTORY: [u8; 4] = [0x50, 0x4B, 0x05, 0x06];
pub(super) const ZIP64_END_OF_CENTRAL_DIRECTORY: [u8; 4] = [0x50, 0x4B, 0x06, 0x06];
pub(super) const ZIP64_LOCATOR: [u8; 4] = [0x50, 0x4B, 0x06, 0x07];
pub(super) const DATA_DESCRIPTOR: [u8; 4] = [0x50, 0x4B, 0x07, 0x08];

/// A 32-bit size field that defers to the ZIP64 extra field.
pub(super) const ZIP64_MARKER_32: u32 = u32::MAX;
/// A 16-bit count field that defers to the ZIP64 end record.
pub(super) const ZIP64_MARKER_16: u16 = u16::MAX;

/// Stored (no compression).
const METHOD_STORED: u16 = 0;
/// Deflate and Deflate64.
const METHOD_DEFLATE: u16 = 8;
const METHOD_DEFLATE64: u16 = 9;
/// The general-purpose flag announcing a trailing data descriptor.
const FLAG_DATA_DESCRIPTOR: u16 = 1 << 3;
/// Deflate cannot expand beyond 1032:1 (a 258-byte match costs at
/// least two bits); the slack covers tiny inputs.
const MAX_DEFLATE_RATIO: u64 = 1032;
const DEFLATE_RATIO_SLACK: u64 = 64;

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

pub(super) fn u64_at(bytes: &[u8], at: usize) -> u64 {
    let mut raw = [0u8; 8];
    raw.copy_from_slice(&bytes[at..at + 8]);
    u64::from_le_bytes(raw)
}

/// The size claims of one member, checked against the expansion
/// rules the profile enforces without inflating anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct MemberSizes {
    pub(super) compressed: u64,
    pub(super) uncompressed: u64,
}

impl MemberSizes {
    /// Reject size claims no real member of `method` can carry.
    pub(super) fn check(self, method: u16) -> Result<(), ZipFault> {
        match method {
            METHOD_STORED if self.compressed != self.uncompressed => {
                Err(ZipFault::StoredSizesDisagree)
            }
            METHOD_DEFLATE | METHOD_DEFLATE64
                if self.uncompressed
                    > self
                        .compressed
                        .saturating_mul(MAX_DEFLATE_RATIO)
                        .saturating_add(DEFLATE_RATIO_SLACK) =>
            {
                Err(ZipFault::ImplausibleExpansion)
            }
            _ => Ok(()),
        }
    }
}

/// The fixed part of a local file header (26 bytes after the
/// signature).
#[derive(Debug, Clone, Copy)]
pub(super) struct LocalHeader {
    pub(super) method: u16,
    pub(super) has_descriptor: bool,
    compressed: u32,
    uncompressed: u32,
    pub(super) name_len: u16,
    pub(super) extra_len: u16,
}

impl LocalHeader {
    pub(super) const FIXED_LEN: usize = 26;

    pub(super) fn parse(bytes: &[u8; Self::FIXED_LEN]) -> Self {
        Self {
            method: u16_at(bytes, 4),
            has_descriptor: u16_at(bytes, 2) & FLAG_DATA_DESCRIPTOR != 0,
            compressed: u32_at(bytes, 14),
            uncompressed: u32_at(bytes, 18),
            name_len: u16_at(bytes, 22),
            extra_len: u16_at(bytes, 24),
        }
    }

    /// Whether the sizes defer to a ZIP64 extra field.
    pub(super) fn needs_zip64(self) -> bool {
        self.compressed == ZIP64_MARKER_32 || self.uncompressed == ZIP64_MARKER_32
    }

    /// The member sizes, resolving ZIP64 markers through `extra`.
    pub(super) fn sizes(self, extra: &[u8]) -> Result<MemberSizes, ZipFault> {
        let mut sizes = MemberSizes {
            compressed: u64::from(self.compressed),
            uncompressed: u64::from(self.uncompressed),
        };
        if self.needs_zip64() {
            let zip64 = Zip64Extra::find(extra)?.ok_or(ZipFault::MissingZip64Sizes)?;
            let mut fields = zip64.fields();
            if self.uncompressed == ZIP64_MARKER_32 {
                sizes.uncompressed = fields.next().ok_or(ZipFault::MissingZip64Sizes)?;
            }
            if self.compressed == ZIP64_MARKER_32 {
                sizes.compressed = fields.next().ok_or(ZipFault::MissingZip64Sizes)?;
            }
        }
        Ok(sizes)
    }
}

/// The fixed part of a central directory header (42 bytes after the
/// signature).
#[derive(Debug, Clone, Copy)]
pub(super) struct CentralHeader {
    pub(super) method: u16,
    compressed: u32,
    uncompressed: u32,
    pub(super) name_len: u16,
    pub(super) extra_len: u16,
    pub(super) comment_len: u16,
    local_offset: u32,
}

impl CentralHeader {
    pub(super) const FIXED_LEN: usize = 42;

    pub(super) fn parse(bytes: &[u8; Self::FIXED_LEN]) -> Self {
        Self {
            method: u16_at(bytes, 6),
            compressed: u32_at(bytes, 16),
            uncompressed: u32_at(bytes, 20),
            name_len: u16_at(bytes, 24),
            extra_len: u16_at(bytes, 26),
            comment_len: u16_at(bytes, 28),
            local_offset: u32_at(bytes, 38),
        }
    }

    /// The entry's sizes and local-header offset, resolving ZIP64
    /// markers through `extra`.
    pub(super) fn resolve(self, extra: &[u8]) -> Result<(MemberSizes, u64), ZipFault> {
        let mut sizes = MemberSizes {
            compressed: u64::from(self.compressed),
            uncompressed: u64::from(self.uncompressed),
        };
        let mut offset = u64::from(self.local_offset);
        let deferred = self.compressed == ZIP64_MARKER_32
            || self.uncompressed == ZIP64_MARKER_32
            || self.local_offset == ZIP64_MARKER_32;
        if deferred {
            let zip64 = Zip64Extra::find(extra)?.ok_or(ZipFault::MissingZip64Sizes)?;
            let mut fields = zip64.fields();
            if self.uncompressed == ZIP64_MARKER_32 {
                sizes.uncompressed = fields.next().ok_or(ZipFault::MissingZip64Sizes)?;
            }
            if self.compressed == ZIP64_MARKER_32 {
                sizes.compressed = fields.next().ok_or(ZipFault::MissingZip64Sizes)?;
            }
            if self.local_offset == ZIP64_MARKER_32 {
                offset = fields.next().ok_or(ZipFault::MissingZip64Sizes)?;
            }
        }
        Ok((sizes, offset))
    }
}

/// The end-of-central-directory record (18 bytes after the
/// signature).
#[derive(Debug, Clone, Copy)]
pub(super) struct EndRecord {
    pub(super) entries_total: u16,
    pub(super) central_size: u32,
    pub(super) central_offset: u32,
    pub(super) comment_len: u16,
}

impl EndRecord {
    pub(super) const FIXED_LEN: usize = 18;

    pub(super) fn parse(bytes: &[u8; Self::FIXED_LEN]) -> Self {
        Self {
            entries_total: u16_at(bytes, 6),
            central_size: u32_at(bytes, 8),
            central_offset: u32_at(bytes, 12),
            comment_len: u16_at(bytes, 16),
        }
    }
}

/// The ZIP64 end-of-central-directory record (52 bytes after the
/// signature, then an extensible data sector).
#[derive(Debug, Clone, Copy)]
pub(super) struct Zip64EndRecord {
    pub(super) entries_total: u64,
    pub(super) central_size: u64,
    pub(super) central_offset: u64,
    /// Bytes of extensible data following the fixed part.
    pub(super) extensible_len: u64,
}

impl Zip64EndRecord {
    pub(super) const FIXED_LEN: usize = 52;
    /// The record's size field counts bytes after itself; the fixed
    /// fields it covers are 44 bytes.
    const COVERED_FIXED: u64 = 44;

    pub(super) fn parse(bytes: &[u8; Self::FIXED_LEN]) -> Result<Self, ZipFault> {
        let size = u64_at(bytes, 0);
        let extensible_len = size
            .checked_sub(Self::COVERED_FIXED)
            .ok_or(ZipFault::CentralDirectoryGeometry)?;
        Ok(Self {
            entries_total: u64_at(bytes, 28),
            central_size: u64_at(bytes, 36),
            central_offset: u64_at(bytes, 44),
            extensible_len,
        })
    }
}

/// The ZIP64 end-of-central-directory locator (16 bytes after the
/// signature); nothing in it steers a forward-only walk.
pub(super) const ZIP64_LOCATOR_FIXED_LEN: usize = 16;

/// The ZIP64 extended-information extra field (id `0x0001`): up to
/// three 64-bit values, present only for the 32-bit fields that
/// carry the marker, in the order uncompressed, compressed, offset.
pub(super) struct Zip64Extra<'a> {
    data: &'a [u8],
}

impl<'a> Zip64Extra<'a> {
    const ID: u16 = 0x0001;

    /// Locate the ZIP64 field inside an extra area, validating the
    /// area's `(id, size, data)*` layout along the way.
    pub(super) fn find(extra: &'a [u8]) -> Result<Option<Self>, ZipFault> {
        let mut at = 0usize;
        while at < extra.len() {
            if extra.len() - at < 4 {
                return Err(ZipFault::MalformedExtraField);
            }
            let id = u16_at(extra, at);
            let size = usize::from(u16_at(extra, at + 2));
            let start = at + 4;
            let end = start
                .checked_add(size)
                .filter(|&end| end <= extra.len())
                .ok_or(ZipFault::MalformedExtraField)?;
            if id == Self::ID {
                return Ok(Some(Self {
                    data: &extra[start..end],
                }));
            }
            at = end;
        }
        Ok(None)
    }

    /// The 64-bit values in field order.
    pub(super) fn fields(&self) -> impl Iterator<Item = u64> + '_ {
        self.data
            .chunks_exact(8)
            .map(|raw| u64::from_le_bytes(raw.try_into().expect("ZIP64 fields use 8-byte values")))
    }
}
