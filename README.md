# async-chunker

[![Crates.io](https://img.shields.io/crates/v/async-chunker.svg?cacheSeconds=300)](https://crates.io/crates/async-chunker)
[![Documentation](https://docs.rs/async-chunker/badge.svg?cacheSeconds=300)](https://docs.rs/async-chunker)
[![CI](https://github.com/legra-ai/async-chunker/actions/workflows/ci.yml/badge.svg?branch=main&cacheSeconds=300)](https://github.com/legra-ai/async-chunker/actions/workflows/ci.yml)
[![Downloads](https://img.shields.io/crates/d/async-chunker.svg?cacheSeconds=300)](https://crates.io/crates/async-chunker)
[![License](https://img.shields.io/crates/l/async-chunker.svg?cacheSeconds=300)](LICENSE-APACHE)

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

`AsyncChunker` includes the generic CDC, structured text, ZIP, ISO-BMFF,
Matroska, MPEG transport stream, and framed-audio boundary profiles. Archive
member decomposition, recursive container traversal, and Office Open XML
semantic profiles are separate profile work; ZIP boundary detection alone
does not unpack an archive.

## Choosing a profile

Three entry points, from most to least explicit:

- **`AsyncChunker::new(profile)`** — the caller names the profile; no bytes
  are inspected.
- **`AsyncChunker::chunk_declared(media_type, registry, reader)`** — the
  caller declares a [`MediaType`], the versioned [`ProfileRegistry`] maps it
  to a profile, and a bounded prefix probe refuses a *positive contradiction*
  (declared `application/zip`, bytes recognized as Matroska). The declaration
  otherwise wins: a generic declaration makes no structural claim, and an
  unrecognized prefix contradicts nothing — the specialist engine remains the
  authority on malformed input.
- **`AsyncChunker::chunk_detected(reader)`** — nothing is declared; the
  [`Detector`] probes at most `PROBE_PREFIX_MAX_BYTES` and chunks with the
  recognized specialist, or with the explicit generic profile when no probe
  matches. Several matches are an error, never a guess.

Media types are parsed and normalized (`Text/HTML; Charset=utf-8` equals
`text/html;charset=utf-8`); lookups key on the `type/subtype` essence. An
unlisted media type selects `generic-cdc-v1` by rule — that is a selection,
not a fallback. Registry and detector versions are frozen: moving a media
type between profiles, or changing a probe, changes every boundary it
produces and therefore lands as a new version.

Probing never loses bytes. The consumed prefix is replayed ahead of the rest
of the reader through [`PrefixReplay`], and because every boundary is a pure
function of the profile and the bytes, a probed stream chunks exactly like an
unprobed one.

```rust
use async_chunker::{AsyncChunker, MediaType, ProfileRegistry};
use futures_util::StreamExt;

# #[tokio::main(flavor = "current_thread")]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let media_type: MediaType = "Text/Markdown; charset=UTF-8".parse()?;
let input = std::io::Cursor::new("# Title\n\nA paragraph.\n".repeat(4096));
let mut chunks =
    AsyncChunker::chunk_declared(&media_type, &ProfileRegistry::V1, input).await?;

let mut total = 0;
while let Some(chunk) = chunks.next().await {
    total += chunk?.len();
}
assert_eq!(total, "# Title\n\nA paragraph.\n".len() * 4096);
# Ok(())
# }
```

```rust
use async_chunker::{AsyncChunker, ChunkError, Detection, Detector, ChunkingProfile};

# #[tokio::main(flavor = "current_thread")]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
// Probe first when the caller wants the verdict before committing.
let input = std::io::Cursor::new(b"PK\x03\x04 not an archive".to_vec());
let (detection, replay) = Detector::V1.probe(input).await?;
assert_eq!(detection, Detection::Recognized(ChunkingProfile::ZipV1));

// The engine, not the probe, is the authority on malformed input.
let mut chunks = AsyncChunker::new(detection.resolve()?)?.chunk(replay);
let outcome = futures_util::StreamExt::next(&mut chunks).await.transpose();
assert!(matches!(outcome, Err(ChunkError::MalformedProfileInput { .. })));
# Ok(())
# }
```

The core profile engines (`Chunker` implementors such as `ZipChunker`) are
synchronous and runtime-free: feed windows with `push`, close with `finish`.
Only the async adapters depend on `tokio::io::AsyncRead`, and they need no
Tokio runtime beyond what the caller's reader already requires.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <https://opensource.org/licenses/MIT>)
