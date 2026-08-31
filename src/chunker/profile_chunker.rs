//! [`Chunker`] — the capability every chunking profile implements —
//! and [`ProfileChunker`], the registry-selected instance a sink
//! drives.

use crate::ChunkError;

use super::framed_audio::FramedAudioChunker;
use super::generic::GenericCdcChunker;
use super::isobmff::IsobmffChunker;
use super::matroska::MatroskaChunker;
use super::mpegts::MpegtsChunker;
use super::office::{OoxmlBerChunker, OoxmlChunker, PackageObserver};
use super::pdf::PdfChunker;
use super::structured_text::StructuredTextChunker;
use super::zip::ZipChunker;
use crate::profile::ChunkingProfile;

/// A streaming boundary detector for one chunking profile.
///
/// Pure CPU — bytes in, boundaries out — holding at most one
/// maximum-size chunk of state. Boundaries are a pure function of
/// the profile and the bytes, never of how the bytes were windowed.
///
/// Emitted chunks concatenate to the profile's **canonical form**
/// of the input. For every profile except `ooxml-v1` that is the
/// input itself; the canonicalizing `ooxml-v1` emits the package's
/// deterministic canonical repackaging instead.
pub trait Chunker {
    /// Feed one input window; `emit` receives every chunk completed
    /// within it.
    ///
    /// # Errors
    ///
    /// A structured profile returns
    /// [`ChunkError::MalformedProfileInput`] when the bytes
    /// stop parsing as the profile's structure; the stream is then
    /// rejected for good.
    fn push(&mut self, window: &[u8], emit: &mut dyn FnMut(&[u8])) -> Result<(), ChunkError>;

    /// End the stream: flush the trailing chunk (the only one that
    /// may fall below the minimum size) and reset to a fresh-stream
    /// state.
    ///
    /// # Errors
    ///
    /// A structured profile rejects a stream that ends inside an
    /// incomplete structure.
    fn finish(&mut self, emit: &mut dyn FnMut(&[u8])) -> Result<(), ChunkError>;
}

/// The chunker the registry selects for one profile. Each chunker
/// carries its own gear table and buffers, so the variants are boxed.
pub enum ProfileChunker {
    /// `generic-cdc-v1`.
    GenericCdc(Box<GenericCdcChunker>),
    /// `structured-text-v1`.
    StructuredText(Box<StructuredTextChunker>),
    /// `zip-v1`.
    Zip(Box<ZipChunker>),
    /// `isobmff-v1`.
    Isobmff(Box<IsobmffChunker>),
    /// `matroska-v1`.
    Matroska(Box<MatroskaChunker>),
    /// `mpegts-v1`.
    Mpegts(Box<MpegtsChunker>),
    /// `framed-audio-v1`.
    FramedAudio(Box<FramedAudioChunker>),
    /// `ooxml-v1` (canonicalizing — chunks concatenate to the
    /// package's canonical form, not to the input).
    Ooxml(Box<OoxmlChunker>),
    /// `ooxml-ber-v1`.
    OoxmlBer(Box<OoxmlBerChunker>),
    /// `pdf-v1`.
    Pdf(Box<PdfChunker>),
}

impl ProfileChunker {
    /// Open the chunker for `profile`.
    ///
    /// # Errors
    ///
    /// Would return [`ChunkError::ProfileUnimplemented`]
    /// for a registered profile without an implementation; since
    /// ELS-11 every registry profile is implemented, so the fallible
    /// signature remains only for that frozen contract.
    pub fn open(profile: ChunkingProfile) -> Result<Self, ChunkError> {
        match profile {
            ChunkingProfile::GenericCdcV1 => Ok(Self::GenericCdc(Box::default())),
            ChunkingProfile::StructuredTextV1 => Ok(Self::StructuredText(Box::default())),
            ChunkingProfile::ZipV1 => Ok(Self::Zip(Box::default())),
            ChunkingProfile::IsobmffV1 => Ok(Self::Isobmff(Box::default())),
            ChunkingProfile::MatroskaV1 => Ok(Self::Matroska(Box::default())),
            ChunkingProfile::MpegtsV1 => Ok(Self::Mpegts(Box::default())),
            ChunkingProfile::FramedAudioV1 => Ok(Self::FramedAudio(Box::default())),
            ChunkingProfile::OoxmlV1 => Ok(Self::Ooxml(Box::default())),
            ChunkingProfile::OoxmlBerV1 => Ok(Self::OoxmlBer(Box::default())),
            ChunkingProfile::PdfV1 => Ok(Self::Pdf(Box::default())),
        }
    }

    /// Attach a package-event tap. Only the canonicalizing
    /// `ooxml-v1` chunker observes package events; returns whether
    /// the observer was accepted.
    pub fn set_package_observer(&mut self, observer: Box<dyn PackageObserver>) -> bool {
        match self {
            Self::Ooxml(chunker) => {
                chunker.set_observer(observer);
                true
            }
            _ => false,
        }
    }

    fn inner(&mut self) -> &mut dyn Chunker {
        match self {
            Self::GenericCdc(chunker) => chunker.as_mut(),
            Self::StructuredText(chunker) => chunker.as_mut(),
            Self::Zip(chunker) => chunker.as_mut(),
            Self::Isobmff(chunker) => chunker.as_mut(),
            Self::Matroska(chunker) => chunker.as_mut(),
            Self::Mpegts(chunker) => chunker.as_mut(),
            Self::FramedAudio(chunker) => chunker.as_mut(),
            Self::Ooxml(chunker) => chunker.as_mut(),
            Self::OoxmlBer(chunker) => chunker.as_mut(),
            Self::Pdf(chunker) => chunker.as_mut(),
        }
    }
}

impl Chunker for ProfileChunker {
    fn push(&mut self, window: &[u8], emit: &mut dyn FnMut(&[u8])) -> Result<(), ChunkError> {
        self.inner().push(window, emit)
    }

    fn finish(&mut self, emit: &mut dyn FnMut(&[u8])) -> Result<(), ChunkError> {
        self.inner().finish(emit)
    }
}
