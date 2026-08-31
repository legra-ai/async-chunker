//! [`CanonicalZipWriter`] — deterministic ZIP reconstruction over
//! the shared canonical record emitter (STORE + data descriptors,
//! zeroed metadata).

use crate::chunker::office::canonical::writer::{
    CentralEntry, descriptor, local_header_unknown, tail,
};

/// Streaming deterministic ZIP writer: every member is written in
/// descriptor mode (STORE, zeroed metadata), so no size or CRC is
/// needed up front.
pub struct CanonicalZipWriter {
    // bounded: one entry per member; the caller bounds member count.
    entries: Vec<CentralEntry>,
    offset: u64,
    open: Option<OpenMember>,
}

struct OpenMember {
    name: Box<[u8]>,
    header_offset: u64,
    crc: crc32fast::Hasher,
    len: u64,
}

impl Default for CanonicalZipWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl CanonicalZipWriter {
    /// A fresh writer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            offset: 0,
            open: None,
        }
    }

    fn emit_counted(&mut self, bytes: &[u8], emit: &mut dyn FnMut(&[u8])) {
        self.offset += bytes.len() as u64;
        emit(bytes);
    }

    /// Write an explicit directory entry (a zero-length member whose
    /// name ends in `/`).
    ///
    /// # Panics
    ///
    /// Panics while a member is open.
    pub fn directory(&mut self, path: &[u8], emit: &mut dyn FnMut(&[u8])) {
        let mut name = path.to_vec();
        if !name.ends_with(b"/") {
            name.push(b'/');
        }
        self.begin_member(&name, emit);
        self.end_member(emit);
    }

    /// Begin a member.
    ///
    /// # Panics
    ///
    /// Panics while another member is open.
    pub fn begin_member(&mut self, path: &[u8], emit: &mut dyn FnMut(&[u8])) {
        assert!(self.open.is_none(), "member inside an open member");
        let utf8 = std::str::from_utf8(path).is_ok();
        let header_offset = self.offset;
        let header = local_header_unknown(path, utf8);
        self.emit_counted(&header, emit);
        self.open = Some(OpenMember {
            name: path.into(),
            header_offset,
            crc: crc32fast::Hasher::new(),
            len: 0,
        });
    }

    /// Write member bytes.
    ///
    /// # Panics
    ///
    /// Panics without an open member.
    pub fn member_bytes(&mut self, bytes: &[u8], emit: &mut dyn FnMut(&[u8])) {
        {
            let open = self.open.as_mut().expect("a member is open");
            open.crc.update(bytes);
            open.len += bytes.len() as u64;
        }
        self.emit_counted(bytes, emit);
    }

    /// Close the member with its data descriptor.
    ///
    /// # Panics
    ///
    /// Panics without an open member, or on a member of 4 GiB or
    /// more (a canonical descriptor is 32-bit).
    pub fn end_member(&mut self, emit: &mut dyn FnMut(&[u8])) {
        let open = self.open.take().expect("a member is open");
        let crc = open.crc.clone().finalize();
        let len = u32::try_from(open.len).expect("canonical zip member under 4 GiB");
        let record = descriptor(crc, len);
        self.emit_counted(&record, emit);
        let utf8 = std::str::from_utf8(&open.name).is_ok();
        self.entries.push(CentralEntry {
            name: open.name,
            utf8,
            crc,
            len: open.len,
            offset: open.header_offset,
            descriptor: true,
        });
    }

    /// Finish the archive: central directory and end records.
    ///
    /// # Panics
    ///
    /// Panics while a member is open.
    pub fn finish(self, emit: &mut dyn FnMut(&[u8])) {
        assert!(self.open.is_none(), "finish with an open member");
        emit(&tail(&self.entries, self.offset));
    }
}
