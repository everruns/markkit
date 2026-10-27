//! Page metadata (`Extraction::title` / `Extraction::metadata`) from HTML.

#![cfg(feature = "html")]

use markkit::{DocumentHint, Extraction, Markkit};

fn extract(html: &str) -> Extraction {
    Markkit::builder()
        .build()
        .extract_document_sync(html.as_bytes(), DocumentHint::content_type("text/html"))
        .unwrap()
}

const ARTICLE: &str = r#"<!doctype html>
<html lang="en-GB">
<head>
  <title>Fallback title | Site</title>
  <meta name="description" content="Plain description">
  <meta property="og:title" content="OG title">
  <meta property="og:description" content="OG description">
  <meta property="og:site_name" content="Example News">
  <meta property="og:image" content="https://example.com/lead.jpg">
  <meta property="article:published_time" content="2026-10-01T09:00:00Z">
  <meta name="author" content="Jane Doe">
  <link rel="canonical" href="https://example.com/a/1">
</head>
<body><h1>Headline</h1><p>Body text.</p></body>
</html>"#;

#[test]
fn meta_and_open_graph_fields() {
    let e = extract(ARTICLE);
    assert_eq!(e.title.as_deref(), Some("OG title"));
    let m = &e.metadata;
    assert_eq!(m.description.as_deref(), Some("OG description"));
    assert_eq!(m.author.as_deref(), Some("Jane Doe"));
    assert_eq!(m.published.as_deref(), Some("2026-10-01T09:00:00Z"));
    assert_eq!(m.language.as_deref(), Some("en-GB"));
    assert_eq!(m.canonical_url.as_deref(), Some("https://example.com/a/1"));
    assert_eq!(m.site_name.as_deref(), Some("Example News"));
    assert_eq!(m.image.as_deref(), Some("https://example.com/lead.jpg"));
    assert_eq!(e.markdown, "# Headline\n\nBody text.\n");
}

#[test]
fn json_ld_fills_missing_fields() {
    let e = extract(
        r#"<html><head>
        <script type="application/ld+json">
          {"@context": "https://schema.org", "@type": "BlogPosting",
           "headline": "LD headline", "description": "LD description",
           "datePublished": "2025-05-05", "inLanguage": "uk",
           "author": {"@type": "Person", "name": "Ivan"},
           "publisher": {"@type": "Organization", "name": "LD Pub"},
           "url": "https://ld.example/post", "image": "https://ld.example/i.png"}
        </script></head><body><p>x</p></body></html>"#,
    );
    assert_eq!(e.title.as_deref(), Some("LD headline"));
    let m = &e.metadata;
    assert_eq!(m.description.as_deref(), Some("LD description"));
    assert_eq!(m.author.as_deref(), Some("Ivan"));
    assert_eq!(m.published.as_deref(), Some("2025-05-05"));
    assert_eq!(m.language.as_deref(), Some("uk"));
    assert_eq!(m.site_name.as_deref(), Some("LD Pub"));
    assert_eq!(m.canonical_url.as_deref(), Some("https://ld.example/post"));
    assert_eq!(m.image.as_deref(), Some("https://ld.example/i.png"));
    assert!(
        !e.markdown.contains("LD headline"),
        "JSON-LD must not leak into markdown"
    );
}

#[test]
fn title_tag_used_without_social_tags() {
    let e =
        extract("<html><head><title>  Just   a title </title></head><body><p>x</p></body></html>");
    assert_eq!(e.title.as_deref(), Some("Just a title"));
    assert!(e.metadata.is_empty());
}

#[test]
fn bare_fragment_has_no_metadata() {
    let e = extract("<p>hello</p>");
    assert_eq!(e.title, None);
    assert!(e.metadata.is_empty());
}

#[test]
fn author_profile_url_is_not_a_name() {
    let e = extract(
        r#"<head><meta property="article:author" content="https://facebook.com/jane"></head>"#,
    );
    assert_eq!(e.metadata.author, None);
}

#[test]
fn metadata_serializes_in_json() {
    let json = serde_json::to_value(extract(ARTICLE)).unwrap();
    assert_eq!(json["title"], "OG title");
    assert_eq!(json["metadata"]["site_name"], "Example News");
}

// ─── Untrusted input ────────────────────────────────────────────────

#[test]
fn dangerous_url_schemes_dropped() {
    let e = extract(
        r#"<head>
        <link rel="canonical" href="javascript:alert(1)">
        <meta property="og:url" content=" JAVASCRIPT:alert(2)">
        <meta property="og:image" content="data:image/svg+xml,<svg onload=alert(3)>">
        <script type="application/ld+json">{"url": "vbscript:x", "image": "javascript:y"}</script>
        </head>"#,
    );
    assert_eq!(e.metadata.canonical_url, None);
    assert_eq!(e.metadata.image, None);
}

#[test]
fn relative_canonical_kept_as_is() {
    let e = extract(r#"<head><link rel="Canonical" href="/a/1"></head>"#);
    assert_eq!(e.metadata.canonical_url.as_deref(), Some("/a/1"));
}

#[test]
fn values_are_single_line_and_capped() {
    let long = "y".repeat(10_000);
    let e = extract(&format!(
        "<head><title>a\n\n\tb\u{7}c</title><meta name=description content=\"{long}\"></head>"
    ));
    assert_eq!(e.title.as_deref(), Some("a bc"));
    assert_eq!(e.metadata.description.unwrap().chars().count(), 2048);
}

#[test]
fn malformed_json_ld_ignored() {
    let e = extract(
        r#"<head><title>T</title>
        <script type="application/ld+json">{"headline": </script>
        <script type="application/ld+json">[1, "x", null, {"headline": 7}]</script></head>"#,
    );
    // Numbers are accepted as text; the broken script is skipped.
    assert_eq!(e.title.as_deref(), Some("7"));
}

#[test]
fn deeply_nested_json_ld_does_not_overflow() {
    let deep = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
    let e = extract(&format!(
        "<head><title>T</title><script type=\"application/ld+json\">{deep}</script></head>"
    ));
    assert_eq!(e.title.as_deref(), Some("T"));
}

#[test]
fn oversized_json_ld_skipped() {
    let pad = " ".repeat(1024 * 1024);
    let e = extract(&format!(
        r#"<head><script type="application/ld+json">{{"headline": "big"}}{pad}</script></head>"#
    ));
    assert_eq!(e.title, None);
}

#[test]
fn many_json_ld_scripts_bounded() {
    let scripts: String = (0..5_000)
        .map(|i| format!(r#"<script type="application/ld+json">{{"n": {i}}}</script>"#))
        .collect();
    let e = extract(&format!(
        r#"<head>{scripts}<script type="application/ld+json">{{"headline": "late"}}</script></head>"#
    ));
    // Only the first 16 scripts are read, so the late headline is never reached.
    assert_eq!(e.title, None);
}
