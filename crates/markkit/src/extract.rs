use crate::config::MarkkitBuilder;
use crate::error::Error;

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
    pub async fn extract_bytes(
        &self,
        bytes: &[u8],
        content_type: Option<&str>,
    ) -> crate::Result<String> {
        if bytes.len() > self.config.max_input_size {
            return Err(Error::ResourceLimit(format!(
                "Input size {} exceeds limit {}",
                bytes.len(),
                self.config.max_input_size
            )));
        }

        let mime = content_type.unwrap_or("application/octet-stream");

        match mime {
            m if m.contains("html") => {
                let text = String::from_utf8_lossy(bytes);
                self.extract_html(&text).await
            }
            m if m.contains("pdf") => self.extract_pdf(bytes).await,
            _ => Err(Error::UnsupportedFormat(mime.to_string())),
        }
    }

    /// Extract markdown from a URL by fetching and converting.
    #[cfg(feature = "http_client")]
    pub async fn extract_url(&self, url: &str) -> crate::Result<String> {
        let response = reqwest::get(url)
            .await
            .map_err(|e| Error::Http(e.to_string()))?;

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let bytes = response
            .bytes()
            .await
            .map_err(|e| Error::Http(e.to_string()))?;

        self.extract_bytes(&bytes, content_type.as_deref()).await
    }

    /// Extract markdown from PDF bytes.
    async fn extract_pdf(&self, _bytes: &[u8]) -> crate::Result<String> {
        #[cfg(feature = "pdf")]
        {
            // TODO: Implement PDF extraction
            Ok(String::new())
        }
        #[cfg(not(feature = "pdf"))]
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
