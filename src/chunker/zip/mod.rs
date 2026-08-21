//! `zip-v1`: the frozen chunking profile for ZIP containers and the
//! OOXML/ODF families — a forward-only, bounded, fail-hard walker
//! that places cuts at member boundaries and never inflates.

mod chunker;
mod fault;
mod records;
mod walker;

#[cfg(test)]
mod tests;

pub use chunker::ZipChunker;
