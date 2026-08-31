//! `zip-v1`: the frozen chunking profile for ZIP containers and the
//! OOXML/ODF families — a forward-only, bounded, fail-hard walker
//! that places cuts at member boundaries and never inflates.

mod chunker;
pub(in crate::chunker) mod fault;
pub(in crate::chunker) mod records;
pub(in crate::chunker) mod walker;

#[cfg(test)]
pub(in crate::chunker) mod tests;

pub use chunker::ZipChunker;
