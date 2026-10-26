//! Page metadata from `<head>`: `<title>`, `<meta>`, Open Graph, `<link rel=canonical>`, JSON-LD.
//!
//! Decisions:
//! - Precedence per field is fixed (documented in `knowledge/formats/html-extraction.md`):
//!   explicit social/structured tags beat generic ones (`og:title` > JSON-LD > `<title>`).
//! - All values are untrusted text: whitespace collapsed, control chars dropped, length
//!   capped at [`MAX_VALUE_CHARS`]. URL fields keep only `http(s)` or scheme-less (relative)
//!   values, so `javascript:` / `data:` never surface as a canonical URL or image.
//! - JSON-LD is bounded: at most [`MAX_JSONLD_SCRIPTS`] scripts of [`MAX_JSONLD_BYTES`] each,
//!   at most [`MAX_JSONLD_NODES`] candidate nodes; serde_json caps nesting at 128. Invalid
//!   JSON is ignored, never an error.

use scraper::{ElementRef, Html, Selector};
use serde_json::Value;

use crate::extract::Metadata;

/// Longest metadata value kept, in chars; longer values are truncated.
const MAX_VALUE_CHARS: usize = 2048;
/// JSON-LD scripts inspected per page.
const MAX_JSONLD_SCRIPTS: usize = 16;
/// Larger JSON-LD scripts are skipped unparsed.
const MAX_JSONLD_BYTES: usize = 1024 * 1024;
/// JSON-LD objects (top level, arrays, `@graph`) considered across all scripts.
const MAX_JSONLD_NODES: usize = 256;

/// Title plus [`Metadata`] read from one parsed page.
#[derive(Debug, Default)]
pub(crate) struct PageInfo {
    pub title: Option<String>,
    pub metadata: Metadata,
}

/// Read title and metadata from a parsed document.
pub(crate) fn extract(dom: &Html) -> PageInfo {
    let meta = MetaTags::collect(dom);
    let ld = JsonLd::collect(dom);

    let title = meta
        .get(&["og:title", "twitter:title"])
        .or_else(|| ld.text("headline"))
        .or_else(|| first_text(dom, "title"))
        .or_else(|| meta.get(&["dc.title", "dcterms.title"]))
        .or_else(|| ld.text("name"));

    let metadata = Metadata {
        description: meta
            .get(&["og:description", "description", "twitter:description"])
            .or_else(|| ld.text("description"))
            .or_else(|| meta.get(&["dc.description", "dcterms.description"])),
        author: meta
            .get(&["author", "dc.creator", "dcterms.creator"])
            .or_else(|| ld.author())
            // `article:author` is usually a profile URL; keep it only when it is a name.
            .or_else(|| meta.get(&["article:author"]).filter(|a| !looks_like_url(a))),
        published: meta
            .get(&["article:published_time"])
            .or_else(|| ld.text("datePublished"))
            .or_else(|| {
                meta.get(&[
                    "date",
                    "dc.date",
                    "dcterms.created",
                    "dc.date.issued",
                    "pubdate",
                ])
            }),
        language: html_lang(dom)
            .or_else(|| meta.http_equiv("content-language"))
            .or_else(|| meta.get(&["og:locale"]))
            .or_else(|| ld.text("inLanguage")),
        canonical_url: canonical_link(dom)
            .or_else(|| meta.get(&["og:url"]).and_then(url_value))
            .or_else(|| ld.text("url").and_then(url_value)),
        site_name: meta
            .get(&["og:site_name", "application-name"])
            .or_else(|| ld.publisher()),
        image: meta
            .get(&[
                "og:image",
                "og:image:url",
                "og:image:secure_url",
                "twitter:image",
            ])
            .and_then(url_value)
            .or_else(|| ld.image()),
    };

    PageInfo { title, metadata }
}

/// `<meta>` values keyed by lowercased `property` / `name`, first occurrence wins.
struct MetaTags {
    named: Vec<(String, String)>,
    http_equiv: Vec<(String, String)>,
}

impl MetaTags {
    fn collect(dom: &Html) -> Self {
        let sel = Selector::parse("meta").unwrap();
        let mut named = Vec::new();
        let mut http_equiv = Vec::new();
        for el in dom.select(&sel) {
            let attrs = el.value();
            let Some(content) = attrs.attr("content").and_then(clean) else {
                continue;
            };
            for key in [
                attrs.attr("property"),
                attrs.attr("name"),
                attrs.attr("itemprop"),
            ]
            .into_iter()
            .flatten()
            {
                named.push((key.trim().to_ascii_lowercase(), content.clone()));
            }
            if let Some(key) = attrs.attr("http-equiv") {
                http_equiv.push((key.trim().to_ascii_lowercase(), content));
            }
        }
        Self { named, http_equiv }
    }

    /// First value of the first key (in priority order) that is present.
    fn get(&self, keys: &[&str]) -> Option<String> {
        keys.iter().find_map(|k| {
            self.named
                .iter()
                .find(|(name, _)| name == k)
                .map(|(_, v)| v.clone())
        })
    }

    fn http_equiv(&self, key: &str) -> Option<String> {
        self.http_equiv
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    }
}

/// Candidate JSON-LD objects, in document order, article-like types first.
struct JsonLd {
    nodes: Vec<Value>,
}

impl JsonLd {
    fn collect(dom: &Html) -> Self {
        let sel = Selector::parse("script").unwrap();
        let mut nodes = Vec::new();
        let scripts = dom
            .select(&sel)
            .filter(|el| {
                el.value()
                    .attr("type")
                    .is_some_and(|t| t.trim().eq_ignore_ascii_case("application/ld+json"))
            })
            .take(MAX_JSONLD_SCRIPTS);
        for script in scripts {
            let raw: String = script.text().collect();
            if raw.len() > MAX_JSONLD_BYTES {
                continue;
            }
            if let Ok(value) = serde_json::from_str::<Value>(&raw) {
                flatten_nodes(value, &mut nodes);
            }
        }
        // Stable: article-like nodes first, otherwise document order.
        nodes.sort_by_key(|n| !is_article(n));
        Self { nodes }
    }

    fn text(&self, key: &str) -> Option<String> {
        self.nodes.iter().find_map(|n| n.get(key).and_then(text_of))
    }

    fn author(&self) -> Option<String> {
        self.nodes
            .iter()
            .find_map(|n| n.get("author").and_then(names_of))
    }

    fn publisher(&self) -> Option<String> {
        self.nodes
            .iter()
            .find_map(|n| n.get("publisher").and_then(names_of))
    }

    fn image(&self) -> Option<String> {
        self.nodes.iter().find_map(|n| {
            let img = n.get("image")?;
            let img = match img {
                Value::Array(items) => items.first()?,
                other => other,
            };
            match img {
                Value::String(s) => clean(s),
                Value::Object(o) => o.get("url").and_then(text_of),
                _ => None,
            }
            .and_then(url_value)
        })
    }
}

/// Push objects from a JSON-LD value: the value itself, array members and `@graph` members.
fn flatten_nodes(value: Value, out: &mut Vec<Value>) {
    let mut stack = vec![value];
    while let Some(v) = stack.pop() {
        if out.len() >= MAX_JSONLD_NODES {
            return;
        }
        match v {
            Value::Array(items) => stack.extend(items.into_iter().rev()),
            Value::Object(mut obj) => {
                let graph = obj.remove("@graph");
                out.push(Value::Object(obj));
                if let Some(g) = graph {
                    stack.push(g);
                }
            }
            _ => {}
        }
    }
}

fn is_article(node: &Value) -> bool {
    const ARTICLE_TYPES: &[&str] = &[
        "Article",
        "NewsArticle",
        "BlogPosting",
        "Report",
        "ScholarlyArticle",
        "TechArticle",
        "SocialMediaPosting",
        "LiveBlogPosting",
        "OpinionNewsArticle",
        "AnalysisNewsArticle",
        "ReportageNewsArticle",
    ];
    let is = |t: &Value| t.as_str().is_some_and(|s| ARTICLE_TYPES.contains(&s));
    match node.get("@type") {
        Some(Value::Array(types)) => types.iter().any(is),
        Some(t) => is(t),
        None => false,
    }
}

/// A scalar JSON-LD value as cleaned text (strings and numbers only).
fn text_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => clean(s),
        Value::Number(n) => clean(&n.to_string()),
        Value::Array(items) => items.iter().find_map(text_of),
        _ => None,
    }
}

/// Names from a JSON-LD person/organization value: `"x"`, `{name}`, or an array of those.
fn names_of(v: &Value) -> Option<String> {
    let one = |v: &Value| match v {
        Value::String(s) if !looks_like_url(s) => clean(s),
        Value::Object(o) => o.get("name").and_then(text_of),
        _ => None,
    };
    let names: Vec<String> = match v {
        Value::Array(items) => items.iter().take(16).filter_map(one).collect(),
        other => one(other).into_iter().collect(),
    };
    if names.is_empty() {
        None
    } else {
        clean(&names.join(", "))
    }
}

fn first_text(dom: &Html, selector: &str) -> Option<String> {
    let sel = Selector::parse(selector).unwrap();
    dom.select(&sel)
        .next()
        .and_then(|el| clean(&el.text().collect::<String>()))
}

fn html_lang(dom: &Html) -> Option<String> {
    let root: ElementRef<'_> = dom.root_element();
    root.value()
        .attr("lang")
        .or_else(|| root.value().attr("xml:lang"))
        .and_then(clean)
}

fn canonical_link(dom: &Html) -> Option<String> {
    let sel = Selector::parse("link[rel][href]").unwrap();
    dom.select(&sel)
        .find(|el| {
            el.value().attr("rel").is_some_and(|r| {
                r.split_ascii_whitespace()
                    .any(|t| t.eq_ignore_ascii_case("canonical"))
            })
        })
        .and_then(|el| el.value().attr("href"))
        .and_then(clean)
        .and_then(url_value)
}

/// Keep a URL only when it is `http(s)` or has no scheme (relative).
fn url_value(url: String) -> Option<String> {
    match url.split_once(':') {
        Some((scheme, _))
            if !scheme.contains(['/', '?', '#'])
                && !scheme.eq_ignore_ascii_case("http")
                && !scheme.eq_ignore_ascii_case("https") =>
        {
            None
        }
        _ => Some(url),
    }
}

fn looks_like_url(s: &str) -> bool {
    let s = s.trim_start();
    s.starts_with("http://") || s.starts_with("https://") || s.starts_with("//")
}

/// Collapse whitespace, drop control chars, cap length; `None` when nothing is left.
fn clean(s: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = 0;
    let mut pending_space = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if ch.is_control() {
            continue;
        }
        if chars >= MAX_VALUE_CHARS {
            break;
        }
        if pending_space {
            out.push(' ');
            chars += 1;
            pending_space = false;
        }
        out.push(ch);
        chars += 1;
    }
    (!out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(html: &str) -> PageInfo {
        extract(&Html::parse_document(html))
    }

    #[test]
    fn clean_collapses_and_caps() {
        assert_eq!(clean("  a \n\t b  "), Some("a b".into()));
        assert_eq!(clean(" \u{0}\u{7} "), None);
        assert_eq!(
            clean(&"x".repeat(5000)).unwrap().chars().count(),
            MAX_VALUE_CHARS
        );
    }

    #[test]
    fn url_value_filters_schemes() {
        assert_eq!(
            url_value("https://a.b/c".into()),
            Some("https://a.b/c".into())
        );
        assert_eq!(url_value("/rel/path".into()), Some("/rel/path".into()));
        assert_eq!(url_value("rel?q=a:b".into()), Some("rel?q=a:b".into()));
        assert_eq!(url_value("javascript:alert(1)".into()), None);
        assert_eq!(url_value("JaVaScRiPt:alert(1)".into()), None);
        assert_eq!(url_value("data:text/html,x".into()), None);
    }

    #[test]
    fn title_precedence() {
        let i = info("<title>T</title><meta property='og:title' content='OG'>");
        assert_eq!(i.title.as_deref(), Some("OG"));
        let i = info("<title> Plain \n title </title>");
        assert_eq!(i.title.as_deref(), Some("Plain title"));
        assert!(info("<p>no head</p>").title.is_none());
    }

    #[test]
    fn jsonld_graph_and_author_forms() {
        let i = info(
            r#"<script type="application/ld+json">
            {"@graph": [
              {"@type": "WebSite", "name": "Site"},
              {"@type": "NewsArticle", "headline": "H", "datePublished": "2024-01-02",
               "author": [{"@type": "Person", "name": "A"}, "B", "https://x/profile"],
               "publisher": {"name": "Pub"}, "image": [{"url": "https://i/1.png"}]}
            ]}</script>"#,
        );
        assert_eq!(i.title.as_deref(), Some("H"));
        assert_eq!(i.metadata.author.as_deref(), Some("A, B"));
        assert_eq!(i.metadata.published.as_deref(), Some("2024-01-02"));
        assert_eq!(i.metadata.site_name.as_deref(), Some("Pub"));
        assert_eq!(i.metadata.image.as_deref(), Some("https://i/1.png"));
    }

    #[test]
    fn invalid_jsonld_ignored() {
        let i = info(r#"<script type="application/ld+json">{not json</script><title>T</title>"#);
        assert_eq!(i.title.as_deref(), Some("T"));
    }
}
