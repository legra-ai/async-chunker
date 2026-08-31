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

## How similar files end up sharing chunks

Two ideas do all the work; every profile is a particular combination of them.

**1. Cuts are decided by the bytes, not by the offset.** A gear rolling
hash is updated per byte and only the last 64 bytes influence it. At a
candidate position the profile asks "does the hash mask to zero here?" —
a question whose answer depends solely on those 64 bytes. So once the
stream is past an edit, the hash "forgets" it: the same bytes seen again
produce the same yes/no at the same content positions, and every later
boundary lands exactly where it did in the previous file. An insertion or
deletion therefore changes the chunk(s) it touches and nothing after them.

**2. Structured formats only offer cuts at their natural seams.** A ZIP
member start, an MP4 box start, a Matroska cluster, an MPEG-TS packet with
a payload-unit start, an audio frame boundary, a line end. Two things
follow. A unit that is byte-identical between two files becomes the same
chunks in both — even if it moved, because nothing in the cut decision
mentions where it sits. And a unit that changed is contained: the cut
before it and the cut after it are both seams the edit did not move. Large
units (a 4 MiB image inside a `.docx`, an `mdat` box, a cluster) are cut
inside by rule 1; small units attach backward to the chunk before them so
a handful of tiny parts never become a handful of tiny blocks.

The size envelope (16 KiB minimum, 64 KiB target, 256 KiB maximum) is the
same for every profile and is the measured reuse optimum: smaller chunks
buy little extra reuse and cost a block per chunk; larger ones make every
edit invalidate more.

### What survives a typical edit

| You upload… | What actually changed in the bytes | What the profile does | Chunks reused |
| --- | --- | --- | --- |
| A Word document (`.docx`) after editing one paragraph | A `.docx` is a ZIP of parts. `word/document.xml` is recompressed (every byte of that member differs) and the central directory is rewritten; `word/media/image1.png`, styles, fonts, and themes are byte-identical members. | `zip-v1` cuts at every member start. Each large image begins its own chunk and is cut internally by the hash; the tiny `.rels`/`[Content_Types].xml` parts attach backward. | Every image and every untouched part: identical blocks. New blocks only for `document.xml`, the small parts riding on it, and the central directory. (Deflate hides *where* inside `document.xml` you edited — seeing through that needs member-level decomposition, which is separate profile work.) Excel and PowerPoint packages behave identically: an edited sheet or slide part changes, embedded media does not. |
| The same document re-saved by a different tool | Same parts, but the tool may recompress every member with different deflate settings. | Member boundaries still line up. | Only members whose compressed bytes are identical (stored, or same compressor) — recompression is a real byte change, and no chunker can undo it. |
| An MP3 after changing its title or cover art | The leading `ID3v2` tag grows or shrinks; every audio frame shifts by that many bytes but is byte-identical. | `framed-audio-v1` treats the tag as an opaque region (hash cuts) and cuts audio only at frame seams. A seam's verdict depends on the last 64 audio bytes, not on the frame's offset. | The whole audio region: the same frames cut at the same seams. Only the tag chunk(s) change; an `ID3v1` trailer edit touches only the last chunk. FLAC behaves the same way — metadata blocks and cover art are opaque regions, the audio region is untouched. |
| An MP4 after a re-mux or metadata edit (`ftyp`/`moov` rewritten, or `moov` moved to the front for fast start) | The box tree is rewritten; the `mdat` payload — the actual media — is byte-identical but at a different offset. | `isobmff-v1` cuts at box starts and inside `mdat` by the hash. | All `mdat` chunks. Fragmented MP4 (`moof`+`mdat` pairs) and HEIF/AVIF items behave the same: unchanged fragments and items yield identical blocks. |
| A Matroska/WebM file after editing `Tags`/`Info`, or a live recording that grew | Segment-level metadata changes size; clusters are appended; existing clusters are byte-identical. | `matroska-v1` cuts at segment-level units (clusters above all) and inside large clusters by the hash. | Every untouched cluster. An appended recording reuses all previous cluster blocks and adds only the new ones. |
| An MPEG transport stream re-segmented (HLS pieces joined or split differently) | Packet content is identical; only where the stream starts and ends moved. | `mpegts-v1` keeps whole packets and cuts only at seams (payload-unit start, discontinuity), judged by the hash. | Identical packet runs re-converge at the first seam after each stream start, so the interior of every segment dedups. |
| A Markdown/JSON/XML/CSV file after changing a few lines | Bytes after the edit shift by the size difference. | `structured-text-v1` cuts only after line ends (or soft breaks once past the target) and never inside a UTF-8 scalar. | Everything before the edited line, and everything after it from the next qualifying line end on. Line-granular edits invalidate a chunk or two. |
| A PDF, PNG, or any other opaque file with a region inserted or removed | Bytes shift; the format has no seams the profile knows. | `generic-cdc-v1` — the hash alone, every byte a candidate. | Everything but the chunk(s) containing the edit; boundaries re-synchronize within ~64 bytes of the last changed byte. |

### What does not dedup, and why

Chunking can only align *bytes that are identical*. It cannot recover reuse
across a transcode (re-encoded audio/video), a recompression (a ZIP member
deflated with different settings), a container conversion (MP4 → MKV), or a
re-encryption whose output differs. Those are new bytes; the profile finds
their seams, but there is nothing on the other side to match. The guarantee
this crate makes is narrower and dependable: **the same bytes, in the same
profile, always become the same chunks — wherever they sit in the file and
whatever surrounds them.**

## Supported formats

Every profile shares one chunk-size envelope — **16 KiB minimum, 64 KiB
target, 256 KiB maximum** — and the same gear rolling hash (a 256-entry
table derived from a per-profile frozen seed, updated per byte so only the
last 64 bytes influence it). What differs is *where* a profile is willing to
cut and *what it insists the bytes are*. A cut is judged only at a profile's
candidate positions: before the target length a strict mask makes early cuts
rare, after it a relaxed mask closes the chunk within a few candidates, and
the maximum forces a cut when no candidate qualified. Structured profiles
parse only the framing they need — nothing is decompressed or decoded — and
reject a stream that stops parsing, so a given media type and byte sequence
always yield exactly one representation.

| Profile | Media types (`ProfileRegistry::V1`) | What it is | How it finds a boundary |
| --- | --- | --- | --- |
| `generic-cdc-v1` | every media type not listed below (`application/octet-stream`, `application/pdf`, `image/png`, …) | Canonical content-defined chunking for opaque bytes. | Every byte is a candidate: cut where the rolling hash masks to zero, so identical regions split identically wherever they occur and later boundaries re-synchronize after an insertion. |
| `structured-text-v1` | `text/*` families, JSON (`application/json`, `+json`), XML/HTML (`application/xml`, `+xml`, `image/svg+xml`), YAML, TOML, JavaScript, the RDF text syntaxes and SPARQL bodies | Well-formed UTF-8 text (Markdown, JSON, XML/HTML, CSV, RDF, …). | Candidates are textual seams only — after a line end (strict) or a soft break (`\t`, space, `,;.}])>`) once the target is reached — so boundaries land between lines and re-synchronize at line granularity after an edit. A chunk never splits a scalar; malformed UTF-8 rejects the stream. |
| `zip-v1` | `application/zip`, `application/java-archive`, `application/epub+zip`, **Word/Excel/PowerPoint** (`.docx`/`.xlsx`/`.pptx`), OpenDocument | ZIP containers: a forward-only walk over local headers, member bytes, data descriptors, the central directory and end records, with every size claim reconciled. Members are never inflated; Office packages are handled purely as containers — no OOXML semantics yet. | Candidates are **member boundaries** (each local header, the central directory). A large member always begins a chunk; small members attach backward to the preceding chunk; inside a large member the gear hash places ordinary cuts. An unchanged part (an OOXML image, a JAR class) therefore yields identical chunks wherever it appears. |
| `isobmff-v1` | `video/mp4`, `audio/mp4`, `application/mp4`, `video/quicktime`, `image/heif`, `image/heic`, `image/avif` | ISO Base Media File Format (MP4/MOV/HEIF/AVIF): a forward-only box walk that descends the pure containers (`moov`, `trak`, `moof`, …) and counts every other payload — `mdat` above all — without decoding. | Candidates are **box boundaries** (every top-level box and every child of a descended container), with the large-begins / small-attach-backward rule and gear cuts inside large boxes. A re-mux or metadata edit rewrites `ftyp`/`moov` while the `mdat` chunks are reused. |
| `matroska-v1` | `video/x-matroska`, `audio/x-matroska`, `video/webm`, `audio/webm` | Matroska/WebM: the EBML header, then `Segment`s whose direct children — `Cluster`s above all — are the units. Known-size elements are opaque; unknown-size clusters close at the next segment-level element. | Candidates are **segment-level unit boundaries** under the same container rule (large unit begins, small elements attach backward, gear cuts inside large clusters). Editing `Info`/`Tags` or appending to a live stream leaves every untouched cluster chunk identical. |
| `mpegts-v1` | `video/mp2t` | MPEG transport streams: a flat sequence of 188-byte packets, each starting with the `0x47` sync byte, validated per packet and never resynchronized by scanning. | Every chunk is a whole number of packets. Candidates are packets whose header marks a seam — payload-unit start or an adaptation-field discontinuity — with the hash consulted only there; a forced cut at the packet-aligned maximum bounds candidate-free streams. |
| `framed-audio-v1` | `audio/mpeg`, `audio/aac`, `audio/flac` | Frame-synchronized audio: MPEG audio frames (lengths from the header tables), ADTS frames (explicit lengths), or FLAC metadata blocks plus the audio region, with leading `ID3v2` tags and the `ID3v1` trailer. CRCs are skipped, never verified. | **Framed** regions cut only at frame seams — every chunk is a whole number of frames — under masks scaled to per-frame spacing, with a forced seam cut before the envelope could overflow. **Opaque** regions (tag bodies, cover art, the FLAC audio region) take per-byte content-defined cuts. |

Not yet a profile: **PDF** (objects and content streams — today it is opaque
`generic-cdc-v1`), archive member decomposition (looking *inside* ZIP/TAR
members, recursively), and semantic Office Open XML boundaries (part and
relationship structure of Word/Excel/PowerPoint packages). Each is separate,
versioned profile work.

The registry table above is frozen as `ProfileRegistry::V1`; the full
essence list is available from `ProfileRegistry::essences`. Moving a media
type between profiles, changing a mask, seed, or candidate rule, or adding a
profile changes the boundaries a stream produces and therefore lands as a new
registry/profile version, never as an in-place tweak.

## Choosing a profile

Three entry points, from most to least explicit:

- **`AsyncChunker::new(profile)`** — the caller names the profile; no bytes
  are inspected.
- **`AsyncChunker::chunk_declared(media_type, registry, reader)`** — the
  caller declares a [`MediaType`][MediaType], the versioned
  [`ProfileRegistry`][ProfileRegistry] maps it to a profile, and a bounded
  prefix probe refuses a *positive contradiction*
  (declared `application/zip`, bytes recognized as Matroska). The declaration
  otherwise wins: a generic declaration makes no structural claim, and an
  unrecognized prefix contradicts nothing — the specialist engine remains the
  authority on malformed input.
- **`AsyncChunker::chunk_detected(reader)`** — nothing is declared; the
  [`Detector`][Detector] probes at most `PROBE_PREFIX_MAX_BYTES` and chunks with the
  recognized specialist, or with the explicit generic profile when no probe
  matches. Several matches are an error, never a guess.

Media types are parsed and normalized (`Text/HTML; Charset=utf-8` equals
`text/html;charset=utf-8`); lookups key on the `type/subtype` essence. An
unlisted media type selects `generic-cdc-v1` by rule — that is a selection,
not a fallback. Registry and detector versions are frozen: moving a media
type between profiles, or changing a probe, changes every boundary it
produces and therefore lands as a new version.

Probing never loses bytes. The consumed prefix is replayed ahead of the rest
of the reader through [`PrefixReplay`][PrefixReplay], and because every boundary is a pure
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

[MediaType]: https://docs.rs/async-chunker/latest/async_chunker/struct.MediaType.html
[ProfileRegistry]: https://docs.rs/async-chunker/latest/async_chunker/struct.ProfileRegistry.html
[Detector]: https://docs.rs/async-chunker/latest/async_chunker/struct.Detector.html
[PrefixReplay]: https://docs.rs/async-chunker/latest/async_chunker/struct.PrefixReplay.html

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <https://opensource.org/licenses/MIT>)
