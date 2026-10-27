/// Errors that can occur during markdown extraction.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("HTML parsing error: {0}")]
    HtmlParse(String),

    #[error("PDF extraction error: {0}")]
    PdfExtract(String),

    #[error("DOCX extraction error: {0}")]
    DocxExtract(String),

    #[error("PPTX extraction error: {0}")]
    PptxExtract(String),
    #[error("ODT extraction error: {0}")]
    OdtExtract(String),

    #[error("Spreadsheet extraction error: {0}")]
    SpreadsheetExtract(String),

    #[error("Notebook extraction error: {0}")]
    NotebookExtract(String),

    #[error("EPUB extraction error: {0}")]
    EpubExtract(String),

    #[error("Email extraction error: {0}")]
    EmailExtract(String),

    #[error("HTTP error: {0}")]
    Http(String),

    /// `extract_url` refused a URL: disallowed scheme, host outside the
    /// allowlist, or an address on a non-public network.
    #[error("URL blocked: {0}")]
    UrlBlocked(String),

    #[error("Encoding error: {0}")]
    Encoding(String),

    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),

    /// A [`Limits`](crate::Limits) value was exceeded; match on
    /// [`LimitExceeded::kind`](crate::LimitExceeded) (or use [`Error::limit_kind`]).
    #[error("Resource limit exceeded: {0}")]
    ResourceLimit(crate::limits::LimitExceeded),

    /// A custom [`crate::registry::FormatParser`] failed or panicked.
    #[error("Parser error: {0}")]
    Parser(String),

    /// A [`crate::transform::Transform`] failed.
    #[error("Transform error: {0}")]
    Transform(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl Error {
    /// The exceeded limit, for [`Error::ResourceLimit`].
    pub fn limit_kind(&self) -> Option<crate::limits::LimitKind> {
        match self {
            Error::ResourceLimit(e) => Some(e.kind),
            _ => None,
        }
    }
}
