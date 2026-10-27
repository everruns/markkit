use std::time::Duration;

use crate::extract::Markkit;
use crate::limits::Limits;

/// Builder for configuring a Markkit instance.
#[derive(Debug, Clone)]
pub struct MarkkitBuilder {
    /// Resource limits (input/output size, time, nesting, nodes, archives,
    /// spreadsheets, redirects). See [`Limits`].
    pub limits: Limits,
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
    /// Render hidden / very hidden sheets. Default: false (skipped with a warning).
    pub include_hidden_sheets: bool,
    /// Let `extract_url` reach loopback, private, link-local and other
    /// non-public addresses. Default: false (SSRF protection).
    pub allow_private_networks: bool,
    /// Hosts `extract_url` may contact, matched exactly and case-insensitively
    /// on every hop (empty = any public host).
    pub allowed_hosts: Vec<String>,
    /// Absolute URL that relative HTML links, images and metadata URLs resolve
    /// against (a document `<base href>` applies on top). `extract_url` uses the
    /// final response URL instead. Default: none (relative URLs kept as is).
    pub base_url: Option<String>,
    /// Record per-block provenance ([`crate::ir::Source`]) in the IR:
    /// HTML element XPath, DOCX block index, spreadsheet sheet and range.
    /// Default: false (no cost, no `source` in IR JSON).
    pub track_sources: bool,
    /// IR passes run after parsing, before rendering (see [`crate::transform`]).
    pub transforms: crate::transform::TransformChain,
    /// Keep only the detected main content (article body) of HTML pages,
    /// Readability-style. Include selectors win; falls back to the full body when
    /// nothing scores. Default: false.
    pub main_content: bool,
}

impl Default for MarkkitBuilder {
    fn default() -> Self {
        Self {
            limits: Limits::default(),
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
            include_hidden_sheets: false,
            allow_private_networks: false,
            allowed_hosts: Vec::new(),
            base_url: None,
            track_sources: false,
            transforms: Default::default(),
            main_content: false,
        }
    }
}

impl MarkkitBuilder {
    pub fn max_input_size(mut self, size: usize) -> Self {
        self.limits.max_input_size = size;
        self
    }

    /// Replace all limits at once.
    pub fn limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// Wall-clock budget per call (fetch + parse + render).
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.limits.timeout = timeout;
        self
    }

    /// [`MarkkitBuilder::timeout`] in whole seconds.
    pub fn max_processing_secs(mut self, secs: u64) -> Self {
        self.limits.timeout = Duration::from_secs(secs);
        self
    }

    pub fn max_output_size(mut self, bytes: usize) -> Self {
        self.limits.max_output_size = bytes;
        self
    }

    /// Clamped to [`crate::limits::HARD_MAX_NESTING_DEPTH`].
    pub fn max_nesting_depth(mut self, depth: usize) -> Self {
        self.limits.max_nesting_depth = depth;
        self
    }

    pub fn max_nodes(mut self, nodes: usize) -> Self {
        self.limits.max_nodes = nodes;
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

    /// Append an IR transform pass; passes run in registration order.
    pub fn transform(mut self, t: impl crate::transform::Transform + 'static) -> Self {
        self.transforms.push(t);
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
        self.limits.max_archive_entries = n;
        self
    }

    pub fn max_archive_entry_size(mut self, bytes: u64) -> Self {
        self.limits.max_archive_entry_size = bytes;
        self
    }

    pub fn max_archive_total_size(mut self, bytes: u64) -> Self {
        self.limits.max_archive_total_size = bytes;
        self
    }

    pub fn max_sheet_rows(mut self, rows: usize) -> Self {
        self.limits.max_sheet_rows = rows;
        self
    }

    pub fn max_sheet_cols(mut self, cols: usize) -> Self {
        self.limits.max_sheet_cols = cols;
        self
    }

    pub fn max_spreadsheet_cells(mut self, cells: u64) -> Self {
        self.limits.max_spreadsheet_cells = cells;
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
        self.limits.max_redirects = n;
        self
    }

    /// Resolve relative HTML URLs against this absolute URL. Ignored when it
    /// does not parse or cannot be a base (`data:`, `javascript:`, ...).
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// Extract only the detected main content of HTML pages (drops sidebars,
    /// comments, share bars and other boilerplate).
    pub fn main_content(mut self, yes: bool) -> Self {
        self.main_content = yes;
        self
    }

    pub fn build(self) -> Markkit {
        Markkit::new(self)
    }
}
