//! Output-safety tests (threat model T1): the Markdown markkit emits must not
//! smuggle raw HTML, script-scheme links, spoofed structure or fence breakouts
//! when a downstream consumer renders it.
//!
//! Each case is checked two ways: string assertions on the output, and an
//! oracle that parses the output with pulldown-cmark (GFM extensions on) and
//! inspects the resulting events.

use markkit::MarkdownRenderer;
use markkit::ir::{Document, ListItem, Node, TableCell};
use proptest::prelude::*;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

fn render(nodes: Vec<Node>) -> String {
    MarkdownRenderer::new().render(&Document::new(nodes))
}

fn text(s: &str) -> Node {
    Node::Text(s.to_string())
}

fn para(children: Vec<Node>) -> Node {
    Node::paragraph(children)
}

fn events(md: &str) -> Vec<Event<'_>> {
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    Parser::new_ext(md, opts).collect()
}

/// Oracle: no raw HTML and every link/image destination uses a safe scheme.
fn assert_safe(md: &str) {
    for ev in events(md) {
        match ev {
            Event::Html(h) | Event::InlineHtml(h) => panic!("raw HTML {h:?} in {md:?}"),
            Event::Start(Tag::Link { dest_url, .. })
            | Event::Start(Tag::Image { dest_url, .. }) => {
                let lower = dest_url.to_ascii_lowercase();
                for bad in ["javascript:", "vbscript:", "data:text", "file:"] {
                    assert!(!lower.contains(bad), "unsafe url {dest_url:?} in {md:?}");
                }
            }
            _ => {}
        }
    }
}

/// Concatenated text events of the rendered Markdown, as a reader would see it.
fn visible_text(md: &str) -> String {
    let mut out = String::new();
    for ev in events(md) {
        match ev {
            Event::Text(t) | Event::Code(t) => out.push_str(&t),
            Event::SoftBreak | Event::HardBreak => out.push('\n'),
            _ => {}
        }
    }
    out
}

fn block_tags(md: &str) -> Vec<String> {
    events(md)
        .into_iter()
        .filter_map(|ev| match ev {
            Event::Start(t) => match t {
                Tag::Paragraph
                | Tag::Heading { .. }
                | Tag::BlockQuote(_)
                | Tag::CodeBlock(_)
                | Tag::List(_)
                | Tag::Item
                | Tag::Table(_) => Some(format!("{t:?}")),
                _ => None,
            },
            Event::Rule => Some("Rule".into()),
            _ => None,
        })
        .collect()
}

// ─── Text escaping ───────────────────────────────────────────────────

#[test]
fn footnote_body_cannot_become_reference_link_destination() {
    for body in ["javascript:alert(1)", "vbscript:run", "file:///etc/passwd"] {
        let md = render(vec![
            Node::paragraph(vec![Node::FootnoteReference {
                label: "note".into(),
            }]),
            Node::FootnoteDefinition {
                label: "note".into(),
                children: vec![para(vec![text(body)])],
                source: None,
            },
        ]);
        assert_safe(&md);
        assert!(
            events(&md)
                .iter()
                .all(|event| !matches!(event, Event::Start(Tag::Link { .. })))
        );
        let footnotes = Options::ENABLE_FOOTNOTES;
        let enabled: Vec<_> = Parser::new_ext(&md, footnotes).collect();
        assert!(
            enabled
                .iter()
                .any(|event| matches!(event, Event::Start(Tag::FootnoteDefinition(_))))
        );
        assert!(
            enabled
                .iter()
                .any(|event| matches!(event, Event::Text(value) if value.contains(body)))
        );
    }
}

#[test]
fn text_with_markdown_syntax_stays_text() {
    let s = "# not heading *x* _y_ `z` ~~w~~ [t](u) ![i](j) <b>";
    let md = render(vec![para(vec![text(s)])]);
    assert_safe(&md);
    assert_eq!(block_tags(&md), vec!["Paragraph"], "{md}");
    assert_eq!(visible_text(&md), s, "{md}");
}

#[test]
fn raw_html_in_text_is_not_emitted() {
    let md = render(vec![para(vec![text("<img src=x onerror=alert(1)>")])]);
    assert!(md.contains("\\<img"), "{md}");
    assert_safe(&md);
    assert_eq!(visible_text(&md), "<img src=x onerror=alert(1)>");
}

#[test]
fn html_entity_text_does_not_decode_to_markup() {
    let md = render(vec![para(vec![text("&lt;script&gt; &amp; &#60;")])]);
    assert_eq!(visible_text(&md), "&lt;script&gt; &amp; &#60;", "{md}");
}

#[test]
fn line_start_block_markers_are_escaped() {
    for s in [
        "# h",
        "> q",
        "- li",
        "+ li",
        "1. li",
        "12) li",
        "=== ",
        "---",
        "| a | b |",
        "```js",
        "~~~",
    ] {
        let md = render(vec![para(vec![text(s)])]);
        assert_eq!(block_tags(&md), vec!["Paragraph"], "{s:?} -> {md:?}");
        assert_eq!(visible_text(&md), s.trim_end(), "{s:?} -> {md:?}");
    }
}

#[test]
fn line_start_marker_lookalikes_render_verbatim() {
    // Not block markers: no whitespace after `#`, `+`, or the ordered `.`/`)`.
    for s in [
        "4.6 out of 5 stars",
        "3.14 is pi",
        "1)x",
        "#hashtag",
        "##tag",
        "+1 for this",
        "2024.10.27 release",
    ] {
        let md = render(vec![para(vec![text(s)])]);
        assert_eq!(md, format!("{s}\n"), "{s:?} over-escaped");
        assert_eq!(block_tags(&md), vec!["Paragraph"], "{s:?} -> {md:?}");
    }
}

#[test]
fn line_start_marker_at_run_end_stays_escaped() {
    // The next inline node may start with whitespace and complete the marker.
    for (a, b) in [
        ("1.", " x"),
        ("#", " h"),
        ("##", " h"),
        ("+", " li"),
        ("7)", "\tx"),
    ] {
        let md = render(vec![para(vec![text(a), text(b)])]);
        assert_eq!(block_tags(&md), vec!["Paragraph"], "{a:?}+{b:?} -> {md:?}");
        let md = render(vec![para(vec![text(a)])]);
        assert_eq!(block_tags(&md), vec!["Paragraph"], "{a:?} -> {md:?}");
        assert_eq!(visible_text(&md), a, "{a:?} -> {md:?}");
    }
}

#[test]
fn line_start_after_hard_break_is_escaped() {
    let md = render(vec![para(vec![
        text("a"),
        Node::LineBreak,
        text("# b"),
        Node::LineBreak,
        text("==="),
    ])]);
    assert_eq!(block_tags(&md), vec!["Paragraph"], "{md}");
    assert_eq!(visible_text(&md), "a\n# b\n===", "{md}");
}

#[test]
fn list_item_text_cannot_become_task_or_heading() {
    let md = render(vec![Node::List {
        source: None,
        ordered: false,
        start: None,
        items: vec![
            ListItem::new(vec![text("[ ] not a task")]),
            ListItem::new(vec![text("# no")]),
        ],
    }]);
    assert!(
        !events(&md)
            .iter()
            .any(|e| matches!(e, Event::TaskListMarker(_))),
        "{md}"
    );
    assert!(
        !block_tags(&md).iter().any(|t| t.starts_with("Heading")),
        "{md}"
    );
}

#[test]
fn newline_in_text_cannot_start_a_block() {
    let md = render(vec![para(vec![text("a\n# b\n<script>")])]);
    assert_eq!(block_tags(&md), vec!["Paragraph"], "{md}");
    assert_safe(&md);
}

#[test]
fn text_bang_before_link_does_not_make_image() {
    let md = render(vec![para(vec![
        text("Wow!"),
        Node::Link {
            url: "https://e.com/x.png".into(),
            title: None,
            children: vec![text("x")],
        },
    ])]);
    assert!(
        !events(&md)
            .iter()
            .any(|e| matches!(e, Event::Start(Tag::Image { .. }))),
        "{md}"
    );
}

#[test]
fn trailing_backslash_does_not_escape_next_node() {
    let md = render(vec![para(vec![
        text("dir\\"),
        Node::Bold {
            children: vec![text("b")],
        },
    ])]);
    assert!(
        events(&md)
            .iter()
            .any(|e| matches!(e, Event::Start(Tag::Strong))),
        "{md}"
    );
    assert_eq!(visible_text(&md), "dir\\b");
}

#[test]
fn snake_case_and_paths_stay_readable() {
    let md = render(vec![para(vec![text("call my_func_name in C:\\dir\\file")])]);
    assert_eq!(md, "call my_func_name in C:\\dir\\file\n");
}

// ─── Links and images ────────────────────────────────────────────────

fn link(url: &str) -> Node {
    Node::Link {
        url: url.into(),
        title: None,
        children: vec![text("click")],
    }
}

#[test]
fn script_scheme_links_are_dropped_text_kept() {
    for url in [
        "javascript:alert(1)",
        "JaVaScRiPt:alert(1)",
        " javascript:alert(1)",
        "java\tscript:alert(1)",
        "java\nscript:alert(1)",
        "\u{1}javascript:alert(1)",
        "vbscript:msgbox(1)",
        "data:text/html,<script>alert(1)</script>",
        "file:///etc/passwd",
    ] {
        let md = render(vec![para(vec![link(url)])]);
        assert_safe(&md);
        assert!(md.contains("click"), "{url:?} -> {md:?}");
        assert!(
            !md.to_ascii_lowercase().contains("script:"),
            "{url:?} -> {md:?}"
        );
    }
}

#[test]
fn long_scheme_is_not_treated_as_relative() {
    // Found by fuzzing: scheme detection used to stop after 64 characters.
    let url = format!("{}javascript:alert(1)", "a".repeat(80));
    let md = render(vec![para(vec![link(&url)])]);
    assert_safe(&md);
    assert_eq!(md, "click\n");
}

#[test]
fn entity_encoded_scheme_does_not_decode_into_link() {
    let md = render(vec![para(vec![link("javascript&colon;alert(1)")])]);
    assert_safe(&md);
}

#[test]
fn safe_links_are_kept() {
    for url in [
        "https://e.com/a?b=1&c=2#f",
        "http://e.com",
        "mailto:a@e.com",
        "/rel/path",
        "../x",
        "#anchor",
        "page.html",
    ] {
        let md = render(vec![para(vec![link(url)])]);
        let dest = events(&md).into_iter().find_map(|e| match e {
            Event::Start(Tag::Link { dest_url, .. }) => Some(dest_url.to_string()),
            _ => None,
        });
        assert_eq!(dest.as_deref(), Some(url), "{md}");
    }
}

#[test]
fn url_with_parens_spaces_and_brackets_stays_one_link() {
    let md = render(vec![para(vec![link("http://e.com/a)b (c)<d>\\e")])]);
    let dests: Vec<String> = events(&md)
        .into_iter()
        .filter_map(|e| match e {
            Event::Start(Tag::Link { dest_url, .. }) => Some(dest_url.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(
        dests,
        vec!["http://e.com/a%29b%20%28c%29%3Cd%3E%5Ce"],
        "{md}"
    );
    assert_eq!(visible_text(&md), "click");
}

#[test]
fn link_text_with_bracket_does_not_break_link() {
    let md = render(vec![para(vec![Node::Link {
        url: "https://e.com".into(),
        title: Some("say \"hi\"".into()),
        children: vec![text("a]b")],
    }])]);
    let titles: Vec<String> = events(&md)
        .into_iter()
        .filter_map(|e| match e {
            Event::Start(Tag::Link { title, .. }) => Some(title.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(titles, vec!["say \"hi\""], "{md}");
    assert_eq!(visible_text(&md), "a]b");
}

#[test]
fn unsafe_image_becomes_alt_text() {
    let md = render(vec![para(vec![Node::Image {
        src: "javascript:alert(1)".into(),
        alt: "pic".into(),
    }])]);
    assert_safe(&md);
    assert_eq!(visible_text(&md), "pic");
    let md = render(vec![para(vec![Node::Image {
        src: "data:image/svg+xml,<svg onload=alert(1)>".into(),
        alt: "svg".into(),
    }])]);
    assert_safe(&md);
    assert!(!md.contains("onload"), "{md}");
}

#[test]
fn raster_data_and_http_images_are_kept() {
    for src in ["https://e.com/a.png", "data:image/png;base64,iVBORw0KGgo="] {
        let md = render(vec![para(vec![Node::Image {
            src: src.into(),
            alt: "a]b".into(),
        }])]);
        let got = events(&md).into_iter().find_map(|e| match e {
            Event::Start(Tag::Image { dest_url, .. }) => Some(dest_url.to_string()),
            _ => None,
        });
        assert_eq!(got.as_deref(), Some(src), "{md}");
        assert_eq!(visible_text(&md), "a]b");
    }
}

// ─── Code ────────────────────────────────────────────────────────────

#[test]
fn code_block_cannot_close_its_fence_early() {
    let code = "before\n```\n# escaped\n<script>x</script>\n````\nafter";
    let md = render(vec![
        Node::CodeBlock {
            source: None,
            language: Some("js".into()),
            code: code.into(),
        },
        para(vec![text("tail")]),
    ]);
    assert_eq!(
        block_tags(&md),
        vec!["CodeBlock(Fenced(Borrowed(\"js\")))", "Paragraph"],
        "{md}"
    );
    assert_safe(&md);
    let body: String = events(&md)
        .into_iter()
        .skip_while(|e| !matches!(e, Event::Start(Tag::CodeBlock(_))))
        .take_while(|e| !matches!(e, Event::End(TagEnd::CodeBlock)))
        .filter_map(|e| match e {
            Event::Text(t) => Some(t.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(body, format!("{code}\n"));
}

#[test]
fn code_block_language_cannot_inject() {
    let md = render(vec![Node::CodeBlock {
        source: None,
        language: Some("js```\n# x".into()),
        code: "a".into(),
    }]);
    assert_eq!(block_tags(&md).len(), 1, "{md}");
}

#[test]
fn inline_code_with_backticks_round_trips() {
    for code in ["a`b", "``x``", "`", "a ``` b"] {
        let md = render(vec![para(vec![Node::InlineCode(code.into())])]);
        assert_eq!(visible_text(&md), code, "{code:?} -> {md:?}");
    }
}

#[test]
fn adjacent_code_spans_do_not_merge_fences() {
    // Found by fuzzing: the closing fence of one span ran into the opening
    // fence of the next, so neither closed and `<title>` leaked as HTML.
    let md = render(vec![para(vec![
        Node::InlineCode("a``b".into()),
        Node::InlineCode("<title>`x".into()),
    ])]);
    assert_safe(&md);
    let codes: Vec<String> = events(&md)
        .into_iter()
        .filter_map(|e| match e {
            Event::Code(c) => Some(c.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(codes, vec!["a``b", "<title>`x"], "{md}");
}

// ─── Tables ──────────────────────────────────────────────────────────

#[test]
fn table_cell_pipes_and_breaks_do_not_split_cells() {
    let md = render(vec![Node::Table {
        source: None,
        alignments: vec![],
        headers: vec![
            TableCell::new(vec![text("h|1")]),
            TableCell::new(vec![text("h2")]),
        ],
        rows: vec![vec![
            TableCell::new(vec![text("a|b"), Node::LineBreak, text("c")]),
            TableCell::new(vec![Node::InlineCode("x|y".into())]),
        ]],
    }]);
    let cells: Vec<String> = {
        let mut out = Vec::new();
        let mut cur: Option<String> = None;
        for e in events(&md) {
            match e {
                Event::Start(Tag::TableCell) => cur = Some(String::new()),
                Event::End(TagEnd::TableCell) => out.push(cur.take().unwrap()),
                Event::Text(t) | Event::Code(t) => {
                    if let Some(c) = cur.as_mut() {
                        c.push_str(&t)
                    }
                }
                _ => {}
            }
        }
        out
    };
    assert_eq!(cells, vec!["h|1", "h2", "a|b c", "x|y"], "{md}");
}

// ─── Block nodes in inline context (found by fuzzing) ───────────────

#[test]
fn code_block_in_table_cell_stays_in_cell() {
    let md = render(vec![Node::Table {
        source: None,
        alignments: vec![],
        headers: vec![TableCell::new(vec![text("h")])],
        rows: vec![vec![TableCell::new(vec![Node::CodeBlock {
            source: None,
            language: None,
            code: "a\n<script>\n```".into(),
        }])]],
    }]);
    assert_safe(&md);
    assert_eq!(block_tags(&md), vec!["Table([None])"], "{md}");
}

#[test]
fn blocks_inside_heading_stay_on_one_line() {
    let md = render(vec![Node::Heading {
        source: None,
        level: 2,
        children: vec![
            text("a"),
            para(vec![text("b")]),
            Node::List {
                source: None,
                ordered: false,
                start: None,
                items: vec![ListItem::new(vec![text("c")])],
            },
            Node::CodeBlock {
                source: None,
                language: None,
                code: "<x>".into(),
            },
        ],
    }]);
    assert_safe(&md);
    assert_eq!(
        block_tags(&md),
        vec!["Heading { level: H2, id: None, classes: [], attrs: [] }"],
        "{md}"
    );
    assert_eq!(visible_text(&md), "a b c <x>", "{md}");
}

#[test]
fn empty_destination_cannot_swallow_title() {
    let md = render(vec![para(vec![Node::Link {
        url: String::new(),
        title: Some("javascript:alert(1)".into()),
        children: vec![text("x")],
    }])]);
    assert_safe(&md);
    let md = render(vec![para(vec![Node::Image {
        src: " ".into(),
        alt: "a".into(),
    }])]);
    assert_safe(&md);
}

#[test]
fn inline_run_next_to_block_at_top_level() {
    // Found by fuzzing: a block right after loose inline text started mid-line,
    // so its fence did not open and the code became raw HTML.
    let md = render(vec![
        text("lead"),
        Node::CodeBlock {
            source: None,
            language: None,
            code: "<?x\n<script>".into(),
        },
        text("# tail"),
        Node::BlockQuote {
            source: None,
            children: vec![para(vec![text("q")])],
        },
        text("after"),
    ]);
    assert_safe(&md);
    assert_eq!(
        block_tags(&md),
        vec![
            "Paragraph",
            "CodeBlock(Fenced(Borrowed(\"\")))",
            "Paragraph",
            "BlockQuote(None)",
            "Paragraph",
            "Paragraph"
        ],
        "{md}"
    );
    assert!(visible_text(&md).contains("# tail"), "{md}");
}

#[test]
fn code_block_in_nested_ordered_list_stays_fenced() {
    // Found by fuzzing: sub-blocks were indented 2 spaces per level, less than
    // an ordered marker's width, so the fence fell out of the item.
    let inner = Node::List {
        source: None,
        ordered: true,
        start: Some(10),
        items: vec![ListItem::new(vec![
            para(vec![text("item")]),
            Node::CodeBlock {
                source: None,
                language: None,
                code: "<title>x</title>".into(),
            },
        ])],
    };
    let md = render(vec![Node::List {
        source: None,
        ordered: true,
        start: None,
        items: vec![ListItem::new(vec![para(vec![text("outer")]), inner])],
    }]);
    assert_safe(&md);
    let tags = block_tags(&md);
    assert_eq!(
        tags.iter().filter(|t| t.starts_with("List")).count(),
        2,
        "{md}"
    );
    assert_eq!(
        tags.iter().filter(|t| t.starts_with("CodeBlock")).count(),
        1,
        "{md}"
    );
    // The code block is inside the inner item: nothing follows the outer list.
    assert!(md.trim_end().ends_with("```"), "{md}");
}

#[test]
fn nested_list_under_ordered_item_is_nested() {
    let md = render(vec![Node::List {
        source: None,
        ordered: true,
        start: None,
        items: vec![ListItem::new(vec![
            para(vec![text("a")]),
            Node::List {
                source: None,
                ordered: false,
                start: None,
                items: vec![ListItem::new(vec![text("b")])],
            },
        ])],
    }]);
    assert_eq!(md, "1. a\n   - b\n");
    let starts = block_tags(&md)
        .iter()
        .filter(|t| t.starts_with("List"))
        .count();
    assert_eq!(starts, 2, "{md}");
}

#[test]
fn repeated_hard_breaks_do_not_end_link_or_paragraph() {
    // Found by fuzzing: two hard breaks made a blank line, ending the link early
    // and leaking its title as raw text.
    let md = render(vec![para(vec![Node::Link {
        url: "https://e.com".into(),
        title: Some("<b>".into()),
        children: vec![
            text("a"),
            Node::LineBreak,
            Node::LineBreak,
            text(" "),
            Node::LineBreak,
            text("b"),
        ],
    }])]);
    assert_safe(&md);
    assert_eq!(block_tags(&md), vec!["Paragraph"], "{md}");
    assert!(
        events(&md)
            .iter()
            .any(|e| matches!(e, Event::Start(Tag::Link { .. }))),
        "{md}"
    );
}

#[test]
fn nested_links_keep_outer_link_intact() {
    // Found by fuzzing: a link inside link text cancels the outer link in
    // CommonMark, leaking the outer title (`<m>`) as raw text.
    let inner = Node::Link {
        url: "https://in.example".into(),
        title: None,
        children: vec![text("in")],
    };
    let md = render(vec![para(vec![Node::Link {
        url: "https://out.example".into(),
        title: Some("<m> \"q\"".into()),
        children: vec![text("a "), inner],
    }])]);
    assert_safe(&md);
    let links: Vec<String> = events(&md)
        .into_iter()
        .filter_map(|e| match e {
            Event::Start(Tag::Link {
                dest_url, title, ..
            }) => Some(format!("{dest_url} {title}")),
            _ => None,
        })
        .collect();
    assert_eq!(links, vec!["https://out.example <m> \"q\""], "{md}");
    assert_eq!(visible_text(&md), "a in");
}

// ─── End to end through the HTML parser ──────────────────────────────

#[cfg(feature = "html")]
#[test]
fn html_payload_page_is_safe() {
    let html = r#"<p># not heading *x* &lt;img src=x onerror=1&gt;</p>
<a href="javascript:alert(1)">click</a> <a href="http://x/a)b">p</a>
<pre><code>```
evil
```</code></pre><p>after</p>"#;
    let md = markkit::Markkit::builder()
        .build()
        .extract_html_sync(html)
        .unwrap();
    assert_safe(&md);
    assert!(md.contains("\\<img"), "{md}");
    assert!(!md.contains("javascript"), "{md}");
    assert!(
        block_tags(&md).iter().all(|t| !t.starts_with("Heading")),
        "{md}"
    );
    assert!(visible_text(&md).contains("after"), "{md}");
    assert!(block_tags(&md).last().unwrap() == "Paragraph", "{md}");
}

// ─── Properties ──────────────────────────────────────────────────────

proptest! {
    /// Any printable text in a paragraph reads back verbatim (modulo whitespace
    /// collapse) as a single paragraph with no markup.
    #[test]
    fn paragraph_text_round_trips(s in "[ -~]{1,80}") {
        let md = render(vec![para(vec![text(&s)])]);
        let collapsed = s.split_whitespace().collect::<Vec<_>>().join(" ");
        prop_assume!(!collapsed.is_empty());
        assert_safe(&md);
        prop_assert_eq!(block_tags(&md), vec!["Paragraph".to_string()], "{:?}", md);
        prop_assert_eq!(visible_text(&md).split_whitespace().collect::<Vec<_>>().join(" "), collapsed, "{:?}", md);
    }

    /// Arbitrary link URLs never yield an unsafe destination or raw HTML.
    #[test]
    fn arbitrary_link_urls_are_safe(url in "\\PC{0,40}", t in "[a-z]{1,5}") {
        let md = render(vec![para(vec![Node::Link { url, title: None, children: vec![text(&t)] }])]);
        assert_safe(&md);
        prop_assert!(visible_text(&md).contains(&t));
    }

    /// Arbitrary code never escapes its fenced block.
    #[test]
    fn arbitrary_code_stays_fenced(code in "[`~a \n]{0,40}") {
        let md = render(vec![Node::CodeBlock { source: None, language: None, code }, para(vec![text("tail")])]);
        let tags = block_tags(&md);
        prop_assert_eq!(tags.len(), 2, "{:?}", md);
        prop_assert!(tags[0].starts_with("CodeBlock"), "{:?}", md);
    }
}

// ─── Math ────────────────────────────────────────────────────────────

/// TeX comes straight from page markup (`<annotation>`), so it is as untrusted
/// as text. Plain GFM has no math syntax: `$...$` content is parsed as
/// Markdown, so math must be inert there.
#[test]
fn inline_math_cannot_emit_html_or_links() {
    for tex in [
        "<img src=x onerror=alert(1)>",
        "[x](javascript:alert(1))",
        "a ** b ** c",
        "`` x ``",
    ] {
        let md = render(vec![para(vec![
            text("see "),
            Node::Math { tex: tex.into() },
        ])]);
        assert_safe(&md);
        assert!(
            visible_text(&md).contains(tex.trim_matches('`').trim()),
            "{tex}: {md}"
        );
        assert_eq!(block_tags(&md), ["Paragraph"], "{tex}: {md}");
    }
}

#[test]
fn display_math_stays_in_its_block() {
    let tex = "a\n```\n<script>alert(1)</script>\n# h\n$$\n- b";
    let md = render(vec![
        Node::MathBlock {
            tex: tex.into(),
            source: None,
        },
        para(vec![text("after")]),
    ]);
    assert_safe(&md);
    let tags = block_tags(&md);
    assert_eq!(tags.len(), 2, "{md}");
    assert!(
        tags[0].starts_with("CodeBlock") && tags[1] == "Paragraph",
        "{md}"
    );
}

#[test]
fn math_in_table_cell_stays_in_cell() {
    let md = render(vec![Node::Table {
        alignments: vec![],
        headers: vec![TableCell::new(vec![Node::Math {
            tex: "a | b".into(),
        }])],
        rows: vec![],
        source: None,
    }]);
    assert_safe(&md);
    assert!(md.starts_with("| $`a \\| b`$ |"), "{md}");
}

#[test]
fn display_math_in_inline_context_is_flattened() {
    let md = render(vec![Node::Table {
        alignments: vec![],
        headers: vec![TableCell::new(vec![Node::MathBlock {
            tex: "x\n= 1".into(),
            source: None,
        }])],
        rows: vec![],
        source: None,
    }]);
    assert_safe(&md);
    assert!(md.starts_with("| $`x = 1`$ |"), "{md}");
}
