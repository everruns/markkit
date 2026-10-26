//! Markdown input: parse with pulldown-cmark into the IR, render back.
//!
//! Decisions:
//! - Markdown is re-parsed, not passed through, so every output goes through
//!   the same IR and renderer (and the same output-safety rules). Raw HTML is
//!   never copied: HTML blocks go through the HTML parser when the `html`
//!   feature is on (scripts/styles stripped), otherwise they are dropped;
//!   inline HTML tags are dropped and the text between them kept.
//! - Extensions: GFM tables, strikethrough, task lists, YAML front matter.
//!   Front matter is not rendered; its `title:` becomes the title, otherwise
//!   the first level-1 heading.
//! - Soft line breaks become spaces (paragraphs reflow); hard breaks stay.
//! - pulldown-cmark is iterative; the IR is built with an explicit stack and
//!   capped at `MAX_DEPTH` open containers. Deeper content is flattened into
//!   the deepest container so the recursive renderer cannot overflow.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, MetadataBlockKind, Options, Parser, Tag};

use crate::config::MarkkitBuilder;
use crate::ir::{Document, ListItem, Node};

/// Max nested containers (block quotes, lists, emphasis, …) kept as structure.
const MAX_DEPTH: usize = 200;

/// Parsed Markdown document.
pub(crate) struct ParsedMarkdown {
    pub doc: Document,
    pub title: Option<String>,
    pub paragraphs: usize,
    pub tables: usize,
}

enum Kind {
    Root,
    Para,
    Heading(u8),
    Quote,
    List {
        ordered: bool,
        start: Option<u64>,
    },
    Item,
    Code {
        language: Option<String>,
    },
    Table,
    Head,
    Row,
    Cell,
    Em,
    Strong,
    Strike,
    Link {
        url: String,
        title: Option<String>,
    },
    Image {
        src: String,
    },
    Html,
    Meta,
    /// Beyond `MAX_DEPTH` or an unsupported container: children go to the parent.
    Flat,
}

struct Frame {
    kind: Kind,
    nodes: Vec<Node>,
    items: Vec<ListItem>,
    cells: Vec<Vec<Node>>,
    rows: Vec<Vec<Vec<Node>>>,
    /// Code, image alt, raw HTML or front matter text.
    text: String,
}

impl Frame {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            nodes: Vec::new(),
            items: Vec::new(),
            cells: Vec::new(),
            rows: Vec::new(),
            text: String::new(),
        }
    }
}

/// What a finished frame hands to its parent.
enum Out {
    Nodes(Vec<Node>),
    Item(ListItem),
    Cell(Vec<Node>),
    Row(Vec<Vec<Node>>),
    Head(Vec<Vec<Node>>),
}

struct Builder<'c> {
    #[cfg_attr(not(feature = "html"), allow(dead_code))]
    config: &'c MarkkitBuilder,
    stack: Vec<Frame>,
    meta_title: Option<String>,
    paragraphs: usize,
    tables: usize,
}

/// Parse Markdown text into the IR.
pub(crate) fn parse(text: &str, config: &MarkkitBuilder) -> ParsedMarkdown {
    let opts = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS;
    let mut b = Builder {
        config,
        stack: vec![Frame::new(Kind::Root)],
        meta_title: None,
        paragraphs: 0,
        tables: 0,
    };
    for event in Parser::new_ext(text, opts) {
        b.event(event);
    }
    while b.stack.len() > 1 {
        b.end();
    }
    let root = b.stack.pop().map(|f| f.nodes).unwrap_or_default();
    let title = b.meta_title.or_else(|| first_h1(&root));
    ParsedMarkdown {
        doc: Document::new(root),
        title,
        paragraphs: b.paragraphs,
        tables: b.tables,
    }
}

impl Builder<'_> {
    fn top(&mut self) -> &mut Frame {
        self.stack.last_mut().expect("root frame")
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(_) => self.end(),
            Event::Text(t) => self.text(&t),
            Event::Code(c) => self.push(Node::InlineCode(c.into_string())),
            Event::Html(h) | Event::InlineHtml(h) => {
                if matches!(self.top().kind, Kind::Html) {
                    self.top().text.push_str(&h);
                }
            }
            Event::SoftBreak => self.text(" "),
            Event::HardBreak => self.push(Node::LineBreak),
            Event::Rule => self.push(Node::ThematicBreak),
            Event::TaskListMarker(done) => self.text(if done { "[x] " } else { "[ ] " }),
            Event::FootnoteReference(r) => self.text(&format!("[^{r}]")),
            Event::InlineMath(m) | Event::DisplayMath(m) => {
                self.push(Node::InlineCode(m.into_string()))
            }
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        let kind = if self.stack.len() > MAX_DEPTH {
            Kind::Flat
        } else {
            match tag {
                Tag::Paragraph => Kind::Para,
                Tag::Heading { level, .. } => Kind::Heading(heading_level(level)),
                Tag::BlockQuote(_) => Kind::Quote,
                Tag::CodeBlock(kind) => Kind::Code {
                    language: match kind {
                        CodeBlockKind::Fenced(info) => info
                            .split_whitespace()
                            .next()
                            .map(|s| s.chars().filter(|c| *c != '`').collect::<String>())
                            .filter(|s| !s.is_empty()),
                        CodeBlockKind::Indented => None,
                    },
                },
                Tag::HtmlBlock => Kind::Html,
                Tag::List(start) => Kind::List {
                    ordered: start.is_some(),
                    start,
                },
                Tag::Item => Kind::Item,
                Tag::Table(_) => Kind::Table,
                Tag::TableHead => Kind::Head,
                Tag::TableRow => Kind::Row,
                Tag::TableCell => Kind::Cell,
                Tag::Emphasis => Kind::Em,
                Tag::Strong => Kind::Strong,
                Tag::Strikethrough => Kind::Strike,
                Tag::Link {
                    dest_url, title, ..
                } => Kind::Link {
                    url: dest_url.into_string(),
                    title: Some(title.into_string()).filter(|t| !t.is_empty()),
                },
                Tag::Image { dest_url, .. } => Kind::Image {
                    src: dest_url.into_string(),
                },
                Tag::MetadataBlock(MetadataBlockKind::YamlStyle) => Kind::Meta,
                _ => Kind::Flat,
            }
        };
        self.stack.push(Frame::new(kind));
    }

    fn end(&mut self) {
        if self.stack.len() <= 1 {
            return;
        }
        let f = self.stack.pop().expect("non-root frame");
        let out = match f.kind {
            Kind::Root | Kind::Flat => Out::Nodes(f.nodes),
            Kind::Para => {
                self.paragraphs += 1;
                Out::Nodes(vec![Node::Paragraph { children: f.nodes }])
            }
            Kind::Heading(level) => {
                self.paragraphs += 1;
                Out::Nodes(vec![Node::Heading {
                    level,
                    children: f.nodes,
                }])
            }
            Kind::Quote => Out::Nodes(vec![Node::BlockQuote {
                children: wrap_inline(f.nodes),
            }]),
            Kind::List { ordered, start } => Out::Nodes(vec![Node::List {
                ordered,
                start,
                items: f.items,
            }]),
            Kind::Item => {
                self.paragraphs += 1;
                let has_block = f.nodes.iter().any(is_block);
                let children = if has_block {
                    wrap_inline(f.nodes)
                } else {
                    f.nodes
                };
                Out::Item(ListItem::new(children))
            }
            Kind::Code { language } => Out::Nodes(vec![Node::CodeBlock {
                language,
                code: f.text,
            }]),
            Kind::Table => {
                self.tables += 1;
                let mut nodes = vec![Node::Table {
                    headers: f.cells,
                    rows: f.rows,
                }];
                nodes.extend(f.nodes);
                Out::Nodes(nodes)
            }
            Kind::Head => Out::Head(f.cells),
            Kind::Row => Out::Row(f.cells),
            Kind::Cell => Out::Cell(f.nodes),
            Kind::Em => Out::Nodes(vec![Node::Italic { children: f.nodes }]),
            Kind::Strong => Out::Nodes(vec![Node::Bold { children: f.nodes }]),
            Kind::Strike => Out::Nodes(vec![Node::Strikethrough { children: f.nodes }]),
            Kind::Link { url, title } => Out::Nodes(vec![Node::Link {
                url,
                title,
                children: f.nodes,
            }]),
            Kind::Image { src } => Out::Nodes(vec![Node::Image { src, alt: f.text }]),
            Kind::Html => Out::Nodes(self.html_block(&f.text)),
            Kind::Meta => {
                if self.meta_title.is_none() {
                    self.meta_title = yaml_title(&f.text);
                }
                Out::Nodes(Vec::new())
            }
        };
        self.accept(out);
    }

    /// Hand a finished frame's output to the new top frame.
    fn accept(&mut self, out: Out) {
        let parent = self.top();
        match (&parent.kind, out) {
            (Kind::List { .. }, Out::Item(item)) => parent.items.push(item),
            (Kind::List { .. }, Out::Nodes(nodes)) if !nodes.is_empty() => {
                parent.items.push(ListItem::new(nodes))
            }
            (Kind::Head | Kind::Row, Out::Cell(cell)) => parent.cells.push(cell),
            (Kind::Table, Out::Head(cells)) => parent.cells = cells,
            (Kind::Table, Out::Row(cells)) => parent.rows.push(cells),
            (_, Out::Nodes(nodes) | Out::Cell(nodes)) => self.push_all(nodes),
            (_, Out::Item(item)) => self.push_all(item.children),
            (_, Out::Row(cells) | Out::Head(cells)) => {
                for cell in cells {
                    self.push_all(cell);
                    self.push(Node::Text(" ".into()));
                }
            }
        }
    }

    fn push_all(&mut self, nodes: Vec<Node>) {
        for n in nodes {
            self.push(n);
        }
    }

    fn push(&mut self, node: Node) {
        let top = self.top();
        match top.kind {
            // Alt text, code and front matter take plain text only.
            Kind::Image { .. } | Kind::Code { .. } => plain_text(&node, &mut top.text),
            Kind::Html | Kind::Meta => {}
            Kind::List { .. } => top.items.push(ListItem::new(vec![node])),
            Kind::Head | Kind::Row => top.cells.push(vec![node]),
            _ => top.nodes.push(node),
        }
    }

    fn text(&mut self, t: &str) {
        let top = self.top();
        match top.kind {
            Kind::Image { .. } | Kind::Code { .. } | Kind::Meta => top.text.push_str(t),
            Kind::Html => {}
            _ => {
                if let Some(Node::Text(prev)) = top.nodes.last_mut() {
                    prev.push_str(t);
                } else {
                    self.push(Node::Text(t.to_string()));
                }
            }
        }
    }

    #[cfg(feature = "html")]
    fn html_block(&self, html: &str) -> Vec<Node> {
        crate::html::HtmlParser::new(self.config)
            .parse(html)
            .map(|d| d.nodes)
            .unwrap_or_default()
    }

    #[cfg(not(feature = "html"))]
    fn html_block(&self, _html: &str) -> Vec<Node> {
        Vec::new()
    }
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn is_block(node: &Node) -> bool {
    matches!(
        node,
        Node::Heading { .. }
            | Node::Paragraph { .. }
            | Node::List { .. }
            | Node::BlockQuote { .. }
            | Node::CodeBlock { .. }
            | Node::Table { .. }
            | Node::ThematicBreak
    )
}

/// Group runs of inline nodes into paragraphs (for containers mixing both).
fn wrap_inline(nodes: Vec<Node>) -> Vec<Node> {
    let mut out = Vec::with_capacity(nodes.len());
    let mut run = Vec::new();
    for n in nodes {
        if is_block(&n) {
            if !run.is_empty() {
                out.push(Node::Paragraph {
                    children: std::mem::take(&mut run),
                });
            }
            out.push(n);
        } else {
            run.push(n);
        }
    }
    if !run.is_empty() {
        out.push(Node::Paragraph { children: run });
    }
    out
}

/// Append the plain text of a node (iteratively; nodes may be nested).
fn plain_text(node: &Node, out: &mut String) {
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        match n {
            Node::Text(t) | Node::InlineCode(t) => out.push_str(t),
            Node::CodeBlock { code, .. } => out.push_str(code),
            Node::Image { alt, .. } => out.push_str(alt),
            Node::LineBreak => out.push(' '),
            Node::Heading { children, .. }
            | Node::Paragraph { children }
            | Node::BlockQuote { children }
            | Node::Bold { children }
            | Node::Italic { children }
            | Node::Strikethrough { children }
            | Node::Link { children, .. } => stack.extend(children.iter().rev()),
            Node::List { items, .. } => {
                stack.extend(items.iter().rev().flat_map(|i| i.children.iter().rev()))
            }
            Node::Table { headers, rows } => {
                stack.extend(headers.iter().chain(rows.iter().flatten()).flatten().rev())
            }
            Node::ThematicBreak => {}
        }
    }
}

fn first_h1(nodes: &[Node]) -> Option<String> {
    nodes.iter().find_map(|n| match n {
        Node::Heading { level: 1, .. } => {
            let mut t = String::new();
            plain_text(n, &mut t);
            let t = t.trim();
            (!t.is_empty()).then(|| t.to_string())
        }
        _ => None,
    })
}

/// `title:` from YAML front matter (top-level key, scalar value only).
fn yaml_title(yaml: &str) -> Option<String> {
    yaml.lines().find_map(|line| {
        let v = line.strip_prefix("title:")?.trim();
        let v = v
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| v.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(v)
            .trim();
        (!v.is_empty()).then(|| v.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn md(src: &str) -> String {
        let p = parse(src, &MarkkitBuilder::default());
        crate::render::MarkdownRenderer::new().render(&p.doc)
    }

    #[test]
    fn roundtrip_common_blocks() {
        let src = "# T\n\nSome *em* and **strong** and ~~del~~ `code`.\n\n> quote\n\n- a\n- b\n  1. c\n\n```rust\nfn x() {}\n```\n\n---\n\n[link](http://e.com \"t\") ![alt *x*](i.png)\n";
        assert_eq!(
            md(src),
            "# T\n\nSome *em* and **strong** and ~~del~~ `code`.\n\n> quote\n\n- a\n- b\n  1. c\n\n```rust\nfn x() {}\n```\n\n---\n\n[link](http://e.com \"t\") ![alt x](i.png)\n"
        );
    }

    #[test]
    fn table_and_tasks() {
        assert_eq!(
            md("| a | b |\n|---|---|\n| 1 | 2 |\n\n- [x] done\n- [ ] todo\n"),
            "| a | b |\n| --- | --- |\n| 1 | 2 |\n\n- [x] done\n- [ ] todo\n"
        );
    }

    #[test]
    fn soft_breaks_reflow_hard_breaks_kept() {
        assert_eq!(md("a\nb  \nc\\\nd"), "a b  \nc  \nd\n");
    }

    #[test]
    fn titles() {
        let p = parse(
            "---\ntitle: \"Doc\"\n---\n\n# H\n",
            &MarkkitBuilder::default(),
        );
        assert_eq!(p.title.as_deref(), Some("Doc"));
        let p = parse("## sub\n\n# Real *one*\n", &MarkkitBuilder::default());
        assert_eq!(p.title.as_deref(), Some("Real one"));
        assert_eq!(parse("text", &MarkkitBuilder::default()).title, None);
    }

    #[test]
    fn loose_item_with_nested_list() {
        assert_eq!(md("- a **b**\n  - c\n"), "- a **b**\n  - c\n");
    }

    #[test]
    fn deep_nesting_is_flattened() {
        let src = format!("{}x", "> ".repeat(MAX_DEPTH * 5));
        let out = md(&src);
        assert!(out.trim_end().ends_with('x'));
        assert!(out.lines().next().unwrap().matches('>').count() <= MAX_DEPTH + 1);
    }
}
