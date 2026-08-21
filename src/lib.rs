#![doc = include_str!("../README.md")]

mod error;
mod stream;

mod chunker;
mod constants;
pub mod profile;

pub use chunker::{
    ChunkBoundaries, Chunker, FramedAudioChunker, GenericCdcChunker, IsobmffChunker,
    MatroskaChunker, MpegtsChunker, ProfileChunker, StructuredTextChunker, ZipChunker,
};
pub use constants::{
    FRAMED_AUDIO_RELAXED_MASK, FRAMED_AUDIO_STRICT_MASK, GENERIC_CDC_CHUNK_MAX_BYTES,
    GENERIC_CDC_CHUNK_MIN_BYTES, GENERIC_CDC_CHUNK_TARGET_BYTES, MPEGTS_RELAXED_MASK,
    MPEGTS_STRICT_MASK, STRUCTURED_TEXT_RELAXED_MASK, STRUCTURED_TEXT_STRICT_MASK,
};
pub use error::ChunkError;
pub use profile::{ChunkingProfile, ChunkingProfileId};
pub use stream::{AsyncChunker, ChunkStream};
