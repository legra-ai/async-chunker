//! The frozen box vocabulary: which types a stream may begin with and
//! which containers the walker descends into.

/// A four-character box type.
pub(super) type BoxType = [u8; 4];

/// `uuid` boxes carry a 16-byte extended type after the header.
pub(super) const UUID: BoxType = *b"uuid";

/// Types an ISO BMFF stream may begin with (ISO 14496-12 §4.3 and
/// the segment/QuickTime variants in the wild).
const FIRST_BOXES: [BoxType; 9] = [
    *b"ftyp", *b"styp", *b"sidx", *b"moov", *b"moof", *b"free", *b"skip", *b"wide", *b"mdat",
];

/// Pure containers the walker descends into (their payload is a
/// sequence of boxes). Everything else — `mdat`, sample tables,
/// `meta` (a `FullBox` whose `QuickTime` variant is not), `uuid`,
/// unknown types — is opaque payload that is counted, never decoded.
const CONTAINERS: [BoxType; 14] = [
    *b"moov", *b"trak", *b"edts", *b"mdia", *b"minf", *b"dinf", *b"stbl", *b"mvex", *b"moof",
    *b"traf", *b"mfra", *b"udta", *b"sinf", *b"schi",
];

/// Whether `kind` may begin a stream.
pub(super) fn may_begin_stream(kind: BoxType) -> bool {
    FIRST_BOXES.contains(&kind)
}

/// Whether the walker descends into `kind`.
pub(super) fn is_container(kind: BoxType) -> bool {
    CONTAINERS.contains(&kind)
}
