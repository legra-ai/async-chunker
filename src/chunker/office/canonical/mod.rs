//! The canonicalizing `ooxml-v1` profile: members are inflated and
//! re-emitted deterministically; chunks concatenate to that
//! canonical form.

mod chunker;
mod core;
mod decoder;
pub(crate) mod writer;

pub use chunker::OoxmlChunker;
