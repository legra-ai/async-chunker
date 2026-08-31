//! Container fixtures: an independent TAR builder (the canonical
//! writer must not validate itself), flate2 gzip, and the shared ZIP
//! writer.

use std::io::Write;

use flate2::Compression;

/// Deterministic pseudo-random bytes.
pub(super) fn noise(seed: &str, len: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; len];
    let mut hasher = blake3::Hasher::new();
    hasher.update(seed.as_bytes());
    hasher.finalize_xof().fill(&mut bytes);
    bytes
}

/// Text-ish member content.
pub(super) fn prose(seed: &str, len: usize) -> Vec<u8> {
    let raw = noise(seed, len);
    let mut out = Vec::with_capacity(len + len / 8);
    for (index, byte) in raw.iter().enumerate() {
        out.push(b'a' + byte % 26);
        if index % 13 == 12 {
            out.push(b' ');
        }
        if index % 67 == 66 {
            out.push(b'\n');
        }
    }
    out
}

/// One TAR entry for the independent builder.
pub(super) enum TarFix<'a> {
    File {
        path: &'a [u8],
        bytes: &'a [u8],
    },
    Dir {
        path: &'a [u8],
    },
    Symlink {
        path: &'a [u8],
        target: &'a [u8],
    },
    /// A pax-extended file whose real path rides in the `x` record.
    PaxFile {
        path: &'a [u8],
        bytes: &'a [u8],
    },
    /// A GNU longname file.
    GnuFile {
        path: &'a [u8],
        bytes: &'a [u8],
    },
}

fn tar_octal(field: &mut [u8], value: u64) {
    let text = format!("{value:0width$o}", width = field.len() - 1);
    field[..text.len()].copy_from_slice(text.as_bytes());
}

fn tar_header(path: &[u8], size: u64, typeflag: u8, link: &[u8], mtime: u64) -> [u8; 512] {
    let mut block = [0u8; 512];
    block[..path.len()].copy_from_slice(path);
    tar_octal(&mut block[100..108], 0o640);
    tar_octal(&mut block[108..116], 0);
    tar_octal(&mut block[116..124], 0);
    tar_octal(&mut block[124..136], size);
    tar_octal(&mut block[136..148], mtime);
    block[156] = typeflag;
    block[157..157 + link.len()].copy_from_slice(link);
    block[257..262].copy_from_slice(b"ustar");
    block[263..265].copy_from_slice(b"00");
    block[148..156].copy_from_slice(b"        ");
    let sum: u64 = block.iter().map(|byte| u64::from(*byte)).sum();
    let text = format!("{sum:06o}");
    block[148..154].copy_from_slice(text.as_bytes());
    block[154] = 0;
    block[155] = b' ';
    block
}

fn pad(out: &mut Vec<u8>) {
    let rem = out.len() % 512;
    if rem != 0 {
        out.extend(std::iter::repeat_n(0u8, 512 - rem));
    }
}

/// Build a TAR archive.
pub(super) fn tar(entries: &[TarFix<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for entry in entries {
        match entry {
            TarFix::File { path, bytes } => {
                out.extend_from_slice(&tar_header(path, bytes.len() as u64, b'0', b"", 1234));
                out.extend_from_slice(bytes);
                pad(&mut out);
            }
            TarFix::Dir { path } => {
                out.extend_from_slice(&tar_header(path, 0, b'5', b"", 1234));
            }
            TarFix::Symlink { path, target } => {
                out.extend_from_slice(&tar_header(path, 0, b'2', target, 1234));
            }
            TarFix::PaxFile { path, bytes } => {
                let mut record = Vec::new();
                let body_len = 1 + 4 + 1 + path.len() + 1;
                let mut total = body_len;
                loop {
                    let digits = total.to_string().len();
                    if digits + body_len == total {
                        break;
                    }
                    total = digits + body_len;
                }
                record.extend_from_slice(format!("{total} path=").as_bytes());
                record.extend_from_slice(path);
                record.push(b'\n');
                out.extend_from_slice(&tar_header(
                    b"PaxHeaders/x",
                    record.len() as u64,
                    b'x',
                    b"",
                    0,
                ));
                out.extend_from_slice(&record);
                pad(&mut out);
                out.extend_from_slice(&tar_header(
                    b"short-name",
                    bytes.len() as u64,
                    b'0',
                    b"",
                    1234,
                ));
                out.extend_from_slice(bytes);
                pad(&mut out);
            }
            TarFix::GnuFile { path, bytes } => {
                out.extend_from_slice(&tar_header(
                    b"././@LongLink",
                    path.len() as u64 + 1,
                    b'L',
                    b"",
                    0,
                ));
                out.extend_from_slice(path);
                out.push(0);
                pad(&mut out);
                out.extend_from_slice(&tar_header(
                    b"short-gnu",
                    bytes.len() as u64,
                    b'0',
                    b"",
                    1234,
                ));
                out.extend_from_slice(bytes);
                pad(&mut out);
            }
        }
    }
    out.extend_from_slice(&[0u8; 1024]);
    out
}

/// Gzip `bytes` (default level, optional stored name).
pub(super) fn gzip(bytes: &[u8], name: Option<&str>) -> Vec<u8> {
    let mut builder = flate2::GzBuilder::new();
    if let Some(name) = name {
        builder = builder.filename(name);
    }
    let mut encoder = builder.write(Vec::new(), Compression::default());
    encoder.write_all(bytes).expect("gzip");
    encoder.finish().expect("gzip")
}
