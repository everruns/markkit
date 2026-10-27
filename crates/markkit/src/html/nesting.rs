//! Nesting cap applied to raw HTML before html5ever sees it.
//!
//! Decisions:
//! - html5ever's tree builder is quadratic in nesting depth: most start tags
//!   scan the stack of open elements ("has a `p` in button scope", table
//!   context resets), so N nested `<div>`s cost O(N²). Measured before this
//!   cap: 687 KiB of nested divs took 12.6 s, 292 KiB of nested tables passed
//!   the 60 s timeout, and html5ever cannot be interrupted. THREAT[T2c].
//! - The walker flattens everything deeper than `max_nesting_depth` to text
//!   anyway, so tags past the cap carry no structure we keep. This pass drops
//!   start tags that would open past `cap` (and as many following end tags),
//!   keeping all text. The tree builder's stack then stays near `cap`, and
//!   cost is O(input × cap): linear in input.
//! - Approximate HTML tokenizing, single pass, bounded state: comments,
//!   `<!...>`/`<?...>`, quoted attribute values and raw-text elements
//!   (`script`, `style`, ...) are skipped so `<` inside them never counts.
//!   Void elements and elements with optional end tags (`p`, `li`, `td`, ...)
//!   do not count as nesting, except table structure, which closes implicitly
//!   the way the HTML spec does. `/>` self-closes only inside `svg`/`math`.
//!   Under-counting only lets more depth through (bounded by `cap` per
//!   counted level); the input is returned unchanged unless the cap is hit.
//! - The stack holds at most `cap` names, so end-tag matching is O(cap).

use std::borrow::Cow;

/// Elements whose content never reaches the output (the walker skips them).
pub(crate) const NON_CONTENT: &[&str] = &[
    "script", "style", "noscript", "iframe", "object", "embed", "form", "input", "button",
    "select", "textarea", "svg", "canvas", "video", "audio", "source", "track", "map", "area",
    "template",
];

const FOREIGN: &[&str] = &["svg", "math"];

/// Elements whose content is raw text up to their end tag.
const RAW_TEXT: &[&str] = &[
    "script", "style", "textarea", "title", "xmp", "iframe", "noembed", "noframes", "noscript",
];

/// Elements that never have content.
const VOID: &[&str] = &[
    "area", "base", "basefont", "bgsound", "br", "col", "embed", "frame", "hr", "image", "img",
    "input", "keygen", "link", "meta", "param", "source", "track", "wbr",
];

/// Optional end tag, cannot nest in itself: not counted.
const UNCOUNTED: &[&str] = &[
    "p", "li", "dt", "dd", "option", "optgroup", "rb", "rt", "rtc", "rp", "html", "head", "body",
    "colgroup", "caption",
];

/// Drop start tags that would nest deeper than `cap` (and matching end tags).
/// Returns the input unchanged when the cap is never reached.
pub(crate) fn cap_nesting(html: &str, cap: usize) -> Cow<'_, str> {
    let b = html.as_bytes();
    let mut stack: Vec<&[u8]> = Vec::new();
    let mut foreign = 0usize; // open svg/math elements on the stack
    let mut overflow = 0usize; // dropped start tags awaiting an end tag
    let mut out: Option<String> = None;
    let mut copied = 0usize; // input[..copied] already in `out`
    let mut i = 0usize;

    let drop_range = |out: &mut Option<String>, copied: &mut usize, start: usize, end: usize| {
        let o = out.get_or_insert_with(|| String::with_capacity(html.len()));
        o.push_str(&html[*copied..start]);
        *copied = end;
    };

    while let Some(off) = memchr(b'<', &b[i..]) {
        let lt = i + off;
        let rest = &b[lt + 1..];
        if rest.starts_with(b"!--") {
            i = find(b, lt + 4, b"-->").map_or(b.len(), |p| p + 3);
            continue;
        }
        if matches!(rest.first(), Some(b'!' | b'?')) {
            i = memchr(b'>', &b[lt..]).map_or(b.len(), |p| lt + p + 1);
            continue;
        }
        let (is_end, name_start) = match rest.first() {
            Some(b'/') => (true, lt + 2),
            _ => (false, lt + 1),
        };
        if !b.get(name_start).is_some_and(u8::is_ascii_alphabetic) {
            i = lt + 1;
            continue;
        }
        let name_end = name_start
            + b[name_start..]
                .iter()
                .position(|c| c.is_ascii_whitespace() || *c == b'/' || *c == b'>')
                .unwrap_or(b.len() - name_start);
        let name = &b[name_start..name_end];
        let (end, self_closing) = tag_end(b, name_end);
        i = end;

        if is_end {
            let counted = !one_of(name, VOID) && !one_of(name, UNCOUNTED);
            if overflow > 0 {
                // Inside the dropped region every end tag goes; only counted
                // ones pair with a dropped start tag.
                overflow -= usize::from(counted);
                drop_range(&mut out, &mut copied, lt, end);
            } else if let Some(pos) = stack.iter().rposition(|n| n.eq_ignore_ascii_case(name)) {
                foreign -= stack[pos..].iter().filter(|n| one_of(n, FOREIGN)).count();
                stack.truncate(pos);
            }
            continue;
        }

        if one_of(name, RAW_TEXT) && foreign == 0 {
            // Skip the content and the end tag itself: it must reach
            // html5ever even while dropping, or the raw text never ends.
            i = find_end_tag(b, i, name).map_or(b.len(), |p| tag_end(b, p + 2 + name.len()).0);
            continue;
        }
        if name.eq_ignore_ascii_case(b"plaintext") && foreign == 0 {
            break;
        }
        let void = one_of(name, VOID) || (self_closing && (foreign > 0 || one_of(name, FOREIGN)));
        let uncounted = void || one_of(name, UNCOUNTED);
        if overflow == 0 {
            if uncounted {
                continue;
            }
            // Table structure closes implicitly (spec "clear the stack back to
            // a table / row context"); pop to the enclosing level first.
            let closes: &[&str] = if one_of(name, &["tbody", "thead", "tfoot"]) {
                &["tbody", "thead", "tfoot", "tr", "td", "th"]
            } else if one_of(name, &["tr"]) {
                &["tr", "td", "th"]
            } else if one_of(name, &["td", "th"]) {
                &["td", "th"]
            } else {
                &[]
            };
            while stack.last().is_some_and(|n| one_of(n, closes)) {
                stack.pop();
            }
        }
        if overflow > 0 || stack.len() >= cap {
            // Past the cap every start tag goes (the walker would flatten it
            // anyway), so html5ever's stack stays at the cap.
            let mut drop_to = end;
            if !void && one_of(name, NON_CONTENT) {
                // Dropping only the tag would expose content the walker
                // skips; drop the whole subtree instead (same output).
                drop_to = skip_subtree(b, end, name);
                i = drop_to;
            } else if !uncounted {
                overflow += 1;
            }
            drop_range(&mut out, &mut copied, lt, drop_to);
            continue;
        }
        if one_of(name, FOREIGN) {
            foreign += 1;
        }
        stack.push(name);
    }

    match out {
        None => Cow::Borrowed(html),
        Some(mut o) => {
            o.push_str(&html[copied..]);
            Cow::Owned(o)
        }
    }
}

/// Index just past the `>` closing a tag whose attributes start at `from`,
/// skipping quoted values; and whether it ends in `/>`.
fn tag_end(b: &[u8], from: usize) -> (usize, bool) {
    let mut i = from;
    let mut quote = None;
    let mut prev = 0u8;
    while i < b.len() {
        let c = b[i];
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == b'"' || c == b'\'' => quote = Some(c),
            None if c == b'>' => return (i + 1, prev == b'/'),
            None => {}
        }
        if !c.is_ascii_whitespace() {
            prev = c;
        }
        i += 1;
    }
    (b.len(), false)
}

/// Index past the end tag matching an open `name` element whose content
/// starts at `from`, counting same-name nesting; input end if unclosed.
fn skip_subtree(b: &[u8], from: usize, name: &[u8]) -> usize {
    let mut depth = 1usize;
    let mut i = from;
    while let Some(off) = memchr(b'<', &b[i..]) {
        let lt = i + off;
        let (is_end, start) = match b.get(lt + 1) {
            Some(b'/') => (true, lt + 2),
            _ => (false, lt + 1),
        };
        let after = start + name.len();
        let matches = b.len() > after
            && b[start..after].eq_ignore_ascii_case(name)
            && (b[after].is_ascii_whitespace() || b[after] == b'/' || b[after] == b'>');
        if !matches {
            i = lt + 1;
            continue;
        }
        let (end, self_closing) = tag_end(b, after);
        i = end;
        if is_end {
            depth -= 1;
            if depth == 0 {
                return end;
            }
        } else if !self_closing {
            depth += 1;
        }
    }
    b.len()
}

/// Start of `</name` (ASCII case-insensitive) at or after `from`.
fn find_end_tag(b: &[u8], from: usize, name: &[u8]) -> Option<usize> {
    let mut i = from;
    while let Some(off) = memchr(b'<', &b[i..]) {
        let p = i + off;
        let tail = &b[p + 1..];
        if tail.first() == Some(&b'/')
            && tail.len() > name.len()
            && tail[1..=name.len()].eq_ignore_ascii_case(name)
        {
            return Some(p);
        }
        i = p + 1;
    }
    None
}

/// ASCII case-insensitive membership; no allocation per tag.
fn one_of(name: &[u8], list: &[&str]) -> bool {
    list.iter().any(|n| n.as_bytes().eq_ignore_ascii_case(name))
}

fn find(b: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    b.get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| from + p)
}

fn memchr(c: u8, b: &[u8]) -> Option<usize> {
    b.iter().position(|&x| x == c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn depth_of(html: &str) -> usize {
        // Max element depth of html5ever's DOM, iteratively.
        let dom = scraper::Html::parse_document(html);
        let mut max = 0;
        let mut stack = vec![(dom.tree.root(), 0usize)];
        while let Some((n, d)) = stack.pop() {
            max = max.max(d);
            stack.extend(n.children().map(|c| (c, d + 1)));
        }
        max
    }

    #[test]
    fn shallow_input_is_borrowed_unchanged() {
        let html = "<html><body><div><p>a<p>b<ul><li>x<li>y</ul><table><tr><td>1<td>2<tr><td>3</table></div></body></html>";
        assert!(matches!(cap_nesting(html, 8), Cow::Borrowed(_)));
    }

    #[test]
    fn deep_divs_are_capped_and_text_kept() {
        let html = format!("{}deep text{}", "<div>".repeat(5000), "</div>".repeat(5000));
        let capped = cap_nesting(&html, 100);
        assert!(capped.contains("deep text"));
        assert_eq!(capped.matches("<div>").count(), 100);
        assert_eq!(capped.matches("</div>").count(), 100);
        assert!(depth_of(&capped) <= 105);
    }

    #[test]
    fn unclosed_divs_are_capped() {
        let html = "<div>x".repeat(3000);
        let capped = cap_nesting(&html, 50);
        assert_eq!(capped.matches("<div>").count(), 50);
        assert_eq!(capped.matches('x').count(), 3000);
    }

    #[test]
    fn nested_tables_are_capped() {
        let html = "<table><tr><td>".repeat(3000);
        let capped = cap_nesting(&html, 60);
        assert!(depth_of(&capped) < 300);
    }

    #[test]
    fn self_closing_div_still_counts_outside_svg() {
        // html5ever opens `<div/>`; only foreign content self-closes.
        let html = "<div/>".repeat(1000);
        let capped = cap_nesting(&html, 10);
        assert_eq!(capped.matches("<div/>").count(), 10);
        let svg = format!("<svg>{}</svg>", "<g/>".repeat(1000));
        assert!(matches!(cap_nesting(&svg, 10), Cow::Borrowed(_)));
    }

    #[test]
    fn markup_in_comments_attributes_and_raw_text_is_ignored() {
        let divs = "<div>".repeat(100);
        let html = format!(
            "<!-- {divs} --><a title='{divs}' href=\"x>y\">a</a><script>{divs}</script><style>{divs}</style><textarea>{divs}</TEXTAREA>"
        );
        assert!(matches!(cap_nesting(&html, 5), Cow::Borrowed(_)));
    }

    #[test]
    fn optional_end_tags_and_voids_do_not_count() {
        let html = "<p>a<li>b<br><img src=x><td>c<option>d".repeat(1000);
        assert!(matches!(cap_nesting(&html, 5), Cow::Borrowed(_)));
    }

    #[test]
    fn implied_close_by_ancestor_end_tag() {
        // `</div>` closes the unclosed spans inside it; depth must not drift.
        let html = "<div><span><b>x</div>".repeat(1000);
        assert!(matches!(cap_nesting(&html, 10), Cow::Borrowed(_)));
    }

    #[test]
    fn raw_text_end_tags_survive_while_dropping() {
        // Dropping `</script>` would turn the rest of the page into script.
        let html = format!(
            "{}<script>x()</script><style>.a{{}}</style><p>after</p>{}",
            "<div>".repeat(50),
            "</div>".repeat(50)
        );
        let capped = cap_nesting(&html, 10);
        assert!(capped.contains("<script>x()</script><style>.a{}</style>"));
        assert!(capped.contains("after"));
    }

    #[test]
    fn uncounted_end_tags_do_not_consume_overflow() {
        let html = format!(
            "{}<p>a</p><li>b</li>{}",
            "<div>".repeat(20),
            "</div>".repeat(20)
        );
        let capped = cap_nesting(&html, 10);
        // Past the cap the p/li tags go too, their text stays, and their end
        // tags do not close dropped divs.
        assert!(capped.contains("ab"));
        assert_eq!(capped.matches("<div>").count(), 10);
        assert_eq!(capped.matches("</div>").count(), 10);
    }

    #[test]
    fn non_content_past_the_cap_is_dropped_whole() {
        let html = format!(
            "{}<template>tpl<template>in</template></template><form><b>f</b></form><p>after</p>",
            "<div>".repeat(20)
        );
        let capped = cap_nesting(&html, 10);
        assert!(!capped.contains("tpl") && !capped.contains("in<") && !capped.contains("f</b>"));
        assert!(capped.contains("after"));
        let objects = "<object>".repeat(100_000) + "x";
        assert!(!cap_nesting(&objects, 10).contains('x'));
    }

    #[test]
    fn malformed_tails_do_not_panic() {
        for s in [
            "<", "</", "<div", "<div a='", "<!--", "<!", "<script>", "</>", "<1>", "<\u{e9}>",
        ] {
            let _ = cap_nesting(s, 1);
            let _ = cap_nesting(&format!("<div>{s}"), 0);
        }
    }
}
