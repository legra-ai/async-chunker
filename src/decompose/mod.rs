//! The recursive decomposition adapter (ELS-16): compound
//! containers — ZIP, TAR, wrapped archives, Office packages — are
//! never chunked as one opaque compressed stream. The adapter emits
//! typed, bounded member events with decompressed member bytes, one
//! member at a time, recursing into nested containers through an
//! explicit bounded stack.

mod decomposer;
mod fault;
mod gzip;
mod media;
mod sink;
mod tar;
pub mod writer;
mod zip;

#[cfg(test)]
mod tests;

pub use decomposer::Decomposer;
pub use fault::{DecomposeError, OpaqueReason};
pub use media::{InferredMember, infer_member_media};
pub use sink::{
    ContainerFacts, ContainerKind, DecompositionSink, EntryKind, MemberFacts, MemberMeta,
};
