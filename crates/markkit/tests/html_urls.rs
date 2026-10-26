//! Relative URL resolution (`MarkkitBuilder::base_url`, `<base href>`).

#![cfg(feature = "html")]

use markkit::{DocumentHint, Markkit};

fn convert(base: Option<&str>, html: &str) -> String {
    let mut b = Markkit::builder();
    if let Some(base) = base {
        b = b.base_url(base);
    }
    b.build().extract_html_sync(html).unwrap()
}

const PAGE: &str = r##"<p><a href="b.html">b</a> <a href="/root">r</a> <a href="//cdn.ex/x">c</a>
<a href="#sec">s</a> <a href="https://other.ex/">o</a> <img src="img/p.png" alt="p"></p>"##;

#[test]
fn relative_links_and_images_resolve_against_base() {
    assert_eq!(
        convert(Some("https://ex.com/a/page"), PAGE),
        "[b](https://ex.com/a/b.html) [r](https://ex.com/root) [c](https://cdn.ex/x) \
         [s](#sec) [o](https://other.ex/) ![p](https://ex.com/a/img/p.png)\n"
    );
}

#[test]
fn without_base_urls_unchanged() {
    assert_eq!(
        convert(None, PAGE),
        "[b](b.html) [r](/root) [c](//cdn.ex/x) [s](#sec) [o](https://other.ex/) ![p](img/p.png)\n"
    );
}

#[test]
fn document_base_href_applies_on_top_of_caller_base() {
    let html = r#"<head><base href="/v2/"></head><body><p><a href="x">x</a></p></body>"#;
    assert_eq!(
        convert(Some("https://ex.com/a/b"), html),
        "[x](https://ex.com/v2/x)\n"
    );
    let html =
        r#"<head><base href="https://docs.ex/"></head><body><p><a href="x">x</a></p></body>"#;
    assert_eq!(convert(None, html), "[x](https://docs.ex/x)\n");
}

#[test]
fn metadata_urls_resolve() {
    let e = Markkit::builder()
        .base_url("https://ex.com/post/1")
        .build()
        .extract_document_sync(
            br#"<head><link rel=canonical href="/post/1"><meta property=og:image content="lead.jpg"></head>"#,
            DocumentHint::content_type("text/html"),
        )
        .unwrap();
    assert_eq!(
        e.metadata.canonical_url.as_deref(),
        Some("https://ex.com/post/1")
    );
    assert_eq!(
        e.metadata.image.as_deref(),
        Some("https://ex.com/post/lead.jpg")
    );
}

// ─── Untrusted input ────────────────────────────────────────────────

#[test]
fn absolute_dangerous_schemes_pass_through_untouched() {
    // Resolution never rewrites (or normalizes) an absolute URL.
    let md = convert(
        Some("https://ex.com/"),
        r#"<p><a href="JaVaScRiPt:alert(1)">j</a> <img src="data:image/png;base64,AA" alt="d"></p>"#,
    );
    assert_eq!(
        md,
        "[j](JaVaScRiPt:alert(1)) ![d](data:image/png;base64,AA)\n"
    );
}

#[test]
fn page_base_cannot_switch_to_local_or_script_scheme() {
    for evil in ["file:///etc/", "javascript:alert(1)//", "data:text/html,"] {
        let html = format!(
            r#"<head><base href="{evil}"></head><body><p><a href="passwd">x</a></p></body>"#
        );
        assert_eq!(
            convert(Some("https://ex.com/"), &html),
            "[x](https://ex.com/passwd)\n",
            "{evil}"
        );
        assert_eq!(convert(None, &html), "[x](passwd)\n", "{evil}");
    }
}

#[test]
fn invalid_caller_base_ignored() {
    for bad in ["not a url", "javascript:alert(1)", ""] {
        assert_eq!(
            convert(Some(bad), r#"<p><a href="x">x</a></p>"#),
            "[x](x)\n"
        );
    }
}
