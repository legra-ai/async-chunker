//! [`ObjectScanner`] — the bounded lexical context inside an
//! indirect object or a trailer dictionary.
//!
//! It tracks exactly enough of the PDF syntax (COS strings, hex
//! strings, comments, dictionary nesting, bare tokens) to recognise
//! the keywords `stream` and `endobj` outside string data and to
//! capture a direct `/Length` value, and nothing more. Content is
//! never decoded.

/// Longest bare token the scanner retains (`startxref` is 9).
const TOKEN_MAX: usize = 10;

/// What a completed byte means to the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ObjectSignal {
    /// Nothing of interest.
    None,
    /// The `stream` keyword completed; the byte that completed it is
    /// the delimiter after the keyword (already consumed).
    StreamBegins,
    /// The `endobj` keyword completed.
    ObjectEnds,
    /// The trailer dictionary closed (dictionary depth returned to
    /// zero after having been positive).
    DictClosed,
}

/// The `/Length` capture state.
#[derive(Debug, Clone, Copy)]
enum LengthCapture {
    Idle,
    /// `/Length` was seen at depth one; the next integer is the
    /// tentative value.
    WantValue,
    /// A tentative direct value; a following `integer R` pair turns
    /// it into an indirect reference and discards it.
    Have {
        value: u64,
        ints_after: u8,
    },
}

/// The scanner.
pub(super) struct ObjectScanner {
    /// Bare-token accumulator.
    token: [u8; TOKEN_MAX],
    token_len: usize,
    /// The token is oversized or began with a delimiter class the
    /// keywords cannot start with.
    token_dead: bool,
    /// The previous token began with `/` (it was a name).
    token_is_name: bool,
    /// Literal-string nesting (0 = outside).
    string_depth: u32,
    string_escape: bool,
    /// Inside `<...>` hex string.
    in_hex: bool,
    /// Inside a `%` comment.
    in_comment: bool,
    /// `<<`/`>>` nesting.
    dict_depth: u32,
    dict_was_open: bool,
    /// Pending `<` or `>` awaiting its pair.
    pending_angle: Option<u8>,
    length: LengthCapture,
}

const fn is_whitespace(byte: u8) -> bool {
    matches!(byte, b'\0' | b'\t' | b'\n' | b'\x0C' | b'\r' | b' ')
}

const fn is_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

impl ObjectScanner {
    pub(super) fn new() -> Self {
        Self {
            token: [0; TOKEN_MAX],
            token_len: 0,
            token_dead: false,
            token_is_name: false,
            string_depth: 0,
            string_escape: false,
            in_hex: false,
            in_comment: false,
            dict_depth: 0,
            dict_was_open: false,
            pending_angle: None,
            length: LengthCapture::Idle,
        }
    }

    /// The captured direct `/Length`, if any.
    pub(super) const fn length(&self) -> Option<u64> {
        match self.length {
            LengthCapture::Have { value, .. } => Some(value),
            _ => None,
        }
    }

    /// Consume one byte of object-level syntax.
    pub(super) fn consume(&mut self, byte: u8) -> ObjectSignal {
        if self.in_comment {
            if byte == b'\n' || byte == b'\r' {
                self.in_comment = false;
            }
            return ObjectSignal::None;
        }
        if self.string_depth > 0 {
            if self.string_escape {
                self.string_escape = false;
            } else {
                match byte {
                    b'\\' => self.string_escape = true,
                    b'(' => self.string_depth += 1,
                    b')' => self.string_depth -= 1,
                    _ => {}
                }
            }
            return ObjectSignal::None;
        }
        if self.in_hex {
            if byte == b'>' {
                self.in_hex = false;
            }
            return ObjectSignal::None;
        }
        if let Some(pending) = self.pending_angle.take() {
            match (pending, byte) {
                (b'<', b'<') => {
                    self.dict_depth += 1;
                    self.dict_was_open = true;
                    return ObjectSignal::None;
                }
                (b'<', _) => {
                    // A hex string; the current byte is inside it
                    // (or closes it).
                    self.in_hex = byte != b'>';
                    return ObjectSignal::None;
                }
                (b'>', b'>') => {
                    self.dict_depth = self.dict_depth.saturating_sub(1);
                    if self.dict_depth == 0 && self.dict_was_open {
                        return ObjectSignal::DictClosed;
                    }
                    return ObjectSignal::None;
                }
                (b'>', _) => {
                    // A stray '>': tolerate and fall through to the
                    // current byte.
                }
                _ => {}
            }
        }
        if is_whitespace(byte) || is_delimiter(byte) {
            let signal = self.close_token();
            match byte {
                b'(' => self.string_depth = 1,
                b'<' | b'>' => self.pending_angle = Some(byte),
                b'%' => self.in_comment = true,
                b'/' => {
                    self.token_len = 0;
                    self.token_dead = false;
                    self.token_is_name = true;
                    return signal;
                }
                _ => {}
            }
            self.token_is_name = false;
            return signal;
        }
        if self.token_len < TOKEN_MAX {
            self.token[self.token_len] = byte;
            self.token_len += 1;
        } else {
            self.token_dead = true;
        }
        ObjectSignal::None
    }

    /// The stream position is between tokens (used at `finish`).
    pub(super) fn close_token(&mut self) -> ObjectSignal {
        if self.token_len == 0 {
            return ObjectSignal::None;
        }
        let token = &self.token[..self.token_len];
        let is_name = self.token_is_name;
        let dead = self.token_dead;
        self.token_len = 0;
        self.token_dead = false;
        self.token_is_name = false;
        if dead {
            self.length_token(None);
            return ObjectSignal::None;
        }
        if is_name {
            if token == b"Length" && self.dict_depth == 1 {
                self.length = LengthCapture::WantValue;
            } else {
                self.length_token(None);
            }
            return ObjectSignal::None;
        }
        match token {
            b"stream" => ObjectSignal::StreamBegins,
            b"endobj" => ObjectSignal::ObjectEnds,
            b"R" => {
                self.length_r();
                ObjectSignal::None
            }
            _ => {
                let value = parse_int(token);
                self.length_token(value);
                ObjectSignal::None
            }
        }
    }

    fn length_token(&mut self, int: Option<u64>) {
        self.length = match (self.length, int) {
            (LengthCapture::WantValue, Some(value)) => LengthCapture::Have {
                value,
                ints_after: 0,
            },
            (LengthCapture::WantValue, None) => LengthCapture::Idle,
            (LengthCapture::Have { value, ints_after }, Some(_)) => LengthCapture::Have {
                value,
                ints_after: ints_after.saturating_add(1),
            },
            (state, _) => state,
        };
    }

    fn length_r(&mut self) {
        if let LengthCapture::Have { ints_after: 1, .. } = self.length {
            // `/Length n g R` — an indirect reference, not a value.
            self.length = LengthCapture::Idle;
        }
    }
}

/// Parse an unsigned ASCII integer token.
pub(super) fn parse_int(token: &[u8]) -> Option<u64> {
    if token.is_empty() || !token.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let mut value: u64 = 0;
    for &byte in token {
        value = value.checked_mul(10)?.checked_add(u64::from(byte - b'0'))?;
    }
    Some(value)
}
