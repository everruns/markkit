//! Custom FormatParser registry.

use markkit::ir::{Document, Node, TableCell};
use markkit::registry::FormatParser;
use markkit::{DocumentHint, Error, Format, Markkit, MarkkitBuilder, ParsedDocument};

/// Naive CSV: comma-split lines, first line is the header (replaces the built-in).
struct Csv;

impl FormatParser for Csv {
    fn name(&self) -> &str {
        "csv"
    }
    fn formats(&self) -> &[Format] {
        &[Format::Csv]
    }
    fn parse(&self, bytes: &[u8], _: &MarkkitBuilder) -> markkit::Result<ParsedDocument> {
        let text = String::from_utf8_lossy(bytes);
        let mut rows = text.lines().map(|l| {
            l.split(',')
                .map(|c| TableCell::new(vec![Node::Text(c.trim().into())]))
                .collect::<Vec<_>>()
        });
        let headers = rows.next().unwrap_or_default();
        let table = Node::Table {
            alignments: vec![],
            headers,
            rows: rows.collect(),
            source: None,
        };
        Ok(ParsedDocument::new(Document::new(vec![table]), Format::Csv))
    }
}

/// Claims inputs starting with a magic prefix.
struct Magic;

/// Replaces the built-in HTML parser.
struct PlainHtml;

impl FormatParser for PlainHtml {
    fn name(&self) -> &str {
        "plain-html"
    }
    fn formats(&self) -> &[Format] {
        &[Format::Html]
    }
    fn parse(&self, bytes: &[u8], _: &MarkkitBuilder) -> markkit::Result<ParsedDocument> {
        let text = String::from_utf8_lossy(bytes).into_owned();
        Ok(ParsedDocument::new(
            Document::new(vec![Node::CodeBlock {
                language: Some("html".into()),
                code: text,
                source: None,
            }]),
            Format::Html,
        ))
    }
}

impl FormatParser for Magic {
    fn name(&self) -> &str {
        "magic"
    }
    fn claims(&self, bytes: &[u8], _: DocumentHint<'_>) -> bool {
        bytes.starts_with(b"MAGIC1\n")
    }
    fn parse(&self, bytes: &[u8], _: &MarkkitBuilder) -> markkit::Result<ParsedDocument> {
        let body = String::from_utf8_lossy(&bytes[7..]).into_owned();
        Ok(ParsedDocument::new(
            Document::new(vec![Node::heading(3, vec![Node::Text(body)])]),
            Format::Unknown,
        ))
    }
}

struct Panics;

impl FormatParser for Panics {
    fn name(&self) -> &str {
        "panics"
    }
    fn formats(&self) -> &[Format] {
        &[Format::Markdown]
    }
    fn parse(&self, _: &[u8], _: &MarkkitBuilder) -> markkit::Result<ParsedDocument> {
        panic!("boom")
    }
}

#[test]
fn custom_parser_for_detected_format() {
    let m = Markkit::builder().parser(Csv).build();
    let e = m
        .extract_document_sync(b"a,b\n1,2\n", DocumentHint::file_name("data.csv"))
        .unwrap();
    assert_eq!(e.format, Format::Csv);
    assert_eq!(e.markdown, "| a | b |\n| --- | --- |\n| 1 | 2 |\n");
}

#[test]
fn unregistered_format_still_unsupported() {
    // CSV parser registered; PPTX has no built-in and no custom parser.
    let m = Markkit::builder().parser(Csv).build();
    let err = m
        .extract_document_sync(b"PK\x03\x04", DocumentHint::file_name("deck.pptx"))
        .unwrap_err();
    assert!(matches!(err, Error::UnsupportedFormat(_)), "{err}");
}

#[test]
fn claims_run_before_detection_and_transforms_apply() {
    let m = Markkit::builder()
        .parser(Csv)
        .parser(Magic)
        .transform(markkit::transform::NormalizeHeadings)
        .build();
    // A .csv name, but Magic claims the bytes first.
    let e = m
        .extract_document_sync(b"MAGIC1\nhello", DocumentHint::file_name("x.csv"))
        .unwrap();
    assert_eq!(e.markdown, "# hello\n");
    assert_eq!(format!("{:?}", m.config().parsers), r#"["csv", "magic"]"#);
}

#[cfg(feature = "html")]
#[test]
fn builtin_runs_when_nothing_claims() {
    let m = Markkit::builder().parser(Magic).build();
    // Without a claim the built-in HTML parser still runs.
    let e = m
        .extract_document_sync(b"<p>x</p>", DocumentHint::content_type("text/html"))
        .unwrap();
    assert_eq!(e.format, Format::Html);
}

#[test]
fn panicking_parser_is_an_error() {
    let m = Markkit::builder().parser(Panics).build();
    let err = m
        .extract_document_sync(b"# x", DocumentHint::file_name("a.md"))
        .unwrap_err();
    assert!(
        matches!(err, Error::Parser(ref s) if s.contains("panics")),
        "{err}"
    );
}

#[test]
fn size_limit_checked_before_custom_parser() {
    let m = Markkit::builder().max_input_size(4).parser(Csv).build();
    let err = m
        .extract_document_sync(b"a,b,c,d", DocumentHint::file_name("x.csv"))
        .unwrap_err();
    assert!(matches!(err, Error::ResourceLimit(_)), "{err}");
}

#[test]
fn custom_parser_replaces_builtin() {
    let m = Markkit::builder().parser(PlainHtml).build();
    let e = m
        .extract_document_sync(b"<p>x</p>", DocumentHint::content_type("text/html"))
        .unwrap();
    assert_eq!(e.markdown, "```html\n<p>x</p>\n```\n");
}

/// Sleeps past any small deadline.
struct Slow;

impl FormatParser for Slow {
    fn name(&self) -> &str {
        "slow"
    }
    fn formats(&self) -> &[Format] {
        &[Format::Csv]
    }
    fn parse(&self, _: &[u8], _: &MarkkitBuilder) -> markkit::Result<ParsedDocument> {
        std::thread::sleep(std::time::Duration::from_millis(50));
        Ok(ParsedDocument::new(Document::new(vec![]), Format::Csv))
    }
}

#[test]
fn deadline_checked_after_custom_parser() {
    let limits = markkit::Limits::default().timeout(std::time::Duration::from_millis(1));
    let m = Markkit::builder().limits(limits).parser(Slow).build();
    let err = m
        .extract_document_sync(b"a", DocumentHint::file_name("x.csv"))
        .unwrap_err();
    assert!(matches!(err, Error::ResourceLimit(_)), "{err}");
}

#[test]
fn output_limit_applies_to_custom_parser() {
    let limits = markkit::Limits::default().max_output_size(8);
    let m = Markkit::builder().limits(limits).parser(Csv).build();
    let err = m
        .extract_document_sync(b"aaaa,bbbb\ncccc,dddd", DocumentHint::file_name("x.csv"))
        .unwrap_err();
    assert!(matches!(err, Error::ResourceLimit(_)), "{err}");
}
