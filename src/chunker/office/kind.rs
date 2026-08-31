//! [`OfficeKind`] — the three Office Open XML package kinds and the
//! frozen part vocabulary that identifies them.

/// The package's first member. Office writers emit it first; a ZIP
/// whose first member is anything else is not treated as an OOXML
/// package.
pub(super) const CONTENT_TYPES: &[u8] = b"[Content_Types].xml";

/// The directory whose presence marks a digitally signed package.
pub(super) const SIGNATURE_DIR: &[u8] = b"_xmlsignatures/";

/// The Office Open XML package kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfficeKind {
    /// WordprocessingML (`.docx`).
    Word,
    /// SpreadsheetML (`.xlsx`).
    Excel,
    /// PresentationML (`.pptx`).
    Powerpoint,
}

impl OfficeKind {
    /// Every kind.
    pub const ALL: [Self; 3] = [Self::Word, Self::Excel, Self::Powerpoint];

    /// The main-part name that identifies the kind.
    #[must_use]
    pub(super) const fn main_part(self) -> &'static [u8] {
        match self {
            Self::Word => b"word/document.xml",
            Self::Excel => b"xl/workbook.xml",
            Self::Powerpoint => b"ppt/presentation.xml",
        }
    }

    /// The kind whose main part `name` is, if any.
    #[must_use]
    pub(super) fn of_main_part(name: &[u8]) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.main_part() == name)
    }

    /// The kind's human-readable name, for diagnostics.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Word => "word",
            Self::Excel => "excel",
            Self::Powerpoint => "powerpoint",
        }
    }
}
