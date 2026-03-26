//! Format detection and content type handling.

/// Detected document format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Html,
    Pdf,
    Docx,
    Xlsx,
    Csv,
    PlainText,
    Markdown,
    Unknown,
}

impl Format {
    /// Detect format from a MIME content type string.
    pub fn from_content_type(content_type: &str) -> Self {
        let ct = content_type.to_lowercase();
        if ct.contains("text/html") || ct.contains("application/xhtml") {
            Format::Html
        } else if ct.contains("application/pdf") {
            Format::Pdf
        } else if ct.contains("wordprocessingml") || ct.contains("msword") {
            Format::Docx
        } else if ct.contains("spreadsheetml") || ct.contains("ms-excel") {
            Format::Xlsx
        } else if ct.contains("text/csv") {
            Format::Csv
        } else if ct.contains("text/plain") {
            Format::PlainText
        } else if ct.contains("text/markdown") {
            Format::Markdown
        } else {
            Format::Unknown
        }
    }

    /// Detect format from a file extension.
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "html" | "htm" | "xhtml" => Format::Html,
            "pdf" => Format::Pdf,
            "docx" | "doc" => Format::Docx,
            "xlsx" | "xls" | "ods" => Format::Xlsx,
            "csv" | "tsv" => Format::Csv,
            "txt" => Format::PlainText,
            "md" | "markdown" => Format::Markdown,
            _ => Format::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_type_detection() {
        assert_eq!(Format::from_content_type("text/html; charset=utf-8"), Format::Html);
        assert_eq!(Format::from_content_type("application/pdf"), Format::Pdf);
        assert_eq!(Format::from_content_type("text/plain"), Format::PlainText);
    }

    #[test]
    fn test_extension_detection() {
        assert_eq!(Format::from_extension("html"), Format::Html);
        assert_eq!(Format::from_extension("PDF"), Format::Pdf);
        assert_eq!(Format::from_extension("docx"), Format::Docx);
    }
}
