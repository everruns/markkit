//! Arbitrary IR → Markdown renderer. Covers trees no parser emits today
//! (future formats, JSON IR from callers) against the S17 oracle.
#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use markkit::MarkdownRenderer;
use markkit::ir::{Alignment, DefinitionItem, Document, ListItem, Node, TableCell};
use markkit_fuzz::check_markdown;

#[derive(Arbitrary, Debug)]
enum F {
    Heading(u8, Vec<F>),
    Paragraph(Vec<F>),
    List(bool, Option<u64>, Vec<(Option<bool>, Vec<F>)>),
    BlockQuote(Vec<F>),
    CodeBlock(Option<String>, String),
    Table(Vec<u8>, Vec<C>, Vec<Vec<C>>),
    ThematicBreak,
    DefinitionList(Vec<(Vec<F>, Vec<Vec<F>>)>),
    Figure(Vec<F>, Option<Vec<F>>),
    FootnoteDefinition(String, Vec<F>),
    Text(String),
    Bold(Vec<F>),
    Italic(Vec<F>),
    Strikethrough(Vec<F>),
    InlineCode(String),
    Link(String, Option<String>, Vec<F>),
    Image(String, String),
    LineBreak,
    FootnoteReference(String),
}

#[derive(Arbitrary, Debug)]
struct C(u8, u8, Vec<F>);

const MAX_DEPTH: usize = 8;

fn conv(nodes: Vec<F>, depth: usize) -> Vec<Node> {
    if depth > MAX_DEPTH {
        return Vec::new();
    }
    nodes.into_iter().map(|n| node(n, depth + 1)).collect()
}

fn cell(c: C, d: usize) -> TableCell {
    TableCell::new(conv(c.2, d)).with_span(u32::from(c.0 % 4) + 1, u32::from(c.1 % 4) + 1)
}

fn align(a: u8) -> Alignment {
    match a % 4 {
        0 => Alignment::None,
        1 => Alignment::Left,
        2 => Alignment::Center,
        _ => Alignment::Right,
    }
}

fn node(f: F, d: usize) -> Node {
    match f {
        F::Heading(level, c) => Node::Heading {
            level: level % 6 + 1,
            children: conv(c, d),
            source: None,
        },
        F::Paragraph(c) => Node::Paragraph {
            children: conv(c, d),
            source: None,
        },
        F::List(ordered, start, items) => Node::List {
            ordered,
            start: if ordered {
                start.map(|s| s % 1_000_000)
            } else {
                None
            },
            items: items
                .into_iter()
                .map(|(checked, i)| match checked {
                    Some(c) => ListItem::task(conv(i, d), c),
                    None => ListItem::new(conv(i, d)),
                })
                .collect(),
            source: None,
        },
        F::BlockQuote(c) => Node::BlockQuote {
            children: conv(c, d),
            source: None,
        },
        F::CodeBlock(language, code) => Node::CodeBlock {
            language,
            code,
            source: None,
        },
        F::Table(a, h, r) => Node::Table {
            alignments: a.into_iter().map(align).collect(),
            headers: h.into_iter().map(|c| cell(c, d)).collect(),
            rows: r
                .into_iter()
                .map(|row| row.into_iter().map(|c| cell(c, d)).collect())
                .collect(),
            source: None,
        },
        F::ThematicBreak => Node::ThematicBreak,
        F::DefinitionList(items) => Node::DefinitionList {
            items: items
                .into_iter()
                .map(|(t, defs)| {
                    DefinitionItem::new(conv(t, d), defs.into_iter().map(|x| conv(x, d)).collect())
                })
                .collect(),
            source: None,
        },
        F::Figure(c, caption) => Node::Figure {
            children: conv(c, d),
            caption: caption.map(|x| conv(x, d)),
            source: None,
        },
        F::FootnoteDefinition(label, c) => Node::FootnoteDefinition {
            label,
            children: conv(c, d),
            source: None,
        },
        F::Text(t) => Node::Text(t),
        F::Bold(c) => Node::Bold {
            children: conv(c, d),
        },
        F::Italic(c) => Node::Italic {
            children: conv(c, d),
        },
        F::Strikethrough(c) => Node::Strikethrough {
            children: conv(c, d),
        },
        F::InlineCode(c) => Node::InlineCode(c),
        F::Link(url, title, c) => Node::Link {
            url,
            title,
            children: conv(c, d),
        },
        F::Image(src, alt) => Node::Image { src, alt },
        F::LineBreak => Node::LineBreak,
        F::FootnoteReference(label) => Node::FootnoteReference { label },
    }
}

fuzz_target!(|nodes: Vec<F>| {
    let doc = Document::new(conv(nodes, 0));
    check_markdown(&MarkdownRenderer::new().render(&doc));
});
