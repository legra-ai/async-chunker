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
pub use error::ChunkError;
pub use profile::{ChunkingProfile, ChunkingProfileId};
pub use stream::{AsyncChunker, ChunkStream};
