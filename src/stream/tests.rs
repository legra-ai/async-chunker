use std::io::Cursor;

use futures_util::StreamExt;
use tokio::io::AsyncReadExt;

use super::AsyncChunker;
use crate::constants::{GENERIC_CDC_CHUNK_MAX_BYTES, PROBE_PREFIX_MAX_BYTES};
use crate::error::ChunkError;
use crate::media_type::MediaType;
use crate::probe::{Detection, Detector, ProfileSet};
use crate::profile::ChunkingProfile;
use crate::registry::ProfileRegistry;

async fn collect(mut stream: super::ChunkStream) -> Result<Vec<Box<[u8]>>, ChunkError> {
    let mut chunks = Vec::new();
    while let Some(chunk) = stream.next().await {
        chunks.push(chunk?);
    }
    Ok(chunks)
}

fn media(text: &str) -> MediaType {
    MediaType::parse(text).expect("valid media type")
}

fn text_input() -> Vec<u8> {
    "line of structured text\n".repeat(20_000).into_bytes()
}

#[tokio::test]
async fn emits_chunks_incrementally_without_collecting_the_input() {
    let mut input = vec![0_u8; 512 << 10];
    input[0] = 1;
    let reader = Cursor::new(input.clone());
    let chunker = AsyncChunker::new(ChunkingProfile::GenericCdcV1)
        .expect("registered profile is implemented");
    let mut stream = chunker.chunk(reader);

    let first = stream
        .next()
        .await
        .expect("the input produces a first chunk")
        .expect("the input is valid");
    assert!(!first.is_empty());
    assert!(first.len() <= GENERIC_CDC_CHUNK_MAX_BYTES);

    let mut remainder = Vec::new();
    while let Some(chunk) = stream.next().await {
        remainder.extend_from_slice(&chunk.expect("the input is valid"));
    }
    assert_eq!(first.len() + remainder.len(), input.len());
}

#[tokio::test]
async fn probe_replays_the_prefix_without_loss() {
    let input: Vec<u8> = (0..(PROBE_PREFIX_MAX_BYTES * 3))
        .map(|index| u8::try_from(index % 251).expect("fits"))
        .collect();
    let (detection, mut replay) = Detector::V1
        .probe(Cursor::new(input.clone()))
        .await
        .expect("cursor reads");
    assert_eq!(detection, Detection::Unrecognized);
    assert_eq!(replay.prefix(), &input[..PROBE_PREFIX_MAX_BYTES]);
    let mut replayed = Vec::new();
    replay
        .read_to_end(&mut replayed)
        .await
        .expect("cursor reads");
    assert_eq!(replayed, input);

    let short = b"short".to_vec();
    let (_, mut replay) = Detector::V1
        .probe(Cursor::new(short.clone()))
        .await
        .expect("cursor reads");
    assert_eq!(replay.prefix(), &short[..]);
    let mut replayed = Vec::new();
    replay
        .read_to_end(&mut replayed)
        .await
        .expect("cursor reads");
    assert_eq!(replayed, short);
}

#[tokio::test]
async fn detected_chunking_matches_declared_chunking_byte_for_byte() {
    let input = text_input();
    let detected = collect(
        AsyncChunker::chunk_detected(Cursor::new(input.clone()))
            .await
            .expect("text is recognized"),
    )
    .await
    .expect("valid text");
    let declared = collect(
        AsyncChunker::new(ChunkingProfile::StructuredTextV1)
            .expect("implemented")
            .chunk(Cursor::new(input.clone())),
    )
    .await
    .expect("valid text");
    assert_eq!(detected, declared, "replay must not move a boundary");
    assert!(detected.len() > 1);
    assert_eq!(detected.concat(), input);
}

#[tokio::test]
async fn unrecognized_bytes_chunk_with_the_explicit_generic_profile() {
    let input = vec![0xAB_u8; 300 << 10];
    let detected = collect(
        AsyncChunker::chunk_detected(Cursor::new(input.clone()))
            .await
            .expect("unrecognized resolves to generic"),
    )
    .await
    .expect("generic never rejects");
    let generic = collect(
        AsyncChunker::new(ChunkingProfile::GenericCdcV1)
            .expect("implemented")
            .chunk(Cursor::new(input.clone())),
    )
    .await
    .expect("generic never rejects");
    assert_eq!(detected, generic);
}

#[tokio::test]
async fn ambiguous_bytes_require_a_declaration() {
    let input = b"ID3 tag text ".repeat(64);
    let error = AsyncChunker::chunk_detected(Cursor::new(input.clone()))
        .await
        .err()
        .expect("ambiguous prefix fails");
    let mut candidates = ProfileSet::single(ChunkingProfile::StructuredTextV1);
    candidates.insert(ChunkingProfile::FramedAudioV1);
    assert_eq!(error, ChunkError::AmbiguousDetection { candidates });

    let declared = collect(
        AsyncChunker::chunk_declared(
            &media("text/plain"),
            &ProfileRegistry::V1,
            Cursor::new(input),
        )
        .await
        .expect("the declaration resolves the ambiguity"),
    )
    .await
    .expect("valid text");
    assert!(!declared.is_empty());
}

#[tokio::test]
async fn declared_specialist_contradicted_by_the_bytes_fails_hard() {
    let input = text_input();
    let error = AsyncChunker::chunk_declared(
        &media("application/zip"),
        &ProfileRegistry::V1,
        Cursor::new(input.clone()),
    )
    .await
    .err()
    .expect("text is not a zip");
    assert_eq!(
        error,
        ChunkError::DeclaredDetectedMismatch {
            declared: "zip-v1",
            detected: ProfileSet::single(ChunkingProfile::StructuredTextV1),
        }
    );

    let generic = AsyncChunker::chunk_declared(
        &media("application/octet-stream"),
        &ProfileRegistry::V1,
        Cursor::new(input),
    )
    .await
    .expect("a generic declaration is never contradicted");
    assert!(
        !collect(generic)
            .await
            .expect("generic never rejects")
            .is_empty()
    );
}

#[tokio::test]
async fn declared_specialist_still_rejects_malformed_input() {
    let input = b"PK\x03\x04 not really a zip".to_vec();
    let stream = AsyncChunker::chunk_declared(
        &media("application/zip"),
        &ProfileRegistry::V1,
        Cursor::new(input),
    )
    .await
    .expect("the probe agrees with the declaration");
    let error = collect(stream).await.expect_err("the engine rejects");
    assert!(
        matches!(
            error,
            ChunkError::MalformedProfileInput {
                profile: "zip-v1",
                ..
            }
        ),
        "{error}"
    );
}

#[test]
fn declared_selects_without_reading_bytes() {
    assert!(AsyncChunker::declared(&media("video/webm"), &ProfileRegistry::V1).is_ok());
}
