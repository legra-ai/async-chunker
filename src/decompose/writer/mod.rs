//! Deterministic canonical writers: reconstruct a container from
//! member streams. Output is deterministic for a given crate
//! version and logical content; byte-identity with an original
//! upload is exactly the byte-exact (BER) storage mode's job, not
//! reconstruction's.

mod gzip;
mod tar;
mod zip;

pub use gzip::GzipWriter;
pub use tar::CanonicalTarWriter;
pub use zip::CanonicalZipWriter;
