//! Output-safety escaping for the Markdown renderer (threat model T1).
//!
//! Decisions:
//! - Escaping happens only here, at render time. The IR always holds raw text, so JSON
//!   IR consumers never see Markdown backslashes.
//! - Text is escaped so that a CommonMark/GFM renderer shows it verbatim: no raw HTML,
//!   no emphasis, links, code or block structure can come from document text.
//!   Characters that are inert in context stay unescaped to keep output readable for
//!   people and LLMs (`snake_case`, `C:\dir`, mid-line `#`).
//! - Link and image destinations are filtered by scheme allowlist: `http`, `https`,
//!   `mailto`, `tel`, `ftp` and scheme-less (relative, `#fragment`) URLs for links;
//!   `http`, `https`, relative and raster `data:image/*` for images. Anything else is
//!   dropped and its text (link children, image alt) kept. Scheme detection ignores
//!   ASCII whitespace and control characters anywhere, as browsers do.
//! - Destinations are emitted bare with `( ) < > \ space` percent-encoded and entity-like
//!   `&` backslash-escaped, so a destination can neither end early nor decode into
//!   another scheme (`javascript&colon;`).
//! - Fences are one backtick longer than the longest backtick run in the code.

/// Where a text run starts, for block-marker escaping.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Position {
    LineStart,
    Inline,
}

/// Escape document text for inline Markdown. Returns whether the output still sits at
/// a line start (the run was empty or whitespace only).
pub(crate) fn push_text(buf: &mut String, s: &str, pos: Position) -> bool {
    // Fast path: most runs hold nothing escapable, so copy them without the
    // per-char buffer. Must agree with `push_text_slow` (tested).
    let inert = !s.bytes().any(|b| {
        matches!(
            b,
            b'`' | b'*'
                | b'['
                | b']'
                | b'<'
                | b'~'
                | b'|'
                | b'\\'
                | b'!'
                | b'_'
                | b'&'
                | b'\n'
                | b'\r'
        )
    });
    if inert {
        let lead = s.bytes().find(|b| !matches!(b, b' ' | b'\t'));
        if pos == Position::Inline {
            buf.push_str(s);
            return false;
        }
        match lead {
            None => {
                buf.push_str(s);
                return true;
            }
            Some(b) if !matches!(b, b'#' | b'>' | b'-' | b'+' | b'=' | b'0'..=b'9') => {
                buf.push_str(s);
                return false;
            }
            Some(_) => {}
        }
    }
    push_text_slow(buf, s, pos)
}

fn push_text_slow(buf: &mut String, s: &str, pos: Position) -> bool {
    let chars: Vec<char> = s
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    let mut line_start = pos == Position::LineStart;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if line_start {
            if c == ' ' || c == '\t' {
                buf.push(c);
                i += 1;
                continue;
            }
            line_start = false;
            // A marker only opens a block when followed by whitespace. At the end of
            // the run the next node may supply it, so escape there too.
            let opens = |at: usize| chars.get(at).is_none_or(|n| *n == ' ' || *n == '\t');
            match c {
                // `-` (thematic break, setext), `=` (setext), `>` (quote): always.
                '>' | '-' | '=' => {
                    buf.push('\\');
                    buf.push(c);
                    i += 1;
                    continue;
                }
                '#' | '+' => {
                    let run = chars[i..].iter().take_while(|d| **d == c).count();
                    if c == '#' && opens(i + run) || c == '+' && opens(i + 1) {
                        buf.push('\\');
                        buf.push(c);
                        i += 1;
                        continue;
                    }
                }
                '0'..='9' => {
                    // Ordered list marker: up to 9 digits, `.` or `)`, then whitespace.
                    let digits = chars[i..].iter().take_while(|d| d.is_ascii_digit()).count();
                    if digits <= 9
                        && matches!(chars.get(i + digits), Some('.') | Some(')'))
                        && opens(i + digits + 1)
                    {
                        buf.extend(&chars[i..i + digits]);
                        buf.push('\\');
                        buf.push(chars[i + digits]);
                        i += digits + 1;
                        continue;
                    }
                }
                _ => {}
            }
        }
        let prev = if i == 0 { None } else { Some(chars[i - 1]) };
        let next = chars.get(i + 1).copied();
        let escape = match c {
            '`' | '*' | '[' | ']' | '<' | '~' | '|' => true,
            // A trailing `\` or `!` could combine with the next node (`\**`, `![`).
            '\\' | '!' => next.is_none_or(|n| n.is_ascii_punctuation()),
            // Intraword `_` cannot open or close emphasis.
            '_' => {
                !(prev.is_some_and(char::is_alphanumeric)
                    && next.is_some_and(char::is_alphanumeric))
            }
            // Entity or numeric character reference (or the start of one in the next node).
            '&' => next.is_none() || entity_like(&chars[i..]),
            _ => false,
        };
        if escape {
            buf.push('\\');
        }
        buf.push(c);
        i += 1;
    }
    line_start
}

/// Push an inline code span that renders `code` verbatim.
pub(crate) fn push_code_span(buf: &mut String, code: &str, in_table: bool) {
    let code: String = code
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    if code.is_empty() {
        return;
    }
    let fence = "`".repeat(longest_run(&code, '`') + 1);
    // A backtick right before the fence would lengthen it and unbalance the span.
    if buf.ends_with('`') {
        buf.push(' ');
    }
    let pad = code.starts_with('`')
        || code.ends_with('`')
        || (code.starts_with(' ') && code.ends_with(' ') && !code.trim().is_empty());
    buf.push_str(&fence);
    if pad {
        buf.push(' ');
    }
    if in_table {
        buf.push_str(&code.replace('|', "\\|"));
    } else {
        buf.push_str(&code);
    }
    if pad {
        buf.push(' ');
    }
    buf.push_str(&fence);
}

/// Opening/closing fence for a fenced code block holding `code`.
pub(crate) fn code_fence(code: &str) -> String {
    "`".repeat((longest_run(code, '`') + 1).max(3))
}

/// Info string for a fenced code block: first word, no backticks or control chars.
pub(crate) fn info_string(lang: &str) -> String {
    lang.split(|c: char| c.is_whitespace())
        .find(|w| !w.is_empty())
        .unwrap_or("")
        .chars()
        .filter(|c| *c != '`' && !c.is_control())
        .collect()
}

/// `&name;`, `&#123;` or `&#x1F;` at the start of `s` (which begins with `&`).
fn entity_like(s: &[char]) -> bool {
    let rest = &s[1..];
    let (body, max): (&[char], usize) = match rest {
        ['#', 'x' | 'X', tail @ ..] => (tail, 6),
        ['#', tail @ ..] => (tail, 7),
        tail => (tail, 32),
    };
    let hex = matches!(rest, ['#', 'x' | 'X', ..]);
    let numeric = matches!(rest, ['#', ..]);
    let n = body
        .iter()
        .take_while(|c| {
            if hex {
                c.is_ascii_hexdigit()
            } else if numeric {
                c.is_ascii_digit()
            } else {
                c.is_ascii_alphanumeric()
            }
        })
        .count();
    n >= 1 && n <= max && body.get(n) == Some(&';')
}

fn longest_run(s: &str, ch: char) -> usize {
    let (mut best, mut cur) = (0, 0);
    for c in s.chars() {
        if c == ch {
            cur += 1;
            best = best.max(cur);
        } else {
            cur = 0;
        }
    }
    best
}

/// Kind of destination, which selects the scheme allowlist.
#[derive(Clone, Copy)]
pub(crate) enum UrlKind {
    Link,
    Image,
}

/// Return a Markdown-safe destination for `url`, or `None` when its scheme is not
/// allowed and the link/image must be dropped.
pub(crate) fn safe_destination(url: &str, kind: UrlKind) -> Option<String> {
    let cleaned: String = url.chars().filter(|c| !c.is_ascii_control()).collect();
    let cleaned = cleaned.trim_matches(|c: char| c.is_ascii_whitespace());
    if !scheme_allowed(cleaned, kind) {
        return None;
    }
    let mut out = String::with_capacity(cleaned.len());
    let chars: Vec<char> = cleaned.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        match c {
            ' ' => out.push_str("%20"),
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            '\\' => out.push_str("%5C"),
            '&' if entity_like(&chars[i..]) => out.push_str("\\&"),
            c if c.is_whitespace() => {
                let mut tmp = [0u8; 4];
                for b in c.encode_utf8(&mut tmp).bytes() {
                    out.push_str(&format!("%{b:02X}"));
                }
            }
            c => out.push(c),
        }
    }
    if out.is_empty() {
        // A bare empty destination would let a following title parse as the URL.
        out.push_str("<>");
    }
    Some(out)
}

fn scheme_allowed(url: &str, kind: UrlKind) -> bool {
    // Browsers ignore tabs/newlines anywhere in a URL; so does scheme detection.
    // The whole scheme is read (no length cap: a capped probe let long schemes
    // pass as relative URLs).
    let mut chars = url
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .map(|c| c.to_ascii_lowercase());
    let mut scheme = String::new();
    loop {
        match chars.next() {
            Some(':') => break,
            Some('/' | '?' | '#') | None => return true, // relative URL, fragment, or path
            Some(c) => scheme.push(c),
        }
    }
    let scheme = scheme.as_str();
    match kind {
        UrlKind::Link => matches!(scheme, "http" | "https" | "mailto" | "tel" | "ftp"),
        UrlKind::Image => match scheme {
            "http" | "https" => true,
            "data" => {
                let mime: String = chars
                    .take_while(|c| *c != ';' && *c != ',')
                    .take(32)
                    .collect();
                let mime = mime.as_str();
                matches!(
                    mime,
                    "image/png"
                        | "image/jpeg"
                        | "image/jpg"
                        | "image/gif"
                        | "image/webp"
                        | "image/avif"
                        | "image/bmp"
                )
            }
            _ => false,
        },
    }
}

/// Escape a link title for use inside `"..."`.
pub(crate) fn link_title(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    for c in t.chars() {
        match c {
            // `<` too: if the link ever fails to parse, the title must not
            // become raw HTML.
            '"' | '\\' | '<' => {
                out.push('\\');
                out.push(c);
            }
            '\n' | '\r' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn esc(s: &str, pos: Position) -> String {
        let mut b = String::new();
        push_text(&mut b, s, pos);
        b
    }

    #[test]
    fn fast_path_matches_slow_path() {
        let samples = [
            "",
            " ",
            "\t ",
            "plain words",
            "  lead",
            "# h",
            "  # h",
            "- x",
            "+x",
            "=",
            ">q",
            "1. one",
            "12) x",
            "1x",
            "a1. b",
            "é # ü",
            "x\ny",
            "\r",
            "a_b",
            "C:\\dir",
            "!",
            "&",
            "trail ",
            "  9",
            "\u{a0}-",
        ];
        for s in samples {
            for pos in [Position::LineStart, Position::Inline] {
                let (mut fast, mut slow) = (String::new(), String::new());
                let a = push_text(&mut fast, s, pos);
                let b = push_text_slow(&mut slow, s, pos);
                assert_eq!((fast, a), (slow, b), "{s:?}");
            }
        }
    }

    #[test]
    fn escapes_inline_syntax() {
        assert_eq!(
            esc("*a* `b` [c] <d> ~e~ |f|", Position::Inline),
            r"\*a\* \`b\` \[c\] \<d> \~e\~ \|f\|"
        );
        assert_eq!(esc("a_b _c_", Position::Inline), r"a_b \_c\_");
        assert_eq!(
            esc("&amp; & x &#60; &#x3c; a&b", Position::Inline),
            r"\&amp; & x \&#60; \&#x3c; a&b"
        );
        assert_eq!(esc("tail&", Position::Inline), r"tail\&");
        assert_eq!(esc("C:\\dir\\x", Position::Inline), r"C:\dir\x");
        assert_eq!(esc("end\\", Position::Inline), r"end\\");
        assert_eq!(esc("hi!", Position::Inline), r"hi\!");
        assert_eq!(esc("hi! yes", Position::Inline), "hi! yes");
    }

    #[test]
    fn escapes_line_start_markers() {
        assert_eq!(esc("# h", Position::LineStart), r"\# h");
        assert_eq!(esc("  - x", Position::LineStart), r"  \- x");
        assert_eq!(esc("12. x", Position::LineStart), r"12\. x");
        assert_eq!(esc("1234567890. x", Position::LineStart), "1234567890. x");
        assert_eq!(esc("# h", Position::Inline), "# h");
    }

    #[test]
    fn whitespace_only_stays_at_line_start() {
        let mut b = String::new();
        assert!(push_text(&mut b, "  ", Position::LineStart));
        assert!(!push_text(&mut b, "x", Position::LineStart));
    }

    #[test]
    fn fences() {
        assert_eq!(code_fence("a"), "```");
        assert_eq!(code_fence("````"), "`````");
        assert_eq!(info_string(" rust extra"), "rust");
        assert_eq!(info_string("js```"), "js");
        let mut b = String::new();
        push_code_span(&mut b, "a`b", false);
        assert_eq!(b, "``a`b``");
        b.clear();
        push_code_span(&mut b, "`", false);
        assert_eq!(b, "`` ` ``");
    }

    #[test]
    fn destinations() {
        assert_eq!(safe_destination("javascript:x", UrlKind::Link), None);
        assert_eq!(safe_destination("JAVA\tSCRIPT:x", UrlKind::Link), None);
        assert_eq!(safe_destination("\u{0}javascript:x", UrlKind::Link), None);
        assert_eq!(safe_destination("data:text/html,x", UrlKind::Image), None);
        assert_eq!(
            safe_destination("data:image/svg+xml,x", UrlKind::Image),
            None
        );
        assert_eq!(safe_destination("mailto:a@b", UrlKind::Image), None);
        assert_eq!(safe_destination("a:b/c", UrlKind::Link), None);
        assert_eq!(
            safe_destination("./a:b", UrlKind::Link).as_deref(),
            Some("./a:b")
        );
        assert_eq!(
            safe_destination("/x?y=a:b", UrlKind::Link).as_deref(),
            Some("/x?y=a:b")
        );
        assert_eq!(
            safe_destination("http://e/a b(c)", UrlKind::Link).as_deref(),
            Some("http://e/a%20b%28c%29")
        );
        assert_eq!(
            safe_destination("http://e/?a=1&b=2&colon;", UrlKind::Link).as_deref(),
            Some("http://e/?a=1&b=2\\&colon;")
        );
    }

    #[test]
    fn titles() {
        assert_eq!(link_title("a \"b\" \\c <d>"), r#"a \"b\" \\c \<d>"#);
    }
}
