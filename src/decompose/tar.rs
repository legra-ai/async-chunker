//! The forward-only TAR walker: ustar, pax extended headers, and
//! GNU long-name/long-link entries, in bounded state.

use super::fault::OpaqueReason;
use super::sink::EntryKind;

/// A TAR block.
pub(super) const BLOCK: usize = 512;
/// Longest pax record set / long name retained per entry.
const OVERRIDE_MAX: usize = 16 << 10;

/// One walked TAR entry header, with pax/GNU overrides applied.
pub(super) struct TarEntry {
    pub(super) path: Vec<u8>,
    pub(super) kind: TarKind,
    pub(super) size: u64,
    pub(super) mode: Option<u32>,
    pub(super) mtime: Option<u64>,
}

/// The entry kinds the walker distinguishes.
pub(super) enum TarKind {
    Regular,
    Directory,
    Symlink { target: Vec<u8> },
    Hardlink { target: Vec<u8> },
    Other { tag: u8 },
}

impl TarKind {
    pub(super) fn to_entry_kind(&self) -> Option<EntryKind> {
        match self {
            Self::Regular => None,
            Self::Directory => Some(EntryKind::Directory),
            Self::Symlink { target } => Some(EntryKind::Symlink {
                target: target.clone().into_boxed_slice(),
            }),
            Self::Hardlink { target } => Some(EntryKind::Hardlink {
                target: target.clone().into_boxed_slice(),
            }),
            Self::Other { tag } => Some(EntryKind::Other { tag: *tag }),
        }
    }
}

/// What the walker reports for one pushed block of input.
pub(super) enum TarEvent {
    /// Nothing externally visible (metadata block consumed).
    None,
    /// A new entry's header completed.
    Entry(TarEntry),
    /// `len` bytes of the current entry's data (the tail of the
    /// pushed block; padding is stripped by the walker).
    Data(usize),
    /// The archive's end marker was seen.
    End,
}

/// What the walker is collecting.
enum State {
    /// A 512-byte header block.
    Header,
    /// Entry data; `remaining` real bytes then `padding` bytes.
    Data { remaining: u64, padding: usize },
    /// A pax/GNU override payload; applied to the next real header.
    OverrideData {
        remaining: u64,
        padding: usize,
        kind: OverrideKind,
    },
    /// The first zero block was seen; the second must follow.
    FirstZero,
    /// The archive ended; only zero padding may follow.
    Ended,
}

#[derive(Debug, Clone, Copy)]
enum OverrideKind {
    PaxNext,
    PaxGlobal,
    GnuName,
    GnuLink,
}

/// Bounded pending overrides for the next header.
#[derive(Default)]
struct Overrides {
    // bounded: OVERRIDE_MAX each.
    path: Option<Vec<u8>>,
    link: Option<Vec<u8>>,
    size: Option<u64>,
    buffer: Vec<u8>,
}

/// The walker. Feed exactly one block at a time via `push_block`,
/// or entry data in arbitrary windows via the caller (the walker
/// only frames; data bytes flow outside it).
pub(super) struct TarWalker {
    state: State,
    // bounded: one block.
    block: Vec<u8>,
    overrides: Overrides,
    offset: u64,
    saw_entry: bool,
}

/// Whether the 512-byte header's checksum field matches its bytes.
pub(super) fn checksum_matches(block: &[u8]) -> bool {
    if block.len() < BLOCK {
        return false;
    }
    let Some(declared) = parse_octal(&block[148..156]) else {
        return false;
    };
    let mut sum: u64 = 0;
    for (index, byte) in block.iter().take(BLOCK).enumerate() {
        sum += if (148..156).contains(&index) {
            u64::from(b' ')
        } else {
            u64::from(*byte)
        };
    }
    sum == declared && sum != 0
}

/// Parse a NUL/space-terminated octal field.
fn parse_octal(field: &[u8]) -> Option<u64> {
    let mut value: u64 = 0;
    let mut seen = false;
    for &byte in field {
        match byte {
            b'0'..=b'7' => {
                value = value.checked_mul(8)?.checked_add(u64::from(byte - b'0'))?;
                seen = true;
            }
            b' ' if !seen => {}
            b' ' | 0 => break,
            _ => return None,
        }
    }
    seen.then_some(value)
}

/// A NUL-terminated byte field.
fn c_string(field: &[u8]) -> Vec<u8> {
    let end = field
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(field.len());
    field[..end].to_vec()
}

impl TarWalker {
    pub(super) fn new() -> Self {
        Self {
            state: State::Header,
            block: Vec::with_capacity(BLOCK),
            overrides: Overrides::default(),
            offset: 0,
            saw_entry: false,
        }
    }

    fn malformed(&self, detail: &'static str) -> OpaqueReason {
        OpaqueReason::Malformed {
            detail,
            offset: self.offset,
        }
    }

    /// Push one byte. Returns an event when this byte completes a
    /// header, contributes entry data, or ends the archive.
    pub(super) fn push(&mut self, byte: u8) -> Result<TarEvent, OpaqueReason> {
        let event = self.step(byte);
        self.offset += 1;
        event
    }

    /// The input ended.
    pub(super) fn finish(&self) -> Result<(), OpaqueReason> {
        match self.state {
            State::Ended => Ok(()),
            // Tolerate a missing end marker only at a block boundary
            // after at least one entry (common with some writers).
            State::Header | State::FirstZero if self.saw_entry && self.block.is_empty() => Ok(()),
            _ => Err(OpaqueReason::Malformed {
                detail: "tar archive is truncated",
                offset: self.offset,
            }),
        }
    }

    fn step(&mut self, byte: u8) -> Result<TarEvent, OpaqueReason> {
        match &mut self.state {
            State::Header | State::FirstZero => {
                self.block.push(byte);
                if self.block.len() < BLOCK {
                    return Ok(TarEvent::None);
                }
                let block = std::mem::take(&mut self.block);
                self.close_header(&block)
            }
            State::Data { remaining, padding } => {
                if *remaining > 0 {
                    *remaining -= 1;
                    if *remaining == 0 && *padding == 0 {
                        self.state = State::Header;
                    }
                    return Ok(TarEvent::Data(1));
                }
                *padding -= 1;
                if byte != 0 {
                    return Err(self.malformed("nonzero tar padding"));
                }
                if *padding == 0 {
                    self.state = State::Header;
                }
                Ok(TarEvent::None)
            }
            State::OverrideData {
                remaining,
                padding,
                kind,
            } => {
                let kind = *kind;
                if *remaining > 0 {
                    *remaining -= 1;
                    let done = *remaining == 0 && *padding == 0;
                    if self.overrides.buffer.len() >= OVERRIDE_MAX {
                        return Err(OpaqueReason::MetadataOverBound);
                    }
                    self.overrides.buffer.push(byte);
                    if done {
                        self.state = State::Header;
                        self.apply_override(kind)?;
                    }
                    return Ok(TarEvent::None);
                }
                *padding -= 1;
                if byte != 0 {
                    return Err(self.malformed("nonzero tar padding"));
                }
                if *padding == 0 {
                    self.state = State::Header;
                    self.apply_override(kind)?;
                }
                Ok(TarEvent::None)
            }
            State::Ended => {
                if byte != 0 {
                    return Err(self.malformed("bytes after the tar end marker"));
                }
                Ok(TarEvent::None)
            }
        }
    }

    fn close_header(&mut self, block: &[u8]) -> Result<TarEvent, OpaqueReason> {
        let zero = block.iter().all(|byte| *byte == 0);
        if zero {
            self.state = match self.state {
                State::FirstZero => State::Ended,
                _ => State::FirstZero,
            };
            return Ok(match self.state {
                State::Ended => TarEvent::End,
                _ => TarEvent::None,
            });
        }
        if matches!(self.state, State::FirstZero) {
            return Err(self.malformed("lone zero block inside the archive"));
        }
        if !checksum_matches(block) {
            return Err(self.malformed("tar header checksum mismatch"));
        }
        let typeflag = block[156];
        let size = parse_octal(&block[124..136])
            .ok_or_else(|| self.malformed("tar size field is not octal"))?;
        #[allow(clippy::cast_possible_truncation)]
        let remainder = (size % BLOCK as u64) as usize;
        let padding = (BLOCK - remainder) % BLOCK;
        match typeflag {
            b'x' | b'X' => {
                self.begin_override(size, padding, OverrideKind::PaxNext);
                Ok(TarEvent::None)
            }
            b'g' => {
                self.begin_override(size, padding, OverrideKind::PaxGlobal);
                Ok(TarEvent::None)
            }
            b'L' => {
                self.begin_override(size, padding, OverrideKind::GnuName);
                Ok(TarEvent::None)
            }
            b'K' => {
                self.begin_override(size, padding, OverrideKind::GnuLink);
                Ok(TarEvent::None)
            }
            b'S' => Err(OpaqueReason::UnsupportedFeature {
                detail: "sparse tar member",
            }),
            _ => {
                let entry = self.build_entry(block, typeflag, size)?;
                let data = matches!(entry.kind, TarKind::Regular) && entry.size > 0;
                self.state = if data {
                    State::Data {
                        remaining: entry.size,
                        padding,
                    }
                } else if size > 0 && !matches!(entry.kind, TarKind::Regular) {
                    // Non-regular entries with payload are malformed.
                    return Err(self.malformed("tar metadata entry declares a payload"));
                } else {
                    State::Header
                };
                self.saw_entry = true;
                Ok(TarEvent::Entry(entry))
            }
        }
    }

    fn begin_override(&mut self, size: u64, padding: usize, kind: OverrideKind) {
        self.overrides.buffer.clear();
        if size == 0 {
            self.state = State::Header;
        } else {
            self.state = State::OverrideData {
                remaining: size,
                padding,
                kind,
            };
        }
    }

    fn apply_override(&mut self, kind: OverrideKind) -> Result<(), OpaqueReason> {
        let buffer = std::mem::take(&mut self.overrides.buffer);
        match kind {
            OverrideKind::GnuName => {
                self.overrides.path = Some(c_string(&buffer));
            }
            OverrideKind::GnuLink => {
                self.overrides.link = Some(c_string(&buffer));
            }
            OverrideKind::PaxGlobal => {
                // Global defaults are accepted and ignored: applying
                // them would make member facts depend on distant
                // state.
            }
            OverrideKind::PaxNext => self.apply_pax(&buffer)?,
        }
        Ok(())
    }

    fn apply_pax(&mut self, records: &[u8]) -> Result<(), OpaqueReason> {
        let mut rest = records;
        while !rest.is_empty() {
            let space = rest
                .iter()
                .position(|byte| *byte == b' ')
                .ok_or_else(|| self.malformed("pax record has no length"))?;
            let len: usize = std::str::from_utf8(&rest[..space])
                .ok()
                .and_then(|text| text.parse().ok())
                .ok_or_else(|| self.malformed("pax record length is not decimal"))?;
            if len <= space + 1 || len > rest.len() {
                return Err(self.malformed("pax record length out of range"));
            }
            let record = &rest[space + 1..len];
            let record = record
                .strip_suffix(b"\n")
                .ok_or_else(|| self.malformed("pax record does not end in newline"))?;
            let eq = record
                .iter()
                .position(|byte| *byte == b'=')
                .ok_or_else(|| self.malformed("pax record has no '='"))?;
            let (key, value) = (&record[..eq], &record[eq + 1..]);
            match key {
                b"path" => self.overrides.path = Some(value.to_vec()),
                b"linkpath" => self.overrides.link = Some(value.to_vec()),
                b"size" => {
                    let size: u64 = std::str::from_utf8(value)
                        .ok()
                        .and_then(|text| text.parse().ok())
                        .ok_or_else(|| self.malformed("pax size is not decimal"))?;
                    self.overrides.size = Some(size);
                }
                _ => {}
            }
            rest = &rest[len..];
        }
        Ok(())
    }

    fn build_entry(
        &mut self,
        block: &[u8],
        typeflag: u8,
        header_size: u64,
    ) -> Result<TarEntry, OpaqueReason> {
        let path = match self.overrides.path.take() {
            Some(path) => path,
            None => {
                let mut path = c_string(&block[0..100]);
                let prefix = c_string(&block[345..500]);
                if !prefix.is_empty() && &block[257..262] == b"ustar" {
                    let mut joined = prefix;
                    joined.push(b'/');
                    joined.extend_from_slice(&path);
                    path = joined;
                }
                path
            }
        };
        let link = self
            .overrides
            .link
            .take()
            .unwrap_or_else(|| c_string(&block[157..257]));
        let size = self.overrides.size.take().unwrap_or(header_size);
        let kind = match typeflag {
            b'0' | 0 | b'7' => TarKind::Regular,
            b'5' => TarKind::Directory,
            b'2' => TarKind::Symlink { target: link },
            b'1' => TarKind::Hardlink { target: link },
            tag @ (b'3' | b'4' | b'6') => TarKind::Other { tag },
            _ => {
                return Err(self.malformed("unknown tar entry type"));
            }
        };
        Ok(TarEntry {
            path,
            kind,
            size,
            mode: parse_octal(&block[100..108]).and_then(|mode| u32::try_from(mode).ok()),
            mtime: parse_octal(&block[136..148]),
        })
    }
}
