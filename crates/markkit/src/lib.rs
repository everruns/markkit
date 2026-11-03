//! # Markkit
//!
//! Extract clean Markdown from any document format, safely.
//!
//! Markkit converts HTML, PDF, Word, PowerPoint, spreadsheets, e-books, email,
//! notebooks and plain-text formats into well-structured Markdown for LLMs and
//! agents. Every input is treated as untrusted: nothing is executed, every
//! resource an input can drive is bounded ([`Limits`]), and the default build
//! has no network stack.
//!
//! The same engine ships as a CLI (`markkit-cli`), Python (`pip install
//! markkit`), Node.js (`@everruns/markkit`) and WebAssembly
//! (`@everruns/markkit-wasm`) packages.
//!
//! ## Quick start
//!
//! ```rust
//! # #[cfg(feature = "html")] {
//! use markkit::Markkit;
//!
//! let markkit = Markkit::builder().build();
//! let md = markkit.extract_html_sync("<h1>Hello</h1><p>World</p>")?;
//! assert_eq!(md, "# Hello\n\nWorld\n");
//! # }
//! # Ok::<(), markkit::Error>(())
//! ```
//!
//! ## Any format from bytes
//!
//! [`Markkit::extract_document_sync`] takes bytes plus an optional
//! [`DocumentHint`]. The format is detected from the content type, then the
//! file extension, then magic bytes. The [`Extraction`] carries Markdown,
//! title, [`Format`], structured [`Warning`]s, [`ExtractionStats`] and HTML
//! [`Metadata`].
//!
//! ```rust
//! use markkit::{DocumentHint, Format, Markkit};
//!
//! let markkit = Markkit::builder().build();
//! let csv = b"name,stars\nmarkkit,1200\n";
//! let doc = markkit.extract_document_sync(csv, DocumentHint::file_name("repos.csv"))?;
//! assert_eq!(doc.format, Format::Csv);
//! assert!(doc.markdown.contains("| markkit | 1200 |"));
//!
//! // Office files work the same way once their feature is enabled:
//! // let doc = markkit.extract_document_sync(&std::fs::read("report.docx")?,
//! //                                         DocumentHint::file_name("report.docx"))?;
//! # Ok::<(), markkit::Error>(())
//! ```
//!
//! Extraction is CPU-bound. The `*_sync` methods are the primitive; the async
//! methods ([`Markkit::extract_html`], [`Markkit::extract_document`]) run the
//! same work inline. In async servers call the sync variant from
//! `tokio::task::spawn_blocking`.
//!
//! ## Configuration
//!
//! [`Markkit::builder`] returns a [`MarkkitBuilder`]. A built [`Markkit`] is
//! immutable, cheap to share across threads, and reusable for many calls.
//!
//! ```rust
//! # #[cfg(feature = "html")] {
//! use markkit::Markkit;
//!
//! let markkit = Markkit::builder()
//!     .main_content(true)                        // keep the article, drop sidebars/comments
//!     .base_url("https://example.com/blog/")     // resolve relative links and images
//!     .include_selector("article")               // CSS selectors to keep...
//!     .exclude_selector(".newsletter-signup")    // ...and to drop
//!     .preserve_images(false)
//!     .build();
//! let md = markkit.extract_html_sync(
//!     r#"<article><h1>Post</h1><p><a href="next">Next</a></p></article>"#,
//! )?;
//! assert_eq!(md, "# Post\n\n[Next](https://example.com/blog/next)\n");
//! # }
//! # Ok::<(), markkit::Error>(())
//! ```
//!
//! ## Resource limits and errors
//!
//! Defaults are generous (50 MiB in, 128 MiB out, 60 s, 5 M nodes, ...).
//! Breaches return [`Error::ResourceLimit`]; [`Error::limit_kind`] names the
//! exceeded [`LimitKind`].
//!
//! ```rust
//! use std::time::Duration;
//! use markkit::{DocumentHint, LimitKind, Limits, Markkit};
//!
//! let markkit = Markkit::builder()
//!     .limits(Limits::default().max_output_size(16).timeout(Duration::from_secs(5)))
//!     .build();
//! let csv = b"city,population\nKyiv,2952301\nLviv,717273\n";
//! let err = markkit.extract_document_sync(csv, DocumentHint::file_name("cities.csv")).unwrap_err();
//! assert_eq!(err.limit_kind(), Some(LimitKind::MaxOutputSize));
//! ```
//!
//! ## Pipeline: parse, transform, render
//!
//! Every format parses into one intermediate representation ([`ir::Document`]),
//! optional [`transform`] passes rewrite it, and [`MarkdownRenderer`] renders
//! it. Use [`Markkit::parse_document_sync`] (or [`Markkit::parse_html`]) to
//! inspect or chunk the IR yourself; with
//! [`track_sources(true)`](MarkkitBuilder::track_sources) each block carries a
//! [`ir::Source`] (HTML XPath, DOCX block index, sheet and cell range, PDF page).
//! Custom formats plug in through [`registry::FormatParser`], site-specific HTML
//! markup through [`rules`].
//!
//! ## Cargo features
//!
//! | Feature | Default | Adds |
//! |---|---|---|
//! | `html` | yes | HTML / XHTML |
//! | `pdf` | | PDF text, reading order, columns and tables (pure Rust); encrypted PDFs are rejected |
//! | `pdf-ocr` | | OCR of textless PDF pages: Hayro rendering + installed Tesseract |
//! | `pdf-ocr-ocrs` | | Rust OCR backend with caller-supplied local ONNX models |
//! | `docx` | | Word `.docx` |
//! | `pptx` | | PowerPoint `.pptx`, one section per slide |
//! | `spreadsheet` | | XLSX/XLSM, XLSB, XLS, ODS, one table per sheet |
//! | `odt` | | OpenDocument text |
//! | `epub` | | EPUB e-books (implies `html`) |
//! | `email` | | `.eml` / `message/rfc822` |
//! | `markdown` | | Markdown normalized through the IR |
//! | `all_formats` | | All format features above except OCR |
//! | `http_client` | | [`Markkit::extract_url`] with SSRF protection (reqwest) |
//! | `tool` | | `tool::MarkkitTool`, an LLM agent tool (Everruns toolkit contract) |
//! | `json-schema` | | `ir::json_schema()` for the serialized IR |
//!
//! Plain text, CSV/TSV and Jupyter notebooks are always available. Legacy
//! `.doc` and `.ppt` are detected and rejected with [`Error::UnsupportedFormat`].
//! Office formats only, no HTML, no network:
//! `markkit = { version = "0.3", default-features = false, features = ["docx", "spreadsheet"] }`.
//!
//! ## Examples
//!
//! Runnable examples live in
//! [`examples/`](https://github.com/everruns/markkit/tree/main/examples):
//! `cargo run -p markkit --example basic`, `documents`, `main_content`,
//! `limits`, `configuration`, `custom_selectors`, `ir_access`, `transforms`,
//! `agent_tool` (`--features tool`) and `convert` (`--features all_formats`,
//! converts DOCX, XLSX and PDF samples or your own files).
//!
//! ## Security
//!
//! Scripts, macros and formulas are never executed; XML entities are never
//! expanded; archives and PDF streams are decompressed under measured byte
//! limits; hidden content (a prompt-injection vector) is dropped by default;
//! `extract_url` refuses private, loopback and metadata addresses on every
//! redirect. See the
//! [threat model](https://github.com/everruns/markkit/blob/main/knowledge/security/threat-model.md).

#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(any(
    feature = "docx",
    feature = "pptx",
    feature = "spreadsheet",
    feature = "odt",
    feature = "epub"
))]
mod archive;
pub mod config;
mod csv;
#[cfg(feature = "docx")]
mod docx;
#[cfg(feature = "email")]
mod eml;
#[cfg(feature = "epub")]
mod epub;
pub mod error;
pub mod extract;
#[cfg(feature = "http_client")]
mod fetch;
pub mod format;
#[cfg(feature = "html")]
pub mod html;
pub mod ir;
pub mod layout;
pub mod limits;
#[cfg(feature = "markdown")]
mod markdown;
mod notebook;
#[cfg(feature = "odt")]
mod odt;
#[cfg(any(feature = "docx", feature = "pptx"))]
mod ooxml;
#[cfg(feature = "pdf")]
mod pdf;
#[cfg(feature = "pptx")]
mod pptx;
pub mod registry;
pub mod render;
pub mod rules;
#[cfg(feature = "spreadsheet")]
mod spreadsheet;
mod text;
#[cfg(feature = "tool")]
pub mod tool;
pub mod transform;
#[cfg(any(
    feature = "docx",
    feature = "pptx",
    feature = "spreadsheet",
    feature = "odt",
    feature = "epub"
))]
#[cfg_attr(not(feature = "docx"), allow(dead_code))] // DOCX uses every helper
mod xml;

// Compile and run the Rust snippets in the user docs as doctests, so the
// README and guides cannot drift from the API. Never part of a normal build.
// The README and guide examples use HTML, so they need the `html` feature.
#[cfg(all(doctest, feature = "html"))]
#[doc = include_str!("../../../README.md")]
struct ReadmeDoctests;
#[cfg(all(doctest, feature = "html"))]
#[doc = include_str!("../../../docs/getting-started.md")]
struct GettingStartedDoctests;
#[cfg(doctest)]
#[doc = include_str!("../../../docs/pdf.md")]
struct PdfGuideDoctests;

pub use config::{MarkkitBuilder, PdfOcrBackend};
pub use error::Error;
pub use extract::{
    Extraction, ExtractionStats, Markkit, Metadata, ParsedDocument, Warning, WarningCode,
};
pub use format::{DocumentHint, Format};
pub use ir::Document;
pub use limits::{LimitExceeded, LimitKind, Limits};
pub use render::MarkdownRenderer;

/// Result type alias for markkit operations.
pub type Result<T> = std::result::Result<T, Error>;
