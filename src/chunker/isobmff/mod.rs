//! `isobmff-v1`: the frozen chunking profile for the ISO Base Media
//! File Format family (MP4/MOV/M4A/HEIF/HEIC/AVIF) — a forward-only,
//! bounded-depth, fail-hard box walker that places cuts at box
//! boundaries and never decodes a payload.

mod boxes;
mod chunker;
mod fault;
mod walker;

#[cfg(test)]
mod tests;

pub use chunker::IsobmffChunker;
