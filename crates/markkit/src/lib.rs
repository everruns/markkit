//! # Markkit
//!
//! Extract clean markdown from any format.
//!
//! Markkit is a Rust library for converting HTML, PDF, DOCX, spreadsheets,
//! and other document formats into clean, well-structured markdown.
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use markkit::{DocumentHint, Markkit};
//!
//! # async fn example() -> anyhow::Result<()> {
//! let markkit = Markkit::builder().build();
//!
//! // Extract markdown from HTML
//! let md = markkit.extract_html("<h1>Hello</h1><p>World</p>").await?;
//!
//! // Convert any supported document from bytes (format detected from the
//! // content type, then the file extension, then magic bytes).
//! let bytes = std::fs::read("report.docx")?;
//! let doc = markkit.extract_document_sync(&bytes, DocumentHint::file_name("report.docx"))?;
//! println!("{} ({}): {}", doc.title.unwrap_or_default(), doc.format, doc.markdown);
//!
//! // Extract from a URL (requires the `http_client` feature)
//! #[cfg(feature = "http_client")]
//! let md = markkit.extract_url("https://example.com").await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Features
//!
//! - **HTML** (`html`, default): Convert HTML to markdown with configurable rules
//! - **DOCX** (`docx`): Convert Word documents
//! - **Spreadsheet** (`spreadsheet`): Convert XLSX/XLSM, XLS and ODS workbooks to tables
//! - **Plain text** (always on): `text/plain`, `.txt`
//! - **CSV / TSV** (always on): `text/csv`, `text/tab-separated-values`, `.csv`, `.tsv`, one table
//! - **Markdown** (`markdown`): `text/markdown`, `.md`, normalized through the IR
//! - **PDF** (`pdf`): Planned; currently returns [`Error::UnsupportedFormat`]
//! - **URL fetching** (`http_client`, opt-in): Fetch and extract from URLs via reqwest
//! - **Agent tool** (`tool`): `markkit::tool::MarkkitTool`, an LLM tool following the
//!   Everruns toolkit-library contract (schemas, OpenAI definition, `tower::Service`)
//! - **JSON Schema** (`json-schema`): `ir::json_schema()` for the serialized IR
//!
//! The default build has no network stack; enable `http_client` for
//! `Markkit::extract_url`. Office formats only, no HTML:
//! `markkit = { version = "0.1", default-features = false, features = ["docx", "spreadsheet"] }`.

#[cfg(any(feature = "docx", feature = "spreadsheet"))]
mod archive;
pub mod config;
mod csv;
#[cfg(feature = "docx")]
mod docx;
pub mod error;
pub mod extract;
#[cfg(feature = "http_client")]
mod fetch;
pub mod format;
#[cfg(feature = "html")]
pub mod html;
pub mod ir;
pub mod limits;
#[cfg(feature = "markdown")]
mod markdown;
pub mod render;
#[cfg(feature = "spreadsheet")]
mod spreadsheet;
mod text;
#[cfg(feature = "tool")]
pub mod tool;
pub mod transform;
#[cfg(any(feature = "docx", feature = "spreadsheet"))]
#[cfg_attr(not(feature = "docx"), allow(dead_code))] // spreadsheet uses only `find`/`text`
mod xml;

pub use config::MarkkitBuilder;
pub use error::Error;
pub use extract::{Extraction, ExtractionStats, Markkit, Metadata, ParsedDocument};
pub use format::{DocumentHint, Format};
pub use ir::Document;
pub use limits::Limits;
pub use render::MarkdownRenderer;

/// Result type alias for markkit operations.
pub type Result<T> = std::result::Result<T, Error>;
