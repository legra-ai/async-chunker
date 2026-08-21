//! Stable constants shared by the built-in chunking profiles.

/// `generic-cdc-v1` minimum chunk length — no cut is judged below
/// this.
pub const GENERIC_CDC_CHUNK_MIN_BYTES: usize = 16 << 10;

/// `generic-cdc-v1` target (expected) chunk length — the measured
/// reuse optimum for the generic profile.
pub const GENERIC_CDC_CHUNK_TARGET_BYTES: usize = 64 << 10;

/// `generic-cdc-v1` maximum chunk length — forced cut, and the
/// upper bound emitted by the generic profile.
pub const GENERIC_CDC_CHUNK_MAX_BYTES: usize = 256 << 10;

/// `structured-text-v1` cut mask judged at *line-end* candidates
/// before the target length: the hash's top twelve bits, so one
/// candidate in 4096 qualifies and with ordinary line lengths few
/// chunks close early. Structured-text masks select the *high* bits
/// of the gear hash because those depend on the full 64-byte
/// window, whereas the low bits depend only on the last few bytes —
/// which at a whitespace candidate are mostly the preceding word and
/// would make cut decisions repeat with the vocabulary.
pub const STRUCTURED_TEXT_STRICT_MASK: u64 = 0x3FF << 54;

/// `structured-text-v1` cut mask judged at any candidate (line end
/// or soft break) once the target length is reached: the hash's top
/// eight bits, so one candidate in 256 qualifies and an overlong
/// chunk closes within a few hundred bytes of the target in
/// whitespace- or punctuation-dense text.
pub const STRUCTURED_TEXT_RELAXED_MASK: u64 = 0x7FF << 53;

/// `mpegts-v1` cut mask judged at seam candidates (payload-unit
/// starts, discontinuities) before the target length: the hash's top
/// four bits, so one candidate in 16 qualifies. Candidates are whole
/// packets apart — sparser by orders of magnitude than the per-byte
/// generic rule — so the masks are correspondingly weaker; high bits
/// for the same reason as `structured-text-v1`.
pub const MPEGTS_STRICT_MASK: u64 = 0xF << 60;

/// `mpegts-v1` cut mask judged at seam candidates once the target
/// length is reached: the hash's top two bits, one candidate in
/// four, so an overlong chunk closes within a few seams.
pub const MPEGTS_RELAXED_MASK: u64 = 0x3 << 62;

/// `framed-audio-v1` cut mask judged at frame seams before the
/// target length: the hash's top seven bits, one candidate in 128.
/// Frames are a few hundred bytes apart — between the per-byte
/// generic rule and `mpegts-v1`'s packet groups — so the mask sits
/// between theirs; high bits for the same reason as
/// `structured-text-v1`.
pub const FRAMED_AUDIO_STRICT_MASK: u64 = 0x7F << 57;

/// `framed-audio-v1` cut mask judged at frame seams once the target
/// length is reached: the hash's top five bits, one candidate in 32.
pub const FRAMED_AUDIO_RELAXED_MASK: u64 = 0x1F << 59;
