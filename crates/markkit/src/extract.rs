use serde::Serialize;

use crate::config::MarkkitBuilder;
use crate::error::Error;
use crate::format::{DocumentHint, Format};

/// Result of [`Markkit::extract_document`]: Markdown plus metadata.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Extraction {
    /// Rendered Markdown.
    pub markdown: String,
    /// Document title when known (DOCX core properties or `Title` style, PDF `/Title`).
    pub title: Option<String>,
    /// Format the document was parsed as.
    pub format: Format,
    /// Non-fatal issues, each prefixed with a stable code
    /// (`truncated_sheet:`, `hidden_sheet_skipped:`, `sheet_error:`,
    /// `truncated_pdf:`, `pdf_unmapped_font:`, `pdf_no_text:`).
    pub warnings: Vec<String>,
    /// Format-specific counters.
    pub stats: ExtractionStats,
}

/// Counters describing the extracted content; `None` when not applicable.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct ExtractionStats {
    /// Pages (paged formats).
    pub pages: Option<usize>,
    /// Sheets rendered (spreadsheets; empty and skipped sheets excluded).
    pub sheets: Option<usize>,
    /// Data rows rendered across all sheets (header rows excluded).
    pub rows: Option<usize>,
    /// Non-empty paragraphs, headings and list items (DOCX).
    pub paragraphs: Option<usize>,
    /// Tables (DOCX).
    pub tables: Option<usize>,
}

impl Extraction {
    fn new(markdown: String, format: Format) -> Self {
        Self {
            markdown,
            title: None,
            format,
            warnings: Vec::new(),
            stats: ExtractionStats::default(),
        }
    }
}

/// Run a parser, converting a panic into an error (defence in depth for
/// third-party parsers; only effective when built with `panic = "unwind"`).
#[cfg(any(feature = "docx", feature = "spreadsheet", feature = "pdf"))]
fn guarded<T>(
    what: &str,
    make_err: fn(String) -> Error,
    f: impl FnOnce() -> crate::Result<T>,
) -> crate::Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_| {
        Err(make_err(format!(
            "{what} parser panicked on malformed input"
        )))
    })
}

/// Core extraction engine.
///
/// Use [`MarkkitBuilder`] to create a configured instance.
pub struct Markkit {
    config: MarkkitBuilder,
}

impl Markkit {
    /// Create a new Markkit instance with the given configuration.
    pub fn new(config: MarkkitBuilder) -> Self {
        Self { config }
    }

    /// Create a builder for configuring extraction.
    pub fn builder() -> MarkkitBuilder {
        MarkkitBuilder::default()
    }

    /// Extract markdown from an HTML string.
    pub async fn extract_html(&self, html: &str) -> crate::Result<String> {
        self.extract_html_sync(html)
    }

    /// Synchronous HTML extraction (no async needed for parsing).
    pub fn extract_html_sync(&self, html: &str) -> crate::Result<String> {
        if html.len() > self.config.max_input_size {
            return Err(Error::ResourceLimit(format!(
                "Input size {} exceeds limit {}",
                html.len(),
                self.config.max_input_size
            )));
        }

        #[cfg(feature = "html")]
        {
            let parser = crate::html::HtmlParser::new(&self.config);
            let doc = parser.parse(html)?;
            let mut renderer = crate::render::MarkdownRenderer::new();
            Ok(renderer.render(&doc))
        }
        #[cfg(not(feature = "html"))]
        Err(Error::UnsupportedFormat(
            "HTML support requires the 'html' feature".to_string(),
        ))
    }

    /// Parse HTML into the IR without rendering.
    #[cfg(feature = "html")]
    pub fn parse_html(&self, html: &str) -> crate::Result<crate::ir::Document> {
        if html.len() > self.config.max_input_size {
            return Err(Error::ResourceLimit(format!(
                "Input size {} exceeds limit {}",
                html.len(),
                self.config.max_input_size
            )));
        }
        let parser = crate::html::HtmlParser::new(&self.config);
        parser.parse(html)
    }

    /// Extract markdown from raw bytes, auto-detecting the format.
    ///
    /// Thin wrapper over [`Markkit::extract_document`] that returns only the Markdown.
    pub async fn extract_bytes(
        &self,
        bytes: &[u8],
        content_type: Option<&str>,
    ) -> crate::Result<String> {
        self.extract_document_sync(
            bytes,
            DocumentHint {
                content_type,
                file_name: None,
            },
        )
        .map(|e| e.markdown)
    }

    /// Convert a document (HTML, PDF, DOCX, XLSX/XLS/ODS) to Markdown with metadata.
    ///
    /// The format is detected from `hint.content_type`, then the extension of
    /// `hint.file_name`, then magic bytes. Parsing is CPU-bound and runs inline;
    /// see [`Markkit::extract_document_sync`] for use with `spawn_blocking`.
    pub async fn extract_document(
        &self,
        bytes: &[u8],
        hint: DocumentHint<'_>,
    ) -> crate::Result<Extraction> {
        self.extract_document_sync(bytes, hint)
    }

    /// Synchronous [`Markkit::extract_document`].
    ///
    /// Errors: [`Error::ResourceLimit`] when input/archive/grid limits are
    /// exceeded, [`Error::UnsupportedFormat`] for unknown formats or formats whose
    /// feature is disabled, and a format-specific variant for malformed input.
    pub fn extract_document_sync(
        &self,
        bytes: &[u8],
        hint: DocumentHint<'_>,
    ) -> crate::Result<Extraction> {
        if bytes.len() > self.config.max_input_size {
            return Err(Error::ResourceLimit(format!(
                "Input size {} exceeds limit {}",
                bytes.len(),
                self.config.max_input_size
            )));
        }

        match Format::detect(bytes, hint) {
            Format::Html => {
                let text = String::from_utf8_lossy(bytes);
                let md = self.extract_html_sync(&text)?;
                Ok(Extraction::new(md, Format::Html))
            }
            Format::Pdf => self.extract_pdf_sync(bytes),
            Format::Docx => self.extract_docx(bytes),
            Format::Xlsx | Format::Xls | Format::Ods => self.extract_spreadsheet(bytes),
            Format::Doc => Err(Error::UnsupportedFormat(
                "legacy Word 97-2003 (.doc) documents are not supported".into(),
            )),
            Format::Pptx => Err(Error::UnsupportedFormat(
                "presentations (.pptx) are not supported".into(),
            )),
            other => Err(Error::UnsupportedFormat(match hint.content_type {
                Some(ct) => format!("{ct} ({other})"),
                None => other.to_string(),
            })),
        }
    }

    #[cfg(feature = "docx")]
    fn extract_docx(&self, bytes: &[u8]) -> crate::Result<Extraction> {
        let out = guarded("DOCX", Error::DocxExtract, || {
            crate::docx::parse(bytes, &self.config)
        })?;
        let mut renderer = crate::render::MarkdownRenderer::new();
        let mut e = Extraction::new(renderer.render(&out.doc), Format::Docx);
        e.title = out.title;
        e.stats.paragraphs = Some(out.paragraphs);
        e.stats.tables = Some(out.tables);
        Ok(e)
    }

    #[cfg(not(feature = "docx"))]
    fn extract_docx(&self, _bytes: &[u8]) -> crate::Result<Extraction> {
        Err(Error::UnsupportedFormat(
            "DOCX support requires the 'docx' feature".to_string(),
        ))
    }

    #[cfg(feature = "spreadsheet")]
    fn extract_spreadsheet(&self, bytes: &[u8]) -> crate::Result<Extraction> {
        let out = guarded("spreadsheet", Error::SpreadsheetExtract, || {
            crate::spreadsheet::parse(bytes, &self.config)
        })?;
        let mut renderer = crate::render::MarkdownRenderer::new();
        let mut e = Extraction::new(renderer.render(&out.doc), out.format);
        e.warnings = out.warnings;
        e.stats.sheets = Some(out.sheets);
        e.stats.rows = Some(out.rows);
        Ok(e)
    }

    #[cfg(not(feature = "spreadsheet"))]
    fn extract_spreadsheet(&self, _bytes: &[u8]) -> crate::Result<Extraction> {
        Err(Error::UnsupportedFormat(
            "spreadsheet support requires the 'spreadsheet' feature".to_string(),
        ))
    }

    /// Fetch a URL and extract markdown from the response.
    ///
    /// Treats the URL as untrusted (threat model T3): only `http`/`https`;
    /// loopback, private, link-local and other non-public addresses are
    /// refused with [`Error::UrlBlocked`] on every hop (including after DNS
    /// resolution) unless [`MarkkitBuilder::allow_private_networks`] is set;
    /// [`MarkkitBuilder::allow_host`] restricts hosts; redirects capped by
    /// `max_redirects`; body capped by `max_input_size`; whole fetch capped
    /// by `max_processing_secs` ([`Error::ResourceLimit`]).
    #[cfg(feature = "http_client")]
    pub async fn extract_url(&self, url: &str) -> crate::Result<String> {
        let fetched = crate::fetch::fetch(url, &self.config).await?;
        self.extract_bytes(&fetched.bytes, fetched.content_type.as_deref())
            .await
    }

    #[cfg(feature = "pdf")]
    fn extract_pdf_sync(&self, bytes: &[u8]) -> crate::Result<Extraction> {
        let out = guarded("PDF", Error::PdfExtract, || {
            crate::pdf::parse(bytes, &self.config)
        })?;
        let mut renderer = crate::render::MarkdownRenderer::new();
        let mut e = Extraction::new(renderer.render(&out.doc), Format::Pdf);
        e.title = out.title;
        e.warnings = out.warnings;
        e.stats.pages = Some(out.pages);
        Ok(e)
    }

    #[cfg(not(feature = "pdf"))]
    fn extract_pdf_sync(&self, _bytes: &[u8]) -> crate::Result<Extraction> {
        Err(Error::UnsupportedFormat(
            "PDF support requires the 'pdf' feature".to_string(),
        ))
    }

    /// Access the current configuration.
    pub fn config(&self) -> &MarkkitBuilder {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_builder_defaults() {
        let m = Markkit::builder().build();
        assert_eq!(m.config().max_input_size, 50 * 1024 * 1024);
        assert!(m.config().preserve_links);
    }

    #[tokio::test]
    async fn test_resource_limit() {
        let m = Markkit::builder().max_input_size(10).build();
        let result = m.extract_html("hello world!").await;
        assert!(matches!(result, Err(Error::ResourceLimit(_))));
    }
}
