//! Main-content detection: a Mozilla Readability-style scorer over the scraper DOM.
//!
//! Decisions:
//! - Opt-in (`MarkkitBuilder::main_content`), third strategy next to include selectors (which
//!   win) and the full body. When no candidate carries enough text, extraction falls back to
//!   the full body, so turning it on never yields an empty result for a page with content.
//! - Scoring follows Readability: paragraph-like blocks (`p`, `pre`, `td`, `blockquote`,
//!   leaf `div`/`section`) with >= 25 chars score `1 + commas + min(chars / 100, 3)`, credited
//!   to the parent (full), grandparent (1/2) and great-grandparent (1/3). Ancestors start
//!   from a tag score plus a +-25 class/id weight. The final score is scaled by
//!   `1 - link density`; siblings of the winner with a related score or prose-like text are kept.
//! - "Unlikely" subtrees (class/id/role such as `sidebar`, `comment`, `share`, `navigation`)
//!   are skipped for scoring and also dropped from the rendered main content.
//! - Bounded work: one iterative traversal; paragraphs are disjoint (leaf blocks only), so
//!   text measurement is linear. Link density is computed only for the top
//!   [`MAX_FINALISTS`] candidates. No recursion, so DOM depth cannot overflow the stack.

use std::collections::HashMap;

use ego_tree::NodeId;
use ego_tree::iter::Edge;
use scraper::{ElementRef, Node, Selector};

/// Candidates whose link density is evaluated.
const MAX_FINALISTS: usize = 10;
/// Shortest paragraph text (chars) that scores.
const MIN_PARAGRAPH_CHARS: usize = 25;
/// Main content with less text than this falls back to the full body.
const MIN_CONTENT_CHARS: usize = 140;

/// Content roots in document order. `whole` = render the element itself, otherwise its children.
pub(crate) struct ContentRoot<'a> {
    pub el: ElementRef<'a>,
    pub whole: bool,
}

/// Pick the main-content roots under `body`, or `None` to fall back to the full body.
pub(crate) fn main_content<'a>(
    body: ElementRef<'a>,
    exclude: &[Selector],
) -> Option<Vec<ContentRoot<'a>>> {
    let mut scores: HashMap<NodeId, f64> = HashMap::new();
    let mut order: Vec<NodeId> = Vec::new();

    let mut skip: Option<NodeId> = None;
    for edge in body.traverse() {
        let node = match edge {
            Edge::Open(n) => n,
            Edge::Close(n) => {
                if skip == Some(n.id()) {
                    skip = None;
                }
                continue;
            }
        };
        if skip.is_some() {
            continue;
        }
        let Some(el) = ElementRef::wrap(node) else {
            continue;
        };
        if el.id() != body.id() && (is_unlikely(el) || exclude.iter().any(|s| s.matches(&el))) {
            skip = Some(el.id());
            continue;
        }
        if !is_paragraph_like(el) {
            continue;
        }
        let len = text_len(el);
        if len < MIN_PARAGRAPH_CHARS {
            continue;
        }
        let commas = el
            .text()
            .map(|t| t.matches([',', '，', '、']).count())
            .sum::<usize>();
        let score = 1.0 + commas as f64 + (len as f64 / 100.0).floor().min(3.0);

        let mut ancestor = el.parent().and_then(ElementRef::wrap);
        for level in 0..3 {
            let Some(a) = ancestor else { break };
            if a.id() == body.id() && level > 0 {
                break;
            }
            let entry = scores.entry(a.id()).or_insert_with(|| {
                order.push(a.id());
                initial_score(a)
            });
            *entry += score / [1.0, 2.0, 3.0][level];
            if a.id() == body.id() {
                break;
            }
            ancestor = a.parent().and_then(ElementRef::wrap);
        }
    }

    let tree = body.tree();
    let element = |id: NodeId| tree.get(id).and_then(ElementRef::wrap);

    // Finalists by raw score (stable: document order breaks ties), then link-density scaling.
    let mut finalists: Vec<(NodeId, f64)> = order.iter().map(|id| (*id, scores[id])).collect();
    finalists.sort_by(|a, b| b.1.total_cmp(&a.1));
    finalists.truncate(MAX_FINALISTS);
    let (top_id, top_score) = finalists
        .into_iter()
        .filter_map(|(id, s)| element(id).map(|el| (id, s * (1.0 - link_density(el)))))
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    let top = element(top_id)?;
    if top.id() == body.id() {
        return None;
    }

    let mut roots = Vec::new();
    let threshold = (top_score * 0.2).max(10.0);
    let parent = top.parent().and_then(ElementRef::wrap);
    let siblings: Vec<ElementRef<'a>> = match parent {
        Some(p) => p.children().filter_map(ElementRef::wrap).collect(),
        None => vec![top],
    };
    for sib in siblings {
        if sib.id() == top.id() {
            roots.push(ContentRoot {
                el: sib,
                whole: false,
            });
            continue;
        }
        if is_unlikely(sib) || exclude.iter().any(|s| s.matches(&sib)) {
            continue;
        }
        let keep = scores
            .get(&sib.id())
            .is_some_and(|s| s * (1.0 - link_density(sib)) >= threshold)
            || (sib.value().name() == "p" && is_prose(sib));
        if keep {
            roots.push(ContentRoot {
                el: sib,
                whole: true,
            });
        }
    }

    let total: usize = roots.iter().map(|r| text_len(r.el)).sum();
    (total >= MIN_CONTENT_CHARS).then_some(roots)
}

/// A paragraph that reads like prose: long enough, few links, or a short sentence.
fn is_prose(p: ElementRef<'_>) -> bool {
    let len = text_len(p);
    let density = link_density(p);
    (len > 80 && density < 0.25)
        || (len > 0 && density == 0.0 && p.text().any(|t| t.contains(". ") || t.ends_with('.')))
}

fn is_paragraph_like(el: ElementRef<'_>) -> bool {
    match el.value().name() {
        "p" | "pre" | "blockquote" => true,
        "td" | "div" | "section" => !el.children().filter_map(ElementRef::wrap).any(is_block),
        _ => false,
    }
}

fn is_block(el: ElementRef<'_>) -> bool {
    matches!(
        el.value().name(),
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "dd"
            | "div"
            | "dl"
            | "dt"
            | "figure"
            | "footer"
            | "form"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "header"
            | "hr"
            | "li"
            | "main"
            | "nav"
            | "ol"
            | "p"
            | "pre"
            | "section"
            | "table"
            | "ul"
    )
}

fn initial_score(el: ElementRef<'_>) -> f64 {
    let tag = match el.value().name() {
        "article" | "main" => 10.0,
        "div" | "section" => 5.0,
        "pre" | "td" | "blockquote" => 3.0,
        "address" | "ol" | "ul" | "dl" | "dd" | "dt" | "li" | "form" | "aside" => -3.0,
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "th" => -5.0,
        _ => 0.0,
    };
    tag + class_weight(el)
}

const POSITIVE: &[&str] = &[
    "article", "body", "content", "entry", "hentry", "h-entry", "main", "page", "post", "text",
    "blog", "story",
];
const NEGATIVE: &[&str] = &[
    "-ad-",
    "hidden",
    "banner",
    "combx",
    "comment",
    "com-",
    "contact",
    "footer",
    "gdpr",
    "masthead",
    "media",
    "meta",
    "outbrain",
    "promo",
    "related",
    "scroll",
    "share",
    "shoutbox",
    "sidebar",
    "skyscraper",
    "sponsor",
    "shopping",
    "tags",
    "widget",
];
const UNLIKELY: &[&str] = &[
    "-ad-",
    "ad-break",
    "agegate",
    "banner",
    "breadcrumb",
    "combx",
    "comment",
    "community",
    "cookie",
    "cover-wrap",
    "disqus",
    "extra",
    "footer",
    "gdpr",
    "header",
    "legends",
    "menu",
    "newsletter",
    "pager",
    "pagination",
    "popup",
    "promo",
    "related",
    "remark",
    "replies",
    "rss",
    "share",
    "shoutbox",
    "sidebar",
    "skyscraper",
    "social",
    "sponsor",
    "subscribe",
    "supplemental",
    "yom-remote",
];
/// Overrides [`UNLIKELY`] when present (e.g. `main-content-header` is kept).
const MAYBE: &[&str] = &[
    "article", "body", "column", "content", "main", "shadow", "post", "entry", "story",
];
const UNLIKELY_TOKENS: &[&str] = &["ad", "ads", "nav", "navbar", "toolbar"];
const UNLIKELY_ROLES: &[&str] = &[
    "menu",
    "menubar",
    "complementary",
    "navigation",
    "alert",
    "alertdialog",
    "dialog",
];

/// Lowercased `class` + `id`.
fn class_and_id(el: ElementRef<'_>) -> String {
    let v = el.value();
    let mut s = String::new();
    for part in [v.attr("class"), v.attr("id")].into_iter().flatten() {
        s.push(' ');
        s.push_str(&part.to_ascii_lowercase());
    }
    s
}

fn class_weight(el: ElementRef<'_>) -> f64 {
    let v = el.value();
    let mut w = 0.0;
    for part in [v.attr("class"), v.attr("id")].into_iter().flatten() {
        let part = part.to_ascii_lowercase();
        if NEGATIVE.iter().any(|n| part.contains(n)) {
            w -= 25.0;
        }
        if POSITIVE.iter().any(|p| part.contains(p)) {
            w += 25.0;
        }
    }
    w
}

pub(crate) fn is_unlikely(el: ElementRef<'_>) -> bool {
    let v = el.value();
    if matches!(v.name(), "body" | "html" | "article" | "main" | "a") {
        return false;
    }
    if v.attr("role")
        .is_some_and(|r| UNLIKELY_ROLES.contains(&r.trim().to_ascii_lowercase().as_str()))
    {
        return true;
    }
    let s = class_and_id(el);
    if s.is_empty() {
        return false;
    }
    let tokens = || {
        s.split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|t| !t.is_empty())
    };
    let unlikely =
        UNLIKELY.iter().any(|u| s.contains(u)) || tokens().any(|t| UNLIKELY_TOKENS.contains(&t));
    unlikely && !MAYBE.iter().any(|m| s.contains(m))
}

/// Visible text length in chars, whitespace runs counted once.
fn text_len(el: ElementRef<'_>) -> usize {
    let mut n = 0;
    let mut space = true;
    for t in el.text() {
        for c in t.chars() {
            if c.is_whitespace() {
                if !space {
                    n += 1;
                }
                space = true;
            } else {
                n += 1;
                space = false;
            }
        }
    }
    n
}

/// Share of `el`'s text inside links (`#` fragment links count 30%, as in Readability).
fn link_density(el: ElementRef<'_>) -> f64 {
    let total = text_len(el);
    if total == 0 {
        return 0.0;
    }
    let mut linked = 0.0;
    for node in el.descendants() {
        if let Node::Element(e) = node.value()
            && e.name() == "a"
            && let Some(a) = ElementRef::wrap(node)
        {
            let coeff = if e
                .attr("href")
                .is_some_and(|h| h.trim_start().starts_with('#'))
            {
                0.3
            } else {
                1.0
            };
            linked += text_len(a) as f64 * coeff;
        }
    }
    (linked / total as f64).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use scraper::Html;

    fn el_with(attrs: &str) -> bool {
        let dom = Html::parse_fragment(&format!("<div {attrs}>x</div>"));
        let sel = Selector::parse("div").unwrap();
        is_unlikely(dom.select(&sel).next().unwrap())
    }

    #[test]
    fn unlikely_classification() {
        assert!(el_with(r#"class="sidebar""#));
        assert!(el_with(r#"id="comments""#));
        assert!(el_with(r#"class="top ads""#));
        assert!(el_with(r#"role="Navigation""#));
        assert!(
            !el_with(r#"class="main-sidebar-content""#),
            "MAYBE overrides"
        );
        assert!(!el_with(r#"class="headline""#));
        assert!(!el_with(r#"class="loading""#), "'ad' only as a whole token");
        assert!(!el_with(""));
    }

    #[test]
    fn text_len_collapses_whitespace() {
        let dom = Html::parse_fragment("<p>  a \n\n b  </p>");
        let p = dom.select(&Selector::parse("p").unwrap()).next().unwrap();
        assert_eq!(text_len(p), 4); // leading space suppressed: "a b "
    }

    #[test]
    fn link_density_counts_fragment_links_less() {
        let dom = Html::parse_fragment(
            r##"<div><a href="/x">aaaaa</a><a href="#y">bbbbbbbbbb</a>ccccc</div>"##,
        );
        let d = dom.select(&Selector::parse("div").unwrap()).next().unwrap();
        assert!((link_density(d) - (5.0 + 3.0) / 20.0).abs() < 1e-9);
    }
}
