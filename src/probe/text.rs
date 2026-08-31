//! The structured-text prefix probe.

use crate::constants::PROBE_PREFIX_MAX_BYTES;

/// Whether `prefix` reads as structured text: non-empty, well-formed
/// UTF-8 (a scalar cut by the prefix bound is tolerated), and free of
/// C0 control characters other than tab, line feed, form feed, and
/// carriage return. The control rule is stricter than the
/// `structured-text-v1` engine, which validates UTF-8 only; it keeps
/// binary containers with printable signatures from also probing as
/// text.
pub(super) fn is_structured_text_prefix(prefix: &[u8]) -> bool {
    if prefix.is_empty() {
        return false;
    }
    let valid = match std::str::from_utf8(prefix) {
        Ok(text) => text,
        Err(error) => {
            let cut_at_bound =
                error.error_len().is_none() && prefix.len() >= PROBE_PREFIX_MAX_BYTES;
            if !cut_at_bound {
                return false;
            }
            std::str::from_utf8(&prefix[..error.valid_up_to()]).expect("valid up to the cut")
        }
    };
    valid
        .bytes()
        .all(|byte| byte >= 0x20 || matches!(byte, b'\t' | b'\n' | b'\x0C' | b'\r'))
}
