//! [`PrefixReplay`] — an [`AsyncRead`] that replays a consumed
//! prefix before the rest of the underlying reader.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, ReadBuf};

/// A reader that yields a bounded, already-consumed prefix first and
/// then continues with the inner reader, so probing never loses
/// bytes and the chunker sees the stream exactly as stored.
#[derive(Debug)]
pub struct PrefixReplay<R> {
    // bounded: at most PROBE_PREFIX_MAX_BYTES.
    prefix: Box<[u8]>,
    replayed: usize,
    inner: R,
}

impl<R> PrefixReplay<R> {
    /// Wrap `inner`, replaying `prefix` before it.
    pub(crate) fn new(prefix: Box<[u8]>, inner: R) -> Self {
        Self {
            prefix,
            replayed: 0,
            inner,
        }
    }

    /// The bytes that will be (or were) replayed before the inner
    /// reader.
    #[must_use]
    pub fn prefix(&self) -> &[u8] {
        &self.prefix
    }

    /// Give back the inner reader.
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for PrefixReplay<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let pending = &this.prefix[this.replayed..];
        if !pending.is_empty() {
            let take = pending.len().min(buf.remaining());
            buf.put_slice(&pending[..take]);
            this.replayed += take;
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut this.inner).poll_read(cx, buf)
    }
}
