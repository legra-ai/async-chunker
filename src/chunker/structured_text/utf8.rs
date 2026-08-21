//! [`Utf8Scanner`] — incremental, allocation-free UTF-8 validation
//! that also reports scalar boundaries.

/// Why a byte was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Utf8Fault {
    /// A byte that cannot begin a scalar (a stray continuation byte,
    /// or `C0`/`C1`/`F5..FF`).
    InvalidLeadByte,
    /// A byte inside a multi-byte scalar outside its permitted range
    /// (overlong form, surrogate, above `U+10FFFF`, or a non-
    /// continuation byte).
    InvalidContinuation,
}

impl Utf8Fault {
    /// The frozen diagnostic text for the fault.
    pub(super) const fn detail(self) -> &'static str {
        match self {
            Self::InvalidLeadByte => "invalid UTF-8 lead byte",
            Self::InvalidContinuation => "invalid UTF-8 continuation byte",
        }
    }
}

/// Byte-at-a-time UTF-8 validator.
///
/// Implements the well-formed byte-sequence table of Unicode §3.9
/// (Table 3-7) directly: the lead byte fixes the sequence length and
/// the admissible range of the second byte, every later byte is
/// `80..=BF`. Holds three bytes of state.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Utf8Scanner {
    /// Continuation bytes still expected for the current scalar.
    remaining: u8,
    /// Admissible range of the next byte (only meaningful while
    /// `remaining > 0`).
    next_lo: u8,
    next_hi: u8,
}

impl Utf8Scanner {
    /// Consume one byte. `Ok(true)` when it completed a scalar,
    /// `Ok(false)` when a multi-byte scalar is still open.
    pub(super) fn push(&mut self, byte: u8) -> Result<bool, Utf8Fault> {
        if self.remaining == 0 {
            return self.begin(byte);
        }
        if byte < self.next_lo || byte > self.next_hi {
            return Err(Utf8Fault::InvalidContinuation);
        }
        self.remaining -= 1;
        self.next_lo = 0x80;
        self.next_hi = 0xBF;
        Ok(self.remaining == 0)
    }

    /// Whether the scanner sits between scalars.
    pub(super) const fn at_boundary(self) -> bool {
        self.remaining == 0
    }

    fn begin(&mut self, lead: u8) -> Result<bool, Utf8Fault> {
        let (remaining, lo, hi) = match lead {
            0x00..=0x7F => return Ok(true),
            0xC2..=0xDF => (1, 0x80, 0xBF),
            0xE0 => (2, 0xA0, 0xBF),
            0xE1..=0xEC | 0xEE..=0xEF => (2, 0x80, 0xBF),
            0xED => (2, 0x80, 0x9F),
            0xF0 => (3, 0x90, 0xBF),
            0xF1..=0xF3 => (3, 0x80, 0xBF),
            0xF4 => (3, 0x80, 0x8F),
            _ => return Err(Utf8Fault::InvalidLeadByte),
        };
        self.remaining = remaining;
        self.next_lo = lo;
        self.next_hi = hi;
        Ok(false)
    }
}
