//! The chunking profiles: streaming boundary detectors for
//! external-literal ingest.
//!
//! Pure CPU — bytes in, boundaries out. No I/O, no async, no
//! allocation beyond one bounded chunk buffer per chunker.

mod assembler;
mod boundaries;
mod framed_audio;
mod gear;
mod generic;
mod isobmff;
mod matroska;
mod mpegts;
mod office;
mod pdf;
mod profile_chunker;
mod structured_text;
mod zip;

#[cfg(test)]
mod tests;

pub use boundaries::ChunkBoundaries;
pub use framed_audio::FramedAudioChunker;
pub use generic::GenericCdcChunker;
pub use isobmff::IsobmffChunker;
pub use matroska::MatroskaChunker;
pub use mpegts::MpegtsChunker;
pub use office::{OfficeKind, OoxmlBerChunker, OoxmlChunker, PackageObserver};
pub use pdf::PdfChunker;
pub use profile_chunker::{Chunker, ProfileChunker};
pub use structured_text::StructuredTextChunker;
pub use zip::ZipChunker;
