//! `mpegts-v1` regression tests: frozen boundaries, packet-aligned
//! seam-candidate cuts, reuse across re-segmentation and splices,
//! and the fail-hard malformed corpus.

mod malformed;
mod reuse;
mod writer;
