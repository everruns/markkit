//! Shared Office Open XML helpers for the DOCX and PPTX readers: package
//! relationships, run formatting → IR inline nodes, nested lists and
//! table-safe cells.
//!
//! Decision: relationship targets are resolved lexically (no filesystem), and
//! only internal targets are followed for parts; external targets are kept as
//! hyperlink URLs only.

use std::borrow::Cow;
use std::collections::HashMap;
use std::rc::Rc;

use crate::ir::{ListItem, Node};
use crate::xml::Element;

/// Resolve a relationship target relative to the directory of the source part.
pub(crate) fn resolve_target(base_dir: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    let mut parts: Vec<&str> = base_dir.split('/').filter(|s| !s.is_empty()).collect();
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

pub(crate) fn split_dir(path: &str) -> (&str, &str) {
    match path.rsplit_once('/') {
        Some((d, f)) => (d, f),
        None => ("", path),
    }
}

pub(crate) struct Relationship {
    pub rel_type: String,
    pub target: String,
    pub external: bool,
}

pub(crate) fn parse_rels(root: Option<Element>) -> HashMap<String, Relationship> {
    let mut map = HashMap::new();
    let Some(root) = root else { return map };
    let Some(rels) = root.child("Relationships") else {
        return map;
    };
    for r in rels.elements().filter(|e| e.name == "Relationship") {
        if let (Some(id), Some(target)) = (r.attr("Id"), r.attr("Target")) {
            map.insert(
                id.to_string(),
                Relationship {
                    rel_type: r.attr("Type").unwrap_or_default().to_string(),
                    target: target.to_string(),
                    external: r
                        .attr("TargetMode")
                        .is_some_and(|m| m.eq_ignore_ascii_case("External")),
                },
            );
        }
    }
    map
}

pub(crate) fn rel_target_by_type<'a>(
    rels: &'a HashMap<String, Relationship>,
    suffix: &str,
) -> Option<&'a str> {
    // Deterministic choice if several match.
    let mut matches: Vec<(&String, &Relationship)> = rels
        .iter()
        .filter(|(_, r)| r.rel_type.ends_with(suffix) && !r.external)
        .collect();
    matches.sort_by(|a, b| a.0.cmp(b.0));
    matches.first().map(|(_, r)| r.target.as_str())
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Fmt {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
}

/// Inline segment. Text borrows from the XML tree until it becomes an IR node;
/// the link is shared by every segment of a hyperlink.
pub(crate) enum Seg<'e> {
    Text {
        text: Cow<'e, str>,
        fmt: Fmt,
        link: Option<Rc<str>>,
    },
    Break {
        link: Option<Rc<str>>,
    },
}

impl Seg<'_> {
    pub(crate) fn link(&self) -> Option<&Rc<str>> {
        match self {
            Seg::Text { link, .. } | Seg::Break { link } => link.as_ref(),
        }
    }
}

/// Merge segments and turn them into IR inline nodes.
pub(crate) fn build_inline(segs: Vec<Seg<'_>>) -> Vec<Node> {
    // 1. Merge adjacent text with identical formatting and link.
    let mut merged: Vec<Seg> = Vec::with_capacity(segs.len());
    for seg in segs {
        if let (
            Some(Seg::Text {
                text: prev,
                fmt: pf,
                link: pl,
            }),
            Seg::Text { text, fmt, link },
        ) = (merged.last_mut(), &seg)
            && pf == fmt
            && pl == link
        {
            prev.to_mut().push_str(text);
            continue;
        }
        merged.push(seg);
    }

    // 2. Group by link.
    let mut out: Vec<Node> = Vec::with_capacity(merged.len());
    let mut segs = merged.into_iter().peekable();
    while let Some(first) = segs.next() {
        let Some(url) = first.link().cloned() else {
            format_seg(first, &mut out);
            continue;
        };
        let mut nodes = Vec::new();
        format_seg(first, &mut nodes);
        while let Some(next) = segs.next_if(|s| s.link() == Some(&url)) {
            format_seg(next, &mut nodes);
        }
        if is_blank(&nodes) {
            out.extend(nodes);
        } else {
            let (lead, core, trail) = split_ws_nodes(nodes);
            out.extend(lead);
            out.push(Node::Link {
                url: url.to_string(),
                title: None,
                children: core,
            });
            out.extend(trail);
        }
    }
    tidy_inline(out)
}

fn format_seg(seg: Seg<'_>, out: &mut Vec<Node>) {
    let (text, fmt) = match seg {
        Seg::Break { .. } => return out.push(Node::LineBreak),
        Seg::Text { text, fmt, .. } => (text, fmt),
    };
    let core = text.trim();
    if core.is_empty() || fmt == Fmt::default() {
        out.push(Node::Text(text.into_owned()));
        return;
    }
    let start = text.len() - text.trim_start().len();
    let lead = &text[..start];
    let trail = &text[start + core.len()..];
    if !lead.is_empty() {
        out.push(Node::Text(lead.to_string()));
    }
    let mut node = Node::Text(core.to_string());
    if fmt.strike {
        node = Node::Strikethrough {
            children: vec![node],
        };
    }
    if fmt.italic {
        node = Node::Italic {
            children: vec![node],
        };
    }
    if fmt.bold {
        node = Node::Bold {
            children: vec![node],
        };
    }
    out.push(node);
    if !trail.is_empty() {
        out.push(Node::Text(trail.to_string()));
    }
}

/// Split leading/trailing whitespace text out of a node list (for link text).
pub(crate) fn split_ws_nodes(mut nodes: Vec<Node>) -> (Vec<Node>, Vec<Node>, Vec<Node>) {
    let mut lead = Vec::new();
    let mut trail = Vec::new();
    if let Some(Node::Text(t)) = nodes.first_mut() {
        let trimmed = t.trim_start().to_string();
        if trimmed.len() != t.len() {
            lead.push(Node::Text(t[..t.len() - trimmed.len()].to_string()));
            *t = trimmed;
        }
    }
    if let Some(Node::Text(t)) = nodes.last_mut() {
        let trimmed = t.trim_end().to_string();
        if trimmed.len() != t.len() {
            trail.push(Node::Text(t[trimmed.len()..].to_string()));
            *t = trimmed;
        }
    }
    nodes.retain(|n| !matches!(n, Node::Text(t) if t.is_empty()));
    (lead, nodes, trail)
}

/// Merge adjacent text nodes and strip leading/trailing line breaks.
pub(crate) fn tidy_inline(mut nodes: Vec<Node>) -> Vec<Node> {
    // In place: `w` is the write cursor, read slots are taken by swap.
    let mut w = 0;
    for r in 0..nodes.len() {
        match std::mem::replace(&mut nodes[r], Node::LineBreak) {
            Node::Text(t) if t.is_empty() => {}
            Node::Text(t) if w > 0 && matches!(nodes[w - 1], Node::Text(_)) => {
                if let Node::Text(prev) = &mut nodes[w - 1] {
                    prev.push_str(&t);
                }
            }
            n => {
                nodes[w] = n;
                w += 1;
            }
        }
    }
    nodes.truncate(w);
    while matches!(nodes.last(), Some(Node::LineBreak)) {
        nodes.pop();
    }
    let lead = nodes
        .iter()
        .take_while(|n| matches!(n, Node::LineBreak))
        .count();
    nodes.drain(..lead);
    nodes
}

pub(crate) fn is_blank(nodes: &[Node]) -> bool {
    nodes.iter().all(|n| match n {
        Node::Text(t) => t.trim().is_empty(),
        Node::LineBreak => true,
        Node::Bold { children }
        | Node::Italic { children }
        | Node::Strikethrough { children }
        | Node::Link { children, .. } => is_blank(children),
        _ => false,
    })
}

pub(crate) fn plain_text(nodes: &[Node], out: &mut String) {
    for n in nodes {
        match n {
            Node::Text(t) | Node::InlineCode(t) => out.push_str(t),
            Node::LineBreak => out.push(' '),
            Node::Bold { children }
            | Node::Italic { children }
            | Node::Strikethrough { children }
            | Node::Link { children, .. }
            | Node::Heading { children, .. }
            | Node::Paragraph { children, .. } => plain_text(children, out),
            _ => {}
        }
    }
}

pub(crate) struct ListEntry {
    pub level: u32,
    pub ordered: bool,
    pub start: Option<u64>,
    pub content: Vec<Node>,
    /// Source block index of the first paragraph (DOCX `track_sources`).
    #[cfg_attr(not(feature = "docx"), allow(dead_code))]
    pub index: usize,
}

/// Build a (nested) list starting at `entries[*pos]` for `level`.
pub(crate) fn build_list(entries: &[ListEntry], pos: &mut usize, level: u32) -> Node {
    let first = &entries[*pos];
    let ordered = first.ordered;
    let start = if ordered {
        first.start.filter(|&s| s != 1)
    } else {
        None
    };
    let mut items: Vec<ListItem> = Vec::new();
    while *pos < entries.len() {
        let e = &entries[*pos];
        if e.level < level || (e.level == level && e.ordered != ordered) {
            break;
        }
        if e.level > level {
            let sub = build_list(entries, pos, e.level);
            match items.last_mut() {
                Some(last) => last.children.push(sub),
                None => items.push(ListItem::new(vec![sub])),
            }
            continue;
        }
        items.push(ListItem::new(vec![Node::Paragraph {
            children: e.content.clone(),
            source: None,
        }]));
        *pos += 1;
    }
    Node::List {
        ordered,
        start,
        items,
        source: None,
    }
}

/// Make cell content single-line: hard breaks become spaces (the renderer escapes pipes).
pub(crate) fn table_cell(nodes: Vec<Node>) -> Vec<Node> {
    fn fix(n: Node) -> Node {
        match n {
            Node::LineBreak => Node::Text(" ".into()),
            Node::Bold { children } => Node::Bold {
                children: children.into_iter().map(fix).collect(),
            },
            Node::Italic { children } => Node::Italic {
                children: children.into_iter().map(fix).collect(),
            },
            Node::Strikethrough { children } => Node::Strikethrough {
                children: children.into_iter().map(fix).collect(),
            },
            Node::Link {
                url,
                title,
                children,
            } => Node::Link {
                url,
                title,
                children: children.into_iter().map(fix).collect(),
            },
            other => other,
        }
    }
    tidy_inline(nodes.into_iter().map(fix).collect())
}
