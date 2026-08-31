//! The canonical package emitter: deterministic ZIP records for the
//! `ooxml-v1` canonical form.
//!
//! Frozen canonical form v1: members in original order with original
//! name bytes; method STORE; timestamps, internal and external
//! attributes, and extra fields zeroed (except the ZIP64 sizes a
//! large member requires); known-size members carry their sizes and
//! CRC in the local header, unknown-size members use a signed 32-bit
//! data descriptor; the central directory and end records follow the
//! same zeroing, with ZIP64 records only when a count, size, or
//! offset requires them.

/// One member's central-directory facts.
pub(crate) struct CentralEntry {
    pub(crate) name: Box<[u8]>,
    pub(crate) utf8: bool,
    pub(crate) crc: u32,
    pub(crate) len: u64,
    pub(crate) offset: u64,
    pub(crate) descriptor: bool,
}

const FLAG_DESCRIPTOR: u16 = 1 << 3;
const FLAG_UTF8: u16 = 1 << 11;
/// Version 4.5 (ZIP64) when needed, else 2.0.
const VERSION_ZIP64: u16 = 45;
const VERSION_PLAIN: u16 = 20;

fn put16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

const fn needs_zip64_len(len: u64) -> bool {
    len >= u32::MAX as u64
}

/// The canonical local header for a member whose decoded size and
/// CRC are already known (the normal Office case).
pub(crate) fn local_header_known(name: &[u8], utf8: bool, crc: u32, len: u64) -> Vec<u8> {
    let zip64 = needs_zip64_len(len);
    let mut out = Vec::with_capacity(30 + name.len() + 20);
    out.extend_from_slice(&[0x50, 0x4B, 0x03, 0x04]);
    put16(&mut out, if zip64 { VERSION_ZIP64 } else { VERSION_PLAIN });
    put16(&mut out, if utf8 { FLAG_UTF8 } else { 0 });
    put16(&mut out, 0); // method: STORE
    put16(&mut out, 0); // time
    put16(&mut out, 0); // date
    put32(&mut out, crc);
    if zip64 {
        put32(&mut out, u32::MAX);
        put32(&mut out, u32::MAX);
    } else {
        #[allow(clippy::cast_possible_truncation)]
        put32(&mut out, len as u32);
        #[allow(clippy::cast_possible_truncation)]
        put32(&mut out, len as u32);
    }
    put16(&mut out, name.len() as u16);
    put16(&mut out, if zip64 { 20 } else { 0 });
    out.extend_from_slice(name);
    if zip64 {
        put16(&mut out, 0x0001);
        put16(&mut out, 16);
        put64(&mut out, len);
        put64(&mut out, len);
    }
    out
}

/// The canonical local header for an unknown-size member: zero
/// sizes, descriptor flag; the descriptor closes it.
pub(crate) fn local_header_unknown(name: &[u8], utf8: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(30 + name.len());
    out.extend_from_slice(&[0x50, 0x4B, 0x03, 0x04]);
    put16(&mut out, VERSION_PLAIN);
    put16(&mut out, FLAG_DESCRIPTOR | if utf8 { FLAG_UTF8 } else { 0 });
    put16(&mut out, 0);
    put16(&mut out, 0);
    put16(&mut out, 0);
    put32(&mut out, 0);
    put32(&mut out, 0);
    put32(&mut out, 0);
    put16(&mut out, name.len() as u16);
    put16(&mut out, 0);
    out.extend_from_slice(name);
    out
}

/// The signed 32-bit data descriptor closing an unknown-size member.
pub(crate) fn descriptor(crc: u32, len: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(16);
    out.extend_from_slice(&[0x50, 0x4B, 0x07, 0x08]);
    put32(&mut out, crc);
    put32(&mut out, len);
    put32(&mut out, len);
    out
}

fn central_header(entry: &CentralEntry) -> Vec<u8> {
    let zip64_len = needs_zip64_len(entry.len);
    let zip64_offset = entry.offset >= u64::from(u32::MAX);
    let zip64 = zip64_len || zip64_offset;
    let extra_len = if zip64 {
        4 + if zip64_len { 16 } else { 0 } + if zip64_offset { 8 } else { 0 }
    } else {
        0
    };
    let mut out = Vec::with_capacity(46 + entry.name.len() + extra_len);
    out.extend_from_slice(&[0x50, 0x4B, 0x01, 0x02]);
    let version = if zip64 { VERSION_ZIP64 } else { VERSION_PLAIN };
    put16(&mut out, version);
    put16(&mut out, version);
    let mut flags = if entry.utf8 { FLAG_UTF8 } else { 0 };
    if entry.descriptor {
        flags |= FLAG_DESCRIPTOR;
    }
    put16(&mut out, flags);
    put16(&mut out, 0); // method: STORE
    put16(&mut out, 0);
    put16(&mut out, 0);
    put32(&mut out, entry.crc);
    if zip64_len {
        put32(&mut out, u32::MAX);
        put32(&mut out, u32::MAX);
    } else {
        #[allow(clippy::cast_possible_truncation)]
        put32(&mut out, entry.len as u32);
        #[allow(clippy::cast_possible_truncation)]
        put32(&mut out, entry.len as u32);
    }
    put16(&mut out, entry.name.len() as u16);
    put16(&mut out, extra_len as u16);
    put16(&mut out, 0); // comment
    put16(&mut out, 0); // disk
    put16(&mut out, 0); // internal attributes
    put32(&mut out, 0); // external attributes
    if zip64_offset {
        put32(&mut out, u32::MAX);
    } else {
        #[allow(clippy::cast_possible_truncation)]
        put32(&mut out, entry.offset as u32);
    }
    out.extend_from_slice(&entry.name);
    if zip64 {
        put16(&mut out, 0x0001);
        #[allow(clippy::cast_possible_truncation)]
        put16(&mut out, (extra_len - 4) as u16);
        if zip64_len {
            put64(&mut out, entry.len);
            put64(&mut out, entry.len);
        }
        if zip64_offset {
            put64(&mut out, entry.offset);
        }
    }
    out
}

/// The canonical central directory and end records, given the
/// members and the canonical offset where the directory begins.
pub(crate) fn tail(entries: &[CentralEntry], central_offset: u64) -> Vec<u8> {
    let mut out = Vec::new();
    for entry in entries {
        let header = central_header(entry);
        out.extend_from_slice(&header);
    }
    let central_size = out.len() as u64;
    let count = entries.len() as u64;
    let zip64 = count >= u64::from(u16::MAX)
        || central_size >= u64::from(u32::MAX)
        || central_offset >= u64::from(u32::MAX)
        || entries
            .iter()
            .any(|entry| needs_zip64_len(entry.len) || entry.offset >= u64::from(u32::MAX));
    if zip64 {
        let zip64_end_offset = central_offset + central_size;
        out.extend_from_slice(&[0x50, 0x4B, 0x06, 0x06]);
        put64(&mut out, 44);
        put16(&mut out, VERSION_ZIP64);
        put16(&mut out, VERSION_ZIP64);
        put32(&mut out, 0);
        put32(&mut out, 0);
        put64(&mut out, count);
        put64(&mut out, count);
        put64(&mut out, central_size);
        put64(&mut out, central_offset);
        out.extend_from_slice(&[0x50, 0x4B, 0x06, 0x07]);
        put32(&mut out, 0);
        put64(&mut out, zip64_end_offset);
        put32(&mut out, 1);
    }
    out.extend_from_slice(&[0x50, 0x4B, 0x05, 0x06]);
    put16(&mut out, 0);
    put16(&mut out, 0);
    if zip64 {
        put16(&mut out, u16::MAX);
        put16(&mut out, u16::MAX);
        put32(&mut out, u32::MAX);
        put32(&mut out, u32::MAX);
    } else {
        #[allow(clippy::cast_possible_truncation)]
        put16(&mut out, count as u16);
        #[allow(clippy::cast_possible_truncation)]
        put16(&mut out, count as u16);
        #[allow(clippy::cast_possible_truncation)]
        put32(&mut out, central_size as u32);
        #[allow(clippy::cast_possible_truncation)]
        put32(&mut out, central_offset as u32);
    }
    put16(&mut out, 0); // comment
    out
}
