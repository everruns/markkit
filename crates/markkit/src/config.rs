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

    pub fn include_selector(mut self, selector: impl Into<String>) -> Self {
        self.include_selectors.push(selector.into());
        self
    }

    pub fn exclude_selector(mut self, selector: impl Into<String>) -> Self {
        self.exclude_selectors.push(selector.into());
        self
    }

    pub fn build(self) -> Markkit {
        Markkit::new(self)
    }
}
