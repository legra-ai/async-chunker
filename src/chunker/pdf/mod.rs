//! The `pdf-v1` profile: object-aligned boundaries over the original
//! PDF bytes.

mod chunker;
mod fault;
mod object;
mod walker;

#[cfg(test)]
mod tests;

pub use chunker::PdfChunker;
