//! The decomposition event vocabulary: what a container walk
//! reports, one bounded event at a time.

use crate::media_type::MediaType;

/// The container kinds the adapter recognizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerKind {
    /// A ZIP archive (including Office packages; the package kind,
    /// when detected, is reported in [`ContainerFacts`]).
    Zip,
    /// A TAR archive.
    Tar,
    /// A gzip stream wrapping a TAR archive (`.tar.gz` / `.tgz`).
    TarGzip,
    /// A gzip stream wrapping a single non-archive member.
    Gzip,
}

impl ContainerKind {
    /// The kind's frozen name, for diagnostics and facts.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::Tar => "tar",
            Self::TarGzip => "tar+gzip",
            Self::Gzip => "gzip",
        }
    }
}

/// What a non-member entry is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    /// An explicit directory entry.
    Directory,
    /// A symbolic link and its target bytes.
    Symlink {
        /// The link target, verbatim.
        target: Box<[u8]>,
    },
    /// A hard link and its target bytes.
    Hardlink {
        /// The link target, verbatim.
        target: Box<[u8]>,
    },
    /// A recognized entry the adapter carries as metadata only
    /// (device nodes, FIFOs).
    Other {
        /// The entry's type tag, format-specific.
        tag: u8,
    },
}

/// One member's (or entry's) metadata, as walked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberMeta {
    /// The normalized relative path, verbatim bytes. Never absolute,
    /// never containing a `..` component (such paths make the whole
    /// container opaque).
    pub path: Box<[u8]>,
    /// The member's position in container order, from zero. Duplicate
    /// paths keep distinct ordinals.
    pub ordinal: u64,
    /// The declared size, when the format declares one.
    pub size: Option<u64>,
    /// POSIX-style mode bits, when the format carries them.
    pub mode: Option<u32>,
    /// Modification time (seconds since the epoch), when carried.
    pub mtime: Option<u64>,
}

/// What the adapter inferred about a member from its name and its
/// first bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberFacts {
    /// The member's decompressed length.
    pub byte_length: u64,
    /// The inferred media type, when the frozen name table and the
    /// byte-prefix detector resolve one.
    pub media_type: Option<MediaType>,
}

/// Container-level facts reported at the end of a walk — everything
/// a deterministic canonical writer needs beyond the members
/// themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerFacts {
    /// The container kind.
    pub kind: ContainerKind,
    /// Members (regular files) walked.
    pub member_count: u64,
    /// Non-member entries walked.
    pub entry_count: u64,
    /// The Office package kind, when the container is a recognized
    /// Office Open XML package ("word", "excel", "powerpoint").
    pub office_kind: Option<&'static str>,
}

/// Receives the decomposition event stream.
///
/// Events arrive in container order, one member in flight at a time;
/// a nested container's events arrive between its `member_start` and
/// `member_end`, at `depth + 1`. Callbacks are synchronous and must
/// stay cheap and bounded — the consumer applies backpressure by
/// pacing the bytes it pushes into the
/// [`Decomposer`](super::Decomposer).
pub trait DecompositionSink {
    /// A container walk begins at `depth` (the upload's outer
    /// container is depth 0).
    fn container_start(&mut self, kind: ContainerKind, depth: u32) {
        let _ = (kind, depth);
    }

    /// A non-member entry (directory, link, device) at `depth`.
    fn entry(&mut self, kind: &EntryKind, meta: &MemberMeta, depth: u32) {
        let _ = (kind, meta, depth);
    }

    /// A regular member begins. `nested` is true when the member is
    /// itself a recognized container about to be walked recursively
    /// (its decompressed container bytes are then **not** delivered
    /// through `member_bytes`; its own events follow instead).
    fn member_start(
        &mut self,
        meta: &MemberMeta,
        media_type: Option<&MediaType>,
        nested: bool,
        depth: u32,
    ) {
        let _ = (meta, media_type, nested, depth);
    }

    /// One window of the current member's decompressed bytes.
    fn member_bytes(&mut self, bytes: &[u8], depth: u32) {
        let _ = (bytes, depth);
    }

    /// The current member ended.
    fn member_end(&mut self, facts: &MemberFacts, depth: u32) {
        let _ = (facts, depth);
    }

    /// The container at `depth` ended.
    fn container_end(&mut self, facts: &ContainerFacts, depth: u32) {
        let _ = (facts, depth);
    }
}
