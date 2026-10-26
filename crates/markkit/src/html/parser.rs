//! HTML → IR parser using scraper/html5ever.
//!
//! Decisions:
//! - Footnotes follow the markup GitHub (`data-footnote-ref`,
//!   `section[data-footnotes]`) and Pandoc (`role="doc-noteref"`,
//!   `.footnote-ref`, `role="doc-endnotes"`, `.footnotes`) emit. References
//!   record `target id -> label`; definitions look their `<li id>` up there
//!   and fall back to their 1-based position. Back-links are dropped.
//! - Table `colspan`/`rowspan` are clamped to the HTML limits (1000 / 65534;
//!   `rowspan=0` means "to the end", stored as 65534); the renderer lays them
//!   out under a grid budget.
//! - `<figure>` becomes `Figure` only when it has a non-empty `<figcaption>`;
//!   otherwise it stays a transparent container as before.

use std::cell::RefCell;
use std::collections::HashMap;

use scraper::{Html, Node as HtmlNode, Selector};

use crate::config::MarkkitBuilder;
use crate::error::Error;
use crate::ir::{Alignment, DefinitionItem, Document, ListItem, Node, TableCell};

/// Maximum recursion depth for nested elements.
const MAX_DEPTH: usize = 200;
/// HTML-spec clamps for table spans.
const MAX_COLSPAN: u32 = 1000;
const MAX_ROWSPAN: u32 = 65534;

/// Parses HTML into the canonical IR.
pub struct HtmlParser<'a> {
    config: &'a MarkkitBuilder,
    /// Footnote target id (without `#`) -> label, filled by references.
    footnote_labels: RefCell<HashMap<String, String>>,
}

impl<'a> HtmlParser<'a> {
    pub fn new(config: &'a MarkkitBuilder) -> Self {
        Self {
            config,
            footnote_labels: RefCell::new(HashMap::new()),
        }
    }

    /// Parse an HTML string into a [`Document`].
    pub fn parse(&self, html: &str) -> Result<Document, Error> {
        let dom = Html::parse_document(html);
        let root = dom.root_element();

        let content_nodes: Vec<scraper::ElementRef<'_>> =
            if !self.config.include_selectors.is_empty() {
                let sels: Vec<Selector> = self
                    .config
                    .include_selectors
                    .iter()
                    .filter_map(|s| Selector::parse(s).ok())
                    .collect();
                sels.iter().flat_map(|sel| dom.select(sel)).collect()
            } else {
                let body_sel = Selector::parse("body").unwrap();
                let body: Vec<_> = dom.select(&body_sel).collect();
                if body.is_empty() { vec![root] } else { body }
            };

        let exclude_selectors: Vec<Selector> = self
            .config
            .exclude_selectors
            .iter()
            .filter_map(|s| Selector::parse(s).ok())
            .collect();

        let mut nodes = Vec::new();
        for content_root in content_nodes {
            self.convert_children(content_root, &exclude_selectors, &mut nodes, 0);
        }

        Ok(Document::new(normalize_blocks(nodes)))
    }

    fn is_excluded(&self, el: &scraper::ElementRef<'_>, exclude: &[Selector]) -> bool {
        exclude.iter().any(|sel| sel.matches(el))
    }

    fn convert_children(
        &self,
        parent: scraper::ElementRef<'_>,
        exclude: &[Selector],
        out: &mut Vec<Node>,
        depth: usize,
    ) {
        if depth >= MAX_DEPTH {
            // At max depth, just extract text
            let text: String = parent.text().collect();
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                out.push(Node::Text(trimmed.to_string()));
            }
            return;
        }

        for child in parent.children() {
            match child.value() {
                HtmlNode::Text(text) => {
                    let t = text.to_string();
                    let collapsed = collapse_whitespace(&t);
                    if !collapsed.is_empty() {
                        out.push(Node::Text(collapsed));
                    } else if t.contains(char::is_whitespace) && !out.is_empty() {
                        out.push(Node::Text(" ".into()));
                    }
                }
                HtmlNode::Element(_) => {
                    if let Some(el) = scraper::ElementRef::wrap(child) {
                        if self.is_excluded(&el, exclude) {
                            continue;
                        }
                        let name = el.value().name();
                        // Captions are consumed by the enclosing figure
                        if name == "figcaption" && parent.value().name() == "figure" {
                            continue;
                        }
                        if is_footnote_section(&el) {
                            self.convert_footnotes(el, exclude, out, depth + 1);
                            continue;
                        }
                        if name == "figure"
                            && let Some(fig) = self.convert_figure(el, exclude, depth + 1)
                        {
                            out.push(fig);
                            continue;
                        }
                        // Transparent containers: recurse directly into their children
                        if is_transparent_container(name) {
                            self.convert_children(el, exclude, out, depth + 1);
                        } else if let Some(node) = self.convert_element(el, exclude, depth + 1) {
                            out.push(node);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn convert_element(
        &self,
        el: scraper::ElementRef<'_>,
        exclude: &[Selector],
        depth: usize,
    ) -> Option<Node> {
        if depth >= MAX_DEPTH {
            let text: String = el.text().collect();
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return None;
            }
            return Some(Node::Text(trimmed.to_string()));
        }

        let tag = el.value().name();

        match tag {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = tag[1..].parse::<u8>().unwrap_or(1);
                let children = self.collect_inline(el, exclude, depth);
                if children.is_empty() {
                    return None;
                }
                Some(Node::Heading { level, children })
            }

            "p" => {
                let children = self.collect_inline(el, exclude, depth);
                if children.is_empty() {
                    return None;
                }
                Some(Node::Paragraph { children })
            }

            "figure" => self
                .convert_figure(el, exclude, depth)
                .or_else(|| self.convert_container(el, exclude, depth)),

            "div" | "section" | "article" | "main" | "span" | "figcaption" => {
                self.convert_container(el, exclude, depth)
            }

            "dl" => self.convert_definition_list(el, exclude, depth),

            "ul" => {
                let items = self.collect_list_items(el, exclude, depth);
                if items.is_empty() {
                    return None;
                }
                Some(Node::List {
                    ordered: false,
                    start: None,
                    items,
                })
            }

            "ol" => {
                let start = el.value().attr("start").and_then(|s| s.parse::<u64>().ok());
                let items = self.collect_list_items(el, exclude, depth);
                if items.is_empty() {
                    return None;
                }
                Some(Node::List {
                    ordered: true,
                    start,
                    items,
                })
            }

            "blockquote" => {
                let mut children = Vec::new();
                self.convert_children(el, exclude, &mut children, depth);
                let children = normalize_blocks(children);
                if children.is_empty() {
                    return None;
                }
                Some(Node::BlockQuote { children })
            }

            "pre" => {
                let code_sel = Selector::parse("code").unwrap();
                if let Some(code_el) = el.select(&code_sel).next() {
                    let language = code_el
                        .value()
                        .attr("class")
                        .and_then(extract_language_from_class);
                    let code = code_el.text().collect::<String>();
                    Some(Node::CodeBlock { language, code })
                } else {
                    let code = el.text().collect::<String>();
                    Some(Node::CodeBlock {
                        language: None,
                        code,
                    })
                }
            }

            "code" => {
                let text = el.text().collect::<String>();
                Some(Node::InlineCode(text))
            }

            "table" => self.convert_table(el, exclude, depth),

            "strong" | "b" => {
                let children = self.collect_inline(el, exclude, depth);
                if children.is_empty() {
                    return None;
                }
                Some(Node::Bold { children })
            }

            "em" | "i" => {
                let children = self.collect_inline(el, exclude, depth);
                if children.is_empty() {
                    return None;
                }
                Some(Node::Italic { children })
            }

            "del" | "s" | "strike" => {
                let children = self.collect_inline(el, exclude, depth);
                if children.is_empty() {
                    return None;
                }
                Some(Node::Strikethrough { children })
            }

            "a" => {
                if is_footnote_backref(&el) {
                    return None;
                }
                if is_footnote_ref(&el)
                    && let Some(node) = self.footnote_reference(el)
                {
                    return Some(node);
                }
                if !self.config.preserve_links {
                    let children = self.collect_inline(el, exclude, depth);
                    return if children.is_empty() {
                        None
                    } else if children.len() == 1 {
                        Some(children.into_iter().next().unwrap())
                    } else {
                        Some(Node::Paragraph { children })
                    };
                }
                let url = el.value().attr("href").unwrap_or("").to_string();
                let title = el.value().attr("title").map(|s| s.to_string());
                let children = self.collect_inline(el, exclude, depth);
                if children.is_empty() {
                    return None;
                }
                Some(Node::Link {
                    url,
                    title,
                    children,
                })
            }

            "img" => {
                if !self.config.preserve_images {
                    return None;
                }
                let src = el.value().attr("src").unwrap_or("").to_string();
                let alt = el.value().attr("alt").unwrap_or("").to_string();
                Some(Node::Image { src, alt })
            }

            "br" => Some(Node::LineBreak),

            "hr" => Some(Node::ThematicBreak),

            // Skip non-content elements
            "script" | "style" | "noscript" | "iframe" | "object" | "embed" | "form" | "input"
            | "button" | "select" | "textarea" | "svg" | "canvas" | "video" | "audio"
            | "source" | "track" | "map" | "area" => None,

            // Unknown element: try to extract inline content
            _ => {
                let children = self.collect_inline(el, exclude, depth);
                if children.is_empty() {
                    None
                } else if children.len() == 1 {
                    Some(children.into_iter().next().unwrap())
                } else {
                    Some(Node::Paragraph { children })
                }
            }
        }
    }

    /// Generic container (`div`, `span`, ...) in inline/element context.
    fn convert_container(
        &self,
        el: scraper::ElementRef<'_>,
        exclude: &[Selector],
        depth: usize,
    ) -> Option<Node> {
        let mut nodes = Vec::new();
        self.convert_children(el, exclude, &mut nodes, depth);
        if nodes.len() == 1 {
            Some(nodes.remove(0))
        } else if nodes.is_empty() {
            None
        } else if nodes.iter().all(Node::is_inline) {
            Some(Node::Paragraph { children: nodes })
        } else {
            None
        }
    }

    /// `<figure>` with a non-empty `<figcaption>`; `None` otherwise.
    fn convert_figure(
        &self,
        el: scraper::ElementRef<'_>,
        exclude: &[Selector],
        depth: usize,
    ) -> Option<Node> {
        let figcaption = el
            .children()
            .filter_map(scraper::ElementRef::wrap)
            .find(|c| c.value().name() == "figcaption" && !self.is_excluded(c, exclude))?;
        let mut caption = self.collect_inline(figcaption, exclude, depth + 1);
        trim_edge_text(&mut caption);
        if caption.is_empty() {
            return None;
        }
        let mut nodes = Vec::new();
        self.convert_children(el, exclude, &mut nodes, depth);
        let mut children = normalize_blocks(nodes);
        if !children.is_empty() && children.iter().all(Node::is_inline) {
            trim_edge_text(&mut children);
            children = vec![Node::Paragraph { children }];
        }
        Some(Node::Figure {
            children,
            caption: Some(caption),
        })
    }

    /// `<dl>`: `<dt>` starts a term, each `<dd>` adds a definition. `<div>`
    /// wrappers (allowed by HTML) are walked through.
    fn convert_definition_list(
        &self,
        el: scraper::ElementRef<'_>,
        exclude: &[Selector],
        depth: usize,
    ) -> Option<Node> {
        let mut items: Vec<DefinitionItem> = Vec::new();
        self.collect_definitions(el, exclude, depth, &mut items);
        items.retain(|i| !i.term.is_empty() || !i.definitions.is_empty());
        if items.is_empty() {
            return None;
        }
        Some(Node::DefinitionList { items })
    }

    fn collect_definitions(
        &self,
        el: scraper::ElementRef<'_>,
        exclude: &[Selector],
        depth: usize,
        items: &mut Vec<DefinitionItem>,
    ) {
        if depth >= MAX_DEPTH {
            return;
        }
        for child in el.children().filter_map(scraper::ElementRef::wrap) {
            if self.is_excluded(&child, exclude) {
                continue;
            }
            match child.value().name() {
                "dt" => {
                    let mut term = self.collect_inline(child, exclude, depth + 1);
                    trim_edge_text(&mut term);
                    if term.is_empty() {
                        continue;
                    }
                    // Consecutive <dt>s share the following definitions.
                    match items.last_mut() {
                        Some(last) if last.definitions.is_empty() && !last.term.is_empty() => {
                            last.term.push(Node::LineBreak);
                            last.term.extend(term);
                        }
                        _ => items.push(DefinitionItem::new(term, Vec::new())),
                    }
                }
                "dd" => {
                    let mut nodes = Vec::new();
                    self.convert_children(child, exclude, &mut nodes, depth + 1);
                    let mut def = normalize_blocks(nodes);
                    if def.iter().all(Node::is_inline) {
                        trim_edge_text(&mut def);
                    }
                    if def.is_empty() {
                        continue;
                    }
                    match items.last_mut() {
                        Some(last) => last.definitions.push(def),
                        None => items.push(DefinitionItem::new(Vec::new(), vec![def])),
                    }
                }
                "div" => self.collect_definitions(child, exclude, depth + 1, items),
                _ => {}
            }
        }
    }

    /// Footnote section: one `FootnoteDefinition` per `<li>` of its list.
    fn convert_footnotes(
        &self,
        el: scraper::ElementRef<'_>,
        exclude: &[Selector],
        out: &mut Vec<Node>,
        depth: usize,
    ) {
        let ol_sel = Selector::parse("ol").unwrap();
        let Some(list) = (if el.value().name() == "ol" {
            Some(el)
        } else {
            el.select(&ol_sel).next()
        }) else {
            return;
        };
        let mut n = 0usize;
        for li in list.children().filter_map(scraper::ElementRef::wrap) {
            if li.value().name() != "li" || self.is_excluded(&li, exclude) {
                continue;
            }
            n += 1;
            let label = li
                .value()
                .attr("id")
                .and_then(|id| self.footnote_labels.borrow().get(id).cloned())
                .unwrap_or_else(|| n.to_string());
            let mut nodes = Vec::new();
            self.convert_children(li, exclude, &mut nodes, depth + 1);
            let mut children = normalize_blocks(nodes);
            if children.iter().all(Node::is_inline) {
                trim_edge_text(&mut children);
                if !children.is_empty() {
                    children = vec![Node::Paragraph { children }];
                }
            }
            if !children.is_empty() {
                out.push(Node::FootnoteDefinition { label, children });
            }
        }
    }

    /// `<a>` that is a footnote reference: record target and build the node.
    fn footnote_reference(&self, el: scraper::ElementRef<'_>) -> Option<Node> {
        let target = el.value().attr("href")?.strip_prefix('#')?;
        let text: String = el.text().collect();
        let text = text
            .trim()
            .trim_start_matches('[')
            .trim_end_matches(']')
            .trim();
        let label = if text.is_empty() { target } else { text }.to_string();
        self.footnote_labels
            .borrow_mut()
            .insert(target.to_string(), label.clone());
        Some(Node::FootnoteReference { label })
    }

    fn collect_inline(
        &self,
        el: scraper::ElementRef<'_>,
        exclude: &[Selector],
        depth: usize,
    ) -> Vec<Node> {
        let mut nodes = Vec::new();
        if depth >= MAX_DEPTH {
            let text: String = el.text().collect();
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                nodes.push(Node::Text(trimmed.to_string()));
            }
            return nodes;
        }

        for child in el.children() {
            match child.value() {
                HtmlNode::Text(text) => {
                    let t = text.to_string();
                    let collapsed = collapse_whitespace(&t);
                    if !collapsed.is_empty() {
                        nodes.push(Node::Text(collapsed));
                    } else if t.contains(char::is_whitespace) && !nodes.is_empty() {
                        nodes.push(Node::Text(" ".into()));
                    }
                }
                HtmlNode::Element(_) => {
                    if let Some(child_el) = scraper::ElementRef::wrap(child) {
                        if self.is_excluded(&child_el, exclude) {
                            continue;
                        }
                        if let Some(node) = self.convert_element(child_el, exclude, depth + 1) {
                            nodes.push(node);
                        }
                    }
                }
                _ => {}
            }
        }
        nodes
    }

    fn collect_list_items(
        &self,
        list_el: scraper::ElementRef<'_>,
        exclude: &[Selector],
        depth: usize,
    ) -> Vec<ListItem> {
        let mut items = Vec::new();
        for child in list_el.children() {
            if let Some(el) = scraper::ElementRef::wrap(child)
                && el.value().name() == "li"
            {
                let mut children = Vec::new();
                self.convert_children(el, exclude, &mut children, depth + 1);
                let mut children = normalize_blocks(children);
                let checked = task_checkbox(el);
                if checked.is_some() {
                    if children.iter().all(Node::is_inline) {
                        trim_edge_text(&mut children);
                    } else if let Some(Node::Paragraph { children: first }) = children.first_mut() {
                        trim_edge_text(first);
                    }
                }
                if !children.is_empty() {
                    items.push(match checked {
                        Some(c) => ListItem::task(children, c),
                        None => ListItem::new(children),
                    });
                }
            }
        }
        items
    }

    fn convert_table(
        &self,
        table_el: scraper::ElementRef<'_>,
        exclude: &[Selector],
        depth: usize,
    ) -> Option<Node> {
        let thead_sel = Selector::parse("thead").unwrap();
        let tbody_sel = Selector::parse("tbody").unwrap();
        let tr_sel = Selector::parse("tr").unwrap();

        let mut headers: Vec<TableCell> = Vec::new();
        let mut alignments: Vec<Alignment> = Vec::new();
        let mut rows: Vec<Vec<TableCell>> = Vec::new();

        if let Some(thead) = table_el.select(&thead_sel).next()
            && let Some(tr) = thead.select(&tr_sel).next()
        {
            let ths = row_cells(tr, &["th", "td"]);
            alignments = column_alignments(&ths);
            headers = ths
                .into_iter()
                .map(|th| self.table_cell(th, exclude, depth))
                .collect();
        }

        let body_rows: Vec<scraper::ElementRef<'_>> =
            if let Some(tbody) = table_el.select(&tbody_sel).next() {
                tbody.select(&tr_sel).collect()
            } else {
                table_el.select(&tr_sel).collect()
            };

        for tr in body_rows {
            let row = row_cells(tr, &["td", "th"]);
            // A leading all-<th> row is the header.
            if headers.is_empty()
                && rows.is_empty()
                && !row.is_empty()
                && row.iter().all(|c| c.value().name() == "th")
            {
                alignments = column_alignments(&row);
                headers = row
                    .into_iter()
                    .map(|th| self.table_cell(th, exclude, depth))
                    .collect();
                continue;
            }

            let cells: Vec<TableCell> = row
                .into_iter()
                .map(|td| self.table_cell(td, exclude, depth))
                .collect();

            if !cells.is_empty() {
                rows.push(cells);
            }
        }

        if headers.is_empty() && rows.is_empty() {
            return None;
        }
        if alignments.iter().all(|a| *a == Alignment::None) {
            alignments.clear();
        }

        Some(Node::Table {
            alignments,
            headers,
            rows,
        })
    }

    fn table_cell(
        &self,
        cell: scraper::ElementRef<'_>,
        exclude: &[Selector],
        depth: usize,
    ) -> TableCell {
        let colspan = span_attr(cell, "colspan").map_or(1, |n| n.clamp(1, MAX_COLSPAN));
        let rowspan = span_attr(cell, "rowspan").map_or(1, |n| {
            if n == 0 {
                MAX_ROWSPAN
            } else {
                n.min(MAX_ROWSPAN)
            }
        });
        TableCell::new(self.collect_inline(cell, exclude, depth + 1)).with_span(colspan, rowspan)
    }
}

/// Direct `th`/`td` children of a row (nested tables' cells excluded).
fn row_cells<'a>(tr: scraper::ElementRef<'a>, names: &[&str]) -> Vec<scraper::ElementRef<'a>> {
    tr.children()
        .filter_map(scraper::ElementRef::wrap)
        .filter(|c| names.contains(&c.value().name()))
        .collect()
}

fn span_attr(cell: scraper::ElementRef<'_>, name: &str) -> Option<u32> {
    let v = cell.value().attr(name)?.trim();
    // Very long digit strings saturate instead of failing to parse.
    let digits: String = v
        .chars()
        .take_while(char::is_ascii_digit)
        .take(10)
        .collect();
    if digits.is_empty() {
        return None;
    }
    Some(
        digits
            .parse::<u64>()
            .map_or(u32::MAX, |n| n.min(u32::MAX as u64) as u32),
    )
}

/// Per-grid-column alignment from a header row (`align` attr or `text-align` style).
fn column_alignments(cells: &[scraper::ElementRef<'_>]) -> Vec<Alignment> {
    let mut out = Vec::new();
    for cell in cells {
        let a = cell_alignment(cell);
        let span = span_attr(*cell, "colspan").map_or(1, |n| n.clamp(1, MAX_COLSPAN));
        for _ in 0..span {
            out.push(a);
        }
    }
    out
}

fn cell_alignment(cell: &scraper::ElementRef<'_>) -> Alignment {
    let from = |v: &str| match v.trim().to_ascii_lowercase().as_str() {
        "left" | "start" => Alignment::Left,
        "center" => Alignment::Center,
        "right" | "end" => Alignment::Right,
        _ => Alignment::None,
    };
    if let Some(style) = cell.value().attr("style") {
        for decl in style.split(';') {
            if let Some((k, v)) = decl.split_once(':')
                && k.trim().eq_ignore_ascii_case("text-align")
            {
                return from(v);
            }
        }
    }
    cell.value().attr("align").map_or(Alignment::None, from)
}

/// Task-list checkbox leading a `<li>` (directly or inside its first `<p>`/`<label>`).
fn task_checkbox(li: scraper::ElementRef<'_>) -> Option<bool> {
    fn first_checkbox(el: scraper::ElementRef<'_>, nested: bool) -> Option<bool> {
        for child in el.children() {
            match child.value() {
                HtmlNode::Text(t) if t.trim().is_empty() => continue,
                HtmlNode::Element(e) => {
                    if e.name() == "input" {
                        return e
                            .attr("type")
                            .filter(|t| t.eq_ignore_ascii_case("checkbox"))
                            .map(|_| e.attr("checked").is_some());
                    }
                    if !nested && matches!(e.name(), "p" | "label") {
                        return first_checkbox(scraper::ElementRef::wrap(child)?, true);
                    }
                    return None;
                }
                HtmlNode::Comment(_) => continue,
                _ => return None,
            }
        }
        None
    }
    first_checkbox(li, false)
}

fn has_class(el: &scraper::ElementRef<'_>, class: &str) -> bool {
    el.value()
        .attr("class")
        .is_some_and(|c| c.split_whitespace().any(|x| x == class))
}

fn is_footnote_ref(el: &scraper::ElementRef<'_>) -> bool {
    let v = el.value();
    v.attr("data-footnote-ref").is_some()
        || v.attr("role") == Some("doc-noteref")
        || has_class(el, "footnote-ref")
}

fn is_footnote_backref(el: &scraper::ElementRef<'_>) -> bool {
    let v = el.value();
    v.attr("data-footnote-backref").is_some()
        || v.attr("role") == Some("doc-backlink")
        || has_class(el, "footnote-back")
        || has_class(el, "footnote-backref")
}

fn is_footnote_section(el: &scraper::ElementRef<'_>) -> bool {
    let v = el.value();
    matches!(v.name(), "section" | "div" | "aside" | "ol")
        && (v.attr("data-footnotes").is_some()
            || matches!(v.attr("role"), Some("doc-endnotes" | "doc-footnotes"))
            || has_class(el, "footnotes"))
}

/// Extract language identifier from a class like "language-rust" or "lang-python".
fn extract_language_from_class(class: &str) -> Option<String> {
    for cls in class.split_whitespace() {
        if let Some(lang) = cls.strip_prefix("language-") {
            return Some(lang.to_string());
        }
        if let Some(lang) = cls.strip_prefix("lang-") {
            return Some(lang.to_string());
        }
    }
    None
}

/// Collapse runs of whitespace into single spaces.
/// Preserves a leading/trailing space if the original had one (important for inline context).
fn collapse_whitespace(s: &str) -> String {
    let has_leading_space = s.starts_with(char::is_whitespace);
    let has_trailing_space = s.ends_with(char::is_whitespace);

    let mut result = String::with_capacity(s.len());
    let mut prev_space = true; // suppress leading space initially
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                result.push(' ');
            }
            prev_space = true;
        } else {
            prev_space = false;
            result.push(ch);
        }
    }
    if result.ends_with(' ') {
        result.pop();
    }

    if has_leading_space && !result.is_empty() {
        result.insert(0, ' ');
    }
    if has_trailing_space && !result.is_empty() {
        result.push(' ');
    }

    result
}

/// Normalize a block-context node list.
///
/// Source indentation between block elements arrives as whitespace-only text,
/// which `convert_children` keeps as `Text(" ")` (it matters between inline
/// siblings). Next to a block it is noise that breaks blank-line separation and
/// leaks a leading space into the next line, so: when the list holds any block,
/// drop whitespace-only inline runs and wrap the remaining inline runs in a
/// `Paragraph` (edge whitespace trimmed). Pure-inline lists are returned as is.
fn normalize_blocks(nodes: Vec<Node>) -> Vec<Node> {
    if nodes.iter().all(Node::is_inline) {
        return nodes;
    }
    let mut out = Vec::with_capacity(nodes.len());
    let mut run: Vec<Node> = Vec::new();
    for node in nodes {
        if node.is_inline() {
            run.push(node);
        } else {
            flush_inline_run(&mut run, &mut out);
            out.push(node);
        }
    }
    flush_inline_run(&mut run, &mut out);
    out
}

fn flush_inline_run(run: &mut Vec<Node>, out: &mut Vec<Node>) {
    let mut children = std::mem::take(run);
    trim_edge_text(&mut children);
    if !children.is_empty() {
        out.push(Node::Paragraph { children });
    }
}

/// Trim whitespace at the outer edges of an inline run, dropping emptied text nodes.
fn trim_edge_text(children: &mut Vec<Node>) {
    while let Some(Node::Text(t)) = children.first_mut() {
        let trimmed = t.trim_start();
        if trimmed.is_empty() {
            children.remove(0);
        } else {
            *t = trimmed.to_string();
            break;
        }
    }
    while let Some(Node::Text(t)) = children.last_mut() {
        let trimmed = t.trim_end();
        if trimmed.is_empty() {
            children.pop();
        } else {
            *t = trimmed.to_string();
            break;
        }
    }
}

/// Check if a tag is a transparent container (should be unwrapped in block context).
fn is_transparent_container(tag: &str) -> bool {
    matches!(
        tag,
        "div" | "section" | "article" | "main" | "figure" | "figcaption"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collapse_whitespace() {
        assert_eq!(collapse_whitespace("  hello   world  "), " hello world ");
        assert_eq!(collapse_whitespace("a"), "a");
        assert_eq!(collapse_whitespace("  "), "");
        assert_eq!(collapse_whitespace("a\n  b"), "a b");
        assert_eq!(collapse_whitespace(" bold "), " bold ");
        assert_eq!(collapse_whitespace("no-spaces"), "no-spaces");
    }

    #[test]
    fn test_extract_language_from_class() {
        assert_eq!(
            extract_language_from_class("language-rust"),
            Some("rust".into())
        );
        assert_eq!(
            extract_language_from_class("lang-python"),
            Some("python".into())
        );
        assert_eq!(
            extract_language_from_class("highlight language-js extra"),
            Some("js".into())
        );
        assert_eq!(extract_language_from_class("highlight"), None);
    }
}
