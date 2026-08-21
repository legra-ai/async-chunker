//! `framed-audio-v1`: the frozen chunking profile for MP3, ADTS/AAC,
//! and FLAC streams — fail-hard frame walking with frame-aligned
//! chunks, and per-byte content-defined cuts inside opaque tag,
//! metadata, and FLAC audio regions.

mod adts;
mod chunker;
mod fault;
mod flac;
mod id3;
mod mp3;
mod walker;

#[cfg(test)]
mod tests;

pub use chunker::FramedAudioChunker;
