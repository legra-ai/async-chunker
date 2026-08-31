//! [`Walker`] — the forward-only PDF structure walker.
//!
//! It reads the document exactly once: the header line, then body
//! items — indirect objects (with their stream payloads skipped by
//! the direct `/Length` when one is present, else by scanning for
//! `endstream`), comments, classic `xref` tables, `trailer`
//! dictionaries, and `startxref` — through any number of
//! incremental-update sections. Nothing is decoded; only item
//! boundaries and gross structure are checked.

use super::fault::PdfFault;
use super::object::{ObjectScanner, ObjectSignal, parse_int};

/// Longest line the xref/startxref parsers retain.
const LINE_MAX: usize = 128;
/// The `%PDF-` magic.
const MAGIC: &[u8] = b"%PDF-";
const ENDSTREAM: &[u8] = b"endstream";

/// What the walker is doing.
enum State {
    /// Matching the `%PDF-` magic.
    Magic { matched: usize },
    /// Consuming the rest of the header line.
    HeaderLine,
    /// Between body items, skipping whitespace. The next non-white
    /// byte begins an item — the profile's cut candidate.
    Item,
    /// A `%` comment line; tracking whether it is `%%EOF`.
    Comment { line: [u8; 5], len: usize },
    /// Collecting `number generation obj`.
    ObjHeader { ints: u8, seen_digits: bool },
    /// Inside an object body.
    Object { scanner: ObjectScanner },
    /// After `stream`, awaiting CR/LF or LF.
    StreamEol {
        scanner: ObjectScanner,
        saw_cr: bool,
    },
    /// Counting a known-length stream payload.
    StreamBytes {
        scanner: ObjectScanner,
        remaining: u64,
    },
    /// After a known-length payload: whitespace then `endstream`.
    StreamClose {
        scanner: ObjectScanner,
        matched: usize,
    },
    /// Scanning an unknown-length payload for `endstream`.
    StreamScan {
        scanner: ObjectScanner,
        matched: usize,
    },
    /// Collecting one keyword at item start (`xref`, `trailer`,
    /// `startxref`).
    ItemKeyword { token: [u8; 10], len: usize },
    /// Collecting one xref line (subsection header or `trailer`).
    XrefLine { line: [u8; LINE_MAX], len: usize },
    /// Skipping fixed-width xref entries.
    XrefEntries { remaining: u64 },
    /// Inside the trailer dictionary.
    Trailer { scanner: ObjectScanner },
    /// Collecting the startxref offset line.
    StartxrefLine { len: usize, seen_digit: bool },
}

/// The walker. State is one small scanner and a line buffer — never
/// the document.
pub(super) struct Walker {
    state: State,
    offset: u64,
    /// The last completed item was the `%%EOF` marker.
    eof_last: bool,
    /// Whether the next consumed byte begins a body item.
    boundary_pending: bool,
}

const fn is_whitespace(byte: u8) -> bool {
    matches!(byte, b'\0' | b'\t' | b'\n' | b'\x0C' | b'\r' | b' ')
}

impl Walker {
    pub(super) fn new() -> Self {
        Self {
            state: State::Magic { matched: 0 },
            offset: 0,
            eof_last: false,
            boundary_pending: false,
        }
    }

    /// Bytes consumed so far.
    pub(super) const fn offset(&self) -> u64 {
        self.offset
    }

    /// Whether the byte just about to be consumed begins a body
    /// item — the profile's cut candidate. Valid before `consume`.
    pub(super) const fn at_item_boundary(&self) -> bool {
        self.boundary_pending
    }

    /// Consume one byte.
    pub(super) fn consume(&mut self, byte: u8) -> Result<(), PdfFault> {
        self.boundary_pending = false;
        let result = self.step(byte);
        self.offset += 1;
        // Peek: in Item state the *next* non-white byte begins an
        // item; the chunker asks before feeding each byte, so flag
        // it now for the byte that will arrive.
        if let State::Item = self.state {
            self.boundary_pending = true;
        }
        result
    }

    /// The stream ended.
    pub(super) fn finish(&self) -> Result<(), PdfFault> {
        match self.state {
            State::Item if self.eof_last => Ok(()),
            State::Item | State::Comment { .. } => Err(PdfFault::MissingEof),
            State::Magic { .. } | State::HeaderLine => Err(PdfFault::NotPdf),
            _ => Err(PdfFault::Truncated),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn step(&mut self, byte: u8) -> Result<(), PdfFault> {
        if let State::XrefLine { line, len } = &mut self.state {
            if byte == b'\n' || byte == b'\r' {
                let text = line[..*len].to_vec();
                return self.close_xref_line(&text);
            }
            if *len >= LINE_MAX {
                return Err(PdfFault::XrefGeometry);
            }
            line[*len] = byte;
            *len += 1;
            return Ok(());
        }
        match &mut self.state {
            State::Magic { matched } => {
                if byte != MAGIC[*matched] {
                    return Err(PdfFault::NotPdf);
                }
                *matched += 1;
                if *matched == MAGIC.len() {
                    self.state = State::HeaderLine;
                }
                Ok(())
            }
            State::HeaderLine => {
                if byte == b'\n' || byte == b'\r' {
                    self.state = State::Item;
                }
                Ok(())
            }
            State::Item => {
                if is_whitespace(byte) {
                    return Ok(());
                }
                self.eof_last = false;
                match byte {
                    b'%' => {
                        let mut line = [0u8; 5];
                        line[0] = b'%';
                        self.state = State::Comment { line, len: 1 };
                        Ok(())
                    }
                    b'0'..=b'9' => {
                        self.state = State::ObjHeader {
                            ints: 0,
                            seen_digits: true,
                        };
                        Ok(())
                    }
                    b'x' | b't' | b's' => {
                        let mut token = [0u8; 10];
                        token[0] = byte;
                        self.state = State::ItemKeyword { token, len: 1 };
                        Ok(())
                    }
                    _ => Err(PdfFault::BadKeyword),
                }
            }
            State::Comment { line, len } => {
                if byte == b'\n' || byte == b'\r' {
                    self.eof_last = *len == 5 && &line[..5] == b"%%EOF";
                    self.state = State::Item;
                    return Ok(());
                }
                if *len < 5 {
                    line[*len] = byte;
                }
                *len += 1;
                Ok(())
            }
            State::ObjHeader { ints, seen_digits } => match byte {
                b'0'..=b'9' => {
                    *seen_digits = true;
                    Ok(())
                }
                _ if is_whitespace(byte) => {
                    if *seen_digits {
                        *ints += 1;
                        *seen_digits = false;
                        if *ints > 2 {
                            return Err(PdfFault::BadObjectHeader);
                        }
                    }
                    Ok(())
                }
                b'o' | b'b' | b'j' => {
                    // The `obj` keyword: require both integers first;
                    // accept the keyword bytes in order.
                    if *seen_digits {
                        *ints += 1;
                        *seen_digits = false;
                    }
                    if *ints != 2 {
                        return Err(PdfFault::BadObjectHeader);
                    }
                    // Consume `obj` loosely: `o` enters, `j` closes.
                    if byte == b'j' {
                        self.state = State::Object {
                            scanner: ObjectScanner::new(),
                        };
                    }
                    Ok(())
                }
                _ => Err(PdfFault::BadObjectHeader),
            },
            State::Object { scanner } => match scanner.consume(byte) {
                ObjectSignal::StreamBegins => {
                    // `byte` is the delimiter that closed the
                    // keyword — it must already be the line end.
                    let scanner = std::mem::replace(scanner, ObjectScanner::new());
                    match byte {
                        b'\n' => {
                            self.state = Self::stream_payload(scanner);
                            Ok(())
                        }
                        b'\r' => {
                            self.state = State::StreamEol {
                                scanner,
                                saw_cr: true,
                            };
                            Ok(())
                        }
                        _ => Err(PdfFault::BadStreamEol),
                    }
                }
                ObjectSignal::ObjectEnds => {
                    self.state = State::Item;
                    Ok(())
                }
                _ => Ok(()),
            },
            State::StreamEol { scanner, saw_cr } => match (byte, *saw_cr) {
                (b'\n', _) => {
                    let scanner = std::mem::replace(scanner, ObjectScanner::new());
                    self.state = Self::stream_payload(scanner);
                    Ok(())
                }
                (b'\r', false) => {
                    *saw_cr = true;
                    Ok(())
                }
                _ => Err(PdfFault::BadStreamEol),
            },
            State::StreamBytes { scanner, remaining } => {
                *remaining -= 1;
                if *remaining == 0 {
                    let scanner = std::mem::replace(scanner, ObjectScanner::new());
                    self.state = State::StreamClose {
                        scanner,
                        matched: 0,
                    };
                }
                Ok(())
            }
            State::StreamClose { scanner, matched } => {
                if *matched == 0 && is_whitespace(byte) {
                    return Ok(());
                }
                if byte == ENDSTREAM[*matched] {
                    *matched += 1;
                    if *matched == ENDSTREAM.len() {
                        let scanner = std::mem::replace(scanner, ObjectScanner::new());
                        self.state = State::Object { scanner };
                    }
                    return Ok(());
                }
                // The declared /Length did not land on `endstream`:
                // fall back to scanning. The mismatched bytes were
                // payload; the partial keyword match resets.
                let scanner = std::mem::replace(scanner, ObjectScanner::new());
                self.state = State::StreamScan {
                    scanner,
                    matched: usize::from(byte == ENDSTREAM[0]),
                };
                Ok(())
            }
            State::StreamScan { scanner, matched } => {
                if byte == ENDSTREAM[*matched] {
                    *matched += 1;
                    if *matched == ENDSTREAM.len() {
                        let scanner = std::mem::replace(scanner, ObjectScanner::new());
                        self.state = State::Object { scanner };
                    }
                } else {
                    *matched = usize::from(byte == ENDSTREAM[0]);
                }
                Ok(())
            }
            State::ItemKeyword { token, len } => {
                if byte.is_ascii_lowercase() {
                    if *len >= token.len() {
                        return Err(PdfFault::BadKeyword);
                    }
                    token[*len] = byte;
                    *len += 1;
                    return Ok(());
                }
                let keyword = &token[..*len];
                match keyword {
                    b"xref" => {
                        self.state = State::XrefLine {
                            line: [0; LINE_MAX],
                            len: 0,
                        };
                        Ok(())
                    }
                    b"trailer" => {
                        let mut scanner = ObjectScanner::new();
                        scanner.consume(byte);
                        self.state = State::Trailer { scanner };
                        Ok(())
                    }
                    b"startxref" => {
                        self.state = State::StartxrefLine {
                            len: 0,
                            seen_digit: byte.is_ascii_digit(),
                        };
                        Ok(())
                    }
                    _ => Err(PdfFault::BadKeyword),
                }
            }
            State::XrefLine { .. } => unreachable!("handled before the match"),
            State::XrefEntries { remaining } => {
                *remaining -= 1;
                if *remaining == 0 {
                    self.state = State::XrefLine {
                        line: [0; LINE_MAX],
                        len: 0,
                    };
                }
                Ok(())
            }
            State::Trailer { scanner } => match scanner.consume(byte) {
                ObjectSignal::DictClosed => {
                    self.state = State::Item;
                    Ok(())
                }
                _ => Ok(()),
            },
            State::StartxrefLine { len, seen_digit } => {
                if byte.is_ascii_digit() {
                    *seen_digit = true;
                    *len += 1;
                    return Ok(());
                }
                if is_whitespace(byte) {
                    if *seen_digit {
                        self.state = State::Item;
                    }
                    return Ok(());
                }
                Err(PdfFault::BadKeyword)
            }
        }
    }

    /// The state that consumes a stream payload whose dictionary
    /// scanner just crossed its EOL.
    fn stream_payload(scanner: ObjectScanner) -> State {
        match scanner.length() {
            Some(0) => State::StreamClose {
                scanner,
                matched: 0,
            },
            Some(length) => State::StreamBytes {
                scanner,
                remaining: length,
            },
            None => State::StreamScan {
                scanner,
                matched: 0,
            },
        }
    }

    /// One completed xref line: blank, a `start count` subsection
    /// header, or the start of `trailer`/another item.
    fn close_xref_line(&mut self, text: &[u8]) -> Result<(), PdfFault> {
        let trimmed: Vec<&[u8]> = text
            .split(|byte| is_whitespace(*byte))
            .filter(|part| !part.is_empty())
            .collect();
        match trimmed.as_slice() {
            [] => {
                self.state = State::XrefLine {
                    line: [0; LINE_MAX],
                    len: 0,
                };
                Ok(())
            }
            [start, count] => {
                let (Some(_), Some(count)) = (parse_int(start), parse_int(count)) else {
                    return Err(PdfFault::XrefGeometry);
                };
                self.state = if count == 0 {
                    State::XrefLine {
                        line: [0; LINE_MAX],
                        len: 0,
                    }
                } else {
                    State::XrefEntries {
                        remaining: count.checked_mul(20).ok_or(PdfFault::XrefGeometry)?,
                    }
                };
                Ok(())
            }
            [first, rest @ ..] if first.starts_with(b"trailer") => {
                let mut scanner = ObjectScanner::new();
                for &byte in &(*first)[7..] {
                    scanner.consume(byte);
                }
                for part in rest {
                    scanner.consume(b' ');
                    for &byte in *part {
                        if scanner.consume(byte) == ObjectSignal::DictClosed {
                            self.state = State::Item;
                            return Ok(());
                        }
                    }
                }
                self.state = State::Trailer { scanner };
                Ok(())
            }
            _ => Err(PdfFault::XrefGeometry),
        }
    }
}
