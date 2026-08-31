//! [`ZipEvents`] — the structural tap a walker caller may observe.
//!
//! `zip-v1` ignores the events; the Office profiles canonicalize and
//! validate through them. Every method has a no-op default, so the
//! tap costs nothing when unused.

use super::super::records::MemberSizes;

/// What the walker saw, as it saw it. Byte-level data arrives
/// per byte (the walker is a per-byte machine); observers buffer.
pub(crate) trait ZipEvents {
    /// A local file header completed: `name` bytes, the compression
    /// `method`, whether the general-purpose flags carried the
    /// UTF-8 bit, and the size claims (`None` for an unknown-size
    /// data-descriptor member).
    fn local_header(
        &mut self,
        name: &[u8],
        method: u16,
        utf8_flag: bool,
        encrypted: bool,
        sizes: Option<MemberSizes>,
        crc: u32,
    ) {
        let _ = (name, method, utf8_flag, encrypted, sizes, crc);
    }

    /// One member data byte (compressed bytes as stored).
    fn member_data(&mut self, byte: u8) {
        let _ = byte;
    }

    /// The member's data ended; `sizes` are the reconciled claims
    /// and `crc` the claimed CRC-32 of the uncompressed bytes
    /// (descriptor members learn both here).
    fn member_end(&mut self, sizes: MemberSizes, crc: u32) {
        let _ = (sizes, crc);
    }

    /// The central directory began: no further members follow.
    fn central_begun(&mut self) {}
}

/// The no-op tap `zip-v1` walks with.
pub(crate) struct NoEvents;

impl ZipEvents for NoEvents {}
