# async-chunker

[![Crates.io](https://img.shields.io/crates/v/async-chunker.svg)](https://crates.io/crates/async-chunker)
[![Documentation](https://docs.rs/async-chunker/badge.svg)](https://docs.rs/async-chunker)
[![CI](https://github.com/legra-ai/async-chunker/actions/workflows/ci.yml/badge.svg)](https://github.com/legra-ai/async-chunker/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/legra-ai/async-chunker#license)

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
