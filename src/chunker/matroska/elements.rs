//! The frozen EBML element vocabulary the walker distinguishes.
//!
//! IDs are the raw marker-carrying bytes as an integer, the form
//! Matroska documents them in. Everything not named here is opaque
//! payload wherever a valid element is admissible.

/// The EBML header element beginning every stream.
pub(super) const EBML_HEADER: u32 = 0x1A45_DFA3;
/// The one top-level container.
pub(super) const SEGMENT: u32 = 0x1853_8067;
/// Dead space, admissible at top level and anywhere an element is.
pub(super) const VOID: u32 = 0xEC;
/// The media container; the profile's principal unit.
pub(super) const CLUSTER: u32 = 0x1F43_B675;

/// Segment-level elements: each begins a unit, and each closes an
/// open unknown-size cluster.
const SEGMENT_LEVEL: [u32; 8] = [
    CLUSTER,
    0x114D_9B74, /* SeekHead */
    0x1549_A966, /* Info */
    0x1654_AE6B, /* Tracks */
    0x1C53_BB6B, /* Cues */
    0x1043_A770, /* Chapters */
    0x1254_C367, /* Tags */
    0x1941_A469, /* Attachments */
];

/// Children an open (unknown-size) cluster may contain.
const CLUSTER_CHILDREN: [u32; 9] = [
    0xE7,   /* Timestamp */
    0xA7,   /* Position */
    0xAB,   /* PrevSize */
    0xA3,   /* SimpleBlock */
    0xA0,   /* BlockGroup */
    0xAF,   /* EncryptedBlock */
    0x5854, /* SilentTracks */
    VOID, 0xBF, /* CRC-32 */
];

/// Whether `id` is a segment-level element.
pub(super) fn is_segment_level(id: u32) -> bool {
    SEGMENT_LEVEL.contains(&id)
}

/// Whether `id` may appear inside an open cluster.
pub(super) fn is_cluster_child(id: u32) -> bool {
    CLUSTER_CHILDREN.contains(&id)
}
