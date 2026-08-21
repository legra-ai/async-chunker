//! `matroska-v1`: the frozen chunking profile for Matroska and `WebM`
//! — a forward-only, bounded, fail-hard EBML walker that places cuts
//! at segment-child boundaries (clusters above all) and never
//! decodes a payload.

mod chunker;
mod elements;
mod fault;
mod varint;
mod walker;

#[cfg(test)]
mod tests;

pub use chunker::MatroskaChunker;
