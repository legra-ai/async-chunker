//! `structured-text-v1`: the frozen chunking profile for Markdown,
//! JSON, XML/HTML text media types, and the RDF/XSD textual
//! datatypes.

mod chunker;
mod utf8;

#[cfg(test)]
mod tests;

pub use chunker::StructuredTextChunker;
