//! `isobmff-v1` regression tests: frozen boundaries on MP4/HEIF
//! corpora, box-boundary cuts and reuse, every header form,
//! feed-order independence, and the fail-hard malformed corpus.

mod malformed;
mod reuse;
mod writer;
