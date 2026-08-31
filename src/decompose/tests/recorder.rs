//! A recording sink for assertions.

use crate::media_type::MediaType;

use super::super::sink::{
    ContainerFacts, ContainerKind, DecompositionSink, EntryKind, MemberFacts, MemberMeta,
};
use super::super::{DecomposeError, Decomposer};

/// One recorded event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Ev {
    Start(ContainerKind, u32),
    Entry(EntryKind, Vec<u8>, u32),
    Member {
        path: Vec<u8>,
        media: Option<String>,
        nested: bool,
        depth: u32,
    },
    Bytes(Vec<u8>, u32),
    MemberEnd(u64, u32),
    End(ContainerFacts, u32),
}

#[derive(Default)]
pub(super) struct Recorder {
    pub(super) events: Vec<Ev>,
}

impl DecompositionSink for Recorder {
    fn container_start(&mut self, kind: ContainerKind, depth: u32) {
        self.events.push(Ev::Start(kind, depth));
    }

    fn entry(&mut self, kind: &EntryKind, meta: &MemberMeta, depth: u32) {
        self.events
            .push(Ev::Entry(kind.clone(), meta.path.to_vec(), depth));
    }

    fn member_start(
        &mut self,
        meta: &MemberMeta,
        media_type: Option<&MediaType>,
        nested: bool,
        depth: u32,
    ) {
        self.events.push(Ev::Member {
            path: meta.path.to_vec(),
            media: media_type.map(|media| media.essence().to_owned()),
            nested,
            depth,
        });
    }

    fn member_bytes(&mut self, bytes: &[u8], depth: u32) {
        if let Some(Ev::Bytes(pending, at)) = self.events.last_mut() {
            if *at == depth {
                pending.extend_from_slice(bytes);
                return;
            }
        }
        self.events.push(Ev::Bytes(bytes.to_vec(), depth));
    }

    fn member_end(&mut self, facts: &MemberFacts, depth: u32) {
        self.events.push(Ev::MemberEnd(facts.byte_length, depth));
    }

    fn container_end(&mut self, facts: &ContainerFacts, depth: u32) {
        self.events.push(Ev::End(facts.clone(), depth));
    }
}

impl Recorder {
    /// The decompressed bytes of the member at `path` (first
    /// occurrence).
    pub(super) fn member_bytes_of(&self, path: &[u8]) -> Vec<u8> {
        let mut collecting = false;
        let mut out = Vec::new();
        for event in &self.events {
            match event {
                Ev::Member { path: at, .. } if at == path => collecting = true,
                Ev::Bytes(bytes, _) if collecting => out.extend_from_slice(bytes),
                Ev::MemberEnd(..) if collecting => return out,
                _ => {}
            }
        }
        out
    }
}

/// Run a full decomposition over `bytes` in `window`-sized pushes.
pub(super) fn run(bytes: &[u8], window: usize) -> Result<Recorder, DecomposeError> {
    let mut decomposer = Decomposer::new();
    let mut recorder = Recorder::default();
    for slice in bytes.chunks(window) {
        decomposer.push(slice, &mut recorder)?;
    }
    decomposer.finish(&mut recorder)?;
    Ok(recorder)
}
