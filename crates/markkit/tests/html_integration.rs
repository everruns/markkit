//! Integration tests for HTML → Markdown conversion.

use markkit::Markkit;

fn convert(html: &str) -> String {
    let m = Markkit::builder().build();
    m.extract_html_sync(html).unwrap()
}

fn convert_with<F>(html: &str, configure: F) -> String
where
    F: FnOnce(markkit::MarkkitBuilder) -> markkit::MarkkitBuilder,
{
    let m = configure(Markkit::builder()).build();
    m.extract_html_sync(html).unwrap()
}

// ─── Headings ────────────────────────────────────────────────────────

#[test]
fn heading_h1() {
    assert_eq!(convert("<h1>Hello World</h1>"), "# Hello World\n");
}

#[test]
fn heading_h2() {
    assert_eq!(convert("<h2>Sub Heading</h2>"), "## Sub Heading\n");
}

#[test]
fn heading_h3_through_h6() {
    assert_eq!(convert("<h3>H3</h3>"), "### H3\n");
    assert_eq!(convert("<h4>H4</h4>"), "#### H4\n");
    assert_eq!(convert("<h5>H5</h5>"), "##### H5\n");
    assert_eq!(convert("<h6>H6</h6>"), "###### H6\n");
}

#[test]
fn heading_with_inline_formatting() {
    assert_eq!(
        convert("<h1>Hello <em>World</em></h1>"),
        "# Hello *World*\n"
    );
}

#[test]
fn heading_empty_is_skipped() {
    assert_eq!(convert("<h1></h1>"), "");
}

// ─── Paragraphs ──────────────────────────────────────────────────────

#[test]
fn paragraph_simple() {
    assert_eq!(convert("<p>Hello world</p>"), "Hello world\n");
}

#[test]
fn paragraph_multiple() {
    let md = convert("<p>First</p><p>Second</p>");
    assert_eq!(md, "First\n\nSecond\n");
}

#[test]
fn paragraph_with_inline_elements() {
    let md = convert("<p>This is <strong>bold</strong> and <em>italic</em></p>");
    assert_eq!(md, "This is **bold** and *italic*\n");
}

#[test]
fn paragraph_whitespace_collapse() {
    let md = convert("<p>  hello    world  </p>");
    assert_eq!(md, "hello world\n");
}

// ─── Inline Formatting ──────────────────────────────────────────────

#[test]
fn bold_strong() {
    assert_eq!(convert("<p><strong>bold</strong></p>"), "**bold**\n");
}

#[test]
fn bold_b_tag() {
    assert_eq!(convert("<p><b>bold</b></p>"), "**bold**\n");
}

#[test]
fn italic_em() {
    assert_eq!(convert("<p><em>italic</em></p>"), "*italic*\n");
}

#[test]
fn italic_i_tag() {
    assert_eq!(convert("<p><i>italic</i></p>"), "*italic*\n");
}

#[test]
fn strikethrough_del() {
    assert_eq!(convert("<p><del>removed</del></p>"), "~~removed~~\n");
}

#[test]
fn strikethrough_s() {
    assert_eq!(convert("<p><s>removed</s></p>"), "~~removed~~\n");
}

#[test]
fn inline_code() {
    assert_eq!(convert("<p><code>let x = 1</code></p>"), "`let x = 1`\n");
}

#[test]
fn nested_inline_bold_italic() {
    assert_eq!(
        convert("<p><strong><em>bold italic</em></strong></p>"),
        "***bold italic***\n"
    );
}

// ─── Links ───────────────────────────────────────────────────────────

#[test]
fn link_basic() {
    assert_eq!(
        convert(r#"<p><a href="https://example.com">Example</a></p>"#),
        "[Example](https://example.com)\n"
    );
}

#[test]
fn link_with_title() {
    assert_eq!(
        convert(r#"<p><a href="https://example.com" title="A site">Example</a></p>"#),
        "[Example](https://example.com \"A site\")\n"
    );
}

#[test]
fn link_with_inline_formatting() {
    assert_eq!(
        convert(r#"<p><a href="/url"><strong>Bold Link</strong></a></p>"#),
        "[**Bold Link**](/url)\n"
    );
}

#[test]
fn link_preservation_disabled() {
    let md = convert_with(
        r#"<p><a href="https://example.com">Example</a></p>"#,
        |b| b.preserve_links(false),
    );
    assert_eq!(md, "Example\n");
}

// ─── Images ──────────────────────────────────────────────────────────

#[test]
fn image_basic() {
    assert_eq!(
        convert(r#"<img src="photo.jpg" alt="A photo">"#),
        "![A photo](photo.jpg)\n"
    );
}

#[test]
fn image_no_alt() {
    assert_eq!(convert(r#"<img src="photo.jpg">"#), "![](photo.jpg)\n");
}

#[test]
fn image_preservation_disabled() {
    let md = convert_with(r#"<img src="photo.jpg" alt="A photo">"#, |b| {
        b.preserve_images(false)
    });
    assert_eq!(md, "");
}

// ─── Lists ───────────────────────────────────────────────────────────

#[test]
fn unordered_list() {
    let md = convert("<ul><li>One</li><li>Two</li><li>Three</li></ul>");
    assert_eq!(md, "- One\n- Two\n- Three\n");
}

#[test]
fn ordered_list() {
    let md = convert("<ol><li>First</li><li>Second</li></ol>");
    assert_eq!(md, "1. First\n2. Second\n");
}

#[test]
fn ordered_list_custom_start() {
    let md = convert(r#"<ol start="5"><li>Five</li><li>Six</li></ol>"#);
    assert_eq!(md, "5. Five\n6. Six\n");
}

#[test]
fn nested_list() {
    let md = convert(
        "<ul><li>Outer<ul><li>Inner A</li><li>Inner B</li></ul></li><li>Other</li></ul>",
    );
    assert!(md.contains("- Outer"));
    assert!(md.contains("  - Inner A"));
    assert!(md.contains("  - Inner B"));
    assert!(md.contains("- Other"));
}

#[test]
fn list_with_inline_formatting() {
    let md = convert("<ul><li><strong>Bold</strong> item</li></ul>");
    assert_eq!(md, "- **Bold** item\n");
}

// ─── Blockquotes ─────────────────────────────────────────────────────

#[test]
fn blockquote_simple() {
    let md = convert("<blockquote><p>Quoted text</p></blockquote>");
    assert_eq!(md, "> Quoted text\n");
}

#[test]
fn blockquote_nested() {
    let md = convert(
        "<blockquote><p>Outer</p><blockquote><p>Inner</p></blockquote></blockquote>",
    );
    assert!(md.contains("> Outer"));
    assert!(md.contains("> > Inner"));
}

// ─── Code Blocks ─────────────────────────────────────────────────────

#[test]
fn code_block_plain() {
    let md = convert("<pre><code>fn main() {}</code></pre>");
    assert_eq!(md, "```\nfn main() {}\n```\n");
}

#[test]
fn code_block_with_language() {
    let md = convert(r#"<pre><code class="language-rust">fn main() {}</code></pre>"#);
    assert_eq!(md, "```rust\nfn main() {}\n```\n");
}

#[test]
fn code_block_lang_prefix() {
    let md = convert(r#"<pre><code class="lang-python">print("hi")</code></pre>"#);
    assert_eq!(md, "```python\nprint(\"hi\")\n```\n");
}

#[test]
fn code_block_pre_without_code() {
    let md = convert("<pre>preformatted text</pre>");
    assert_eq!(md, "```\npreformatted text\n```\n");
}

// ─── Tables ──────────────────────────────────────────────────────────

#[test]
fn table_basic() {
    let html = r#"
    <table>
        <thead><tr><th>Name</th><th>Value</th></tr></thead>
        <tbody>
            <tr><td>A</td><td>1</td></tr>
            <tr><td>B</td><td>2</td></tr>
        </tbody>
    </table>"#;
    let md = convert(html);
    assert!(md.contains("| Name | Value |"));
    assert!(md.contains("| --- | --- |"));
    assert!(md.contains("| A | 1 |"));
    assert!(md.contains("| B | 2 |"));
}

#[test]
fn table_th_in_body() {
    let html = r#"
    <table>
        <tr><th>Col1</th><th>Col2</th></tr>
        <tr><td>a</td><td>b</td></tr>
    </table>"#;
    let md = convert(html);
    assert!(md.contains("| Col1 | Col2 |"));
    assert!(md.contains("| a | b |"));
}

#[test]
fn table_with_inline_formatting() {
    let html = r#"
    <table>
        <thead><tr><th>Name</th></tr></thead>
        <tbody><tr><td><strong>Bold</strong></td></tr></tbody>
    </table>"#;
    let md = convert(html);
    assert!(md.contains("| **Bold** |"));
}

// ─── Thematic Break ──────────────────────────────────────────────────

#[test]
fn horizontal_rule() {
    let md = convert("<p>Above</p><hr><p>Below</p>");
    assert!(md.contains("Above"));
    assert!(md.contains("---"));
    assert!(md.contains("Below"));
}

// ─── Line Break ──────────────────────────────────────────────────────

#[test]
fn line_break_br() {
    let md = convert("<p>Line one<br>Line two</p>");
    assert!(md.contains("Line one  \nLine two"));
}

// ─── Non-Content Elements ────────────────────────────────────────────

#[test]
fn script_stripped() {
    let md = convert("<p>Hello</p><script>alert('xss')</script><p>World</p>");
    assert!(!md.contains("alert"));
    assert!(md.contains("Hello"));
    assert!(md.contains("World"));
}

#[test]
fn style_stripped() {
    let md = convert("<style>body { color: red }</style><p>Content</p>");
    assert!(!md.contains("color"));
    assert!(md.contains("Content"));
}

#[test]
fn nav_excluded_by_default() {
    let md = convert("<nav><a href='/'>Home</a></nav><p>Content</p>");
    assert!(!md.contains("Home"));
    assert!(md.contains("Content"));
}

#[test]
fn footer_excluded_by_default() {
    let md = convert("<footer>Copyright 2024</footer><p>Content</p>");
    assert!(!md.contains("Copyright"));
    assert!(md.contains("Content"));
}

#[test]
fn form_elements_stripped() {
    let md = convert("<p>Before</p><form><input type='text'></form><p>After</p>");
    assert!(!md.contains("input"));
    assert!(md.contains("Before"));
    assert!(md.contains("After"));
}

// ─── Include/Exclude Selectors ───────────────────────────────────────

#[test]
fn include_selector() {
    let html = r#"<div id="sidebar">Sidebar</div><article>Main content</article>"#;
    let md = convert_with(html, |b| b.include_selector("article"));
    assert!(md.contains("Main content"));
    assert!(!md.contains("Sidebar"));
}

#[test]
fn exclude_selector_custom() {
    let html = r#"<p>Keep this</p><div class="ad">Buy stuff</div><p>And this</p>"#;
    let md = convert_with(html, |b| b.exclude_selector(".ad"));
    assert!(md.contains("Keep this"));
    assert!(md.contains("And this"));
    assert!(!md.contains("Buy stuff"));
}

// ─── Full Document ───────────────────────────────────────────────────

#[test]
fn full_html_document() {
    let html = r#"<!DOCTYPE html>
<html>
<head><title>Test</title></head>
<body>
    <h1>Title</h1>
    <p>First paragraph with <strong>bold</strong> and <a href="/link">a link</a>.</p>
    <h2>Section</h2>
    <ul>
        <li>Item one</li>
        <li>Item two</li>
    </ul>
    <blockquote><p>A quote</p></blockquote>
    <pre><code class="language-rust">fn main() {}</code></pre>
</body>
</html>"#;
    let md = convert(html);
    assert!(md.contains("# Title"));
    assert!(md.contains("**bold**"));
    assert!(md.contains("[a link](/link)"));
    assert!(md.contains("## Section"));
    assert!(md.contains("- Item one"));
    assert!(md.contains("- Item two"));
    assert!(md.contains("> A quote"));
    assert!(md.contains("```rust"));
    assert!(md.contains("fn main() {}"));
}

// ─── Edge Cases ──────────────────────────────────────────────────────

#[test]
fn empty_input() {
    assert_eq!(convert(""), "");
}

#[test]
fn whitespace_only() {
    assert_eq!(convert("   \n\n   "), "");
}

#[test]
fn plain_text_no_tags() {
    let md = convert("Just plain text");
    assert!(md.contains("Just plain text"));
}

#[test]
fn deeply_nested_divs() {
    let html = "<div><div><div><div><p>Deep content</p></div></div></div></div>";
    let md = convert(html);
    assert!(md.contains("Deep content"));
}

#[test]
fn mixed_block_and_inline() {
    let html = "<h1>Title</h1><p>Text with <code>code</code> inside.</p>";
    let md = convert(html);
    assert!(md.contains("# Title"));
    assert!(md.contains("Text with `code` inside."));
}

// ─── Resource Limits ─────────────────────────────────────────────────

#[test]
fn resource_limit_enforced() {
    let m = Markkit::builder().max_input_size(10).build();
    let result = m.extract_html_sync("a]".repeat(100).as_str());
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, markkit::Error::ResourceLimit(_)));
}

// ─── IR Roundtrip ────────────────────────────────────────────────────

#[test]
fn parse_then_render() {
    let m = Markkit::builder().build();
    let doc = m.parse_html("<h1>Hello</h1><p>World</p>").unwrap();
    assert!(!doc.is_empty());

    let mut renderer = markkit::MarkdownRenderer::new();
    let md = renderer.render(&doc);
    assert!(md.contains("# Hello"));
    assert!(md.contains("World"));
}

#[test]
fn ir_serialization_roundtrip() {
    let m = Markkit::builder().build();
    let doc = m
        .parse_html("<h1>Test</h1><p><strong>Bold</strong> text</p>")
        .unwrap();
    let json = serde_json::to_string(&doc).unwrap();
    let doc2: markkit::Document = serde_json::from_str(&json).unwrap();
    assert_eq!(doc, doc2);
}

// ─── Determinism ─────────────────────────────────────────────────────

#[test]
fn deterministic_output() {
    let html = r#"
    <h1>Title</h1>
    <p>Some <strong>bold</strong> and <em>italic</em> text.</p>
    <ul><li>A</li><li>B</li></ul>
    <table><thead><tr><th>X</th></tr></thead><tbody><tr><td>1</td></tr></tbody></table>
    "#;
    let md1 = convert(html);
    let md2 = convert(html);
    assert_eq!(md1, md2, "Output must be deterministic");
}

#[test]
fn idempotent_double_extract() {
    let html = "<h1>Title</h1><p>Content</p>";
    let md1 = convert(html);
    let md2 = convert(html);
    assert_eq!(md1, md2);
}
