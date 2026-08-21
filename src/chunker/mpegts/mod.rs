//! `mpegts-v1`: the frozen chunking profile for MPEG transport
//! streams — a fail-hard 188-byte packet framer with packet-aligned,
//! seam-candidate chunking.

mod chunker;
mod fault;
mod packet;

#[cfg(test)]
mod tests;

pub use chunker::MpegtsChunker;
