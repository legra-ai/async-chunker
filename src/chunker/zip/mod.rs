//! `zip-v1`: the frozen chunking profile for ZIP containers and the
//! OOXML/ODF families — a forward-only, bounded, fail-hard walker
//! that places cuts at member boundaries and never inflates.

mod chunker;
pub(crate) mod fault;
pub(crate) mod records;
pub(crate) mod walker;

#[cfg(test)]
pub(crate) mod tests;

pub use chunker::ZipChunker;
