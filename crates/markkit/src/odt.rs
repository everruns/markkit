//! OpenDocument text (`.odt`) → IR.
//!
//! Decisions:
//! - Behind the `odt` feature; reuses the bounded ZIP reader (`archive`) and the
//!   depth-capped XML tree (`xml`) that DOCX uses, so T6 zip-bomb and entity
//!   protections apply unchanged.
//! - Reads `content.xml` (required), `styles.xml` and `meta.xml` (optional).
//!   Styles from all three style containers are merged; `parent-style-name`
//!   chains are followed at most `MAX_STYLE_CHAIN` deep.
//! - Headings: `text:h` (`outline-level`, default 1, clamped to 1..6), or a
//!   paragraph style with `default-outline-level`; style `Title` → level 1 and
//!   the title fallback after `meta.xml` `dc:title`.
//! - Inline formatting comes from text properties (`font-weight` bold/≥600,
//!   `font-style` italic/oblique, `text-line-through-style` ≠ none). Content is
//!   flattened to styled runs and regrouped, so adjacent spans with the same
//!   formatting merge and edge whitespace moves outside the markers.
//! - Lists: ordered when the list style's level is `list-level-style-number`.
//!   Tables: first row is the header; `number-columns-repeated` /
//!   `number-rows-repeated` are capped by `max_sheet_cols` / `max_sheet_rows`
//!   and trailing empty cells/rows are dropped (producers pad to 1024 columns).
//! - Skipped: footnotes/endnotes, annotations, tracked-change metadata, frames
//!   and drawings, table of contents bodies, page/section layout.
//! - Hidden content (T9) is skipped unless `include_hidden_content`: styles
//!   with `text:display="none"`, hidden sections, `text:hidden-text` and
//!   `text:hidden-paragraph` fields; a `hidden_content_skipped` warning counts
//!   them.
//! - Shares the call's `Budget`: XML parts are bounded by depth/node count and
//!   each block ticks the deadline.
//! - Whitespace follows ODF: runs collapse to one space; `text:s c=N` (capped
//!   at 64), `text:tab` and `text:line-break` are explicit.

use std::collections::HashMap;

use crate::archive::{ArchiveError, ZipGuard};
use crate::config::MarkkitBuilder;
use crate::error::Error;
use crate::extract::{Warning, WarningCode};
use crate::ir::{Document, ListItem, Node, TableCell};
use crate::limits::{Budget, LimitExceeded};
use crate::xml::{self, Element, XNode, XmlError};

const MAX_STYLE_CHAIN: usize = 16;
const MAX_SPACES: usize = 64;

/// Result of parsing an ODT package.
pub(crate) struct OdtOutput {
    pub doc: Document,
    pub title: Option<String>,
    pub warnings: Vec<Warning>,
    pub paragraphs: usize,
    pub tables: usize,
}

fn archive_err(e: ArchiveError) -> Error {
    match e {
        ArchiveError::Limit(m) => Error::ResourceLimit(m),
        ArchiveError::Invalid(m) => Error::OdtExtract(m),
    }
}

fn xml_err(part: &str, e: XmlError) -> Error {
    match e {
        XmlError::Limit(m) => {
            Error::ResourceLimit(LimitExceeded::new(m.kind, format!("{part}: {}", m.detail)))
        }
        XmlError::Invalid(m) => Error::OdtExtract(format!("{part}: {m}")),
    }
}

fn read_xml(
    zip: &mut ZipGuard<'_>,
    path: &str,
    max_depth: usize,
    budget: &Budget,
) -> Result<Option<Element>, Error> {
    match zip.read(path).map_err(archive_err)? {
        Some(bytes) => xml::parse(&bytes, max_depth, budget)
            .map(Some)
            .map_err(|e| xml_err(path, e)),
        None => Ok(None),
    }
}

// ─── Styles ─────────────────────────────────────────────────────────

#[derive(Default, Clone, Copy, PartialEq, Eq)]
struct Fmt {
    bold: bool,
    italic: bool,
    strike: bool,
}

#[derive(Default)]
struct Style {
    parent: Option<String>,
    bold: Option<bool>,
    italic: Option<bool>,
    strike: Option<bool>,
    outline: Option<u8>,
    display_name: Option<String>,
    /// `text:display="none"` on the style's text properties.
    hidden: Option<bool>,
}

#[derive(Default)]
struct Styles {
    styles: HashMap<String, Style>,
    /// List style name → ordered flag per level (index 0 = level 1).
    lists: HashMap<String, Vec<bool>>,
}

impl Styles {
    fn add(&mut self, root: Option<&Element>) {
        let Some(root) = root else { return };
        let mut stack = vec![root];
        while let Some(e) = stack.pop() {
            match e.name.as_str() {
                "style" => {
                    if let Some(name) = e.attr("name") {
                        self.styles.insert(name.to_string(), parse_style(e));
                    }
                }
                "list-style" => {
                    if let Some(name) = e.attr("name") {
                        let mut levels: Vec<(usize, bool)> = e
                            .elements()
                            .filter_map(|l| {
                                let level = l.attr("level")?.parse::<usize>().ok()?;
                                Some((level, l.name == "list-level-style-number"))
                            })
                            .filter(|(l, _)| (1..=10).contains(l))
                            .collect();
                        levels.sort_unstable();
                        let mut v = vec![false; 10];
                        for (l, ordered) in levels {
                            v[l - 1] = ordered;
                        }
                        self.lists.insert(name.to_string(), v);
                    }
                }
                // Style containers and the document root hold the definitions.
                "document-styles" | "document-content" | "styles" | "automatic-styles"
                | "master-styles" | "" => stack.extend(e.elements()),
                _ => {}
            }
        }
    }

    /// Resolve a style chain into (text formatting, outline level, is_title).
    fn resolve(&self, name: Option<&str>) -> (Fmt, Option<u8>, bool) {
        let mut fmt = (None, None, None);
        let mut outline = None;
        let mut title = false;
        let mut cur = name;
        for _ in 0..MAX_STYLE_CHAIN {
            let Some(n) = cur else { break };
            let Some(s) = self.styles.get(n) else { break };
            fmt.0 = fmt.0.or(s.bold);
            fmt.1 = fmt.1.or(s.italic);
            fmt.2 = fmt.2.or(s.strike);
            outline = outline.or(s.outline);
            let shown = s.display_name.as_deref().unwrap_or(n);
            title |= shown.eq_ignore_ascii_case("title");
            cur = s.parent.as_deref();
        }
        (
            Fmt {
                bold: fmt.0.unwrap_or(false),
                italic: fmt.1.unwrap_or(false),
                strike: fmt.2.unwrap_or(false),
            },
            outline,
            title,
        )
    }

    /// Whether a style chain hides its text (`text:display="none"`).
    fn hidden(&self, name: Option<&str>) -> bool {
        let mut cur = name;
        for _ in 0..MAX_STYLE_CHAIN {
            let Some(s) = cur.and_then(|n| self.styles.get(n)) else {
                break;
            };
            if let Some(h) = s.hidden {
                return h;
            }
            cur = s.parent.as_deref();
        }
        false
    }

    fn list_ordered(&self, style: Option<&str>, level: usize) -> bool {
        style
            .and_then(|s| self.lists.get(s))
            .and_then(|v| v.get(level.min(9)))
            .copied()
            .unwrap_or(false)
    }
}

fn parse_style(e: &Element) -> Style {
    let props = e.child("text-properties");
    let bold = props
        .and_then(|p| p.attr("font-weight"))
        .map(|w| w == "bold" || w.parse::<u32>().map(|n| n >= 600).unwrap_or(false));
    let italic = props
        .and_then(|p| p.attr("font-style"))
        .map(|s| s == "italic" || s == "oblique");
    let strike = props
        .and_then(|p| p.attr("text-line-through-style"))
        .map(|s| s != "none");
    Style {
        parent: e.attr("parent-style-name").map(str::to_string),
        bold,
        italic,
        strike,
        outline: e
            .attr("default-outline-level")
            .and_then(|l| l.parse::<u8>().ok())
            .filter(|l| *l > 0)
            .map(|l| l.min(6)),
        display_name: e.attr("display-name").map(str::to_string),
        hidden: props.and_then(|p| p.attr("display")).map(|d| d == "none"),
    }
}

// ─── Inline runs ────────────────────────────────────────────────────

enum Run {
    Text {
        text: String,
        fmt: Fmt,
        link: Option<String>,
    },
    Break,
}

struct Inline<'s> {
    styles: &'s Styles,
    include_hidden: bool,
    /// Hidden spans/fields skipped.
    hidden: usize,
    runs: Vec<Run>,
}

impl Inline<'_> {
    fn push_text(&mut self, text: &str, fmt: Fmt, link: &Option<String>) {
        if text.is_empty() {
            return;
        }
        if let Some(Run::Text {
            text: prev,
            fmt: f,
            link: l,
        }) = self.runs.last_mut()
            && *f == fmt
            && l == link
        {
            prev.push_str(text);
            return;
        }
        self.runs.push(Run::Text {
            text: text.to_string(),
            fmt,
            link: link.clone(),
        });
    }

    /// Walk inline content iteratively (XML depth is already capped).
    fn walk(&mut self, el: &Element, fmt: Fmt, link: Option<String>) {
        let mut stack: Vec<(std::slice::Iter<'_, XNode>, Fmt, Option<String>)> =
            vec![(el.children.iter(), fmt, link)];
        while let Some((iter, fmt, link)) = stack.last_mut() {
            let Some(node) = iter.next() else {
                stack.pop();
                continue;
            };
            let (fmt, link) = (*fmt, link.clone());
            match node {
                XNode::Text(t) => self.push_text(&collapse(t), fmt, &link),
                XNode::Element(e) => match e.name.as_str() {
                    "span" if !self.include_hidden && self.styles.hidden(e.attr("style-name")) => {
                        self.hidden += 1;
                    }
                    "hidden-text" | "hidden-paragraph" if !self.include_hidden => {
                        self.hidden += 1;
                    }
                    "span" => {
                        let (f, _, _) = self.styles.resolve(e.attr("style-name"));
                        let merged = Fmt {
                            bold: fmt.bold || f.bold,
                            italic: fmt.italic || f.italic,
                            strike: fmt.strike || f.strike,
                        };
                        stack.push((e.children.iter(), merged, link));
                    }
                    "a" => {
                        let href = e
                            .attr("href")
                            .map(str::trim)
                            .filter(|h| !h.is_empty())
                            .map(|h| h.replace(' ', "%20"));
                        stack.push((e.children.iter(), fmt, href.or(link)));
                    }
                    "s" => {
                        let n = e
                            .attr("c")
                            .and_then(|c| c.parse::<usize>().ok())
                            .unwrap_or(1)
                            .clamp(1, MAX_SPACES);
                        self.push_text(&" ".repeat(n), fmt, &link);
                    }
                    "tab" => self.push_text(" ", fmt, &link),
                    "line-break" => self.runs.push(Run::Break),
                    // Not body text.
                    "note"
                    | "annotation"
                    | "annotation-end"
                    | "frame"
                    | "custom-shape"
                    | "bookmark"
                    | "bookmark-start"
                    | "bookmark-end"
                    | "reference-mark"
                    | "reference-mark-start"
                    | "reference-mark-end"
                    | "change"
                    | "change-start"
                    | "change-end"
                    | "soft-page-break"
                    | "toc-mark"
                    | "alphabetical-index-mark"
                    | "user-index-mark" => {}
                    // Fields (dates, page numbers, …) and anything else: keep text.
                    _ => stack.push((e.children.iter(), fmt, link)),
                },
            }
        }
    }

    fn into_nodes(self) -> Vec<Node> {
        let mut out: Vec<Node> = Vec::new();
        let mut runs = self.runs.into_iter().peekable();
        while let Some(run) = runs.next() {
            match run {
                Run::Break => out.push(Node::LineBreak),
                Run::Text { text, fmt, link } => {
                    // Group following runs that share the link into one link node.
                    let mut group = vec![(text, fmt)];
                    while let Some(Run::Text { link: l, .. }) = runs.peek() {
                        if *l != link {
                            break;
                        }
                        if let Some(Run::Text { text, fmt, .. }) = runs.next() {
                            group.push((text, fmt));
                        }
                    }
                    let mut children = Vec::new();
                    for (text, fmt) in group {
                        children.extend(styled(text, fmt));
                    }
                    match link {
                        Some(url) => {
                            let (lead, inner, trail) = split_edges(children);
                            out.extend(lead);
                            if !inner.is_empty() {
                                out.push(Node::Link {
                                    url,
                                    title: None,
                                    children: inner,
                                });
                            }
                            out.extend(trail);
                        }
                        None => out.extend(children),
                    }
                }
            }
        }
        tidy(out)
    }
}

/// Collapse XML whitespace runs to single spaces (ODF text semantics).
fn collapse(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
    out
}

/// Wrap text in formatting, keeping edge whitespace outside the markers.
fn styled(text: String, fmt: Fmt) -> Vec<Node> {
    if fmt == Fmt::default() || text.trim().is_empty() {
        return vec![Node::Text(text)];
    }
    let start = text.len() - text.trim_start().len();
    let end = text.trim_end().len();
    let mut node = Node::Text(text[start..end].to_string());
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
    let mut out = Vec::new();
    if start > 0 {
        out.push(Node::Text(text[..start].to_string()));
    }
    out.push(node);
    if end < text.len() {
        out.push(Node::Text(text[end..].to_string()));
    }
    out
}

/// Split leading/trailing whitespace-only text nodes off a node list.
fn split_edges(mut nodes: Vec<Node>) -> (Vec<Node>, Vec<Node>, Vec<Node>) {
    let mut lead = Vec::new();
    while matches!(nodes.first(), Some(Node::Text(t)) if t.trim().is_empty()) {
        lead.push(nodes.remove(0));
    }
    let mut trail = Vec::new();
    while matches!(nodes.last(), Some(Node::Text(t)) if t.trim().is_empty()) {
        trail.insert(0, nodes.pop().expect("non-empty"));
    }
    (lead, nodes, trail)
}

fn tidy(nodes: Vec<Node>) -> Vec<Node> {
    let mut out: Vec<Node> = Vec::with_capacity(nodes.len());
    for n in nodes {
        match (out.last_mut(), n) {
            (Some(Node::Text(prev)), Node::Text(t)) => {
                if prev.ends_with(' ') && t.starts_with(' ') {
                    prev.push_str(t.trim_start());
                } else {
                    prev.push_str(&t);
                }
            }
            (_, Node::Text(t)) if t.is_empty() => {}
            (_, n) => out.push(n),
        }
    }
    while matches!(out.first(), Some(Node::LineBreak)) {
        out.remove(0);
    }
    while matches!(out.last(), Some(Node::LineBreak)) {
        out.pop();
    }
    out
}

fn is_blank(nodes: &[Node]) -> bool {
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

fn plain(nodes: &[Node]) -> String {
    let mut out = String::new();
    let mut stack: Vec<&Node> = nodes.iter().rev().collect();
    while let Some(n) = stack.pop() {
        match n {
            Node::Text(t) => out.push_str(t),
            Node::LineBreak => out.push(' '),
            Node::Bold { children }
            | Node::Italic { children }
            | Node::Strikethrough { children }
            | Node::Link { children, .. } => stack.extend(children.iter().rev()),
            _ => {}
        }
    }
    out
}

// ─── Blocks ─────────────────────────────────────────────────────────

struct Ctx<'c> {
    styles: &'c Styles,
    config: &'c MarkkitBuilder,
    budget: &'c Budget,
    /// Hidden paragraphs, sections, spans and fields skipped.
    hidden: usize,
    title: Option<String>,
    warnings: Vec<Warning>,
    paragraphs: usize,
    tables: usize,
}

impl Ctx<'_> {
    fn inline(&mut self, el: &Element, fmt: Fmt) -> Vec<Node> {
        let mut inline = Inline {
            styles: self.styles,
            include_hidden: self.config.include_hidden_content,
            hidden: 0,
            runs: Vec::new(),
        };
        inline.walk(el, fmt, None);
        self.hidden += inline.hidden;
        inline.into_nodes()
    }

    fn is_hidden(&self, el: &Element) -> bool {
        !self.config.include_hidden_content
            && (el.attr("display") == Some("none") || self.styles.hidden(el.attr("style-name")))
    }

    /// Convert block-level children of `el`. `list` carries the active list
    /// style and nesting level for `text:list` children.
    fn blocks(
        &mut self,
        el: &Element,
        list: (Option<&str>, usize),
        out: &mut Vec<Node>,
    ) -> Result<(), Error> {
        for child in el.elements() {
            if self.budget.expired() {
                return Err(self.budget.timeout_error());
            }
            if matches!(child.name.as_str(), "h" | "p" | "section") && self.is_hidden(child) {
                self.hidden += 1;
                continue;
            }
            match child.name.as_str() {
                "h" | "p" => self.paragraph(child, out),
                "list" => {
                    let style = child.attr("style-name").or(list.0);
                    let level = if el.name == "list-item" || el.name == "list-header" {
                        list.1 + 1
                    } else {
                        0
                    };
                    let ordered = self.styles.list_ordered(style, level);
                    let mut items = Vec::new();
                    for item in child.elements() {
                        if item.name != "list-item" && item.name != "list-header" {
                            continue;
                        }
                        let mut children = Vec::new();
                        self.blocks(item, (style, level), &mut children)?;
                        if !children.is_empty() {
                            items.push(ListItem::new(children));
                        }
                    }
                    if !items.is_empty() {
                        out.push(Node::List {
                            ordered,
                            start: None,
                            items,
                            source: None,
                        });
                    }
                }
                "table" => self.table(child, out),
                "section" => self.blocks(child, list, out)?,
                _ => {}
            }
        }
        Ok(())
    }

    fn paragraph(&mut self, el: &Element, out: &mut Vec<Node>) {
        let (fmt, outline, is_title) = self.styles.resolve(el.attr("style-name"));
        // Paragraph-level weight usually means "styled heading", not inline bold.
        let level = if el.name == "h" {
            Some(
                el.attr("outline-level")
                    .and_then(|l| l.parse::<u8>().ok())
                    .unwrap_or(1)
                    .clamp(1, 6),
            )
        } else if is_title {
            Some(1)
        } else {
            outline
        };
        let base = if level.is_some() { Fmt::default() } else { fmt };
        let children = self.inline(el, base);
        if is_blank(&children) {
            return;
        }
        self.paragraphs += 1;
        if is_title && self.title.is_none() {
            let t = plain(&children).trim().to_string();
            if !t.is_empty() {
                self.title = Some(t);
            }
        }
        out.push(match level {
            Some(level) => Node::heading(level, children),
            None => Node::paragraph(children),
        });
    }

    fn table(&mut self, el: &Element, out: &mut Vec<Node>) {
        let max_cols = self.config.limits.max_sheet_cols.max(1);
        let max_rows = self.config.limits.max_sheet_rows.saturating_add(1);
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut truncated = false;
        // Rows may sit in header-rows / row-groups; walk them in order.
        let mut stack: Vec<std::slice::Iter<'_, XNode>> = vec![el.children.iter()];
        'walk: while let Some(iter) = stack.last_mut() {
            let Some(node) = iter.next() else {
                stack.pop();
                continue;
            };
            let XNode::Element(e) = node else { continue };
            match e.name.as_str() {
                "table-header-rows" | "table-rows" | "table-row-group" => {
                    stack.push(e.children.iter())
                }
                "table-row" => {
                    let row = self.table_row(e, max_cols, &mut truncated);
                    let repeat = e
                        .attr("number-rows-repeated")
                        .and_then(|r| r.parse::<usize>().ok())
                        .unwrap_or(1)
                        .max(1);
                    for _ in 0..repeat {
                        if rows.len() >= max_rows {
                            // Padding rows past the cap are common and empty.
                            if row.iter().any(|c| !c.is_empty()) {
                                truncated = true;
                            }
                            break 'walk;
                        }
                        rows.push(row.clone());
                    }
                }
                _ => {}
            }
        }
        while rows.last().is_some_and(|r| r.iter().all(String::is_empty)) {
            rows.pop();
        }
        let width = rows
            .iter()
            .map(|r| r.iter().rposition(|c| !c.is_empty()).map_or(0, |i| i + 1))
            .max()
            .unwrap_or(0);
        if width == 0 {
            return;
        }
        if truncated {
            self.warnings.push(Warning::new(
                WarningCode::TruncatedTable,
                format!(
                    "table truncated to {} rows / {max_cols} columns",
                    max_rows - 1
                ),
            ));
        }
        let cell = |s: &String| {
            TableCell::new(if s.is_empty() {
                vec![]
            } else {
                vec![Node::Text(s.clone())]
            })
        };
        let mut iter = rows.iter().map(|r| {
            let mut cells: Vec<TableCell> = r.iter().take(width).map(cell).collect();
            cells.resize_with(width, || TableCell::new(vec![]));
            cells
        });
        let headers = iter.next().unwrap_or_default();
        self.tables += 1;
        out.push(Node::Table {
            alignments: Vec::new(),
            headers,
            rows: iter.collect(),
            source: None,
        });
    }

    fn table_row(&mut self, row: &Element, max_cols: usize, truncated: &mut bool) -> Vec<String> {
        let mut cells = Vec::new();
        for c in row.elements() {
            if c.name != "table-cell" && c.name != "covered-table-cell" {
                continue;
            }
            let text = if c.name == "table-cell" {
                let mut parts = Vec::new();
                // Paragraph text of the cell in document order (nested tables flattened).
                let mut stack: Vec<std::slice::Iter<'_, XNode>> = vec![c.children.iter()];
                while let Some(iter) = stack.last_mut() {
                    let Some(node) = iter.next() else {
                        stack.pop();
                        continue;
                    };
                    let XNode::Element(child) = node else {
                        continue;
                    };
                    match child.name.as_str() {
                        "p" | "h" if self.is_hidden(child) => self.hidden += 1,
                        "p" | "h" => {
                            let t = plain(&self.inline(child, Fmt::default()));
                            if !t.trim().is_empty() {
                                parts.push(t.trim().to_string());
                            }
                        }
                        _ => stack.push(child.children.iter()),
                    }
                }
                parts.join(" ")
            } else {
                String::new()
            };
            let repeat = c
                .attr("number-columns-repeated")
                .and_then(|r| r.parse::<usize>().ok())
                .unwrap_or(1)
                .max(1);
            for _ in 0..repeat {
                if cells.len() >= max_cols {
                    if !text.is_empty() {
                        *truncated = true;
                    }
                    return cells;
                }
                cells.push(text.clone());
            }
        }
        cells
    }
}

// ─── Entry point ────────────────────────────────────────────────────

pub(crate) fn parse(
    bytes: &[u8],
    config: &MarkkitBuilder,
    budget: &Budget,
) -> Result<OdtOutput, Error> {
    let mut zip = ZipGuard::open(bytes, config).map_err(archive_err)?;
    let depth = config.limits.nesting_depth();
    if let Some(mime) = zip.read("mimetype").map_err(archive_err)? {
        let mime = String::from_utf8_lossy(&mime);
        let mime = mime.trim();
        if !mime.starts_with("application/vnd.oasis.opendocument.text") {
            return Err(Error::OdtExtract(format!(
                "not an OpenDocument text document ({mime})"
            )));
        }
    }
    let content = read_xml(&mut zip, "content.xml", depth, budget)?
        .ok_or_else(|| Error::OdtExtract("missing content.xml".into()))?;
    let styles_xml = read_xml(&mut zip, "styles.xml", depth, budget)?;
    let meta_title = read_xml(&mut zip, "meta.xml", depth, budget)?
        .and_then(|m| {
            m.find("title")
                .map(|t| collapse(&t.text()).trim().to_string())
        })
        .filter(|t| !t.is_empty());

    let mut styles = Styles::default();
    styles.add(styles_xml.as_ref());
    styles.add(Some(&content));

    let text = content
        .find("body")
        .and_then(|b| b.child("text"))
        .ok_or_else(|| Error::OdtExtract("content.xml has no office:text body".into()))?;

    let mut ctx = Ctx {
        styles: &styles,
        config,
        budget,
        hidden: 0,
        title: None,
        warnings: Vec::new(),
        paragraphs: 0,
        tables: 0,
    };
    let mut nodes = Vec::new();
    ctx.blocks(text, (None, 0), &mut nodes)?;
    budget.check_time()?;
    if ctx.hidden > 0 {
        ctx.warnings.push(Warning::new(
            WarningCode::HiddenContentSkipped,
            format!("{} hidden element(s) skipped", ctx.hidden),
        ));
    }
    Ok(OdtOutput {
        doc: Document::new(nodes),
        title: meta_title.or(ctx.title),
        warnings: ctx.warnings,
        paragraphs: ctx.paragraphs,
        tables: ctx.tables,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapse_whitespace() {
        assert_eq!(collapse("a \n\t b"), "a b");
        assert_eq!(collapse("  "), " ");
    }

    #[test]
    fn styled_moves_edge_whitespace() {
        let nodes = styled(
            " x ".into(),
            Fmt {
                bold: true,
                ..Fmt::default()
            },
        );
        assert_eq!(
            nodes,
            vec![
                Node::Text(" ".into()),
                Node::Bold {
                    children: vec![Node::Text("x".into())]
                },
                Node::Text(" ".into()),
            ]
        );
    }
}
