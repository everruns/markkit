//! HTML → IR parser using scraper/html5ever.

use scraper::{Html, Node as HtmlNode, Selector};

use crate::config::MarkkitBuilder;
use crate::error::Error;
use crate::ir::{Document, ListItem, Node};
use crate::limits::Budget;

enum BudgetRef<'a> {
    Owned(Budget),
    Borrowed(&'a Budget),
}

/// Parses HTML into the canonical IR.
///
/// Limits: nesting past `limits.max_nesting_depth` is flattened to text; DOM
/// node count and the deadline are enforced ([`Error::ResourceLimit`]).
pub struct HtmlParser<'a> {
    config: &'a MarkkitBuilder,
    budget: BudgetRef<'a>,
    max_depth: usize,
}

impl<'a> HtmlParser<'a> {
    /// Parser with a fresh per-call budget (deadline starts now).
    pub fn new(config: &'a MarkkitBuilder) -> Self {
        Self {
            config,
            budget: BudgetRef::Owned(Budget::new(&config.limits)),
            max_depth: config.limits.nesting_depth(),
        }
    }

    pub(crate) fn with_budget(config: &'a MarkkitBuilder, budget: &'a Budget) -> Self {
        Self {
            config,
            budget: BudgetRef::Borrowed(budget),
            max_depth: config.limits.nesting_depth(),
        }
    }

    fn budget(&self) -> &Budget {
        match &self.budget {
            BudgetRef::Owned(b) => b,
            BudgetRef::Borrowed(b) => b,
        }
    }

    /// Parse an HTML string into a [`Document`].
    pub fn parse(&self, html: &str) -> Result<Document, Error> {
        let dom = Html::parse_document(html);
        // html5ever is not interruptible: check right after it finishes.
        self.budget().check_time()?;
        self.budget().add_nodes(dom.tree.nodes().count())?;
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
        // The walk stops early once the deadline passes; report it here.
        self.budget().check_time()?;

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
        if self.budget().expired() {
            return;
        }
        if depth >= self.max_depth {
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
                        // Transparent containers: recurse directly into their children
                        if is_transparent_container(el.value().name()) {
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
        if self.budget().expired() {
            return None;
        }
        if depth >= self.max_depth {
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

            "div" | "section" | "article" | "main" | "span" | "figure" | "figcaption" => {
                let mut nodes = Vec::new();
                self.convert_children(el, exclude, &mut nodes, depth);
                if nodes.len() == 1 {
                    Some(nodes.remove(0))
                } else if nodes.is_empty() {
                    None
                } else if nodes.iter().all(is_inline) {
                    Some(Node::Paragraph { children: nodes })
                } else {
                    None
                }
            }

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

    fn collect_inline(
        &self,
        el: scraper::ElementRef<'_>,
        exclude: &[Selector],
        depth: usize,
    ) -> Vec<Node> {
        let mut nodes = Vec::new();
        if self.budget().expired() {
            return nodes;
        }
        if depth >= self.max_depth {
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
                let children = normalize_blocks(children);
                if !children.is_empty() {
                    items.push(ListItem::new(children));
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
        let th_sel = Selector::parse("th").unwrap();
        let td_sel = Selector::parse("td").unwrap();

        let mut headers: Vec<Vec<Node>> = Vec::new();
        let mut rows: Vec<Vec<Vec<Node>>> = Vec::new();

        if let Some(thead) = table_el.select(&thead_sel).next()
            && let Some(tr) = thead.select(&tr_sel).next()
        {
            for th in tr.select(&th_sel) {
                headers.push(self.collect_inline(th, exclude, depth + 1));
            }
        }

        let body_rows: Vec<scraper::ElementRef<'_>> =
            if let Some(tbody) = table_el.select(&tbody_sel).next() {
                tbody.select(&tr_sel).collect()
            } else {
                table_el.select(&tr_sel).collect()
            };

        for tr in body_rows {
            if headers.is_empty() {
                let ths: Vec<_> = tr.select(&th_sel).collect();
                if !ths.is_empty() {
                    for th in ths {
                        headers.push(self.collect_inline(th, exclude, depth + 1));
                    }
                    continue;
                }
            }

            let cells: Vec<Vec<Node>> = tr
                .select(&td_sel)
                .map(|td| self.collect_inline(td, exclude, depth + 1))
                .collect();

            if !cells.is_empty() {
                rows.push(cells);
            }
        }

        if headers.is_empty() && rows.is_empty() {
            return None;
        }

        Some(Node::Table { headers, rows })
    }
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
    if nodes.iter().all(is_inline) {
        return nodes;
    }
    let mut out = Vec::with_capacity(nodes.len());
    let mut run: Vec<Node> = Vec::new();
    for node in nodes {
        if is_inline(&node) {
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

/// Check if a node is inline content.
fn is_inline(node: &Node) -> bool {
    matches!(
        node,
        Node::Text(_)
            | Node::Bold { .. }
            | Node::Italic { .. }
            | Node::Strikethrough { .. }
            | Node::InlineCode(_)
            | Node::Link { .. }
            | Node::Image { .. }
            | Node::LineBreak
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
