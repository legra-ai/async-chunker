//! [`ProfileRegistry`] — the versioned media-type → chunking-profile
//! table.

mod table;

#[cfg(test)]
mod tests;

pub use table::{ProfileRegistry, RegistryVersion};
