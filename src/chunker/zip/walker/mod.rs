//! The forward-only, bounded-state ZIP structure walker.
//!
//! It reads the archive exactly once, in stream order, and never
//! seeks: local file headers are parsed as they arrive, member bytes
//! are counted (never inflated), data-descriptor members are closed
//! by recognising their descriptor, and the central directory plus
//! end records are reconciled against what streamed past. Only
//! boundary detection and structural validation happen here.

mod core;
mod descriptor;
mod dispatch;
mod events;
mod state;

pub(in crate::chunker) use core::Walker;
pub(in crate::chunker) use events::{NoEvents, ZipEvents};
