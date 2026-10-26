//! Relative URL resolution for links, images and metadata URLs.
//!
//! Decisions:
//! - Base = `MarkkitBuilder::base_url` (or the final `extract_url` response URL), with the
//!   first document `<base href>` resolved on top, as browsers do. A page-supplied base is
//!   accepted only when the result is `http(s)`, so untrusted markup cannot rebase links onto
//!   `file:` or other local schemes. A caller base must parse and be able to act as a base
//!   (`data:` / `javascript:` cannot); otherwise it is ignored.
//! - Only scheme-less references are rewritten. Absolute URLs (any scheme, including
//!   `javascript:`) are returned byte-for-byte, so resolution never changes what a scheme
//!   filter downstream sees. Empty and fragment-only (`#id`) references stay in-document.
//! - Without a base nothing changes.

use scraper::{Html, Selector};
use url::Url;

/// Parse a caller-supplied base URL.
pub(crate) fn parse_base(raw: Option<&str>) -> Option<Url> {
    Url::parse(raw?.trim())
        .ok()
        .filter(|u| !u.cannot_be_a_base())
}

/// Effective base for a document: `base` with the first `<base href>` applied on top.
pub(crate) fn document_base(dom: &Html, base: Option<Url>) -> Option<Url> {
    let sel = Selector::parse("base[href]").unwrap();
    let Some(href) = dom
        .select(&sel)
        .next()
        .and_then(|el| el.value().attr("href"))
    else {
        return base;
    };
    let href = href.trim();
    let doc_base = match &base {
        Some(b) => b.join(href).ok(),
        None => Url::parse(href).ok(),
    }
    .filter(|u| matches!(u.scheme(), "http" | "https"));
    doc_base.or(base)
}

/// Resolve `raw` against `base` when it is a relative reference; otherwise return it unchanged.
pub(crate) fn resolve(base: Option<&Url>, raw: &str) -> String {
    let Some(base) = base else {
        return raw.to_string();
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || has_scheme(trimmed) {
        return raw.to_string();
    }
    base.join(trimmed)
        .map(String::from)
        .unwrap_or_else(|_| raw.to_string())
}

/// RFC 3986 scheme: `ALPHA *( ALPHA / DIGIT / "+" / "-" / "." ) ":"`.
fn has_scheme(s: &str) -> bool {
    let Some((scheme, _)) = s.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(s: &str) -> Option<Url> {
        parse_base(Some(s))
    }

    #[test]
    fn resolves_relative_forms() {
        let b = base("https://ex.com/a/b/page.html?q=1");
        let r = |s| resolve(b.as_ref(), s);
        assert_eq!(r("img.png"), "https://ex.com/a/b/img.png");
        assert_eq!(r("../c"), "https://ex.com/a/c");
        assert_eq!(r("/root"), "https://ex.com/root");
        assert_eq!(r("//cdn.ex/x.js"), "https://cdn.ex/x.js");
        assert_eq!(r("?p=2"), "https://ex.com/a/b/page.html?p=2");
        assert_eq!(r("rel?q=a:b"), "https://ex.com/a/b/rel?q=a:b");
    }

    #[test]
    fn leaves_absolute_fragment_and_empty() {
        let b = base("https://ex.com/");
        let r = |s| resolve(b.as_ref(), s);
        assert_eq!(r("https://other/x"), "https://other/x");
        assert_eq!(r("JavaScript:alert(1)"), "JavaScript:alert(1)");
        assert_eq!(r("mailto:a@b"), "mailto:a@b");
        assert_eq!(r("data:image/png;base64,AA"), "data:image/png;base64,AA");
        assert_eq!(r("#top"), "#top");
        assert_eq!(r(""), "");
        assert_eq!(resolve(None, "x.png"), "x.png");
    }

    #[test]
    fn invalid_or_opaque_caller_base_ignored() {
        assert!(base("not a url").is_none());
        assert!(base("javascript:alert(1)").is_none());
        assert!(base("data:text/html,x").is_none());
        assert!(base("file:///tmp/").is_some());
    }

    #[test]
    fn document_base_rules() {
        let dom = |h: &str| Html::parse_document(h);
        let d = dom(r#"<base href="/docs/">"#);
        assert_eq!(
            document_base(&d, base("https://ex.com/a"))
                .unwrap()
                .as_str(),
            "https://ex.com/docs/"
        );
        let d = dom(r#"<base href="https://cdn.ex/v2/">"#);
        assert_eq!(
            document_base(&d, None).unwrap().as_str(),
            "https://cdn.ex/v2/"
        );
        // A relative <base> without a caller base has nothing to resolve against.
        assert!(document_base(&dom(r#"<base href="/docs/">"#), None).is_none());
        // Page-supplied non-http(s) bases are refused; the caller base stays.
        for evil in ["javascript:alert(1)", "file:///etc/", "data:text/html,x"] {
            let d = dom(&format!(r#"<base href="{evil}">"#));
            assert_eq!(
                document_base(&d, base("https://ex.com/")).unwrap().as_str(),
                "https://ex.com/"
            );
        }
    }
}
