//! [`CanonicalTarWriter`] — deterministic ustar/pax reconstruction.
//!
//! Canonical form: ustar headers with zeroed uid/gid/uname/gname and
//! mtime, mode 0644 for members and 0755 for directories (the walked
//! mode and mtime live as graph facts, not in the canonical bytes),
//! a pax `x` record when a path or link target exceeds the ustar
//! fields, and the two-block end marker.

use super::super::sink::EntryKind;

const BLOCK: usize = 512;

/// Streaming deterministic TAR writer. Members must be announced
/// with their exact byte length (TAR headers carry the size before
/// the data).
pub struct CanonicalTarWriter {
    /// `(announced size, bytes written)` of the open member.
    open: Option<(u64, u64)>,
}

impl Default for CanonicalTarWriter {
    fn default() -> Self {
        Self::new()
    }
}

fn octal(field: &mut [u8], value: u64) {
    let text = format!("{value:0width$o}", width = field.len() - 1);
    field[..text.len()].copy_from_slice(text.as_bytes());
    field[text.len()] = 0;
}

/// A ustar header block for `path` with `size`, `mode`, `typeflag`,
/// and `link` target.
fn header_block(path: &[u8], size: u64, mode: u32, typeflag: u8, link: &[u8]) -> [u8; BLOCK] {
    let mut block = [0u8; BLOCK];
    block[..path.len().min(100)].copy_from_slice(&path[..path.len().min(100)]);
    octal(&mut block[100..108], u64::from(mode));
    octal(&mut block[108..116], 0);
    octal(&mut block[116..124], 0);
    octal(&mut block[124..136], size);
    octal(&mut block[136..148], 0);
    block[156] = typeflag;
    block[157..157 + link.len().min(100)].copy_from_slice(&link[..link.len().min(100)]);
    block[257..262].copy_from_slice(b"ustar");
    block[263..265].copy_from_slice(b"00");
    // Checksum over the block with the checksum field as spaces.
    block[148..156].copy_from_slice(b"        ");
    let sum: u64 = block.iter().map(|byte| u64::from(*byte)).sum();
    let text = format!("{sum:06o}");
    block[148..154].copy_from_slice(text.as_bytes());
    block[154] = 0;
    block[155] = b' ';
    block
}

/// A pax `x` record set for over-long paths or link targets.
fn pax_records(path: &[u8], link: &[u8]) -> Vec<u8> {
    fn record(key: &str, value: &[u8]) -> Vec<u8> {
        // len " " key "=" value "\n", where len counts the whole
        // record including itself.
        let body_len = 1 + key.len() + 1 + value.len() + 1;
        let mut total = body_len;
        loop {
            let digits = total.to_string().len();
            if digits + body_len == total {
                break;
            }
            total = digits + body_len;
        }
        let mut out = format!("{total} {key}=").into_bytes();
        out.extend_from_slice(value);
        out.push(b'\n');
        out
    }
    let mut records = Vec::new();
    if path.len() > 100 {
        records.extend_from_slice(&record("path", path));
    }
    if link.len() > 100 {
        records.extend_from_slice(&record("linkpath", link));
    }
    records
}

fn emit_padded(bytes: &[u8], emit: &mut dyn FnMut(&[u8])) {
    emit(bytes);
    let rem = bytes.len() % BLOCK;
    if rem != 0 {
        emit(&vec![0u8; BLOCK - rem]);
    }
}

fn emit_header(
    path: &[u8],
    size: u64,
    mode: u32,
    typeflag: u8,
    link: &[u8],
    emit: &mut dyn FnMut(&[u8]),
) {
    let pax = pax_records(path, link);
    if !pax.is_empty() {
        let pax_header = header_block(b"@PaxHeader", pax.len() as u64, 0o644, b'x', b"");
        emit(&pax_header);
        emit_padded(&pax, emit);
    }
    let block = header_block(path, size, mode, typeflag, link);
    emit(&block);
}

impl CanonicalTarWriter {
    /// A fresh writer.
    #[must_use]
    pub fn new() -> Self {
        Self { open: None }
    }

    /// Write a non-member entry (directory, link, device).
    ///
    /// # Panics
    ///
    /// Panics while a member is open.
    pub fn entry(&mut self, kind: &EntryKind, path: &[u8], emit: &mut dyn FnMut(&[u8])) {
        assert!(self.open.is_none(), "entry inside an open member");
        match kind {
            EntryKind::Directory => emit_header(path, 0, 0o755, b'5', b"", emit),
            EntryKind::Symlink { target } => emit_header(path, 0, 0o644, b'2', target, emit),
            EntryKind::Hardlink { target } => emit_header(path, 0, 0o644, b'1', target, emit),
            EntryKind::Other { tag } => emit_header(path, 0, 0o644, *tag, b"", emit),
        }
    }

    /// Begin a member of exactly `size` bytes.
    ///
    /// # Panics
    ///
    /// Panics while another member is open.
    pub fn begin_member(&mut self, path: &[u8], size: u64, emit: &mut dyn FnMut(&[u8])) {
        assert!(self.open.is_none(), "member inside an open member");
        emit_header(path, size, 0o644, b'0', b"", emit);
        self.open = Some((size, 0));
    }

    /// Write member bytes.
    ///
    /// # Panics
    ///
    /// Panics without an open member or past its announced size.
    pub fn member_bytes(&mut self, bytes: &[u8], emit: &mut dyn FnMut(&[u8])) {
        let (size, written) = self.open.as_mut().expect("a member is open");
        assert!(
            *written + bytes.len() as u64 <= *size,
            "member bytes exceed the announced size"
        );
        *written += bytes.len() as u64;
        emit(bytes);
    }

    /// Close the member (pads to the block boundary).
    ///
    /// # Panics
    ///
    /// Panics unless exactly the announced bytes were written.
    pub fn end_member(&mut self, emit: &mut dyn FnMut(&[u8])) {
        let (size, written) = self.open.take().expect("a member is open");
        assert_eq!(written, size, "member closed before its announced size");
        #[allow(clippy::cast_possible_truncation)]
        let remainder = (size % BLOCK as u64) as usize;
        if remainder != 0 {
            emit(&vec![0u8; BLOCK - remainder]);
        }
    }

    /// Finish the archive: the two-block end marker.
    ///
    /// # Panics
    ///
    /// Panics while a member is open.
    pub fn finish(self, emit: &mut dyn FnMut(&[u8])) {
        assert!(self.open.is_none(), "finish with an open member");
        emit(&[0u8; BLOCK]);
        emit(&[0u8; BLOCK]);
    }
}
