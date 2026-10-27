//! Plain text and Markdown → Markdown integration and security tests.

use markkit::format::Format;
use markkit::{DocumentHint, Error, Markkit};

fn extract(bytes: &[u8], hint: DocumentHint<'_>) -> markkit::Extraction {
    Markkit::builder()
        .build()
        .extract_document_sync(bytes, hint)
        .unwrap()
}

#[test]
fn plain_text_by_content_type_and_extension() {
    let e = extract(
        b"Hello\nworld\n\nSecond paragraph\n",
        DocumentHint::content_type("text/plain; charset=utf-8"),
    );
    assert_eq!(e.format, Format::PlainText);
    assert_eq!(e.markdown, "Hello  \nworld\n\nSecond paragraph\n");
    assert_eq!(e.stats.paragraphs, Some(2));
    assert_eq!(e.title, None);
    assert!(e.warnings.is_empty());

    let e = extract(b"notes", DocumentHint::file_name("dir/NOTES.TXT"));
    assert_eq!(e.format, Format::PlainText);
    assert_eq!(e.markdown, "notes\n");
}

#[test]
fn plain_text_crlf_and_blank_runs() {
    let e = extract(
        b"a\r\nb\r\n\r\n\r\n\r\nc   \r\n",
        DocumentHint::file_name("x.txt"),
    );
    assert_eq!(e.markdown, "a  \nb\n\nc\n");
}

#[test]
fn plain_text_empty() {
    let e = extract(b"", DocumentHint::file_name("empty.txt"));
    assert_eq!(e.markdown, "");
    assert_eq!(e.stats.paragraphs, Some(0));
}

#[cfg(feature = "markdown")]
#[test]
fn markdown_normalized_with_title() {
    let src = "\u{feff}---\ntitle: Guide\n---\n\n# Intro\n\nSome *text*\nwrapped.\n\n```rust\nfn main() {}\n```\n\n* item\n\n\n";
    let e = extract(src.as_bytes(), DocumentHint::file_name("guide.md"));
    assert_eq!(e.format, Format::Markdown);
    assert_eq!(e.title.as_deref(), Some("Guide"));
    assert_eq!(
        e.markdown,
        "# Intro\n\nSome *text* wrapped.\n\n```rust\nfn main() {}\n```\n\n- item\n"
    );
    assert_eq!(e.stats.tables, Some(0));
}

#[cfg(feature = "markdown")]
#[test]
fn markdown_title_from_heading() {
    for ct in ["text/markdown", "text/x-markdown; charset=utf-8"] {
        let e = extract(b"# Title\n\nbody", DocumentHint::content_type(ct));
        assert_eq!(e.format, Format::Markdown, "{ct}");
        assert_eq!(e.title.as_deref(), Some("Title"));
        assert_eq!(e.markdown, "# Title\n\nbody\n");
    }
}

#[cfg(not(feature = "markdown"))]
#[test]
fn markdown_requires_feature() {
    let m = Markkit::builder().build();
    let err = m
        .extract_document_sync(b"# x", DocumentHint::file_name("a.md"))
        .unwrap_err();
    assert!(matches!(err, Error::UnsupportedFormat(_)), "{err:?}");
}

#[test]
fn text_is_never_sniffed_without_hint() {
    let m = Markkit::builder().build();
    let err = m
        .extract_document_sync(b"# just text", DocumentHint::default())
        .unwrap_err();
    assert!(matches!(err, Error::UnsupportedFormat(_)), "{err:?}");
}

#[tokio::test]
async fn extract_bytes_text_plain() {
    let m = Markkit::builder().build();
    let md = m.extract_bytes(b"x\ny", Some("text/plain")).await;
    assert_eq!(md.unwrap(), "x  \ny\n");
}

// ─── Security (threat model T7) ─────────────────────────────────────

/// File names for every text-like format compiled in.
fn names() -> Vec<&'static str> {
    let mut v = vec!["a.txt"];
    if cfg!(feature = "markdown") {
        v.push("a.md");
    }
    v
}

fn has_control(s: &str) -> bool {
    s.chars()
        .any(|c| c.is_ascii_control() && c != '\n' && c != '\t')
}

#[test]
fn security_control_chars_stripped() {
    for name in names() {
        let e = extract(
            b"safe\x1b]0;pwned\x07\x00text\x7f\x08",
            DocumentHint::file_name(name),
        );
        assert!(!has_control(&e.markdown), "{name}: {:?}", e.markdown);
        assert_eq!(e.markdown, "safe\\]0;pwnedtext\n");
    }
}

#[test]
fn security_invalid_utf8_lossy_with_warning() {
    let e = extract(b"caf\xE9 \xC3", DocumentHint::file_name("a.txt"));
    assert_eq!(e.markdown, "caf\u{FFFD} \u{FFFD}\n");
    assert_eq!(e.warnings.len(), 1);
    assert!(e.warnings[0].starts_with("invalid_utf8:"));
}

#[test]
fn security_utf16_malformed_no_panic() {
    for bytes in [
        &b"\xFF\xFE"[..],
        b"\xFF\xFE\x41",
        b"\xFE\xFF\xD8\x00\xD8\x00",
        b"\xFF\xFE\x00\xDC",
    ] {
        let e = extract(bytes, DocumentHint::file_name("a.txt"));
        assert!(e.markdown.len() <= 16, "{bytes:?}");
    }
}

#[test]
fn security_input_size_limit() {
    let m = Markkit::builder().max_input_size(8).build();
    for name in ["a.txt", "a.md"] {
        let err = m
            .extract_document_sync(b"0123456789", DocumentHint::file_name(name))
            .unwrap_err();
        assert!(matches!(err, Error::ResourceLimit(_)), "{err:?}");
    }
}

#[test]
fn security_output_bounded_by_input() {
    // Each line becomes a hard break ("  \n" = +2 bytes per line): output
    // stays linear in input size.
    let input = "a\n".repeat(100_000);
    let e = extract(input.as_bytes(), DocumentHint::file_name("a.txt"));
    assert!(e.markdown.len() <= input.len() * 2);
    let blanks = "\n".repeat(1_000_000);
    for name in names() {
        let e = extract(blanks.as_bytes(), DocumentHint::file_name(name));
        assert_eq!(e.markdown, "", "{name}");
    }
}

#[test]
fn security_long_line() {
    let line = "x".repeat(2_000_000);
    for name in names() {
        let e = extract(line.as_bytes(), DocumentHint::file_name(name));
        assert_eq!(e.markdown.len(), line.len() + 1, "{name}");
    }
}

#[test]
fn security_arbitrary_bytes_never_panic() {
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    for _ in 0..300 {
        let len = (seed % 512) as usize;
        let bytes: Vec<u8> = (0..len)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                // Bias towards Markdown syntax so the parser is exercised.
                b"#*_`[]()!<>|-~\n \\x\x00\x1b\xff"[(seed % 21) as usize]
            })
            .collect();
        for name in names() {
            let e = extract(&bytes, DocumentHint::file_name(name));
            assert!(!has_control(&e.markdown), "{name}: {bytes:?}");
        }
    }
}

#[cfg(feature = "markdown")]
#[test]
fn security_markdown_raw_html_never_passes_through() {
    let src = b"<script>alert(1)</script>\n\n<div onclick=\"x()\">Hi <b>there</b></div>\n\npara <img src=x onerror=alert(1)> <span>kept</span>\n\n<style>p{}</style>\n";
    let e = extract(src, DocumentHint::file_name("a.md"));
    for bad in [
        "<script", "alert(1)", "onclick", "onerror", "<style", "<img", "<span", "<div",
    ] {
        assert!(!e.markdown.contains(bad), "{bad}: {:?}", e.markdown);
    }
    assert!(e.markdown.contains("kept"), "{:?}", e.markdown);
    #[cfg(feature = "html")]
    assert!(e.markdown.contains("Hi **there**"), "{:?}", e.markdown);
}

#[cfg(feature = "markdown")]
#[test]
fn security_markdown_deep_nesting_no_stack_overflow() {
    for src in [
        format!("{}x", "> ".repeat(100_000)),
        format!("{}x", "- ".repeat(50_000)),
        format!("{}x{}", "*".repeat(50_000), "*".repeat(50_000)),
        format!("{}x{}", "[".repeat(50_000), "](u)".repeat(50_000)),
    ] {
        let e = extract(src.as_bytes(), DocumentHint::file_name("a.md"));
        assert!(e.markdown.contains('x'));
    }
}

#[test]
fn security_text_and_markdown_share_limits() {
    use std::time::Duration;
    let m = Markkit::builder().timeout(Duration::ZERO).build();
    for name in ["a.txt", "a.md"] {
        let r = m.extract_document_sync(b"x", DocumentHint::file_name(name));
        if cfg!(not(feature = "markdown")) && name == "a.md" {
            continue;
        }
        assert!(
            matches!(&r, Err(Error::ResourceLimit(msg)) if msg.contains("timeout")),
            "{name}: {r:?}"
        );
    }
    let m = Markkit::builder().max_nodes(100).build();
    let big = "line\n".repeat(1000);
    for name in ["a.txt", "a.md"] {
        if cfg!(not(feature = "markdown")) && name == "a.md" {
            continue;
        }
        let r = m.extract_document_sync(big.as_bytes(), DocumentHint::file_name(name));
        let r2 = m.extract_document_sync(
            "p\n\n".repeat(1000).as_bytes(),
            DocumentHint::file_name(name),
        );
        assert!(
            matches!(&r, Err(Error::ResourceLimit(_)))
                || matches!(&r2, Err(Error::ResourceLimit(_))),
            "{name}: {r:?}"
        );
    }
}

#[test]
fn security_plain_text_markup_is_escaped() {
    let e = extract(
        b"# not heading\n<img src=x onerror=alert(1)>",
        DocumentHint::file_name("a.txt"),
    );
    assert!(e.markdown.contains("\\<img"), "{:?}", e.markdown);
    assert!(e.markdown.starts_with("\\#"), "{:?}", e.markdown);
}
