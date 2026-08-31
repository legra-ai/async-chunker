//! A deliberately small ZIP writer for fixtures: enough of APPNOTE
//! to produce stored and deflated members, data descriptors (signed
//! and unsigned), ZIP64 sizes and end records, and archive comments.
//! Tests mutate its output to build the malformed corpus.

use std::io::Write;

use flate2::Compression;
use flate2::write::DeflateEncoder;

/// How one member is framed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::chunker) enum Framing {
    /// Sizes in the local header, no descriptor.
    Plain,
    /// Sizes zero in the local header; a signed descriptor follows.
    SignedDescriptor,
    /// Sizes known in the local header *and* an unsigned descriptor.
    UnsignedDescriptorKnownSize,
    /// ZIP64 extra field carries the sizes (plus a signed descriptor
    /// with 64-bit sizes when `descriptor` is set).
    Zip64 { descriptor: bool },
}

/// One member to write.
pub(in crate::chunker) struct Member<'a> {
    pub(in crate::chunker) name: &'a str,
    pub(in crate::chunker) bytes: &'a [u8],
    pub(in crate::chunker) deflate: bool,
    pub(in crate::chunker) framing: Framing,
}

impl<'a> Member<'a> {
    pub(in crate::chunker) const fn stored(name: &'a str, bytes: &'a [u8]) -> Self {
        Self {
            name,
            bytes,
            deflate: false,
            framing: Framing::Plain,
        }
    }

    pub(in crate::chunker) const fn deflated(name: &'a str, bytes: &'a [u8]) -> Self {
        Self {
            name,
            bytes,
            deflate: true,
            framing: Framing::Plain,
        }
    }

    pub(in crate::chunker) const fn framed(self, framing: Framing) -> Self {
        Self { framing, ..self }
    }
}

struct Written {
    name: Vec<u8>,
    method: u16,
    flags: u16,
    crc: u32,
    compressed: u64,
    uncompressed: u64,
    local_offset: u64,
    zip64: bool,
}

/// Archive-level options.
#[derive(Debug, Clone, Copy, Default)]
pub(in crate::chunker) struct Options<'a> {
    /// Emit a ZIP64 end record + locator and mark the EOCD counts
    /// as deferred.
    pub(in crate::chunker) zip64_end: bool,
    pub(in crate::chunker) comment: &'a [u8],
}

fn put16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = flate2::Crc::new();
    crc.update(bytes);
    crc.sum()
}

fn deflate(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).expect("deflate");
    encoder.finish().expect("deflate")
}

/// Write a complete archive.
pub(in crate::chunker) fn archive(members: &[Member<'_>], options: Options<'_>) -> Vec<u8> {
    let mut out = Vec::new();
    let mut written = Vec::new();
    for member in members {
        written.push(write_member(&mut out, member));
    }
    let central_start = out.len() as u64;
    for entry in &written {
        write_central(&mut out, entry);
    }
    let central_size = out.len() as u64 - central_start;
    if options.zip64_end {
        let zip64_end_offset = out.len() as u64;
        out.extend_from_slice(&[0x50, 0x4B, 0x06, 0x06]);
        put64(&mut out, 44);
        put16(&mut out, 45);
        put16(&mut out, 45);
        put32(&mut out, 0);
        put32(&mut out, 0);
        put64(&mut out, written.len() as u64);
        put64(&mut out, written.len() as u64);
        put64(&mut out, central_size);
        put64(&mut out, central_start);
        out.extend_from_slice(&[0x50, 0x4B, 0x06, 0x07]);
        put32(&mut out, 0);
        put64(&mut out, zip64_end_offset);
        put32(&mut out, 1);
    }
    out.extend_from_slice(&[0x50, 0x4B, 0x05, 0x06]);
    put16(&mut out, 0);
    put16(&mut out, 0);
    if options.zip64_end {
        put16(&mut out, u16::MAX);
        put16(&mut out, u16::MAX);
        put32(&mut out, u32::MAX);
        put32(&mut out, u32::MAX);
    } else {
        put16(&mut out, written.len() as u16);
        put16(&mut out, written.len() as u16);
        put32(&mut out, central_size as u32);
        put32(&mut out, central_start as u32);
    }
    put16(&mut out, options.comment.len() as u16);
    out.extend_from_slice(options.comment);
    out
}

fn write_member(out: &mut Vec<u8>, member: &Member<'_>) -> Written {
    let local_offset = out.len() as u64;
    let payload = if member.deflate {
        deflate(member.bytes)
    } else {
        member.bytes.to_vec()
    };
    let method = if member.deflate { 8 } else { 0 };
    let crc = crc32(member.bytes);
    let (compressed, uncompressed) = (payload.len() as u64, member.bytes.len() as u64);
    let (flags, zip64, descriptor) = match member.framing {
        Framing::Plain => (0u16, false, false),
        Framing::SignedDescriptor | Framing::UnsignedDescriptorKnownSize => (1 << 3, false, true),
        Framing::Zip64 { descriptor } => (u16::from(descriptor) << 3, true, descriptor),
    };
    out.extend_from_slice(&[0x50, 0x4B, 0x03, 0x04]);
    put16(out, if zip64 { 45 } else { 20 });
    put16(out, flags);
    put16(out, method);
    put16(out, 0);
    put16(out, 0x21);
    put32(out, crc);
    match member.framing {
        Framing::SignedDescriptor => {
            put32(out, 0);
            put32(out, 0);
        }
        Framing::Zip64 { .. } => {
            put32(out, u32::MAX);
            put32(out, u32::MAX);
        }
        Framing::Plain | Framing::UnsignedDescriptorKnownSize => {
            put32(out, compressed as u32);
            put32(out, uncompressed as u32);
        }
    }
    put16(out, member.name.len() as u16);
    put16(out, if zip64 { 20 } else { 0 });
    out.extend_from_slice(member.name.as_bytes());
    if zip64 {
        put16(out, 0x0001);
        put16(out, 16);
        put64(out, uncompressed);
        put64(out, compressed);
    }
    out.extend_from_slice(&payload);
    if descriptor {
        if member.framing != Framing::UnsignedDescriptorKnownSize {
            out.extend_from_slice(&[0x50, 0x4B, 0x07, 0x08]);
        }
        put32(out, crc);
        if zip64 {
            put64(out, compressed);
            put64(out, uncompressed);
        } else {
            put32(out, compressed as u32);
            put32(out, uncompressed as u32);
        }
    }
    Written {
        name: member.name.as_bytes().to_vec(),
        method,
        flags,
        crc,
        compressed,
        uncompressed,
        local_offset,
        zip64,
    }
}

fn write_central(out: &mut Vec<u8>, entry: &Written) {
    out.extend_from_slice(&[0x50, 0x4B, 0x01, 0x02]);
    put16(out, 45);
    put16(out, if entry.zip64 { 45 } else { 20 });
    put16(out, entry.flags);
    put16(out, entry.method);
    put16(out, 0);
    put16(out, 0x21);
    put32(out, entry.crc);
    if entry.zip64 {
        put32(out, u32::MAX);
        put32(out, u32::MAX);
    } else {
        put32(out, entry.compressed as u32);
        put32(out, entry.uncompressed as u32);
    }
    put16(out, entry.name.len() as u16);
    put16(out, if entry.zip64 { 28 } else { 0 });
    put16(out, 0);
    put16(out, 0);
    put16(out, 0);
    put32(out, 0);
    if entry.zip64 {
        put32(out, u32::MAX);
    } else {
        put32(out, entry.local_offset as u32);
    }
    out.extend_from_slice(&entry.name);
    if entry.zip64 {
        put16(out, 0x0001);
        put16(out, 24);
        put64(out, entry.uncompressed);
        put64(out, entry.compressed);
        put64(out, entry.local_offset);
    }
}
