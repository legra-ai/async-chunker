//! Gear rolling-hash tables and cut masks shared by the
//! content-defined profiles.
//!
//! A table is derived deterministically from a versioned seed so
//! every node computes identical boundaries; a different seed is a
//! different profile, not a tuning change.

use crate::constants::GENERIC_CDC_CHUNK_TARGET_BYTES;

/// Frozen seed for the `generic-cdc-v1` gear table.
pub(super) const GENERIC_CDC_GEAR_SEED: &str = "legra/generic-cdc-v1/gear";

/// Frozen seed for the `structured-text-v1` gear table.
pub(super) const STRUCTURED_TEXT_GEAR_SEED: &str = "legra/structured-text-v1/gear";

/// Frozen seed for the `zip-v1` gear table.
pub(super) const ZIP_GEAR_SEED: &str = "legra/zip-v1/gear";

/// Frozen seed for the `isobmff-v1` gear table.
pub(super) const ISOBMFF_GEAR_SEED: &str = "legra/isobmff-v1/gear";

/// Frozen seed for the `matroska-v1` gear table.
pub(super) const MATROSKA_GEAR_SEED: &str = "legra/matroska-v1/gear";

/// Frozen seed for the `mpegts-v1` gear table.
pub(super) const MPEGTS_GEAR_SEED: &str = "legra/mpegts-v1/gear";

/// Frozen seed for the `framed-audio-v1` gear table.
pub(super) const FRAMED_AUDIO_GEAR_SEED: &str = "legra/framed-audio-v1/gear";

/// Frozen seed for the `ooxml-v1` gear table.
pub(super) const OOXML_GEAR_SEED: &str = "legra/ooxml-v1/gear";

/// Frozen seed for the `ooxml-ber-v1` gear table.
pub(super) const OOXML_BER_GEAR_SEED: &str = "legra/ooxml-ber-v1/gear";

/// Frozen seed for the `pdf-v1` gear table.
pub(super) const PDF_GEAR_SEED: &str = "legra/pdf-v1/gear";

/// Byte values a gear table covers.
const TABLE_SIZE: usize = 256;

/// A 256-entry gear table plus the rolling hash it drives.
///
/// The hash is updated per byte as `hash << 1 + table[byte]`, so
/// only the most recent 64 bytes influence it: boundaries are local
/// to content and re-synchronize after an edit.
#[derive(Debug, Clone)]
pub(super) struct GearHash {
    table: [u64; TABLE_SIZE],
    hash: u64,
}

impl GearHash {
    /// A fresh hash over the table derived from `seed`.
    pub(super) fn new(seed: &str) -> Self {
        let mut raw = [0u8; TABLE_SIZE * 8];
        let mut hasher = blake3::Hasher::new();
        hasher.update(seed.as_bytes());
        hasher.finalize_xof().fill(&mut raw);
        let mut table = [0u64; TABLE_SIZE];
        for (entry, chunk) in table.iter_mut().zip(raw.chunks_exact(8)) {
            *entry = u64::from_be_bytes(chunk.try_into().expect("gear table uses 8-byte entries"));
        }
        Self { table, hash: 0 }
    }

    /// Roll one byte in.
    pub(super) fn update(&mut self, byte: u8) {
        self.hash = (self.hash << 1).wrapping_add(self.table[usize::from(byte)]);
    }

    /// Whether the current hash masks to zero under `mask`.
    pub(super) fn cuts(&self, mask: u64) -> bool {
        self.hash & mask == 0
    }

    /// Restart the hash for a fresh chunk.
    pub(super) fn reset(&mut self) {
        self.hash = 0;
    }
}

/// `generic-cdc-v1` cut mask applied *before* the target length:
/// four times the target's expected spacing, so early cuts are rare
/// and chunks gravitate toward the target.
pub(super) const fn generic_strict_mask() -> u64 {
    ((GENERIC_CDC_CHUNK_TARGET_BYTES as u64) * 4).next_power_of_two() - 1
}

/// `generic-cdc-v1` cut mask applied *after* the target length: a
/// quarter of the target's spacing, so an overlong chunk closes
/// quickly.
pub(super) const fn generic_relaxed_mask() -> u64 {
    ((GENERIC_CDC_CHUNK_TARGET_BYTES as u64) / 4).next_power_of_two() - 1
}
