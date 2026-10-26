//! Mira eval study for markkit extraction quality.
//!
//! Run through the `mira` host CLI (`cargo install mira-cli`):
//!
//! ```text
//! mira list --study-bin markkit-eval
//! mira run --study-bin markkit-eval markkit_extract
//! ```
//!
//! See `knowledge/operations/eval.md` for the design.

pub mod checks;
pub mod dataset;
pub mod mira_study;
