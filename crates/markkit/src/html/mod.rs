//! HTML parsing module.
//!
//! Converts HTML input into the canonical IR.

mod metadata;
mod nesting;
mod parser;
mod readability;
mod urls;

pub use parser::HtmlParser;
