//! The shared inspection engine behind the `inspect`, `dedup`, and
//! `corpus` examples: stream one file through detection and either
//! decomposition (member-wise chunking under inferred profiles) or
//! whole-file chunking, collecting a report and the chunk hashes.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use async_chunker::decompose::{
    ContainerFacts, ContainerKind, DecomposeError, Decomposer, DecompositionSink, EntryKind,
    MemberFacts, MemberMeta, OpaqueReason,
};
use async_chunker::{
    Chunker, ChunkingProfile, MediaType, ProfileChunker, ProfileRegistry,
    decompose::infer_member_media,
};

/// One file's analysis.
pub struct Analysis {
    /// How the bytes were routed.
    pub route: Route,
    /// Every produced chunk as `(blake3 hash, length)`.
    pub chunks: Vec<(blake3::Hash, u64)>,
    /// Input file length.
    pub input_bytes: u64,
}

/// How the file was processed.
pub enum Route {
    /// Decomposed into members, each chunked under its inferred
    /// profile.
    Decomposed {
        /// Rendered member-tree lines.
        tree: Vec<String>,
        /// Facts per container level, innermost last.
        facts: Vec<ContainerFacts>,
    },
    /// Chunked whole under one profile.
    Chunked {
        /// The selected profile.
        profile: ChunkingProfile,
    },
    /// Decomposition or the selected profile rejected the bytes;
    /// chunked opaquely under `generic-cdc-v1`.
    Opaque {
        /// Why.
        reason: String,
    },
}

/// Aggregate chunk statistics.
pub struct ChunkStats {
    pub count: usize,
    pub total: u64,
    pub min: u64,
    pub max: u64,
}

impl Analysis {
    /// Statistics over the produced chunks.
    pub fn stats(&self) -> ChunkStats {
        let mut stats = ChunkStats {
            count: self.chunks.len(),
            total: 0,
            min: u64::MAX,
            max: 0,
        };
        for (_, len) in &self.chunks {
            stats.total += len;
            stats.min = stats.min.min(*len);
            stats.max = stats.max.max(*len);
        }
        if stats.count == 0 {
            stats.min = 0;
        }
        stats
    }

    /// Bytes of `other` whose chunks also appear here.
    pub fn shared_bytes_with(&self, other: &Analysis) -> (u64, usize) {
        let mine: HashMap<blake3::Hash, ()> =
            self.chunks.iter().map(|(hash, _)| (*hash, ())).collect();
        let mut bytes = 0;
        let mut count = 0;
        for (hash, len) in &other.chunks {
            if mine.contains_key(hash) {
                bytes += len;
                count += 1;
            }
        }
        (bytes, count)
    }
}

/// A short human size.
pub fn human(bytes: u64) -> String {
    match bytes {
        0..=1023 => format!("{bytes} B"),
        1024..=1048575 => format!("{:.1} KiB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MiB", bytes as f64 / 1048576.0),
    }
}

const WINDOW: usize = 64 << 10;

fn read_prefix(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let mut prefix = vec![0u8; 512];
    let mut at = 0;
    while at < prefix.len() {
        let read = file.read(&mut prefix[at..])?;
        if read == 0 {
            break;
        }
        at += read;
    }
    prefix.truncate(at);
    Ok(prefix)
}

/// Analyze one file. `declared` overrides name-based inference.
pub fn analyze(path: &Path, declared: Option<&MediaType>) -> std::io::Result<Analysis> {
    let prefix = read_prefix(path)?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let inferred = infer_member_media(name.as_bytes(), &prefix);
    let media = declared.cloned().or(inferred.media_type);
    if inferred.container.is_some() {
        match decompose_file(path) {
            Ok(analysis) => return Ok(analysis),
            Err(DecomposeError::Opaque(reason)) => {
                return opaque_file(path, reason.to_string());
            }
            Err(other) => return opaque_file(path, other.to_string()),
        }
    }
    let profile = media
        .as_ref()
        .map(|media| ProfileRegistry::V2.select(media))
        .unwrap_or(ChunkingProfile::GenericCdcV1);
    match chunk_file(path, profile) {
        Ok(chunks) => Ok(Analysis {
            route: Route::Chunked { profile },
            input_bytes: file_len(path)?,
            chunks,
        }),
        Err(error) => opaque_file(path, error),
    }
}

fn file_len(path: &Path) -> std::io::Result<u64> {
    Ok(std::fs::metadata(path)?.len())
}

/// Whole-file chunking under one profile; a profile rejection is
/// returned as its message.
fn chunk_file(path: &Path, profile: ChunkingProfile) -> Result<Vec<(blake3::Hash, u64)>, String> {
    let mut chunker = ProfileChunker::open(profile).map_err(|error| error.to_string())?;
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut chunks = Vec::new();
    let mut buffer = vec![0u8; WINDOW];
    let mut record = |chunk: &[u8]| chunks.push((blake3::hash(chunk), chunk.len() as u64));
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        chunker
            .push(&buffer[..read], &mut record)
            .map_err(|error| error.to_string())?;
    }
    chunker
        .finish(&mut record)
        .map_err(|error| error.to_string())?;
    Ok(chunks)
}

fn opaque_file(path: &Path, reason: String) -> std::io::Result<Analysis> {
    let chunks = chunk_file(path, ChunkingProfile::GenericCdcV1)
        .expect("the generic profile accepts any bytes");
    Ok(Analysis {
        route: Route::Opaque { reason },
        input_bytes: file_len(path)?,
        chunks,
    })
}

/// Decompose a compound file, chunking each member under its
/// inferred profile.
fn decompose_file(path: &Path) -> Result<Analysis, DecomposeError> {
    struct Sink {
        tree: Vec<String>,
        facts: Vec<ContainerFacts>,
        chunks: Vec<(blake3::Hash, u64)>,
        chunker: Option<ProfileChunker>,
        member_chunks_at: usize,
    }
    impl DecompositionSink for Sink {
        fn container_start(&mut self, kind: ContainerKind, depth: u32) {
            self.tree
                .push(format!("{}[{}]", "  ".repeat(depth as usize), kind.name()));
        }
        fn entry(&mut self, kind: &EntryKind, meta: &MemberMeta, depth: u32) {
            let label = match kind {
                EntryKind::Directory => "dir",
                EntryKind::Symlink { .. } => "symlink",
                EntryKind::Hardlink { .. } => "hardlink",
                EntryKind::Other { .. } => "special",
            };
            self.tree.push(format!(
                "{}{} ({label})",
                "  ".repeat(depth as usize + 1),
                String::from_utf8_lossy(&meta.path),
            ));
        }
        fn member_start(
            &mut self,
            meta: &MemberMeta,
            media_type: Option<&MediaType>,
            nested: bool,
            depth: u32,
        ) {
            let type_label = media_type
                .map(|media| media.essence().to_owned())
                .unwrap_or_else(|| "unknown".to_owned());
            self.tree.push(format!(
                "{}{} ({}{})",
                "  ".repeat(depth as usize + 1),
                String::from_utf8_lossy(&meta.path),
                type_label,
                if nested { ", nested container" } else { "" },
            ));
            if !nested {
                let profile = media_type
                    .map(|media| ProfileRegistry::V2.select(media))
                    .unwrap_or(ChunkingProfile::GenericCdcV1);
                self.chunker =
                    Some(ProfileChunker::open(profile).expect("registry profiles exist"));
                self.member_chunks_at = self.chunks.len();
            }
        }
        fn member_bytes(&mut self, bytes: &[u8], _depth: u32) {
            let Some(chunker) = self.chunker.as_mut() else {
                return;
            };
            let chunks = &mut self.chunks;
            let mut record = |chunk: &[u8]| chunks.push((blake3::hash(chunk), chunk.len() as u64));
            chunker.push(bytes, &mut record).expect("accepted member");
        }
        fn member_end(&mut self, facts: &MemberFacts, _depth: u32) {
            if let Some(mut chunker) = self.chunker.take() {
                let chunks = &mut self.chunks;
                let mut record =
                    |chunk: &[u8]| chunks.push((blake3::hash(chunk), chunk.len() as u64));
                chunker.finish(&mut record).expect("accepted member");
                let produced = self.chunks.len() - self.member_chunks_at;
                if let Some(line) = self.tree.last_mut() {
                    line.push_str(&format!(
                        " — {} in {produced} chunks",
                        human(facts.byte_length)
                    ));
                }
            }
        }
        fn container_end(&mut self, facts: &ContainerFacts, _depth: u32) {
            self.facts.push(facts.clone());
        }
    }

    let mut sink = Sink {
        tree: Vec::new(),
        facts: Vec::new(),
        chunks: Vec::new(),
        chunker: None,
        member_chunks_at: 0,
    };
    let mut decomposer = Decomposer::new();
    let mut file = File::open(path).map_err(|_| {
        DecomposeError::Opaque(OpaqueReason::Malformed {
            detail: "unreadable file",
            offset: 0,
        })
    })?;
    let mut buffer = vec![0u8; WINDOW];
    let mut total = 0u64;
    loop {
        let read = file.read(&mut buffer).map_err(|_| {
            DecomposeError::Opaque(OpaqueReason::Malformed {
                detail: "read failed",
                offset: total,
            })
        })?;
        if read == 0 {
            break;
        }
        total += read as u64;
        decomposer.push(&buffer[..read], &mut sink)?;
    }
    decomposer.finish(&mut sink)?;
    Ok(Analysis {
        route: Route::Decomposed {
            tree: sink.tree,
            facts: sink.facts,
        },
        input_bytes: total,
        chunks: sink.chunks,
    })
}
