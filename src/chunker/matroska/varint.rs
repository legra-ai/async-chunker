//! EBML variable-length integers, decoded byte-at-a-time.

/// The length an ID varint declares, from its first byte: one
/// leading-zero count up to four bytes. IDs keep their marker bit.
pub(super) fn id_len(first: u8) -> Option<usize> {
    match first.leading_zeros() {
        0 => Some(1),
        1 => Some(2),
        2 => Some(3),
        3 => Some(4),
        _ => None,
    }
}

/// The length a size varint declares, from its first byte: up to
/// eight bytes. A zero first byte would mean more and is invalid.
pub(super) fn size_len(first: u8) -> Option<usize> {
    let zeros = first.leading_zeros() as usize;
    (zeros < 8).then_some(zeros + 1)
}

/// The ID's documented form: its raw bytes as an integer.
pub(super) fn id_value(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .fold(0u32, |acc, &byte| (acc << 8) | u32::from(byte))
}

/// A size varint's value with the marker stripped, and whether it is
/// the reserved all-ones "unknown size".
pub(super) fn size_value(bytes: &[u8]) -> (u64, bool) {
    let marker = 0x80u8 >> (bytes.len() - 1);
    let mut value = u64::from(bytes[0] & (marker - 1));
    for &byte in &bytes[1..] {
        value = (value << 8) | u64::from(byte);
    }
    let all_ones = (1u64 << (7 * bytes.len())) - 1;
    (value, value == all_ones)
}
