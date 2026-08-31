//! Async adapters for bounded profile chunkers.

use std::pin::Pin;

use futures_core::Stream;
use tokio::io::{AsyncRead, AsyncReadExt};

use crate::media_type::MediaType;
use crate::probe::Detector;
use crate::profile::ChunkingProfile;
use crate::registry::ProfileRegistry;
use crate::{ChunkError, Chunker, ProfileChunker};

const READ_WINDOW_BYTES: usize = 64 << 10;

/// A bounded-memory asynchronous chunking operation.
pub struct AsyncChunker {
    profile: ProfileChunker,
}

impl AsyncChunker {
    /// Opens a chunker for `profile`.
    ///
    /// # Errors
    ///
    /// Returns [`ChunkError::ProfileUnimplemented`] if a registered profile
    /// has no implementation.
    pub fn new(profile: ChunkingProfile) -> Result<Self, ChunkError> {
        Ok(Self {
            profile: ProfileChunker::open(profile)?,
        })
    }

    /// Opens the chunker `registry` selects for a declared
    /// `media_type`, without looking at any bytes.
    ///
    /// # Errors
    ///
    /// Returns [`ChunkError::ProfileUnimplemented`] if the selected
    /// profile has no implementation.
    pub fn declared(
        media_type: &MediaType,
        registry: &ProfileRegistry,
    ) -> Result<Self, ChunkError> {
        Self::new(registry.select(media_type))
    }

    /// Probes the first bytes of `reader` with [`Detector::V1`],
    /// chunks with the recognized specialist — or the explicit
    /// generic profile when nothing matched — and replays the probed
    /// prefix so no byte is lost.
    ///
    /// # Errors
    ///
    /// Returns [`ChunkError::AmbiguousDetection`] when more than one
    /// specialist matched (declare a media type instead), or
    /// [`ChunkError::Io`] when the probe read fails.
    pub async fn chunk_detected<R>(reader: R) -> Result<ChunkStream, ChunkError>
    where
        R: AsyncRead + Unpin + Send + 'static,
    {
        let (detection, replay) = Detector::V1.probe(reader).await?;
        Ok(Self::new(detection.resolve()?)?.chunk(replay))
    }

    /// Chunks `reader` with the profile `registry` selects for the
    /// declared `media_type`, after probing the first bytes with
    /// [`Detector::V1`] and refusing a positive contradiction (see
    /// [`Detection::reconcile`](crate::Detection::reconcile)).
    ///
    /// # Errors
    ///
    /// Returns [`ChunkError::DeclaredDetectedMismatch`] when the
    /// bytes are recognized as a different specialist than the one
    /// declared, or [`ChunkError::Io`] when the probe read fails.
    pub async fn chunk_declared<R>(
        media_type: &MediaType,
        registry: &ProfileRegistry,
        reader: R,
    ) -> Result<ChunkStream, ChunkError>
    where
        R: AsyncRead + Unpin + Send + 'static,
    {
        let declared = registry.select(media_type);
        let (detection, replay) = Detector::V1.probe(reader).await?;
        Ok(Self::new(detection.reconcile(declared)?)?.chunk(replay))
    }

    /// Turns an asynchronous byte reader into a stream of owned chunks.
    ///
    /// The stream reads at most 64 KiB at a time and yields each completed
    /// chunk before processing the next input byte. It never collects the
    /// input or the output chunks, so a caller can apply backpressure by
    /// awaiting the next item.
    ///
    /// # Panics
    ///
    /// Panics if a profile violates the chunker contract by emitting two
    /// chunks while processing one input byte.
    pub fn chunk<R>(self, mut reader: R) -> ChunkStream
    where
        R: AsyncRead + Unpin + Send + 'static,
    {
        let stream = async_stream::try_stream! {
            let mut reader_buffer = vec![0_u8; READ_WINDOW_BYTES].into_boxed_slice();
            let mut chunker = self.profile;
            let mut pending = None;

            loop {
                let read = reader
                    .read(&mut reader_buffer)
                    .await
                    .map_err(|error| ChunkError::Io(error.to_string()))?;
                if read == 0 {
                    chunker.finish(&mut |chunk| {
                        assert!(pending.is_none(), "a chunker emitted two chunks for one byte");
                        pending = Some(chunk.to_vec().into_boxed_slice());
                    })?;
                    if let Some(chunk) = pending.take() {
                        yield chunk;
                    }
                    break;
                }

                for byte in &reader_buffer[..read] {
                    chunker.push(std::slice::from_ref(byte), &mut |chunk| {
                        assert!(pending.is_none(), "a chunker emitted two chunks for one byte");
                        pending = Some(chunk.to_vec().into_boxed_slice());
                    })?;
                    if let Some(chunk) = pending.take() {
                        yield chunk;
                    }
                }
            }
        };

        Box::pin(stream)
    }
}

/// The asynchronous stream returned by [`AsyncChunker::chunk`].
pub type ChunkStream = Pin<Box<dyn Stream<Item = Result<Box<[u8]>, ChunkError>> + Send>>;
