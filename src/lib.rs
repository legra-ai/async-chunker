#![doc = include_str!("../README.md")]

mod error;
mod inflate;
mod media_type;
mod probe;
mod registry;
mod replay;
mod stream;

mod chunker;
mod constants;
pub mod decompose;
pub mod profile;

pub use chunker::{
    ChunkBoundaries, Chunker, FramedAudioChunker, GenericCdcChunker, IsobmffChunker,
    MatroskaChunker, MpegtsChunker, OfficeKind, OoxmlBerChunker, OoxmlChunker, PackageObserver,
    PdfChunker, ProfileChunker, StructuredTextChunker, ZipChunker,
};
pub use constants::{
    FRAMED_AUDIO_RELAXED_MASK, FRAMED_AUDIO_STRICT_MASK, GENERIC_CDC_CHUNK_MAX_BYTES,
    GENERIC_CDC_CHUNK_MIN_BYTES, GENERIC_CDC_CHUNK_TARGET_BYTES, MAX_MEDIA_TYPE_NAME_BYTES,
    MAX_MEDIA_TYPE_PARAMETERS, MPEGTS_RELAXED_MASK, MPEGTS_STRICT_MASK, PROBE_PREFIX_MAX_BYTES,
    STRUCTURED_TEXT_RELAXED_MASK, STRUCTURED_TEXT_STRICT_MASK,
};
pub use error::ChunkError;
pub use media_type::{MediaType, MediaTypeError};
pub use probe::{Detection, Detector, ProfileSet};
pub use profile::{ChunkingProfile, ChunkingProfileId};
pub use registry::{ProfileRegistry, RegistryVersion};
pub use replay::PrefixReplay;
pub use stream::{AsyncChunker, ChunkStream};
