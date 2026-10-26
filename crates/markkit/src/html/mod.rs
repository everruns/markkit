//! HTML parsing module.
//!
//! Converts HTML input into the canonical IR.

mod metadata;
mod parser;
mod urls;

pub use parser::HtmlParser;
