//! Shared helpers for markkit fuzz targets.
//!
//! Every target asserts the output-safety oracle (invariant S17), not just
//! "no panic": rendered Markdown parsed with pulldown-cmark (GFM) has no raw
//! HTML and no link/image with a non-allowlisted scheme.

use markkit::{DocumentHint, Markkit};
use pulldown_cmark::{Event, Options, Parser, Tag};

/// In-memory DOCX/XLSX/ODS/XLS builders shared with the integration tests.
#[path = "../../tests/common/mod.rs"]
pub mod common;

/// Inputs above this size are skipped: limits are tested elsewhere and large
/// inputs only slow the fuzzer down.
pub const MAX_INPUT: usize = 64 * 1024;

/// Extractor with default (safe) configuration.
pub fn markkit() -> Markkit {
    Markkit::builder().build()
}

/// Extract a document and check the oracle on success. Errors are fine.
pub fn extract_and_check(bytes: &[u8], hint: DocumentHint<'_>) {
    if let Ok(e) = markkit().extract_document_sync(bytes, hint) {
        check_markdown(&e.markdown);
    }
}

/// Panic when `md` would emit raw HTML or an unsafe destination when rendered.
pub fn check_markdown(md: &str) {
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    for ev in Parser::new_ext(md, opts) {
        match ev {
            Event::Html(h) | Event::InlineHtml(h) => panic!("raw HTML {h:?} in {md:?}"),
            Event::Start(Tag::Link { dest_url, .. }) => check_scheme(&dest_url, false, md),
            Event::Start(Tag::Image { dest_url, .. }) => check_scheme(&dest_url, true, md),
            _ => {}
        }
    }
}

fn check_scheme(url: &str, image: bool, md: &str) {
    let probe: String = url
        .trim_start_matches(|c: char| c <= ' ')
        .chars()
        // Browsers drop tabs, newlines and leading C0/space, not inner spaces.
        .filter(|c| !matches!(c, '\t' | '\n' | '\r') && !c.is_ascii_control())
        .collect::<String>()
        .to_ascii_lowercase();
    let Some(i) = probe.find([':', '/', '?', '#']) else {
        return;
    };
    if probe.as_bytes()[i] != b':' {
        return;
    }
    let scheme = &probe[..i];
    // Not a scheme per the URL spec (e.g. `ht%20tps`): a relative URL.
    let valid = scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !valid {
        return;
    }
    let ok = if image {
        matches!(scheme, "http" | "https") || probe.starts_with("data:image/")
    } else {
        matches!(scheme, "http" | "https" | "mailto" | "tel" | "ftp")
    };
    assert!(ok, "unsafe destination {url:?} in {md:?}");
    assert!(
        !probe.starts_with("data:image/svg"),
        "svg data image {url:?} in {md:?}"
    );
}
