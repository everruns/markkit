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
//! - **PDF** (`pdf`): Text layer of PDF documents (no OCR); encrypted PDFs are
//!   rejected with [`Error::UnsupportedFormat`]
//! - **URL fetching** (`http_client`, default): Fetch and extract from URLs via reqwest
//! - **Agent tool** (`tool`): `markkit::tool::MarkkitTool`, an LLM tool following the
//!   Everruns toolkit-library contract (schemas, OpenAI definition, `tower::Service`)
//!
//! Embedders that do their own fetching can drop reqwest entirely:
//! `markkit = { version = "0.1", default-features = false, features = ["docx", "spreadsheet"] }`.

#[cfg(any(feature = "docx", feature = "spreadsheet"))]
mod archive;
pub mod config;
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
#[cfg(feature = "pdf")]
mod pdf;
pub mod render;
#[cfg(feature = "spreadsheet")]
mod spreadsheet;
#[cfg(feature = "tool")]
pub mod tool;
#[cfg(feature = "docx")]
mod xml;

pub use config::MarkkitBuilder;
pub use error::Error;
pub use extract::{Extraction, ExtractionStats, Markkit, Metadata, ParsedDocument};
pub use format::{DocumentHint, Format};
pub use ir::Document;
pub use render::MarkdownRenderer;

/// Result type alias for markkit operations.
pub type Result<T> = std::result::Result<T, Error>;
