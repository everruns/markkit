//! Format detection and content type handling.
//!
//! Decision: detection order is content type → file extension → magic bytes.
//! Generic types (`application/octet-stream`, `application/zip`, …) map to
//! [`Format::Unknown`] so they fall through to the next signal. Magic-byte
//! sniffing of ZIP/OLE2 containers scans raw bytes for well-known part / stream
//! names, so detection needs no archive dependency and works without any
//! format feature enabled. Parsers re-validate the container themselves.

use serde::{Deserialize, Serialize};

/// Detected document format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Format {
    Html,
    Pdf,
    /// Office Open XML word processing (`.docx`, `.docm`, `.dotx`).
    Docx,
    /// Legacy Word 97-2003 binary (`.doc`). Detected but not supported.
    Doc,
    /// Office Open XML spreadsheet (`.xlsx`, `.xlsm`, `.xltx`).
    Xlsx,
    /// Legacy Excel 97-2003 binary (`.xls`).
    Xls,
    /// OpenDocument spreadsheet (`.ods`).
    Ods,
    /// Office Open XML presentation (`.pptx`). Detected but not supported.
    Pptx,
    Csv,
    PlainText,
    Markdown,
    Unknown,
}

/// Optional hints that help identify a document's format.
///
/// Both fields are untrusted metadata (e.g. an HTTP `Content-Type` header and a
/// URL path or upload file name).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DocumentHint<'a> {
    /// MIME content type, parameters allowed (`text/html; charset=utf-8`).
    pub content_type: Option<&'a str>,
    /// File name or URL path; only the extension is used.
    pub file_name: Option<&'a str>,
}

impl<'a> DocumentHint<'a> {
    /// Hint with only a content type.
    pub fn content_type(content_type: &'a str) -> Self {
        Self {
            content_type: Some(content_type),
            file_name: None,
        }
    }

    /// Hint with only a file name / URL path.
    pub fn file_name(file_name: &'a str) -> Self {
        Self {
            content_type: None,
            file_name: Some(file_name),
        }
    }
}

const OLE2_MAGIC: &[u8] = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1";
const ODS_MIMETYPE: &[u8] = b"mimetypeapplication/vnd.oasis.opendocument.spreadsheet";

impl Format {
    /// Short lowercase name (`"docx"`, `"xlsx"`, …).
    pub fn as_str(&self) -> &'static str {
        match self {
            Format::Html => "html",
            Format::Pdf => "pdf",
            Format::Docx => "docx",
            Format::Doc => "doc",
            Format::Xlsx => "xlsx",
            Format::Xls => "xls",
            Format::Ods => "ods",
            Format::Pptx => "pptx",
            Format::Csv => "csv",
            Format::PlainText => "text",
            Format::Markdown => "markdown",
            Format::Unknown => "unknown",
        }
    }

    /// Detect format from a MIME content type string.
    pub fn from_content_type(content_type: &str) -> Self {
        let ct = content_type.to_lowercase();
        let ct = ct.split(';').next().unwrap_or("").trim();
        if ct.contains("text/html") || ct.contains("application/xhtml") {
            Format::Html
        } else if ct.contains("application/pdf") {
            Format::Pdf
        } else if ct.contains("wordprocessingml") || ct.contains("ms-word.document") {
            Format::Docx
        } else if ct.contains("msword") {
            Format::Doc
        } else if ct.contains("spreadsheetml") || ct.contains("ms-excel.sheet") {
            Format::Xlsx
        } else if ct.contains("ms-excel") {
            Format::Xls
        } else if ct.contains("opendocument.spreadsheet") {
            Format::Ods
        } else if ct.contains("presentationml") || ct.contains("ms-powerpoint") {
            Format::Pptx
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

    /// Detect format from a file extension (without the dot).
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "html" | "htm" | "xhtml" => Format::Html,
            "pdf" => Format::Pdf,
            "docx" | "docm" | "dotx" | "dotm" => Format::Docx,
            "doc" | "dot" => Format::Doc,
            "xlsx" | "xlsm" | "xltx" | "xltm" => Format::Xlsx,
            "xls" | "xlt" => Format::Xls,
            "ods" => Format::Ods,
            "pptx" | "pptm" | "ppt" => Format::Pptx,
            "csv" | "tsv" => Format::Csv,
            "txt" => Format::PlainText,
            "md" | "markdown" => Format::Markdown,
            _ => Format::Unknown,
        }
    }

    /// Detect format from a file name or URL path (query/fragment ignored).
    pub fn from_file_name(name: &str) -> Self {
        let path = name.split(['?', '#']).next().unwrap_or("");
        let base = path.rsplit(['/', '\\']).next().unwrap_or("");
        match base.rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() => Self::from_extension(ext),
            _ => Format::Unknown,
        }
    }

    /// Detect format from magic bytes / container contents.
    pub fn sniff(bytes: &[u8]) -> Self {
        if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
            return sniff_zip(bytes);
        }
        if bytes.starts_with(OLE2_MAGIC) {
            return sniff_ole2(bytes);
        }
        // PDF header may be preceded by junk within the first 1 KiB.
        if contains(&bytes[..bytes.len().min(1024)], b"%PDF-") {
            return Format::Pdf;
        }
        if looks_like_html(bytes) {
            return Format::Html;
        }
        Format::Unknown
    }

    /// Detect format: content type, then file extension, then magic bytes.
    pub fn detect(bytes: &[u8], hint: DocumentHint<'_>) -> Self {
        if let Some(ct) = hint.content_type {
            let f = Self::from_content_type(ct);
            if f != Format::Unknown {
                return f;
            }
        }
        if let Some(name) = hint.file_name {
            let f = Self::from_file_name(name);
            if f != Format::Unknown {
                return f;
            }
        }
        Self::sniff(bytes)
    }
}

impl std::fmt::Display for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

fn sniff_zip(bytes: &[u8]) -> Format {
    // ODF stores an uncompressed `mimetype` entry first; OOXML part names are
    // stored uncompressed in local and central directory headers.
    if contains(&bytes[..bytes.len().min(256)], ODS_MIMETYPE) {
        Format::Ods
    } else if contains(bytes, b"word/document") {
        Format::Docx
    } else if contains(bytes, b"xl/workbook.") {
        Format::Xlsx
    } else if contains(bytes, b"ppt/presentation.") {
        Format::Pptx
    } else if contains(bytes, ODS_MIMETYPE) {
        Format::Ods
    } else {
        Format::Unknown
    }
}

fn sniff_ole2(bytes: &[u8]) -> Format {
    let utf16 = |s: &str| -> Vec<u8> { s.encode_utf16().flat_map(u16::to_le_bytes).collect() };
    if contains(bytes, &utf16("WordDocument")) {
        Format::Doc
    } else if contains(bytes, &utf16("Workbook")) || contains(bytes, &utf16("Book")) {
        Format::Xls
    } else {
        Format::Unknown
    }
}

fn looks_like_html(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(512)];
    let head = head.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(head);
    let start = head
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(head.len());
    let head = head[start..].to_ascii_lowercase();
    [&b"<!doctype html"[..], b"<html", b"<head", b"<body"]
        .iter()
        .any(|p| head.starts_with(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_type_detection() {
        assert_eq!(
            Format::from_content_type("text/html; charset=utf-8"),
            Format::Html
        );
        assert_eq!(Format::from_content_type("application/pdf"), Format::Pdf);
        assert_eq!(Format::from_content_type("text/plain"), Format::PlainText);
    }

    #[test]
    fn test_extension_detection() {
        assert_eq!(Format::from_extension("html"), Format::Html);
        assert_eq!(Format::from_extension("PDF"), Format::Pdf);
        assert_eq!(Format::from_extension("docx"), Format::Docx);
    }

    #[test]
    fn test_file_name_detection() {
        assert_eq!(Format::from_file_name("a/b/c.xlsx"), Format::Xlsx);
        assert_eq!(Format::from_file_name("https://x/y.ods?z=1"), Format::Ods);
        assert_eq!(Format::from_file_name(".docx"), Format::Unknown);
        assert_eq!(Format::from_file_name("noext"), Format::Unknown);
        assert_eq!(Format::from_file_name("dir.d/file"), Format::Unknown);
    }
}
