use serde::Serialize;

use crate::config::MarkkitBuilder;
use crate::error::Error;
use crate::format::{DocumentHint, Format};
use crate::ir::Document;
use crate::limits::Budget;

/// Result of [`Markkit::extract_document`]: Markdown plus metadata.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Extraction {
    /// Rendered Markdown. Non-empty output always ends with exactly one `\n`
    /// (POSIX text convention, safe to concatenate); empty documents render `""`.
    pub markdown: String,
    /// Document title when known: HTML `og:title` / JSON-LD / `<title>`; DOCX
    /// core properties or `Title` style; XLSX `docProps/core.xml`, ODS
    /// `meta.xml`, XLS `SummaryInformation`; PDF `/Info /Title`;
    /// Markdown front matter `title:` or first `# ` heading.
    pub title: Option<String>,
    /// Format the document was parsed as.
    pub format: Format,
    /// Non-fatal issues, each prefixed with a stable code
    /// (`truncated_sheet:`, `hidden_sheet_skipped:`, `sheet_error:`,
    /// `truncated_pdf:`, `pdf_unmapped_font:`, `pdf_no_text:`, `invalid_utf8:`,
    /// `malformed_csv:`, `truncated_table:`).
    pub warnings: Vec<String>,
    /// Format-specific counters.
    pub stats: ExtractionStats,
    /// Document metadata (HTML `<meta>`, Open Graph, JSON-LD); empty when unknown.
    pub metadata: Metadata,
}

/// Document-level metadata. Every field is `None` when absent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Metadata {
    /// Summary (`og:description`, `<meta name=description>`, JSON-LD `description`).
    pub description: Option<String>,
    /// Author name(s), comma-separated (`<meta name=author>`, JSON-LD `author`).
    pub author: Option<String>,
    /// Publication date as written in the source, typically ISO 8601.
    pub published: Option<String>,
    /// Content language (`<html lang>`, `Content-Language`, `og:locale`).
    pub language: Option<String>,
    /// Canonical URL (`<link rel=canonical>`, `og:url`, JSON-LD `url`).
    pub canonical_url: Option<String>,
    /// Site / publisher name (`og:site_name`, JSON-LD `publisher.name`).
    pub site_name: Option<String>,
    /// Lead image URL (`og:image`, `twitter:image`, JSON-LD `image`).
    pub image: Option<String>,
}

impl Metadata {
    /// True when no field is set.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Counters describing the extracted content; `None` when not applicable.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct ExtractionStats {
    /// Pages (paged formats).
    pub pages: Option<usize>,
    /// Sheets rendered (spreadsheets; empty and skipped sheets excluded).
    pub sheets: Option<usize>,
    /// Data rows rendered across all sheets / the CSV table (header rows excluded).
    pub rows: Option<usize>,
    /// Non-empty paragraphs, headings and list items (DOCX, plain text, Markdown).
    pub paragraphs: Option<usize>,
    /// Tables (DOCX, Markdown, CSV/TSV).
    pub tables: Option<usize>,
    /// Non-empty cells rendered (Jupyter notebooks).
    pub cells: Option<usize>,
}

/// Result of [`Markkit::parse_document`]: the IR plus metadata, not rendered.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
pub struct ParsedDocument {
    /// Canonical IR (blocks carry [`crate::ir::Source`] when
    /// [`MarkkitBuilder::track_sources`] is enabled).
    pub document: Document,
    /// Document title when known.
    pub title: Option<String>,
    /// Format the document was parsed as.
    pub format: Format,
    /// Non-fatal issues (see [`Extraction::warnings`]).
    pub warnings: Vec<String>,
    /// Format-specific counters.
    pub stats: ExtractionStats,
    /// Document metadata (see [`Extraction::metadata`]).
    pub metadata: Metadata,
}

impl ParsedDocument {
    /// IR plus format with empty metadata (for custom
    /// [`crate::registry::FormatParser`]s).
    pub fn new(document: Document, format: Format) -> Self {
        Self {
            document,
            title: None,
            format,
            warnings: Vec::new(),
            stats: ExtractionStats::default(),
            metadata: Metadata::default(),
        }
    }

    /// Render to Markdown, keeping the metadata.
    pub fn render(self) -> Extraction {
        let markdown = crate::render::MarkdownRenderer::new().render(&self.document);
        self.into_extraction(markdown)
    }

    /// [`ParsedDocument::render`] with `max_output_size` and the deadline enforced.
    fn render_bounded(self, max_output: usize, budget: &Budget) -> crate::Result<Extraction> {
        let markdown = crate::render::MarkdownRenderer::new().render_bounded(
            &self.document,
            max_output,
            budget,
        )?;
        Ok(self.into_extraction(markdown))
    }

    fn into_extraction(self, markdown: String) -> Extraction {
        Extraction {
            markdown,
            title: self.title,
            format: self.format,
            warnings: self.warnings,
            stats: self.stats,
            metadata: self.metadata,
        }
    }
}

/// Run a parser, converting a panic into an error (defence in depth for
/// third-party parsers; only effective when built with `panic = "unwind"`, which
/// markkit's own release profile uses).
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
        self.config.limits.check_input(html.len())?;

        #[cfg(feature = "html")]
        {
            let budget = Budget::new(&self.config.limits);
            let doc = self.parse_html_with(html, &budget)?;
            crate::render::MarkdownRenderer::new().render_bounded(
                &doc,
                self.config.limits.max_output_size,
                &budget,
            )
        }
        #[cfg(not(feature = "html"))]
        Err(Error::UnsupportedFormat(
            "HTML support requires the 'html' feature".to_string(),
        ))
    }

    /// Parse HTML into the IR without rendering.
    #[cfg(feature = "html")]
    pub fn parse_html(&self, html: &str) -> crate::Result<crate::ir::Document> {
        self.config.limits.check_input(html.len())?;
        self.parse_html_with(html, &Budget::new(&self.config.limits))
    }

    #[cfg(feature = "html")]
    fn parse_html_with(&self, html: &str, budget: &Budget) -> crate::Result<crate::ir::Document> {
        let mut doc = crate::html::HtmlParser::with_budget(&self.config, budget).parse(html)?;
        self.config.transforms.apply(&mut doc)?;
        Ok(doc)
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

    /// Convert a document (HTML, PDF, DOCX, XLSX/XLSB/XLS/ODS, plain text, Markdown, CSV/TSV,
    /// Jupyter notebooks) to Markdown with metadata.
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
    /// Errors: [`Error::ResourceLimit`] when any [`Limits`](crate::Limits) value is
    /// exceeded, [`Error::UnsupportedFormat`] for unknown formats or formats whose
    /// feature is disabled, and a format-specific variant for malformed input.
    pub fn extract_document_sync(
        &self,
        bytes: &[u8],
        hint: DocumentHint<'_>,
    ) -> crate::Result<Extraction> {
        self.extract_document_with(bytes, hint, &Budget::new(&self.config.limits))
    }

    fn extract_document_with(
        &self,
        bytes: &[u8],
        hint: DocumentHint<'_>,
        budget: &Budget,
    ) -> crate::Result<Extraction> {
        let parsed = self.parse_document_with(bytes, hint, budget)?;
        parsed.render_bounded(self.config.limits.max_output_size, budget)
    }

    /// Parse a document (HTML, DOCX, XLSX/XLS/ODS, plain text, Markdown) into the IR without
    /// rendering. Same detection and errors as [`Markkit::extract_document`].
    pub async fn parse_document(
        &self,
        bytes: &[u8],
        hint: DocumentHint<'_>,
    ) -> crate::Result<ParsedDocument> {
        self.parse_document_sync(bytes, hint)
    }

    /// Synchronous [`Markkit::parse_document`].
    pub fn parse_document_sync(
        &self,
        bytes: &[u8],
        hint: DocumentHint<'_>,
    ) -> crate::Result<ParsedDocument> {
        self.parse_document_with(bytes, hint, &Budget::new(&self.config.limits))
    }

    fn parse_document_with(
        &self,
        bytes: &[u8],
        hint: DocumentHint<'_>,
        budget: &Budget,
    ) -> crate::Result<ParsedDocument> {
        self.config.limits.check_input(bytes.len())?;

        let format = Format::detect(bytes, hint);
        if let Some(p) = self.config.parsers.select(bytes, hint, format) {
            return self.parse_custom(p, bytes, budget);
        }

        match format {
            Format::Html => self.parse_html_doc(bytes, budget),
            Format::Pdf => self.parse_pdf(bytes, budget),
            Format::Docx => self.parse_docx(bytes, budget),
            Format::Xlsx | Format::Xlsb | Format::Xls | Format::Ods => {
                self.parse_spreadsheet(bytes, budget)
            }
            Format::PlainText => self.parse_plain_text(bytes, budget),
            Format::Markdown => self.parse_markdown(bytes, budget),
            Format::Csv | Format::Tsv => self.parse_csv(bytes, format, budget),
            Format::Notebook => self.parse_notebook(bytes, budget),
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

    fn parse_custom(
        &self,
        p: &dyn crate::registry::FormatParser,
        bytes: &[u8],
        budget: &Budget,
    ) -> crate::Result<ParsedDocument> {
        let mut parsed = guarded(p.name(), Error::Parser, || p.parse(bytes, &self.config))?;
        // Custom parsers are not interruptible: check the deadline afterwards.
        budget.check_time()?;
        self.config.transforms.apply(&mut parsed.document)?;
        Ok(parsed)
    }

    #[cfg(feature = "html")]
    fn parse_html_doc(&self, bytes: &[u8], budget: &Budget) -> crate::Result<ParsedDocument> {
        let text = String::from_utf8_lossy(bytes);
        let parser = crate::html::HtmlParser::with_budget(&self.config, budget);
        let (mut doc, page) = parser.parse_page(&text)?;
        self.config.transforms.apply(&mut doc)?;
        let mut p = ParsedDocument::new(doc, Format::Html);
        p.title = page.title;
        p.metadata = page.metadata;
        Ok(p)
    }

    #[cfg(not(feature = "html"))]
    fn parse_html_doc(&self, _bytes: &[u8], _budget: &Budget) -> crate::Result<ParsedDocument> {
        Err(Error::UnsupportedFormat(
            "HTML support requires the 'html' feature".to_string(),
        ))
    }

    fn parse_plain_text(&self, bytes: &[u8], budget: &Budget) -> crate::Result<ParsedDocument> {
        let decoded = crate::text::decode(bytes);
        budget.check_time()?;
        let (mut doc, paragraphs) = crate::text::plain_to_ir(&decoded.text, budget)?;
        self.config.transforms.apply(&mut doc)?;
        let mut p = ParsedDocument::new(doc, Format::PlainText);
        p.warnings = decoded.warnings;
        p.stats.paragraphs = Some(paragraphs);
        Ok(p)
    }

    fn parse_csv(
        &self,
        bytes: &[u8],
        format: Format,
        budget: &Budget,
    ) -> crate::Result<ParsedDocument> {
        let decoded = crate::text::decode(bytes);
        budget.check_time()?;
        let out = crate::csv::parse(&decoded.text, format == Format::Tsv, &self.config, budget)?;
        let mut doc = out.doc;
        self.config.transforms.apply(&mut doc)?;
        let mut p = ParsedDocument::new(doc, format);
        p.warnings = decoded.warnings;
        p.warnings.extend(out.warnings);
        p.stats.rows = Some(out.rows);
        p.stats.tables = Some(out.tables);
        Ok(p)
    }

    fn parse_notebook(&self, bytes: &[u8], budget: &Budget) -> crate::Result<ParsedDocument> {
        let decoded = crate::text::decode(bytes);
        budget.check_time()?;
        let out = crate::notebook::parse(&decoded.text, &self.config, budget)?;
        let mut doc = out.doc;
        self.config.transforms.apply(&mut doc)?;
        let mut p = ParsedDocument::new(doc, Format::Notebook);
        p.title = out.title;
        p.warnings = decoded.warnings;
        p.stats.cells = Some(out.cells);
        Ok(p)
    }

    #[cfg(feature = "markdown")]
    fn parse_markdown(&self, bytes: &[u8], budget: &Budget) -> crate::Result<ParsedDocument> {
        let decoded = crate::text::decode(bytes);
        budget.check_time()?;
        let out = crate::markdown::parse(&decoded.text, &self.config, budget)?;
        let mut doc = out.doc;
        self.config.transforms.apply(&mut doc)?;
        let mut p = ParsedDocument::new(doc, Format::Markdown);
        p.title = out.title;
        p.warnings = decoded.warnings;
        p.stats.paragraphs = Some(out.paragraphs);
        p.stats.tables = Some(out.tables);
        Ok(p)
    }

    #[cfg(not(feature = "markdown"))]
    fn parse_markdown(&self, _bytes: &[u8], _budget: &Budget) -> crate::Result<ParsedDocument> {
        Err(Error::UnsupportedFormat(
            "Markdown support requires the 'markdown' feature".to_string(),
        ))
    }

    #[cfg(feature = "docx")]
    fn parse_docx(&self, bytes: &[u8], budget: &Budget) -> crate::Result<ParsedDocument> {
        let out = guarded("DOCX", Error::DocxExtract, || {
            crate::docx::parse(bytes, &self.config, budget)
        })?;
        let mut doc = out.doc;
        self.config.transforms.apply(&mut doc)?;
        let mut p = ParsedDocument::new(doc, Format::Docx);
        p.title = out.title;
        p.warnings = out.warnings;
        p.stats.paragraphs = Some(out.paragraphs);
        p.stats.tables = Some(out.tables);
        Ok(p)
    }

    #[cfg(not(feature = "docx"))]
    fn parse_docx(&self, _bytes: &[u8], _budget: &Budget) -> crate::Result<ParsedDocument> {
        Err(Error::UnsupportedFormat(
            "DOCX support requires the 'docx' feature".to_string(),
        ))
    }

    #[cfg(feature = "spreadsheet")]
    fn parse_spreadsheet(&self, bytes: &[u8], budget: &Budget) -> crate::Result<ParsedDocument> {
        let out = guarded("spreadsheet", Error::SpreadsheetExtract, || {
            crate::spreadsheet::parse(bytes, &self.config, budget)
        })?;
        let mut doc = out.doc;
        self.config.transforms.apply(&mut doc)?;
        let mut p = ParsedDocument::new(doc, out.format);
        p.title = out.title;
        p.warnings = out.warnings;
        p.stats.sheets = Some(out.sheets);
        p.stats.rows = Some(out.rows);
        Ok(p)
    }

    #[cfg(not(feature = "spreadsheet"))]
    fn parse_spreadsheet(&self, _bytes: &[u8], _budget: &Budget) -> crate::Result<ParsedDocument> {
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
    /// `max_redirects`; body capped by `max_input_size`; fetch, parse and render
    /// together capped by `limits.timeout` ([`Error::ResourceLimit`]).
    ///
    /// Relative links and images resolve against the final URL after redirects.
    #[cfg(feature = "http_client")]
    pub async fn extract_url(&self, url: &str) -> crate::Result<String> {
        self.extract_url_document(url).await.map(|e| e.markdown)
    }

    /// [`Markkit::extract_url`] with metadata (format, title, warnings, stats).
    ///
    /// The response `Content-Type` is the format hint; same URL policy and limits.
    /// Relative HTML URLs resolve against the final URL after redirects.
    #[cfg(feature = "http_client")]
    pub async fn extract_url_document(&self, url: &str) -> crate::Result<Extraction> {
        let start = std::time::Instant::now();
        let fetched = crate::fetch::fetch(url, &self.config).await?;
        // Fetch, parse and render share one `timeout` budget.
        let budget = Budget::starting_at(&self.config.limits, start);
        let mut config = self.config.clone();
        config.base_url = Some(fetched.url);
        Markkit::new(config).extract_document_with(
            &fetched.bytes,
            DocumentHint {
                content_type: fetched.content_type.as_deref(),
                file_name: None,
            },
            &budget,
        )
    }

    #[cfg(feature = "pdf")]
    fn parse_pdf(&self, bytes: &[u8], budget: &Budget) -> crate::Result<ParsedDocument> {
        let out = guarded("PDF", Error::PdfExtract, || {
            crate::pdf::parse(bytes, &self.config, budget)
        })?;
        let mut p = ParsedDocument::new(out.doc, Format::Pdf);
        p.title = out.title;
        p.warnings = out.warnings;
        p.stats.pages = Some(out.pages);
        Ok(p)
    }

    #[cfg(not(feature = "pdf"))]
    fn parse_pdf(&self, _bytes: &[u8], _budget: &Budget) -> crate::Result<ParsedDocument> {
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
        assert_eq!(m.config().limits.max_input_size, 50 * 1024 * 1024);
        assert!(m.config().preserve_links);
    }

    #[tokio::test]
    async fn test_resource_limit() {
        let m = Markkit::builder().max_input_size(10).build();
        let result = m.extract_html("hello world!").await;
        assert!(matches!(result, Err(Error::ResourceLimit(_))));
    }
}
