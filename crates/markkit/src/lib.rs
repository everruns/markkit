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
//! use markkit::Markkit;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let markkit = Markkit::builder().build();
//!
//! // Extract markdown from HTML
//! let md = markkit.extract_html("<h1>Hello</h1><p>World</p>").await?;
//! assert!(md.contains("# Hello"));
//!
//! // Extract from a URL
//! let md = markkit.extract_url("https://example.com").await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Features
//!
//! - **HTML**: Convert HTML to markdown with configurable rules
//! - **PDF**: Extract text from PDF documents (requires `pdf` feature)
//! - **DOCX**: Convert Word documents (requires `docx` feature)
//! - **Spreadsheet**: Convert Excel/ODS files (requires `spreadsheet` feature)
//! - **URL fetching**: Fetch and extract from URLs (requires `http_client` feature)

pub mod config;
pub mod error;
pub mod extract;
pub mod format;

pub use config::MarkkitBuilder;
pub use error::Error;
pub use extract::Markkit;

/// Result type alias for markkit operations.
pub type Result<T> = std::result::Result<T, Error>;
