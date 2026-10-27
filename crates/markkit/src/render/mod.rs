//! Rendering module.
//!
//! Converts the canonical IR into output formats.

mod escape;
mod markdown;

pub use markdown::MarkdownRenderer;
