//! Byte-prefix detection: bounded, deterministic probes that pick a
//! specialist profile from the first bytes of a stream.

mod detection;
mod detector;
mod probes;
mod set;
mod text;

#[cfg(test)]
mod tests;

pub use detection::Detection;
pub use detector::Detector;
pub use set::ProfileSet;
