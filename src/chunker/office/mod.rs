//! The Office Open XML profiles: `ooxml-v1` (canonicalizing) and
//! `ooxml-ber-v1` (byte-exact-reversible), both part-aware.

mod ber;
mod canonical;
mod fault;
mod kind;
mod observer;

#[cfg(test)]
mod tests;

pub use ber::OoxmlBerChunker;
pub use canonical::OoxmlChunker;
pub use kind::OfficeKind;
pub use observer::PackageObserver;
