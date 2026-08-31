//! [`PdfFault`] — why the walker rejected a PDF.

use crate::ChunkError;
use crate::profile::ChunkingProfile;

/// The frozen name, for diagnostics.
const PROFILE: &str = ChunkingProfile::PdfV1.name();

/// A structural rejection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PdfFault {
    /// The stream does not begin with `%PDF-`.
    NotPdf,
    /// Digits where an indirect object must begin do not form
    /// `number generation obj`.
    BadObjectHeader,
    /// A body keyword is none of `obj`-header, comment, `xref`,
    /// `trailer`, or `startxref`.
    BadKeyword,
    /// The `stream` keyword is not followed by a line end.
    BadStreamEol,
    /// An `xref` subsection line is not two integers, `trailer`, or
    /// a record keyword.
    XrefGeometry,
    /// The stream ended inside an object, a stream, an xref table,
    /// or a trailer.
    Truncated,
    /// The stream ended without a final `%%EOF` marker.
    MissingEof,
}

impl PdfFault {
    /// The frozen diagnostic text.
    pub(super) const fn detail(self) -> &'static str {
        match self {
            Self::NotPdf => "stream does not begin with %PDF-",
            Self::BadObjectHeader => "indirect object header is not 'number generation obj'",
            Self::BadKeyword => "unknown body keyword",
            Self::BadStreamEol => "stream keyword is not followed by a line end",
            Self::XrefGeometry => "malformed xref subsection",
            Self::Truncated => "document ends inside an object, stream, xref table, or trailer",
            Self::MissingEof => "document ends without %%EOF",
        }
    }

    /// The typed error for a fault at `offset`.
    pub(super) const fn into_error(self, offset: u64) -> ChunkError {
        ChunkError::MalformedProfileInput {
            profile: PROFILE,
            offset,
            detail: self.detail(),
        }
    }
}

/// The stream was already rejected.
pub(super) const fn stream_rejected() -> ChunkError {
    ChunkError::ProfileStreamRejected { profile: PROFILE }
}
