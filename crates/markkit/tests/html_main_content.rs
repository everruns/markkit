//! Main-content detection (`MarkkitBuilder::main_content`).

#![cfg(feature = "html")]

use std::time::{Duration, Instant};

use markkit::Markkit;

fn main(html: &str) -> String {
    Markkit::builder()
        .main_content(true)
        .build()
        .extract_html_sync(html)
        .unwrap()
}

const PROSE: &str = "This paragraph is ordinary article prose, long enough to score, with commas, \
                     clauses, and a full stop at the end of it.";

fn news_page() -> String {
    std::fs::read_to_string("tests/fixtures/html/news_page.html").unwrap()
}

#[test]
fn off_by_default_keeps_boilerplate() {
    let md = Markkit::builder()
        .build()
        .extract_html_sync(&news_page())
        .unwrap();
    assert!(md.contains("Most read"));
    assert!(md.contains("First comment"));
}

#[test]
fn drops_sidebar_comments_share_and_banner() {
    let md = main(&news_page());
    for noise in [
        "Most read",
        "First comment",
        "Share on X",
        "Subscribe today",
        "Log in",
        "Related:",
    ] {
        assert!(!md.contains(noise), "{noise} leaked:\n{md}");
    }
    assert!(md.contains("edition = \"2026\""));
}

#[test]
fn classless_layout_prefers_prose_over_link_lists() {
    let links: String = (0..12)
        .map(|i| {
            format!("<p><a href=\"/l{i}\">A link teaser headline number {i}, read more</a></p>")
        })
        .collect();
    let html = format!(
        "<body><div>{links}</div><div><p>{PROSE}</p><p>{PROSE}</p><p>{PROSE}</p></div></body>"
    );
    let md = main(&html);
    assert!(md.contains("ordinary article prose"));
    assert!(!md.contains("teaser"), "{md}");
}

#[test]
fn article_siblings_with_prose_are_kept() {
    let html = format!(
        "<body><div><div class=\"content\"><p>{PROSE}</p><p>{PROSE}</p></div>\
         <p>A short closing sentence after the article.</p>\
         <p><a href=\"/x\">Next story link</a></p></div></body>"
    );
    let md = main(&html);
    assert!(md.contains("closing sentence"), "{md}");
    assert!(!md.contains("Next story"), "{md}");
}

#[test]
fn short_page_falls_back_to_full_body() {
    assert_eq!(main("<p>Hello</p><div>world</div>"), "Hello\n\nworld\n");
    assert_eq!(main(""), "");
}

#[test]
fn include_selectors_win() {
    let md = Markkit::builder()
        .main_content(true)
        .include_selector(".sidebar")
        .build()
        .extract_html_sync(&news_page())
        .unwrap();
    assert!(md.contains("Most read"));
    assert!(!md.contains("What changed"));
}

// ─── Untrusted input ────────────────────────────────────────────────

#[test]
fn deep_nesting_does_not_overflow() {
    let html = format!(
        "{}<p>{PROSE}</p>{}",
        "<div>".repeat(5_000),
        "</div>".repeat(5_000)
    );
    let md = main(&html);
    assert!(md.contains("ordinary article prose"));
}

#[test]
fn many_paragraphs_stay_linear() {
    let html = format!(
        "<body><div>{}</div></body>",
        format!("<p>{PROSE}</p>").repeat(20_000)
    );
    let start = Instant::now();
    let md = main(&html);
    assert!(md.len() > 1_000_000);
    assert!(
        start.elapsed() < Duration::from_secs(20),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn many_sibling_containers_stay_bounded() {
    let block = format!("<div><p>{PROSE}</p></div>");
    let html = format!("<body>{}</body>", block.repeat(5_000));
    let start = Instant::now();
    let md = main(&html);
    assert!(md.contains("ordinary article prose"));
    assert!(
        start.elapsed() < Duration::from_secs(20),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn non_content_elements_still_stripped() {
    let html = format!(
        "<body><article><p>{PROSE}</p><script>alert(1)</script><p>{PROSE}</p>\
         <style>p{{}}</style><iframe src=x></iframe></article></body>"
    );
    let md = main(&html);
    assert!(!md.contains("alert") && !md.contains("<script") && !md.contains("<style"));
}

#[test]
fn provenance_tracked_in_main_content_mode() {
    let doc = Markkit::builder()
        .main_content(true)
        .track_sources(true)
        .build()
        .parse_html(&news_page())
        .unwrap();
    assert!(!doc.nodes.is_empty());
    for node in &doc.nodes {
        let path = node
            .source()
            .and_then(|s| s.path.clone())
            .unwrap_or_default();
        assert!(
            path.starts_with("/html[1]/body[1]/"),
            "{path:?} for {node:?}"
        );
    }
}
