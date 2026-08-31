//! [`MediaType`] — a parsed, normalized media type (RFC 6838 §4.2
//! naming; RFC 2045 §5.1 parameters), the typed key of the profile
//! registry.

mod parse;

#[cfg(test)]
mod tests;

use std::fmt;
use std::str::FromStr;

/// A media type: `type/subtype` plus zero or more `name=value`
/// parameters.
///
/// Normalized on construction: the type, subtype, and parameter
/// names are ASCII lower-case, parameter values are unquoted, and
/// parameters are ordered by name — so two spellings of the same
/// media type compare and hash equal. Registry lookups key on the
/// [essence](Self::essence) (`type/subtype`) and ignore parameters.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MediaType {
    /// `type/subtype`, lower-case.
    essence: Box<str>,
    /// Byte offset of the `/` in `essence`.
    slash: usize,
    // bounded: at most MAX_MEDIA_TYPE_PARAMETERS entries.
    parameters: Box<[Parameter]>,
}

/// One normalized `name=value` parameter.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Parameter {
    /// Lower-case token.
    name: Box<str>,
    /// Unquoted value.
    value: Box<str>,
}

impl MediaType {
    /// Parse and normalize `input`.
    ///
    /// # Errors
    ///
    /// Returns a [`MediaTypeError`] naming the first grammar
    /// violation: a missing `/`, an invalid or over-long name, a
    /// malformed or duplicated parameter, or too many parameters.
    pub fn parse(input: &str) -> Result<Self, MediaTypeError> {
        parse::parse(input)
    }

    /// `type/subtype`, lower-case, without parameters.
    #[must_use]
    pub fn essence(&self) -> &str {
        &self.essence
    }

    /// The top-level type (`text` in `text/plain`).
    #[must_use]
    pub fn top_level(&self) -> &str {
        &self.essence[..self.slash]
    }

    /// The subtype (`plain` in `text/plain`; `svg+xml` in
    /// `image/svg+xml`).
    #[must_use]
    pub fn subtype(&self) -> &str {
        &self.essence[self.slash + 1..]
    }

    /// The structured-syntax suffix (RFC 6838 §4.2.8): `xml` in
    /// `image/svg+xml`, `None` when the subtype has no `+`.
    #[must_use]
    pub fn structured_suffix(&self) -> Option<&str> {
        self.subtype()
            .rfind('+')
            .map(|plus| &self.subtype()[plus + 1..])
    }

    /// The value of parameter `name` (matched case-insensitively).
    #[must_use]
    pub fn parameter(&self, name: &str) -> Option<&str> {
        self.parameters
            .iter()
            .find(|parameter| parameter.name.eq_ignore_ascii_case(name))
            .map(|parameter| &*parameter.value)
    }

    /// Every parameter as `(name, value)`, ordered by name.
    pub fn parameters(&self) -> impl Iterator<Item = (&str, &str)> {
        self.parameters
            .iter()
            .map(|parameter| (&*parameter.name, &*parameter.value))
    }
}

impl FromStr for MediaType {
    type Err = MediaTypeError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse(input)
    }
}

impl fmt::Display for MediaType {
    /// The canonical rendering: the essence, then each parameter as
    /// `;name=value`, quoting a value only when it is not a token.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.essence)?;
        for parameter in &self.parameters {
            write!(f, ";{}=", parameter.name)?;
            if parse::is_token(&parameter.value) {
                f.write_str(&parameter.value)?;
            } else {
                f.write_str("\"")?;
                for ch in parameter.value.chars() {
                    if ch == '"' || ch == '\\' {
                        f.write_str("\\")?;
                    }
                    write!(f, "{ch}")?;
                }
                f.write_str("\"")?;
            }
        }
        Ok(())
    }
}

/// A media type failed to parse.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MediaTypeError {
    /// The input was empty or whitespace.
    #[error("media type is empty")]
    Empty,
    /// The input has no `/` between type and subtype.
    #[error("media type has no '/' separating type and subtype")]
    MissingSlash,
    /// The top-level type is empty or holds a character outside the
    /// RFC 6838 restricted-name grammar.
    #[error("invalid top-level type character at byte {offset}")]
    InvalidTopLevel {
        /// Byte offset into the input.
        offset: usize,
    },
    /// The subtype is empty or holds a character outside the
    /// RFC 6838 restricted-name grammar.
    #[error("invalid subtype character at byte {offset}")]
    InvalidSubtype {
        /// Byte offset into the input.
        offset: usize,
    },
    /// The type or subtype exceeds the RFC 6838 127-byte limit.
    #[error("{component} exceeds {limit} bytes")]
    NameTooLong {
        /// `type` or `subtype`.
        component: &'static str,
        /// The frozen limit.
        limit: usize,
    },
    /// A parameter is not `token=token` or `token="quoted-string"`.
    #[error("malformed parameter at byte {offset}")]
    MalformedParameter {
        /// Byte offset into the input.
        offset: usize,
    },
    /// The same parameter name appears twice.
    #[error("duplicate parameter '{name}'")]
    DuplicateParameter {
        /// The lower-case name.
        name: String,
    },
    /// More parameters than the frozen bound.
    #[error("more than {limit} parameters")]
    TooManyParameters {
        /// The frozen limit.
        limit: usize,
    },
}
