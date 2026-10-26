/// Errors that can occur during markdown extraction.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("HTML parsing error: {0}")]
    HtmlParse(String),

    #[error("PDF extraction error: {0}")]
    PdfExtract(String),

    #[error("DOCX extraction error: {0}")]
    DocxExtract(String),

    #[error("Spreadsheet extraction error: {0}")]
    SpreadsheetExtract(String),

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

    #[error("Resource limit exceeded: {0}")]
    ResourceLimit(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
