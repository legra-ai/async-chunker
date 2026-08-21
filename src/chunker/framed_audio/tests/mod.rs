//! `framed-audio-v1` regression tests: frozen boundaries per format,
//! frame-aligned cuts, metadata and multi-stream reuse, and the
//! fail-hard malformed corpus.

mod malformed;
mod reuse;
mod writer;
