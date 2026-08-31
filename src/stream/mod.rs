//! Async adapters: the bounded chunk stream and the probing entry
//! points.

mod chunker;
mod probe;

#[cfg(test)]
mod tests;

pub use chunker::{AsyncChunker, ChunkStream};
