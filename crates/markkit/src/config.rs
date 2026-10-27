use crate::extract::Markkit;

/// Builder for configuring a Markkit instance.
#[derive(Debug, Clone)]
pub struct MarkkitBuilder {
    /// Maximum input size in bytes (default: 50MB).
    pub max_input_size: usize,
    /// Maximum processing time in seconds (default: 60).
    pub max_processing_secs: u64,
    /// Whether to preserve link URLs in output.
    pub preserve_links: bool,
    /// Whether to preserve image references in output.
    pub preserve_images: bool,
    /// Whether to extract content from code blocks.
    pub extract_code_blocks: bool,
    /// CSS selectors for content to include (empty = include all).
    pub include_selectors: Vec<String>,
    /// CSS selectors for content to exclude.
    pub exclude_selectors: Vec<String>,
    /// Maximum number of entries in a ZIP container (DOCX/XLSX/ODS). Default: 10 000.
    pub max_archive_entries: usize,
    /// Maximum decompressed size of a single ZIP entry in bytes. Default: 64 MiB.
    pub max_archive_entry_size: u64,
    /// Maximum total decompressed size of a ZIP container in bytes. Default: 256 MiB.
    pub max_archive_total_size: u64,
    /// Maximum data rows rendered per sheet (header excluded). Default: 10 000.
    pub max_sheet_rows: usize,
    /// Maximum columns rendered per sheet. Default: 256.
    pub max_sheet_cols: usize,
    /// Maximum cell grid a workbook may expand to while parsing (XLS/ODS
    /// pre-scan, summed over sheets). Default: 4 000 000.
    pub max_spreadsheet_cells: u64,
    /// Render hidden / very hidden sheets. Default: false (skipped with a warning).
    pub include_hidden_sheets: bool,
    /// Let `extract_url` reach loopback, private, link-local and other
    /// non-public addresses. Default: false (SSRF protection).
    pub allow_private_networks: bool,
    /// Hosts `extract_url` may contact, matched exactly and case-insensitively
    /// on every hop (empty = any public host).
    pub allowed_hosts: Vec<String>,
    /// Maximum redirects `extract_url` follows. Default: 5.
    pub max_redirects: usize,
    /// Maximum PDF pages rendered (rest skipped with a warning). Default: 10 000.
    pub max_pdf_pages: usize,
    /// Maximum decoded size of a single PDF stream in bytes. Default: 64 MiB.
    pub max_pdf_stream_size: usize,
    /// Maximum total decoded size of all PDF streams, and of content
    /// interpreted (form XObjects count once per use), in bytes. Default: 256 MiB.
    pub max_pdf_decoded_size: usize,
    /// Record per-block provenance ([`crate::ir::Source`]) in the IR:
    /// HTML element XPath, DOCX block index, spreadsheet sheet and range.
    /// Default: false (no cost, no `source` in IR JSON).
    pub track_sources: bool,
}

impl Default for MarkkitBuilder {
    fn default() -> Self {
        Self {
            max_input_size: 50 * 1024 * 1024,
            max_processing_secs: 60,
            preserve_links: true,
            preserve_images: true,
            extract_code_blocks: true,
            include_selectors: Vec::new(),
            exclude_selectors: vec![
                "nav".into(),
                "footer".into(),
                "header".into(),
                "script".into(),
                "style".into(),
                "noscript".into(),
            ],
            max_archive_entries: 10_000,
            max_archive_entry_size: 64 * 1024 * 1024,
            max_archive_total_size: 256 * 1024 * 1024,
            max_sheet_rows: 10_000,
            max_sheet_cols: 256,
            max_spreadsheet_cells: 4_000_000,
            include_hidden_sheets: false,
            allow_private_networks: false,
            allowed_hosts: Vec::new(),
            max_redirects: 5,
            max_pdf_pages: 10_000,
            max_pdf_stream_size: 64 * 1024 * 1024,
            max_pdf_decoded_size: 256 * 1024 * 1024,
            track_sources: false,
        }
    }
}

impl MarkkitBuilder {
    pub fn max_input_size(mut self, size: usize) -> Self {
        self.max_input_size = size;
        self
    }

    pub fn max_processing_secs(mut self, secs: u64) -> Self {
        self.max_processing_secs = secs;
        self
    }

    pub fn preserve_links(mut self, yes: bool) -> Self {
        self.preserve_links = yes;
        self
    }

    pub fn preserve_images(mut self, yes: bool) -> Self {
        self.preserve_images = yes;
        self
    }

    /// Record per-block provenance in the IR (see [`MarkkitBuilder::track_sources`] field).
    pub fn track_sources(mut self, yes: bool) -> Self {
        self.track_sources = yes;
        self
    }

    pub fn include_selector(mut self, selector: impl Into<String>) -> Self {
        self.include_selectors.push(selector.into());
        self
    }

    pub fn exclude_selector(mut self, selector: impl Into<String>) -> Self {
        self.exclude_selectors.push(selector.into());
        self
    }

    pub fn max_archive_entries(mut self, n: usize) -> Self {
        self.max_archive_entries = n;
        self
    }

    pub fn max_archive_entry_size(mut self, bytes: u64) -> Self {
        self.max_archive_entry_size = bytes;
        self
    }

    pub fn max_archive_total_size(mut self, bytes: u64) -> Self {
        self.max_archive_total_size = bytes;
        self
    }

    pub fn max_sheet_rows(mut self, rows: usize) -> Self {
        self.max_sheet_rows = rows;
        self
    }

    pub fn max_sheet_cols(mut self, cols: usize) -> Self {
        self.max_sheet_cols = cols;
        self
    }

    pub fn max_spreadsheet_cells(mut self, cells: u64) -> Self {
        self.max_spreadsheet_cells = cells;
        self
    }

    pub fn include_hidden_sheets(mut self, yes: bool) -> Self {
        self.include_hidden_sheets = yes;
        self
    }

    /// Allow `extract_url` to reach non-public addresses (loopback, RFC 1918,
    /// link-local, cloud metadata, ...). Only for trusted URLs.
    pub fn allow_private_networks(mut self, yes: bool) -> Self {
        self.allow_private_networks = yes;
        self
    }

    /// Restrict `extract_url` to this host (repeatable). Checked on every redirect hop.
    pub fn allow_host(mut self, host: impl Into<String>) -> Self {
        self.allowed_hosts.push(host.into());
        self
    }

    pub fn max_redirects(mut self, n: usize) -> Self {
        self.max_redirects = n;
        self
    }

    pub fn max_pdf_pages(mut self, n: usize) -> Self {
        self.max_pdf_pages = n;
        self
    }

    pub fn max_pdf_stream_size(mut self, bytes: usize) -> Self {
        self.max_pdf_stream_size = bytes;
        self
    }

    pub fn max_pdf_decoded_size(mut self, bytes: usize) -> Self {
        self.max_pdf_decoded_size = bytes;
        self
    }

    pub fn build(self) -> Markkit {
        Markkit::new(self)
    }
}
