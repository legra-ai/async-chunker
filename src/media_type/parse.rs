//! The media-type grammar: RFC 6838 §4.2 restricted names and
//! RFC 2045 §5.1 / RFC 7231 §3.1.1.1 parameters.

use super::{MediaType, MediaTypeError, Parameter};
use crate::constants::{MAX_MEDIA_TYPE_NAME_BYTES, MAX_MEDIA_TYPE_PARAMETERS};

/// Whether `byte` may appear in an RFC 6838 restricted name after
/// its first (alphanumeric) character.
const fn is_restricted_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#' | b'$' | b'&' | b'-' | b'^' | b'_' | b'.' | b'+'
        )
}

/// Whether `byte` is an RFC 7230 token character.
const fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

/// Whether `text` is a non-empty RFC 7230 token.
pub(super) fn is_token(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(is_token_byte)
}

/// Parse and normalize `input` (see [`MediaType::parse`]).
pub(super) fn parse(input: &str) -> Result<MediaType, MediaTypeError> {
    let trimmed = input.trim_matches(|ch: char| ch == ' ' || ch == '\t');
    if trimmed.is_empty() {
        return Err(MediaTypeError::Empty);
    }
    let base = input.len() - input.trim_start_matches([' ', '\t']).len();
    let (essence_text, parameter_text) = match trimmed.find(';') {
        Some(semicolon) => (
            &trimmed[..semicolon],
            Some((&trimmed[semicolon + 1..], base + semicolon + 1)),
        ),
        None => (trimmed, None),
    };
    let essence_text = essence_text.trim_end_matches([' ', '\t']);
    let slash = essence_text.find('/').ok_or(MediaTypeError::MissingSlash)?;
    let top_level = &essence_text[..slash];
    let subtype = &essence_text[slash + 1..];
    check_restricted_name(top_level, base, "type", |offset| {
        MediaTypeError::InvalidTopLevel { offset }
    })?;
    check_restricted_name(subtype, base + slash + 1, "subtype", |offset| {
        MediaTypeError::InvalidSubtype { offset }
    })?;

    let parameters = match parameter_text {
        Some((text, offset)) => parse_parameters(text, offset)?,
        None => Vec::new(),
    };

    Ok(MediaType {
        essence: essence_text.to_ascii_lowercase().into_boxed_str(),
        slash,
        parameters: parameters.into_boxed_slice(),
    })
}

/// Validate one restricted name that begins at `offset` in the
/// input; `invalid` builds the positional error.
fn check_restricted_name(
    name: &str,
    offset: usize,
    component: &'static str,
    invalid: fn(usize) -> MediaTypeError,
) -> Result<(), MediaTypeError> {
    if name.len() > MAX_MEDIA_TYPE_NAME_BYTES {
        return Err(MediaTypeError::NameTooLong {
            component,
            limit: MAX_MEDIA_TYPE_NAME_BYTES,
        });
    }
    let Some(first) = name.bytes().next() else {
        return Err(invalid(offset));
    };
    if !first.is_ascii_alphanumeric() {
        return Err(invalid(offset));
    }
    if let Some(index) = name.bytes().position(|byte| !is_restricted_name_byte(byte)) {
        return Err(invalid(offset + index));
    }
    Ok(())
}

/// Parse `name=value` parameters separated by `;`; `base` is the
/// input offset of the first byte of `text`.
fn parse_parameters(text: &str, base: usize) -> Result<Vec<Parameter>, MediaTypeError> {
    let mut cursor = Cursor { text, pos: 0, base };
    // bounded: MAX_MEDIA_TYPE_PARAMETERS entries.
    let mut parameters: Vec<Parameter> = Vec::new();
    loop {
        cursor.skip_whitespace();
        let name = cursor.token()?.to_ascii_lowercase();
        cursor.expect(b'=')?;
        let value = cursor.value()?;
        if parameters.iter().any(|existing| *existing.name == *name) {
            return Err(MediaTypeError::DuplicateParameter { name });
        }
        if parameters.len() == MAX_MEDIA_TYPE_PARAMETERS {
            return Err(MediaTypeError::TooManyParameters {
                limit: MAX_MEDIA_TYPE_PARAMETERS,
            });
        }
        parameters.push(Parameter {
            name: name.into_boxed_str(),
            value: value.into_boxed_str(),
        });
        cursor.skip_whitespace();
        if cursor.at_end() {
            break;
        }
        cursor.expect(b';')?;
    }
    parameters.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(parameters)
}

/// A byte cursor over the parameter text.
struct Cursor<'a> {
    text: &'a str,
    pos: usize,
    base: usize,
}

impl Cursor<'_> {
    fn at_end(&self) -> bool {
        self.pos >= self.text.len()
    }

    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.pos).copied()
    }

    fn malformed(&self) -> MediaTypeError {
        MediaTypeError::MalformedParameter {
            offset: self.base + self.pos,
        }
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), MediaTypeError> {
        if self.peek() == Some(byte) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.malformed())
        }
    }

    /// A non-empty token.
    fn token(&mut self) -> Result<&str, MediaTypeError> {
        let start = self.pos;
        while matches!(self.peek(), Some(byte) if is_token_byte(byte)) {
            self.pos += 1;
        }
        if self.pos == start {
            return Err(self.malformed());
        }
        Ok(&self.text[start..self.pos])
    }

    /// A token or a quoted string, unescaped.
    fn value(&mut self) -> Result<String, MediaTypeError> {
        if self.peek() != Some(b'"') {
            return self.token().map(str::to_owned);
        }
        self.pos += 1;
        let mut value = String::new();
        let mut chars = self.text[self.pos..].char_indices();
        while let Some((index, ch)) = chars.next() {
            match ch {
                '"' => {
                    self.pos += index + 1;
                    return Ok(value);
                }
                '\\' => {
                    let (_, escaped) = chars.next().ok_or_else(|| self.malformed())?;
                    value.push(escaped);
                }
                _ => value.push(ch),
            }
        }
        self.pos = self.text.len();
        Err(self.malformed())
    }
}
