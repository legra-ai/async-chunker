//! [`Decomposer`] — the recursion driver: outer-container
//! detection, per-level readers, member media inference, and the
//! bounded nesting stack.

use std::collections::VecDeque;

use crate::chunker::OfficeKind;
use crate::constants::PROBE_PREFIX_MAX_BYTES;
use crate::media_type::MediaType;

use super::fault::{DecomposeError, OpaqueReason};
use super::gzip::GzipReader;
use super::media::{self, InferredMember, infer_member_media};
use super::sink::{
    ContainerFacts, ContainerKind, DecompositionSink, EntryKind, MemberFacts, MemberMeta,
};
use super::tar::{TarEvent, TarKind, TarWalker};
use super::zip::ZipReader;

/// Deepest container nesting the adapter follows.
pub(super) const MAX_DEPTH: usize = 8;
/// Most members (and entries) across the whole walk.
const MAX_TOTAL_MEMBERS: u64 = 1_000_000;
/// Longest member path accepted.
const PATH_MAX: usize = 4096;

/// One event a reader hands the driver.
pub(super) enum ReaderEvent {
    /// Announce the level's container kind (deferred for gzip until
    /// its inner shape is known).
    Announce(ContainerKind),
    /// A non-member entry.
    Entry(EntryKind, MemberMeta),
    /// A regular member begins.
    MemberStart(MemberMeta),
    /// Decompressed member bytes.
    Bytes(Vec<u8>),
    /// The member ended with this decompressed length.
    MemberEnd(u64),
}

/// The bounded event queue a reader fills per pushed byte.
#[derive(Default)]
pub(super) struct ReaderOut {
    // bounded: drained after every pushed byte.
    events: VecDeque<ReaderEvent>,
}

impl ReaderOut {
    pub(super) fn push(&mut self, event: ReaderEvent) {
        self.events.push_back(event);
    }

    /// Append bytes, coalescing into a trailing bytes event.
    pub(super) fn push_bytes(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        if let Some(ReaderEvent::Bytes(pending)) = self.events.back_mut() {
            pending.extend_from_slice(bytes);
            return;
        }
        self.events.push_back(ReaderEvent::Bytes(bytes.to_vec()));
    }
}

/// The TAR reader: the walker plus member framing.
pub(super) struct TarReader {
    walker: TarWalker,
    remaining: Option<u64>,
    size: u64,
    ordinal: u64,
}

impl TarReader {
    pub(super) fn new() -> Self {
        Self {
            walker: TarWalker::new(),
            remaining: None,
            size: 0,
            ordinal: 0,
        }
    }

    fn push(&mut self, byte: u8, out: &mut ReaderOut) -> Result<(), OpaqueReason> {
        match self.walker.push(byte)? {
            TarEvent::None | TarEvent::End => Ok(()),
            TarEvent::Entry(entry) => {
                let meta = MemberMeta {
                    path: entry.path.clone().into_boxed_slice(),
                    ordinal: self.ordinal,
                    size: Some(entry.size),
                    mode: entry.mode,
                    mtime: entry.mtime,
                };
                self.ordinal += 1;
                match entry.kind.to_entry_kind() {
                    Some(kind) => out.push(ReaderEvent::Entry(kind, meta)),
                    None => {
                        out.push(ReaderEvent::MemberStart(meta));
                        if entry.size == 0 {
                            out.push(ReaderEvent::MemberEnd(0));
                        } else {
                            self.remaining = Some(entry.size);
                            self.size = entry.size;
                        }
                    }
                }
                let _ = matches!(entry.kind, TarKind::Regular);
                Ok(())
            }
            TarEvent::Data(len) => {
                out.push_bytes(std::slice::from_ref(&byte));
                debug_assert_eq!(len, 1);
                if let Some(remaining) = self.remaining.as_mut() {
                    *remaining -= 1;
                    if *remaining == 0 {
                        self.remaining = None;
                        out.push(ReaderEvent::MemberEnd(self.size));
                    }
                }
                Ok(())
            }
        }
    }

    fn finish(&mut self, _out: &mut ReaderOut) -> Result<(), OpaqueReason> {
        self.walker.finish()?;
        if self.remaining.is_some() {
            return Err(OpaqueReason::Malformed {
                detail: "tar archive ends inside a member",
                offset: 0,
            });
        }
        Ok(())
    }
}

/// What a gzip level wraps.
enum GzInner {
    /// Buffering decompressed bytes until the inner shape is known.
    // bounded: PROBE_PREFIX_MAX_BYTES.
    Detect(Vec<u8>),
    Tar(TarReader),
    /// A single non-archive member; decompressed length so far.
    Single(u64),
}

/// A gzip stream, wrapping either a TAR or one plain member.
pub(super) struct GzReader {
    gz: GzipReader,
    inner: GzInner,
}

impl GzReader {
    pub(super) fn new() -> Self {
        Self {
            gz: GzipReader::new(),
            inner: GzInner::Detect(Vec::new()),
        }
    }

    fn single_member_meta(&self) -> MemberMeta {
        MemberMeta {
            path: self.gz.stored_name().unwrap_or(b"").into(),
            ordinal: 0,
            size: None,
            mode: None,
            mtime: None,
        }
    }

    fn route(&mut self, bytes: &[u8], out: &mut ReaderOut) -> Result<(), OpaqueReason> {
        match &mut self.inner {
            GzInner::Detect(buffer) => {
                buffer.extend_from_slice(bytes);
                if buffer.len() >= PROBE_PREFIX_MAX_BYTES.max(super::tar::BLOCK) {
                    self.decide(out)?;
                }
                Ok(())
            }
            GzInner::Tar(tar) => {
                for &byte in bytes {
                    tar.push(byte, out)?;
                }
                Ok(())
            }
            GzInner::Single(produced) => {
                *produced += bytes.len() as u64;
                out.push_bytes(bytes);
                Ok(())
            }
        }
    }

    fn decide(&mut self, out: &mut ReaderOut) -> Result<(), OpaqueReason> {
        let GzInner::Detect(buffer) = &mut self.inner else {
            return Ok(());
        };
        let buffer = std::mem::take(buffer);
        if media::is_tar_prefix(&buffer) {
            out.push(ReaderEvent::Announce(ContainerKind::TarGzip));
            let mut tar = TarReader::new();
            for &byte in &buffer {
                tar.push(byte, out)?;
            }
            self.inner = GzInner::Tar(tar);
        } else {
            out.push(ReaderEvent::Announce(ContainerKind::Gzip));
            out.push(ReaderEvent::MemberStart(self.single_member_meta()));
            out.push_bytes(&buffer);
            self.inner = GzInner::Single(buffer.len() as u64);
        }
        Ok(())
    }

    fn push(&mut self, byte: u8, out: &mut ReaderOut) -> Result<(), OpaqueReason> {
        self.gz.push(byte)?;
        let bytes = self.gz.take_pending();
        if !bytes.is_empty() {
            self.route(&bytes, out)?;
        }
        Ok(())
    }

    fn finish(&mut self, out: &mut ReaderOut) -> Result<(), OpaqueReason> {
        self.gz.finish()?;
        if matches!(self.inner, GzInner::Detect(_)) {
            self.decide(out)?;
        }
        match &mut self.inner {
            GzInner::Detect(_) => Ok(()),
            GzInner::Tar(tar) => tar.finish(out),
            GzInner::Single(produced) => {
                out.push(ReaderEvent::MemberEnd(*produced));
                Ok(())
            }
        }
    }
}

/// A level's reader.
pub(super) enum LevelReader {
    Zip(ZipReader),
    Tar(TarReader),
    Gz(GzReader),
}

impl LevelReader {
    fn push(&mut self, byte: u8, out: &mut ReaderOut) -> Result<(), OpaqueReason> {
        match self {
            Self::Zip(reader) => reader.push(byte, out),
            Self::Tar(reader) => reader.push(byte, out),
            Self::Gz(reader) => reader.push(byte, out),
        }
    }

    fn finish(&mut self, out: &mut ReaderOut) -> Result<(), OpaqueReason> {
        match self {
            Self::Zip(reader) => reader.finish(out),
            Self::Tar(reader) => reader.finish(out),
            Self::Gz(reader) => reader.finish(out),
        }
    }
}

/// One open member at a level.
struct OpenMember {
    meta: MemberMeta,
    /// Buffered first bytes while media inference is undecided.
    // bounded: PROBE_PREFIX_MAX_BYTES + one block.
    probe: Option<Vec<u8>>,
    nested: bool,
    media: Option<MediaType>,
}

/// One stack level.
struct Level {
    reader: LevelReader,
    kind: Option<ContainerKind>,
    depth: u32,
    member_count: u64,
    entry_count: u64,
    office_main: Option<OfficeKind>,
    open: Option<OpenMember>,
}

impl Level {
    fn facts(&self) -> ContainerFacts {
        ContainerFacts {
            kind: self.kind.unwrap_or(ContainerKind::Gzip),
            member_count: self.member_count,
            entry_count: self.entry_count,
            office_kind: self.office_main.map(OfficeKind::name),
        }
    }
}

/// What the decomposer is doing.
enum Phase {
    /// Buffering the outer prefix for container detection.
    // bounded: one probe prefix + one TAR block.
    Detect(Vec<u8>),
    Run,
    Done,
    Rejected,
}

/// The streaming decomposition driver. Push the compound's bytes;
/// typed events arrive on the sink; `finish` closes the walk.
pub struct Decomposer {
    phase: Phase,
    // bounded: MAX_DEPTH levels.
    levels: Vec<Level>,
    total_members: u64,
}

impl Default for Decomposer {
    fn default() -> Self {
        Self::new()
    }
}

impl Decomposer {
    /// A decomposer that detects the outer container from the bytes.
    #[must_use]
    pub fn new() -> Self {
        Self {
            phase: Phase::Detect(Vec::new()),
            levels: Vec::new(),
            total_members: 0,
        }
    }

    /// Push one window of the compound's bytes.
    ///
    /// # Errors
    ///
    /// [`DecomposeError::NotAContainer`] when the first bytes match
    /// no recognized container; [`DecomposeError::Opaque`] when the
    /// container cannot be decomposed (store the source bytes as one
    /// flagged opaque literal); [`DecomposeError::StreamRejected`]
    /// after either.
    pub fn push(
        &mut self,
        window: &[u8],
        sink: &mut dyn DecompositionSink,
    ) -> Result<(), DecomposeError> {
        match &mut self.phase {
            Phase::Rejected | Phase::Done => return Err(DecomposeError::StreamRejected),
            Phase::Run => {}
            Phase::Detect(buffer) => {
                buffer.extend_from_slice(window);
                let threshold = PROBE_PREFIX_MAX_BYTES.max(super::tar::BLOCK);
                if buffer.len() < threshold {
                    return Ok(());
                }
                let buffer = std::mem::take(buffer);
                self.start_outer(&buffer, sink)?;
                return Ok(());
            }
        }
        self.guarded(|this| this.feed_level(0, window, sink))
    }

    /// The compound's bytes ended.
    ///
    /// # Errors
    ///
    /// As [`Self::push`]; truncated containers are opaque.
    pub fn finish(&mut self, sink: &mut dyn DecompositionSink) -> Result<(), DecomposeError> {
        match std::mem::replace(&mut self.phase, Phase::Run) {
            Phase::Rejected | Phase::Done => {
                self.phase = Phase::Rejected;
                return Err(DecomposeError::StreamRejected);
            }
            Phase::Detect(buffer) => {
                self.start_outer(&buffer, sink)?;
            }
            Phase::Run => {}
        }
        self.guarded(|this| {
            this.finish_levels_from(0, sink)?;
            Ok(())
        })?;
        self.phase = Phase::Done;
        Ok(())
    }

    fn guarded(
        &mut self,
        action: impl FnOnce(&mut Self) -> Result<(), DecomposeError>,
    ) -> Result<(), DecomposeError> {
        match action(self) {
            Ok(()) => Ok(()),
            Err(error) => {
                self.phase = Phase::Rejected;
                Err(error)
            }
        }
    }

    fn start_outer(
        &mut self,
        buffer: &[u8],
        sink: &mut dyn DecompositionSink,
    ) -> Result<(), DecomposeError> {
        let Some(kind) = media::container_by_prefix(buffer) else {
            self.phase = Phase::Rejected;
            return Err(DecomposeError::NotAContainer);
        };
        self.levels.push(new_level(kind, 0, sink));
        self.phase = Phase::Run;
        self.guarded(|this| this.feed_level(0, buffer, sink))
    }

    fn feed_level(
        &mut self,
        index: usize,
        bytes: &[u8],
        sink: &mut dyn DecompositionSink,
    ) -> Result<(), DecomposeError> {
        for &byte in bytes {
            let mut out = ReaderOut::default();
            self.levels[index]
                .reader
                .push(byte, &mut out)
                .map_err(DecomposeError::Opaque)?;
            self.dispatch(index, out, sink)?;
        }
        Ok(())
    }

    fn dispatch(
        &mut self,
        index: usize,
        mut out: ReaderOut,
        sink: &mut dyn DecompositionSink,
    ) -> Result<(), DecomposeError> {
        while let Some(event) = out.events.pop_front() {
            match event {
                ReaderEvent::Announce(kind) => {
                    let level = &mut self.levels[index];
                    if level.kind.is_none() {
                        level.kind = Some(kind);
                        sink.container_start(kind, level.depth);
                    }
                }
                ReaderEvent::Entry(kind, meta) => {
                    check_path(&meta.path)?;
                    self.count_one()?;
                    let level = &mut self.levels[index];
                    level.entry_count += 1;
                    sink.entry(&kind, &meta, level.depth);
                }
                ReaderEvent::MemberStart(meta) => {
                    check_path(&meta.path)?;
                    self.count_one()?;
                    let level = &mut self.levels[index];
                    if let (LevelReader::Zip(reader), None) = (&level.reader, level.office_main) {
                        if reader.office_package() {
                            level.office_main = OfficeKind::of_main_part(&meta.path);
                        }
                    }
                    level.open = Some(OpenMember {
                        meta,
                        probe: Some(Vec::new()),
                        nested: false,
                        media: None,
                    });
                }
                ReaderEvent::Bytes(bytes) => self.route_bytes(index, &bytes, sink)?,
                ReaderEvent::MemberEnd(byte_length) => {
                    self.close_member(index, byte_length, sink)?;
                }
            }
        }
        Ok(())
    }

    fn route_bytes(
        &mut self,
        index: usize,
        bytes: &[u8],
        sink: &mut dyn DecompositionSink,
    ) -> Result<(), DecomposeError> {
        let level = &mut self.levels[index];
        let Some(open) = level.open.as_mut() else {
            return Ok(());
        };
        if open.nested {
            return self.feed_level(index + 1, bytes, sink);
        }
        if let Some(probe) = open.probe.as_mut() {
            probe.extend_from_slice(bytes);
            if probe.len() >= PROBE_PREFIX_MAX_BYTES.max(super::tar::BLOCK) {
                return self.decide_member(index, sink);
            }
            return Ok(());
        }
        sink.member_bytes(bytes, level.depth);
        Ok(())
    }

    fn decide_member(
        &mut self,
        index: usize,
        sink: &mut dyn DecompositionSink,
    ) -> Result<(), DecomposeError> {
        let depth = self.levels[index].depth;
        let open = self.levels[index]
            .open
            .as_mut()
            .expect("a member is open when deciding");
        let probe = open.probe.take().expect("undecided member has a probe");
        let InferredMember {
            media_type,
            container,
        } = infer_member_media(&open.meta.path, &probe);
        open.media = media_type.clone();
        let meta = open.meta.clone();
        match container {
            Some(kind) => {
                if self.levels.len() >= MAX_DEPTH {
                    return Err(DecomposeError::Opaque(OpaqueReason::DepthExceeded));
                }
                self.levels[index].open.as_mut().expect("still open").nested = true;
                sink.member_start(&meta, media_type.as_ref(), true, depth);
                self.levels.push(new_level(kind, depth + 1, sink));
                self.feed_level(index + 1, &probe, sink)
            }
            None => {
                sink.member_start(&meta, media_type.as_ref(), false, depth);
                sink.member_bytes(&probe, depth);
                Ok(())
            }
        }
    }

    fn close_member(
        &mut self,
        index: usize,
        byte_length: u64,
        sink: &mut dyn DecompositionSink,
    ) -> Result<(), DecomposeError> {
        if self.levels[index]
            .open
            .as_ref()
            .is_some_and(|open| open.probe.is_some())
        {
            // The whole member fit inside the probe buffer.
            self.decide_member(index, sink)?;
        }
        let level_count = self.levels.len();
        let open = self.levels[index].open.take();
        let Some(open) = open else {
            return Ok(());
        };
        if open.nested && level_count > index + 1 {
            self.finish_levels_from(index + 1, sink)?;
        }
        let level = &mut self.levels[index];
        level.member_count += 1;
        let facts = MemberFacts {
            byte_length,
            media_type: open.media,
        };
        sink.member_end(&facts, level.depth);
        Ok(())
    }

    /// Finish and pop every level from `from` upward, top first.
    fn finish_levels_from(
        &mut self,
        from: usize,
        sink: &mut dyn DecompositionSink,
    ) -> Result<(), DecomposeError> {
        while self.levels.len() > from {
            let index = self.levels.len() - 1;
            // An open member below the top means truncation.
            let mut out = ReaderOut::default();
            self.levels[index]
                .reader
                .finish(&mut out)
                .map_err(DecomposeError::Opaque)?;
            self.dispatch(index, out, sink)?;
            let level = self.levels.pop().expect("level exists");
            if level.open.is_some() {
                return Err(DecomposeError::Opaque(OpaqueReason::Malformed {
                    detail: "container ends inside a member",
                    offset: 0,
                }));
            }
            sink.container_end(&level.facts(), level.depth);
        }
        Ok(())
    }

    fn count_one(&mut self) -> Result<(), DecomposeError> {
        self.total_members += 1;
        if self.total_members > MAX_TOTAL_MEMBERS {
            return Err(DecomposeError::Opaque(OpaqueReason::MetadataOverBound));
        }
        Ok(())
    }
}

/// Build a level, announcing kinds that are known up front.
fn new_level(kind: ContainerKind, depth: u32, sink: &mut dyn DecompositionSink) -> Level {
    let (reader, known) = match kind {
        ContainerKind::Zip => (LevelReader::Zip(ZipReader::new()), Some(ContainerKind::Zip)),
        ContainerKind::Tar => (LevelReader::Tar(TarReader::new()), Some(ContainerKind::Tar)),
        // Gzip defers its kind until the inner shape is known.
        ContainerKind::Gzip | ContainerKind::TarGzip => (LevelReader::Gz(GzReader::new()), None),
    };
    if let Some(kind) = known {
        sink.container_start(kind, depth);
    }
    Level {
        reader,
        kind: known,
        depth,
        member_count: 0,
        entry_count: 0,
        office_main: None,
        open: None,
    }
}

/// Reject absolute paths and `..` components; an empty path is
/// permitted only where a format has no name (a bare gzip member).
fn check_path(path: &[u8]) -> Result<(), DecomposeError> {
    if path.len() > PATH_MAX {
        return Err(DecomposeError::Opaque(OpaqueReason::MetadataOverBound));
    }
    if path.starts_with(b"/") || path.starts_with(b"\\") {
        return Err(DecomposeError::Opaque(OpaqueReason::UnsafePath));
    }
    for component in path.split(|byte| *byte == b'/') {
        if component == b".." {
            return Err(DecomposeError::Opaque(OpaqueReason::UnsafePath));
        }
    }
    Ok(())
}
