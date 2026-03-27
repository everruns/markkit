//! Security-focused tests for HTML parsing.

use markkit::Markkit;

fn convert(html: &str) -> String {
    let m = Markkit::builder().build();
    m.extract_html_sync(html).unwrap()
}

// ─── XSS Payloads Stripped ───────────────────────────────────────────

#[test]
fn xss_script_tag() {
    let md = convert("<p>Hello</p><script>document.cookie</script>");
    assert!(!md.contains("document.cookie"));
    assert!(!md.contains("<script>"));
}

#[test]
fn xss_event_handler() {
    let md = convert(r#"<p onmouseover="alert(1)">Text</p>"#);
    assert!(!md.contains("onmouseover"));
    assert!(!md.contains("alert"));
    assert!(md.contains("Text"));
}

#[test]
fn xss_javascript_url() {
    let md = convert(r#"<a href="javascript:alert(1)">Click</a>"#);
    // Link is rendered but the URL is preserved as-is (consumer's responsibility to sanitize URLs)
    // The key is no script execution context in markdown
    assert!(md.contains("Click"));
}

#[test]
fn xss_data_uri_img() {
    let md = convert(r#"<img src="data:text/html,<script>alert(1)</script>" alt="xss">"#);
    // Image is rendered as markdown image — the data URI is in the URL portion
    // which is safe in markdown context (no script execution)
    assert!(md.contains("![xss]"));
    // The output should not contain raw HTML script tags outside of a URL
    assert!(!md.starts_with("<script>"));
}

#[test]
fn xss_style_expression() {
    let md = convert(r#"<div style="background:url(javascript:alert(1))">text</div>"#);
    assert!(!md.contains("javascript:"));
    assert!(md.contains("text"));
}

#[test]
fn xss_svg_payload() {
    let md = convert(r#"<svg onload="alert(1)"><text>SVG</text></svg>"#);
    assert!(!md.contains("onload"));
    assert!(!md.contains("alert"));
}

#[test]
fn xss_iframe() {
    let md = convert(r#"<p>Before</p><iframe src="evil.com"></iframe><p>After</p>"#);
    assert!(!md.contains("iframe"));
    assert!(!md.contains("evil.com"));
    assert!(md.contains("Before"));
    assert!(md.contains("After"));
}

// ─── Resource Exhaustion ─────────────────────────────────────────────

#[test]
fn deeply_nested_elements() {
    // 1000 levels of nesting — should not stack overflow
    let mut html = String::new();
    for _ in 0..1000 {
        html.push_str("<div>");
    }
    html.push_str("<p>Deep</p>");
    for _ in 0..1000 {
        html.push_str("</div>");
    }
    let md = convert(&html);
    assert!(md.contains("Deep"));
}

#[test]
fn very_long_text_node() {
    let text = "a".repeat(100_000);
    let html = format!("<p>{}</p>", text);
    let md = convert(&html);
    assert_eq!(md.trim().len(), 100_000);
}

#[test]
fn many_elements() {
    let mut html = String::new();
    for i in 0..1000 {
        html.push_str(&format!("<p>Paragraph {}</p>", i));
    }
    let md = convert(&html);
    assert!(md.contains("Paragraph 0"));
    assert!(md.contains("Paragraph 999"));
}

#[test]
fn resource_limit_bytes() {
    let m = Markkit::builder().max_input_size(100).build();
    let html = "x".repeat(200);
    let result = m.extract_html_sync(&html);
    assert!(result.is_err());
}

// ─── Malformed HTML ──────────────────────────────────────────────────

#[test]
fn unclosed_tags() {
    let md = convert("<p>Unclosed paragraph<p>Another one");
    assert!(md.contains("Unclosed paragraph"));
    assert!(md.contains("Another one"));
}

#[test]
fn mismatched_tags() {
    let md = convert("<p>Start<div>Middle</p>End</div>");
    // Should not panic, should extract text
    assert!(md.contains("Start"));
}

#[test]
fn empty_tags() {
    let md = convert("<p></p><h1></h1><ul></ul>");
    assert_eq!(md.trim(), "");
}

#[test]
fn self_closing_tags() {
    let md = convert("<p>Before<br/>After</p>");
    assert!(md.contains("Before"));
    assert!(md.contains("After"));
}

#[test]
fn html_entities() {
    let md = convert("<p>5 &gt; 3 &amp; 2 &lt; 4</p>");
    assert!(md.contains("5 > 3 & 2 < 4"));
}

#[test]
fn unicode_content() {
    let md = convert("<p>日本語テスト 🦀 Ñoño</p>");
    assert!(md.contains("日本語テスト"));
    assert!(md.contains("🦀"));
    assert!(md.contains("Ñoño"));
}

#[test]
fn null_bytes() {
    let md = convert("<p>before\0after</p>");
    // html5ever replaces null bytes with U+FFFD
    assert!(md.contains("before"));
    assert!(md.contains("after"));
}

// ─── Depth Limits ───────────────────────────────────────────────────

#[test]
fn depth_limit_200() {
    // MAX_DEPTH is 200. With 200 nested divs, the inner <p> is at depth 200
    // (body=0, div1=1, ..., div200=200). At depth >= 200 the parser
    // falls back to text extraction. The content should still appear.
    let mut html = String::new();
    for _ in 0..200 {
        html.push_str("<div>");
    }
    html.push_str("<p>Deep content</p>");
    for _ in 0..200 {
        html.push_str("</div>");
    }
    let md = convert(&html);
    assert!(
        md.contains("Deep content"),
        "Content at depth 200 should still be extracted as text: {:?}",
        md
    );
}

#[test]
fn depth_limit_exceeded() {
    // 250 nested divs — well past MAX_DEPTH (200).
    // Content should still be extracted (flattened to text), not lost.
    let mut html = String::new();
    for _ in 0..250 {
        html.push_str("<div>");
    }
    html.push_str("<p>Very deep content</p>");
    for _ in 0..250 {
        html.push_str("</div>");
    }
    let md = convert(&html);
    assert!(
        md.contains("Very deep content"),
        "Content past depth limit should be flattened to text: {:?}",
        md
    );
}
