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
//! - Provenance (`track_sources`): blocks get the absolute XPath of the
//!   element they came from. Sibling indexes are computed once per parent and
//!   cached, so cost is O(depth) per block, not O(siblings).
//! - Math: MathML `<math>` (also inside KaTeX `.katex` / `.katex-display`
//!   and MathJax `<mjx-container>`) maps to `Math` / `MathBlock` using the
//!   `application/x-tex` annotation; without one, the text content is kept as
//!   plain text. KaTeX's visual HTML duplicate is dropped.
//! - `<figure>` becomes `Figure` only when it has a non-empty `<figcaption>`;
//!   otherwise it stays a transparent container as before.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use ego_tree::{NodeId, NodeRef};
use scraper::{Html, Node as HtmlNode, Selector};

use crate::config::MarkkitBuilder;
use crate::error::Error;
use crate::ir::{Alignment, DefinitionItem, Document, ListItem, Node, Source, TableCell};
use crate::limits::Budget;

use url::Url;

use super::metadata::PageInfo;
use super::nesting::{self, NON_CONTENT};
use super::{readability, urls};

/// Extra levels kept above `max_nesting_depth` before raw tags are dropped:
/// the walker flattens there anyway, the slack covers `html`/`body` and
/// implied elements the pre-scan does not count.
const NESTING_SLACK: usize = 8;

/// HTML-spec clamps for table spans.
const MAX_COLSPAN: u32 = 1000;
const MAX_ROWSPAN: u32 = 65534;

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
    /// Footnote target id (without `#`) -> label, filled by references.
    footnote_labels: RefCell<HashMap<String, String>>,
    /// Base for relative URLs (caller base, then the document `<base href>`).
    base: Option<Url>,
    /// Cached 1-based index of an element among same-name element siblings.
    sibling_index: RefCell<HashMap<NodeId, usize>>,
    /// Main-content mode: also drop "unlikely" subtrees (sidebars, share bars, ...).
    drop_unlikely: Cell<bool>,
}

impl<'a> HtmlParser<'a> {
    /// Parser with a fresh per-call budget (deadline starts now).
    pub fn new(config: &'a MarkkitBuilder) -> Self {
        Self {
            config,
            budget: BudgetRef::Owned(Budget::new(&config.limits)),
            max_depth: config.limits.nesting_depth(),
            footnote_labels: RefCell::new(HashMap::new()),
            base: urls::parse_base(config.base_url.as_deref()),
            sibling_index: RefCell::new(HashMap::new()),
            drop_unlikely: Cell::new(false),
        }
    }

    pub(crate) fn with_budget(config: &'a MarkkitBuilder, budget: &'a Budget) -> Self {
        Self {
            config,
            budget: BudgetRef::Borrowed(budget),
            max_depth: config.limits.nesting_depth(),
            footnote_labels: RefCell::new(HashMap::new()),
            base: urls::parse_base(config.base_url.as_deref()),
            sibling_index: RefCell::new(HashMap::new()),
            drop_unlikely: Cell::new(false),
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
        self.parse_dom(&self.parse_html5ever(html))
    }

    /// html5ever parse of `html` with nesting capped first (see `nesting`).
    fn parse_html5ever(&self, html: &str) -> Html {
        Html::parse_document(&nesting::cap_nesting(html, self.max_depth + NESTING_SLACK))
    }

    /// Parse an HTML string into a [`Document`] plus page title and metadata.
    pub(crate) fn parse_page(&self, html: &str) -> Result<(Document, PageInfo), Error> {
        let dom = self.parse_html5ever(html);
        let this = self.for_document(&dom);
        Ok((
            this.convert_dom(&dom)?,
            super::metadata::extract(&dom, this.base.as_ref()),
        ))
    }

    fn parse_dom(&self, dom: &Html) -> Result<Document, Error> {
        self.for_document(dom).convert_dom(dom)
    }

    /// Parser whose base includes the document's `<base href>`; shares this
    /// parser's budget.
    fn for_document(&self, dom: &Html) -> HtmlParser<'_> {
        HtmlParser {
            config: self.config,
            budget: BudgetRef::Borrowed(self.budget()),
            max_depth: self.max_depth,
            footnote_labels: RefCell::new(HashMap::new()),
            sibling_index: RefCell::new(HashMap::new()),
            base: urls::document_base(dom, self.base.clone()),
            drop_unlikely: Cell::new(false),
        }
    }

    fn convert_dom(&self, dom: &Html) -> Result<Document, Error> {
        // html5ever is not interruptible: check right after it finishes.
        self.budget().check_time()?;
        self.budget().add_nodes(dom.tree.nodes().count())?;
        let root = dom.root_element();

        let exclude_selectors: Vec<Selector> = self
            .config
            .exclude_selectors
            .iter()
            .filter_map(|s| Selector::parse(s).ok())
            .collect();

        let mut nodes = Vec::new();
        if !self.config.include_selectors.is_empty() {
            let sels: Vec<Selector> = self
                .config
                .include_selectors
                .iter()
                .filter_map(|s| Selector::parse(s).ok())
                .collect();
            for content_root in sels.iter().flat_map(|sel| dom.select(sel)) {
                self.convert_children(content_root, &exclude_selectors, &mut nodes, 0);
            }
            return Ok(Document::new(normalize_blocks(nodes)));
        }
        // The walk stops early once the deadline passes; report it here.
        self.budget().check_time()?;

        let body_sel = Selector::parse("body").unwrap();
        let body = dom.select(&body_sel).next().unwrap_or(root);

        if self.config.main_content
            && let Some(roots) = readability::main_content(body, &exclude_selectors)
        {
            self.drop_unlikely.set(true);
            for r in roots {
                if r.whole && !is_transparent_container(r.el.value().name()) {
                    if let Some(mut node) = self.convert_element(r.el, &exclude_selectors, 1) {
                        self.mark_source(&mut node, r.el);
                        nodes.push(node);
                    }
                } else {
                    self.convert_children(r.el, &exclude_selectors, &mut nodes, 0);
                }
            }
            return Ok(Document::new(normalize_blocks(nodes)));
        }

        self.convert_children(body, &exclude_selectors, &mut nodes, 0);
        Ok(Document::new(normalize_blocks(nodes)))
    }

    /// Attach the element's XPath to a block that has no source yet.
    fn mark_source(&self, node: &mut Node, el: scraper::ElementRef<'_>) {
        if self.config.track_sources && node.is_block() && node.source().is_none() {
            node.set_source(Source::path(self.xpath(*el)));
        }
    }

    /// Absolute XPath (`/html[1]/body[1]/p[3]`) of an element.
    fn xpath(&self, node: NodeRef<'_, HtmlNode>) -> String {
        let mut segs: Vec<String> = Vec::new();
        let mut cur = Some(node);
        while let Some(n) = cur {
            if let Some(e) = n.value().as_element() {
                segs.push(format!("{}[{}]", e.name(), self.index_among_siblings(n)));
            }
            cur = n.parent();
        }
        let mut out = String::new();
        for s in segs.iter().rev() {
            out.push('/');
            out.push_str(s);
        }
        out
    }

    fn index_among_siblings(&self, node: NodeRef<'_, HtmlNode>) -> usize {
        if let Some(&i) = self.sibling_index.borrow().get(&node.id()) {
            return i;
        }
        let Some(parent) = node.parent() else {
            return 1;
        };
        // Index every element child of this parent in one pass.
        let mut counts: HashMap<&str, usize> = HashMap::new();
        let mut cache = self.sibling_index.borrow_mut();
        for child in parent.children() {
            if let Some(e) = child.value().as_element() {
                let n = counts.entry(e.name()).or_insert(0);
                *n += 1;
                cache.insert(child.id(), *n);
            }
        }
        cache.get(&node.id()).copied().unwrap_or(1)
    }

    fn is_excluded(&self, el: &scraper::ElementRef<'_>, exclude: &[Selector]) -> bool {
        exclude.iter().any(|sel| sel.matches(el))
            || (self.drop_unlikely.get() && readability::is_unlikely(*el))
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
            let text = flat_text(parent);
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
                            && let Some(mut fig) = self.convert_figure(el, exclude, depth + 1)
                        {
                            self.mark_source(&mut fig, el);
                            out.push(fig);
                            continue;
                        }
                        if let Some(mut m) = self.math_wrapper(el) {
                            self.mark_source(&mut m, el);
                            out.push(m);
                            continue;
                        }
                        // Transparent containers: recurse directly into their children
                        if is_transparent_container(name) {
                            self.convert_children(el, exclude, out, depth + 1);
                        } else if let Some(mut node) = self.convert_element(el, exclude, depth + 1)
                        {
                            self.mark_source(&mut node, el);
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
            let text = flat_text(el);
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return None;
            }
            return Some(Node::Text(trimmed.to_string()));
        }

        if let Some(m) = self.math_wrapper(el) {
            return Some(m);
        }

        let tag = el.value().name();

        match tag {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = tag[1..].parse::<u8>().unwrap_or(1);
                let children = self.collect_inline(el, exclude, depth);
                if children.is_empty() {
                    return None;
                }
                Some(Node::Heading {
                    level,
                    children,
                    source: None,
                })
            }

            "p" => {
                let children = self.collect_inline(el, exclude, depth);
                if children.is_empty() {
                    return None;
                }
                Some(Node::Paragraph {
                    children,
                    source: None,
                })
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
                    source: None,
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
                    source: None,
                })
            }

            "blockquote" => {
                let mut children = Vec::new();
                self.convert_children(el, exclude, &mut children, depth);
                let children = normalize_blocks(children);
                if children.is_empty() {
                    return None;
                }
                Some(Node::BlockQuote {
                    children,
                    source: None,
                })
            }

            "pre" => {
                let code_sel = Selector::parse("code").unwrap();
                if let Some(code_el) = el.select(&code_sel).next() {
                    let language = code_el
                        .value()
                        .attr("class")
                        .and_then(extract_language_from_class);
                    let code = code_el.text().collect::<String>();
                    Some(Node::CodeBlock {
                        language,
                        code,
                        source: None,
                    })
                } else {
                    let code = el.text().collect::<String>();
                    Some(Node::CodeBlock {
                        language: None,
                        code,
                        source: None,
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
                        Some(Node::Paragraph {
                            children,
                            source: None,
                        })
                    };
                }
                let url = urls::resolve(self.base.as_ref(), el.value().attr("href").unwrap_or(""));
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
                let src = urls::resolve(self.base.as_ref(), el.value().attr("src").unwrap_or(""));
                let alt = el.value().attr("alt").unwrap_or("").to_string();
                Some(Node::Image { src, alt })
            }

            "br" => Some(Node::LineBreak),

            "hr" => Some(Node::ThematicBreak),

            // Skip non-content elements
            name if NON_CONTENT.contains(&name) => None,

            // Unknown element: try to extract inline content
            _ => {
                let children = self.collect_inline(el, exclude, depth);
                if children.is_empty() {
                    None
                } else if children.len() == 1 {
                    Some(children.into_iter().next().unwrap())
                } else {
                    Some(Node::Paragraph {
                        children,
                        source: None,
                    })
                }
            }
        }
    }

    /// MathML or a KaTeX / MathJax wrapper around it.
    fn math_wrapper(&self, el: scraper::ElementRef<'_>) -> Option<Node> {
        let v = el.value();
        let (display, math) = match v.name() {
            "math" => (v.attr("display") == Some("block"), el),
            "mjx-container" => (v.attr("display") == Some("true"), find_math(el)?),
            _ if has_class(&el, "katex-display") => (true, find_math(el)?),
            _ if has_class(&el, "katex") => (false, find_math(el)?),
            _ => return None,
        };
        let display = display || math.value().attr("display") == Some("block");
        let annotation = math
            .descendants()
            .filter_map(scraper::ElementRef::wrap)
            .find(|e| {
                e.value().name() == "annotation"
                    && e.value()
                        .attr("encoding")
                        .is_some_and(|enc| enc.eq_ignore_ascii_case("application/x-tex"))
            });
        match annotation {
            Some(a) => {
                let tex = a.text().collect::<String>().trim().to_string();
                if tex.is_empty() {
                    None
                } else if display {
                    Some(Node::MathBlock { tex, source: None })
                } else {
                    Some(Node::Math { tex })
                }
            }
            None => {
                let text = collapse_whitespace(&math.text().collect::<String>());
                let text = text.trim();
                (!text.is_empty()).then(|| Node::Text(text.to_string()))
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
            Some(Node::Paragraph {
                children: nodes,
                source: None,
            })
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
            children = vec![Node::Paragraph {
                children,
                source: None,
            }];
        }
        Some(Node::Figure {
            children,
            caption: Some(caption),
            source: None,
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
        Some(Node::DefinitionList {
            items,
            source: None,
        })
    }

    fn collect_definitions(
        &self,
        el: scraper::ElementRef<'_>,
        exclude: &[Selector],
        depth: usize,
        items: &mut Vec<DefinitionItem>,
    ) {
        if depth >= self.max_depth || self.budget().expired() {
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
                    children = vec![Node::Paragraph {
                        children,
                        source: None,
                    }];
                }
            }
            if !children.is_empty() {
                let mut def = Node::FootnoteDefinition {
                    label,
                    children,
                    source: None,
                };
                self.mark_source(&mut def, li);
                out.push(def);
            }
        }
    }

    /// `<a>` that is a footnote reference: record target and build the node.
    fn footnote_reference(&self, el: scraper::ElementRef<'_>) -> Option<Node> {
        let target = el.value().attr("href")?.strip_prefix('#')?;
        let text = flat_text(el);
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
        if self.budget().expired() {
            return nodes;
        }
        if depth >= self.max_depth {
            let text = flat_text(el);
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
                    } else if let Some(Node::Paragraph {
                        children: first, ..
                    }) = children.first_mut()
                    {
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
        let mut headers: Vec<TableCell> = Vec::new();
        let mut alignments: Vec<Alignment> = Vec::new();
        let mut rows: Vec<Vec<TableCell>> = Vec::new();

        // Direct structure only: a descendant query would also pick up the
        // rows of nested tables, duplicating them into every enclosing table
        // (output and work grew 4x per nesting level). THREAT[T2c].
        let (head_row, body_rows) = table_rows(table_el);
        if let Some(tr) = head_row {
            let ths = row_cells(tr, &["th", "td"]);
            alignments = column_alignments(&ths);
            headers = ths
                .into_iter()
                .map(|th| self.table_cell(th, exclude, depth))
                .collect();
        }

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
            source: None,
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
/// Text of a subtree flattened at `max_nesting_depth`, without the content of
/// [`NON_CONTENT`] elements (`el.text()` leaked `<script>`/`<style>` bodies).
/// Iterative, so depth costs no stack.
fn flat_text(el: scraper::ElementRef<'_>) -> String {
    let mut out = String::new();
    let mut skipping = 0usize;
    for edge in el.traverse() {
        match edge {
            ego_tree::iter::Edge::Open(n) => match n.value() {
                HtmlNode::Element(e) if skipping > 0 || NON_CONTENT.contains(&e.name()) => {
                    skipping += 1;
                }
                HtmlNode::Text(t) if skipping == 0 => out.push_str(t),
                _ => {}
            },
            ego_tree::iter::Edge::Close(n) => {
                if skipping > 0 && n.value().is_element() {
                    skipping -= 1;
                }
            }
        }
    }
    out
}

/// First row of the first direct `<thead>`, and the body rows: `<tr>`
/// children of the table and of its direct `<tbody>` sections, in order.
fn table_rows<'a>(
    table: scraper::ElementRef<'a>,
) -> (
    Option<scraper::ElementRef<'a>>,
    Vec<scraper::ElementRef<'a>>,
) {
    let head = child_elements(table, "thead").find_map(|thead| child_elements(thead, "tr").next());
    let mut body = Vec::new();
    for child in table.children().filter_map(scraper::ElementRef::wrap) {
        match child.value().name() {
            "tbody" => body.extend(child_elements(child, "tr")),
            "tr" => body.push(child),
            _ => {}
        }
    }
    (head, body)
}

fn child_elements<'a>(
    el: scraper::ElementRef<'a>,
    name: &'static str,
) -> impl Iterator<Item = scraper::ElementRef<'a>> {
    el.children()
        .filter_map(scraper::ElementRef::wrap)
        .filter(move |c| c.value().name() == name)
}

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

/// First `<math>` element inside a wrapper.
fn find_math(el: scraper::ElementRef<'_>) -> Option<scraper::ElementRef<'_>> {
    el.descendants()
        .filter_map(scraper::ElementRef::wrap)
        .find(|e| e.value().name() == "math")
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
        out.push(Node::Paragraph {
            children,
            source: None,
        });
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
