//! Rendering module.
//!
//! Converts the canonical IR into output formats.

mod markdown;

pub use markdown::MarkdownRenderer;
#[cfg(any(feature = "docx", feature = "spreadsheet"))]
pub(crate) use markdown::escape_table_cell;
