//! Node.js bindings (`@everruns/markkit`).
//!
//! Decisions:
//! - Every entry point takes an optional trailing `options` object (camelCase
//!   builder fields), so existing calls keep working and one options shape is
//!   shared with `@everruns/markkit-wasm` and Python's keyword arguments.
//! - Selectors add to the defaults (as in Python and WASM).
//! - URL policy knobs (`allowPrivateNetworks`, `allowedHosts`, `maxRedirects`)
//!   are opt-in; defaults keep the SSRF guard on.

use napi::bindgen_prelude::Buffer;
use napi_derive::napi;

/// Extraction options; every field is optional and defaults to the Rust builder.
#[napi(object)]
#[derive(Default)]
pub struct MarkkitOptions {
    /// Maximum input size in bytes (default 50 MiB).
    pub max_input_size: Option<u32>,
    /// Upper bound for `extractUrl` in seconds (default 60).
    pub max_processing_secs: Option<u32>,
    pub preserve_links: Option<bool>,
    pub preserve_images: Option<bool>,
    /// CSS selectors to include (empty = whole document).
    pub include_selectors: Option<Vec<String>>,
    /// CSS selectors to drop, added to nav/footer/header/script/style/noscript.
    pub exclude_selectors: Option<Vec<String>>,
    pub max_archive_entries: Option<u32>,
    pub max_archive_entry_size: Option<f64>,
    pub max_archive_total_size: Option<f64>,
    pub max_sheet_rows: Option<u32>,
    pub max_sheet_cols: Option<u32>,
    pub max_spreadsheet_cells: Option<f64>,
    pub include_hidden_sheets: Option<bool>,
    /// Let `extractUrl` reach loopback/private/link-local addresses. Trusted URLs only.
    pub allow_private_networks: Option<bool>,
    /// Hosts `extractUrl` may contact (checked on every redirect).
    pub allowed_hosts: Option<Vec<String>>,
    /// Maximum redirects `extractUrl` follows (default 5).
    pub max_redirects: Option<u32>,
}

/// Format hints for `extractDocument`.
#[napi(object)]
#[derive(Default)]
pub struct DocumentHint {
    /// MIME type, e.g. from a `Content-Type` header.
    pub content_type: Option<String>,
    /// File name or URL path; only the extension is used.
    pub file_name: Option<String>,
}

#[napi(object)]
pub struct ExtractionStats {
    pub pages: Option<u32>,
    pub sheets: Option<u32>,
    pub rows: Option<u32>,
    pub paragraphs: Option<u32>,
    pub tables: Option<u32>,
}

/// Document metadata (HTML meta, Open Graph, JSON-LD); fields absent when unknown.
#[napi(object)]
pub struct Metadata {
    pub description: Option<String>,
    pub author: Option<String>,
    pub published: Option<String>,
    pub language: Option<String>,
    pub canonical_url: Option<String>,
    pub site_name: Option<String>,
    pub image: Option<String>,
}

/// Markdown plus metadata.
#[napi(object)]
pub struct Extraction {
    pub markdown: String,
    pub title: Option<String>,
    /// Detected format (`html`, `docx`, `xlsx`, `xls`, `ods`).
    pub format: String,
    pub warnings: Vec<String>,
    pub stats: ExtractionStats,
    pub metadata: Metadata,
}

fn invalid(field: &str) -> napi::Error {
    napi::Error::from_reason(format!(
        "invalid options: `{field}` must be a non-negative integer"
    ))
}

/// JS numbers above 2^32 arrive as f64; accept only whole, non-negative values.
fn to_u64(v: f64, field: &str) -> napi::Result<u64> {
    if v.is_finite() && v >= 0.0 && v.fract() == 0.0 && v <= u64::MAX as f64 {
        Ok(v as u64)
    } else {
        Err(invalid(field))
    }
}

fn markkit(options: Option<MarkkitOptions>) -> napi::Result<markkit::Markkit> {
    let o = options.unwrap_or_default();
    let mut b = markkit::Markkit::builder();
    if let Some(v) = o.max_input_size {
        b = b.max_input_size(v as usize);
    }
    if let Some(v) = o.max_processing_secs {
        b = b.max_processing_secs(v.into());
    }
    if let Some(v) = o.preserve_links {
        b = b.preserve_links(v);
    }
    if let Some(v) = o.preserve_images {
        b = b.preserve_images(v);
    }
    for s in o.include_selectors.unwrap_or_default() {
        b = b.include_selector(s);
    }
    for s in o.exclude_selectors.unwrap_or_default() {
        b = b.exclude_selector(s);
    }
    if let Some(v) = o.max_archive_entries {
        b = b.max_archive_entries(v as usize);
    }
    if let Some(v) = o.max_archive_entry_size {
        b = b.max_archive_entry_size(to_u64(v, "maxArchiveEntrySize")?);
    }
    if let Some(v) = o.max_archive_total_size {
        b = b.max_archive_total_size(to_u64(v, "maxArchiveTotalSize")?);
    }
    if let Some(v) = o.max_sheet_rows {
        b = b.max_sheet_rows(v as usize);
    }
    if let Some(v) = o.max_sheet_cols {
        b = b.max_sheet_cols(v as usize);
    }
    if let Some(v) = o.max_spreadsheet_cells {
        b = b.max_spreadsheet_cells(to_u64(v, "maxSpreadsheetCells")?);
    }
    if let Some(v) = o.include_hidden_sheets {
        b = b.include_hidden_sheets(v);
    }
    if let Some(v) = o.allow_private_networks {
        b = b.allow_private_networks(v);
    }
    for h in o.allowed_hosts.unwrap_or_default() {
        b = b.allow_host(h);
    }
    if let Some(v) = o.max_redirects {
        b = b.max_redirects(v as usize);
    }
    Ok(b.build())
}

fn err(e: markkit::Error) -> napi::Error {
    napi::Error::from_reason(e.to_string())
}

fn count(v: Option<usize>) -> Option<u32> {
    v.map(|n| u32::try_from(n).unwrap_or(u32::MAX))
}

fn to_js(e: markkit::Extraction) -> Extraction {
    Extraction {
        markdown: e.markdown,
        title: e.title,
        format: e.format.as_str().to_string(),
        warnings: e.warnings,
        stats: ExtractionStats {
            pages: count(e.stats.pages),
            sheets: count(e.stats.sheets),
            rows: count(e.stats.rows),
            paragraphs: count(e.stats.paragraphs),
            tables: count(e.stats.tables),
        },
        metadata: Metadata {
            description: e.metadata.description,
            author: e.metadata.author,
            published: e.metadata.published,
            language: e.metadata.language,
            canonical_url: e.metadata.canonical_url,
            site_name: e.metadata.site_name,
            image: e.metadata.image,
        },
    }
}

fn document(
    data: &[u8],
    hint: Option<DocumentHint>,
    options: Option<MarkkitOptions>,
) -> napi::Result<Extraction> {
    let hint = hint.unwrap_or_default();
    markkit(options)?
        .extract_document_sync(
            data,
            markkit::DocumentHint {
                content_type: hint.content_type.as_deref(),
                file_name: hint.file_name.as_deref(),
            },
        )
        .map(to_js)
        .map_err(err)
}

/// Extract markdown from an HTML string (async).
#[napi]
pub async fn extract_html(html: String, options: Option<MarkkitOptions>) -> napi::Result<String> {
    markkit(options)?.extract_html(&html).await.map_err(err)
}

/// Extract markdown from an HTML string (synchronous).
#[napi]
pub fn extract_html_sync(html: String, options: Option<MarkkitOptions>) -> napi::Result<String> {
    markkit(options)?.extract_html_sync(&html).map_err(err)
}

/// Extract markdown from a URL. Non-public addresses are refused unless
/// `allowPrivateNetworks` is set.
#[napi]
pub async fn extract_url(url: String, options: Option<MarkkitOptions>) -> napi::Result<String> {
    markkit(options)?.extract_url(&url).await.map_err(err)
}

/// Extract markdown from a buffer with optional content type.
#[napi]
pub async fn extract_bytes(
    data: Buffer,
    content_type: Option<String>,
    options: Option<MarkkitOptions>,
) -> napi::Result<String> {
    markkit(options)?
        .extract_bytes(&data, content_type.as_deref())
        .await
        .map_err(err)
}

/// Convert a document (HTML, DOCX, XLSX/XLS/ODS) with metadata (async).
#[napi]
pub async fn extract_document(
    data: Buffer,
    hint: Option<DocumentHint>,
    options: Option<MarkkitOptions>,
) -> napi::Result<Extraction> {
    document(&data, hint, options)
}

/// Convert a document (HTML, DOCX, XLSX/XLS/ODS) with metadata (synchronous).
#[napi]
pub fn extract_document_sync(
    data: Buffer,
    hint: Option<DocumentHint>,
    options: Option<MarkkitOptions>,
) -> napi::Result<Extraction> {
    document(&data, hint, options)
}
