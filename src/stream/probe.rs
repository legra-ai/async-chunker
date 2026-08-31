//! Probing an async reader: read the bounded prefix, detect, hand
//! back a replaying reader.

use tokio::io::{AsyncRead, AsyncReadExt};

use crate::constants::PROBE_PREFIX_MAX_BYTES;
use crate::error::ChunkError;
use crate::probe::{Detection, Detector};
use crate::replay::PrefixReplay;

impl Detector {
    /// Read at most [`PROBE_PREFIX_MAX_BYTES`] from `reader`, probe
    /// them, and return the detection with a reader that replays
    /// the prefix before the remainder — no byte is lost.
    ///
    /// # Errors
    ///
    /// Returns [`ChunkError::Io`] when the reader fails.
    pub async fn probe<R>(&self, mut reader: R) -> Result<(Detection, PrefixReplay<R>), ChunkError>
    where
        R: AsyncRead + Unpin,
    {
        // bounded: PROBE_PREFIX_MAX_BYTES.
        let mut prefix = vec![0_u8; PROBE_PREFIX_MAX_BYTES];
        let mut filled = 0;
        while filled < PROBE_PREFIX_MAX_BYTES {
            let read = reader
                .read(&mut prefix[filled..])
                .await
                .map_err(|error| ChunkError::Io(error.to_string()))?;
            if read == 0 {
                break;
            }
            filled += read;
        }
        prefix.truncate(filled);
        let detection = self.detect(&prefix);
        Ok((
            detection,
            PrefixReplay::new(prefix.into_boxed_slice(), reader),
        ))
    }
}
