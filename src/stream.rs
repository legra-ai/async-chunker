//! Async adapters for bounded profile chunkers.

use std::pin::Pin;

use futures_core::Stream;
use tokio::io::{AsyncRead, AsyncReadExt};

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
    pub fn new(profile: crate::ChunkingProfile) -> Result<Self, ChunkError> {
        Ok(Self {
            profile: ProfileChunker::open(profile)?,
        })
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

#[cfg(test)]
mod tests {
    use futures_util::StreamExt;

    use super::*;

    #[tokio::test]
    async fn emits_chunks_incrementally_without_collecting_the_input() {
        let mut input = vec![0_u8; 512 << 10];
        input[0] = 1;
        let reader = std::io::Cursor::new(input.clone());
        let chunker = AsyncChunker::new(crate::ChunkingProfile::GenericCdcV1)
            .expect("registered profile is implemented");
        let mut stream = chunker.chunk(reader);

        let first = stream
            .next()
            .await
            .expect("the input produces a first chunk")
            .expect("the input is valid");
        assert!(!first.is_empty());
        assert!(first.len() <= crate::constants::GENERIC_CDC_CHUNK_MAX_BYTES);

        let mut remainder = Vec::new();
        while let Some(chunk) = stream.next().await {
            remainder.extend_from_slice(&chunk.expect("the input is valid"));
        }
        assert_eq!(first.len() + remainder.len(), input.len());
    }
}
