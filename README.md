# async-chunker

[![Crates.io](https://img.shields.io/crates/v/async-chunker.svg)](https://crates.io/crates/async-chunker)
[![Documentation](https://docs.rs/async-chunker/badge.svg)](https://docs.rs/async-chunker)
[![CI](https://github.com/legra-ai/async-chunker/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/legra-ai/async-chunker/actions/workflows/ci.yml)
[![Downloads](https://img.shields.io/crates/d/async-chunker.svg)](https://crates.io/crates/async-chunker)
[![License](https://img.shields.io/crates/l/async-chunker.svg)](LICENSE-APACHE)

Bounded-memory asynchronous content-defined chunking for structured and media
streams.

## Why this crate exists

Large uploads must not become a `Vec<u8>`, a collection of chunks, or an
implicit in-memory archive. `async-chunker` consumes a `tokio::io::AsyncRead`
and yields one owned chunk at a time as a `futures_core::Stream`. The caller
controls progress by awaiting the next item, so downstream storage can apply
backpressure without a hidden queue.

The public async adapter reads a fixed 64 KiB window. Profile state is bounded
by the profile's maximum chunk size and a single emitted chunk is retained only
until the caller polls the next stream item.

## Why content-defined chunks?

Chunk boundaries are derived from the bytes and the selected profile rather
than from absolute offsets. That makes the same content produce the same
chunk boundaries, and makes insertions or deletions less likely to shift every
later boundary. When files share identical regions—or successive versions
change only a small region—those regions are therefore more likely to reuse
the same content-addressed blocks instead of producing new blocks for the
entire remainder.

This is a reuse opportunity, not a guarantee: boundaries depend on the
profile, the surrounding bytes, and any format-specific structure. The
important contract is deterministic, bounded-memory streaming: each chunk is
emitted once and can be persisted or deduplicated immediately.

## Example

```rust
use async_chunker::{AsyncChunker, ChunkingProfile};
use futures_util::StreamExt;

# #[tokio::main(flavor = "current_thread")]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let input = tokio::io::repeat(b'R');
let chunks = AsyncChunker::new(ChunkingProfile::GenericCdcV1)?.chunk(input);
tokio::pin!(chunks);

let first = chunks.next().await.transpose()?.expect("infinite input");
assert!(!first.is_empty());
# Ok(())
# }
```

`AsyncChunker` currently includes the generic CDC, structured text, ZIP,
ISO-BMFF, Matroska, MPEG transport stream, and framed-audio boundary profiles.
Archive member decomposition, recursive container traversal, and Office Open
XML semantic profiles are separate profile work; ZIP boundary detection alone
does not unpack an archive.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <https://opensource.org/licenses/MIT>)
