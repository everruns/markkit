//! Resource limits (threat model T2/T6): every limit in `markkit::Limits` is
//! enforced, defaults are generous, and the nesting hard ceiling keeps deep
//! input off the host stack.

#![cfg(any(feature = "html", feature = "docx", feature = "spreadsheet"))]

mod common;

use std::time::Duration;

use markkit::{DocumentHint, Error, LimitKind, Limits, Markkit};

#[allow(dead_code)]
const DOCX_CT: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
#[allow(dead_code)]
const XLSX_CT: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

/// The exceeded limit; also checks the message names it and `limit_kind` agrees.
fn limit<T: std::fmt::Debug>(r: markkit::Result<T>) -> LimitKind {
    match r {
        Err(e @ Error::ResourceLimit(_)) => {
            let Error::ResourceLimit(m) = &e else {
                unreachable!()
            };
            assert_eq!(e.limit_kind(), Some(m.kind));
            assert!(m.detail.contains(m.kind.as_str()), "{m}");
            m.kind
        }
        other => panic!("expected ResourceLimit, got {other:?}"),
    }
}

#[test]
fn defaults_are_generous() {
    let l = Limits::default();
    assert_eq!(l.max_input_size, 50 * 1024 * 1024);
    assert_eq!(l.max_output_size, 128 * 1024 * 1024);
    assert_eq!(l.timeout, Duration::from_secs(60));
    assert_eq!(l.max_nesting_depth, 256);
    assert_eq!(l.max_nodes, 5_000_000);
    assert_eq!(l.max_redirects, 5);
    assert_eq!(Markkit::builder().build().config().limits, l);
}

#[test]
fn builder_limits_replaces_all() {
    let l = Limits::default().max_output_size(7).max_nodes(9);
    let m = Markkit::builder().limits(l.clone()).build();
    assert_eq!(m.config().limits, l);
    let m = Markkit::builder()
        .max_processing_secs(3)
        .max_nesting_depth(12)
        .build();
    assert_eq!(m.config().limits.timeout, Duration::from_secs(3));
    assert_eq!(m.config().limits.max_nesting_depth, 12);
}

/// Stack for hard-ceiling tests: 2 MiB (Rust's default thread stack) in
/// release; debug frames are ~6x larger, so debug runs get 8 MiB.
#[allow(dead_code)]
fn small_stack() -> usize {
    if cfg!(debug_assertions) {
        8 << 20
    } else {
        2 << 20
    }
}

// ─── HTML ───────────────────────────────────────────────────────────

#[cfg(feature = "html")]
mod html {
    use super::*;
    use markkit::limits::HARD_MAX_NESTING_DEPTH;

    fn deep(tag: &str, n: usize, inner: &str) -> String {
        format!(
            "{}{inner}{}",
            format!("<{tag}>").repeat(n),
            format!("</{tag}>").repeat(n)
        )
    }

    #[test]
    fn output_cap_rejects_large_output() {
        let html = "<p>hello world</p>".repeat(1000);
        let m = Markkit::builder().max_output_size(1000).build();
        assert!(limit(m.extract_html_sync(&html)) == LimitKind::MaxOutputSize);
        let m = Markkit::builder().max_output_size(100_000).build();
        assert!(m.extract_html_sync(&html).is_ok());
    }

    #[test]
    fn output_cap_stops_table_amplification() {
        // One wide row pads every other row: output ~ rows x cols.
        let wide = format!("<tr>{}</tr>", "<td>x</td>".repeat(2000));
        let html = format!(
            "<table>{wide}{}</table>",
            "<tr><td>y</td></tr>".repeat(2000)
        );
        let m = Markkit::builder().max_output_size(1 << 20).build();
        let start = std::time::Instant::now();
        assert!(limit(m.extract_html_sync(&html)) == LimitKind::MaxOutputSize);
        assert!(start.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn output_cap_applies_inside_nested_renderers() {
        let wide = format!("<tr>{}</tr>", "<td>x</td>".repeat(500));
        let table = format!("<table>{wide}{}</table>", "<tr><td>y</td></tr>".repeat(500));
        for html in [
            format!("<blockquote>{table}</blockquote>"),
            format!("<ul><li><p>a</p>{table}</li></ul>"),
        ] {
            let m = Markkit::builder().max_output_size(64 * 1024).build();
            assert!(limit(m.extract_html_sync(&html)) == LimitKind::MaxOutputSize);
        }
    }

    #[test]
    fn zero_timeout_rejects() {
        let m = Markkit::builder().timeout(Duration::ZERO).build();
        assert!(limit(m.extract_html_sync("<p>x</p>")) == LimitKind::Timeout);
        let r = m.extract_document_sync(b"<p>x</p>", DocumentHint::content_type("text/html"));
        assert!(limit(r) == LimitKind::Timeout);
    }

    #[test]
    fn node_cap_rejects() {
        let html = "<p>a</p>".repeat(1000);
        let m = Markkit::builder().max_nodes(500).build();
        assert!(limit(m.extract_html_sync(&html)) == LimitKind::MaxNodes);
        assert!(limit(m.parse_html(&html)) == LimitKind::MaxNodes);
        let m = Markkit::builder().max_nodes(10_000).build();
        assert!(m.extract_html_sync(&html).is_ok());
    }

    #[test]
    fn nesting_depth_configurable_flattens() {
        let html = deep("blockquote", 20, "<p>Deep</p>");
        let full = Markkit::builder().build().extract_html_sync(&html).unwrap();
        assert!(full.contains(&"> ".repeat(20)));
        let m = Markkit::builder().max_nesting_depth(5).build();
        let md = m.extract_html_sync(&html).unwrap();
        assert!(md.contains("Deep"));
        assert!(!md.contains(&"> ".repeat(6)));
    }

    #[test]
    fn hard_ceiling_survives_small_stack() {
        // Configured depth is clamped; deep nesting of recursive constructs
        // must fit a small thread stack.
        let inputs = [
            deep("blockquote", HARD_MAX_NESTING_DEPTH * 4, "<p>q</p>"),
            deep("ul", HARD_MAX_NESTING_DEPTH * 4, "<li>l</li>"),
            deep("div", HARD_MAX_NESTING_DEPTH * 4, "<p>d</p>"),
            deep("b", HARD_MAX_NESTING_DEPTH * 4, "t"),
        ];
        std::thread::Builder::new()
            .stack_size(small_stack())
            .spawn(move || {
                let m = Markkit::builder().max_nesting_depth(usize::MAX).build();
                for html in inputs {
                    m.extract_html_sync(&html).unwrap();
                }
            })
            .unwrap()
            .join()
            .expect("deep input overflowed a small stack");
    }
}

// ─── DOCX ───────────────────────────────────────────────────────────

#[cfg(feature = "docx")]
mod docx {
    use super::*;
    use common::*;
    use markkit::limits::HARD_MAX_NESTING_DEPTH;

    fn run(m: &Markkit, bytes: &[u8]) -> markkit::Result<markkit::Extraction> {
        m.extract_document_sync(bytes, DocumentHint::content_type(DOCX_CT))
    }

    #[test]
    fn node_cap_rejects() {
        let body: String = (0..200).map(|i| para(&format!("p{i}"))).collect();
        let bytes = docx(&body);
        let m = Markkit::builder().max_nodes(100).build();
        assert!(limit(run(&m, &bytes)) == LimitKind::MaxNodes);
        assert!(run(&Markkit::builder().build(), &bytes).is_ok());
    }

    #[test]
    fn nesting_depth_configurable() {
        let body = format!(
            "{}{}{}",
            "<w:sdt><w:sdtContent>".repeat(10),
            para("x"),
            "</w:sdtContent></w:sdt>".repeat(10)
        );
        let bytes = docx(&body);
        assert!(run(&Markkit::builder().build(), &bytes).is_ok());
        let m = Markkit::builder().max_nesting_depth(8).build();
        assert!(limit(run(&m, &bytes)) == LimitKind::MaxNestingDepth);
    }

    #[test]
    fn hard_ceiling_survives_small_stack() {
        let n = HARD_MAX_NESTING_DEPTH / 2 - 8;
        let body = format!(
            "{}{}{}",
            "<w:sdt><w:sdtContent>".repeat(n),
            para("x"),
            "</w:sdtContent></w:sdt>".repeat(n)
        );
        let bytes = docx(&body);
        let deeper = docx(&"<w:sdt>".repeat(HARD_MAX_NESTING_DEPTH * 2));
        std::thread::Builder::new()
            .stack_size(small_stack())
            .spawn(move || {
                let m = Markkit::builder().max_nesting_depth(usize::MAX).build();
                assert!(run(&m, &bytes).unwrap().markdown.contains('x'));
                assert!(limit(run(&m, &deeper)) == LimitKind::MaxNestingDepth);
            })
            .unwrap()
            .join()
            .expect("deep DOCX overflowed a small stack");
    }

    #[test]
    fn zero_timeout_rejects() {
        let bytes = docx(&para("x"));
        let m = Markkit::builder().timeout(Duration::ZERO).build();
        assert!(limit(run(&m, &bytes)) == LimitKind::Timeout);
    }

    #[test]
    fn output_cap_rejects() {
        let body: String = (0..200).map(|i| para(&format!("paragraph {i}"))).collect();
        let bytes = docx(&body);
        let m = Markkit::builder().max_output_size(500).build();
        assert!(limit(run(&m, &bytes)) == LimitKind::MaxOutputSize);
    }
}

// ─── Spreadsheets ───────────────────────────────────────────────────

#[cfg(feature = "spreadsheet")]
mod spreadsheet {
    use super::*;
    use common::*;

    fn sheet() -> Vec<u8> {
        let rows = (0..200)
            .map(|i| vec![Cell::Str(format!("row {i}")), Cell::N(i as f64)])
            .collect();
        xlsx(&[Sheet::new("S", rows)])
    }

    fn run(m: &Markkit, bytes: &[u8]) -> markkit::Result<markkit::Extraction> {
        m.extract_document_sync(bytes, DocumentHint::content_type(XLSX_CT))
    }

    #[test]
    fn zero_timeout_rejects() {
        let m = Markkit::builder().timeout(Duration::ZERO).build();
        assert!(limit(run(&m, &sheet())) == LimitKind::Timeout);
    }

    #[test]
    fn output_cap_rejects() {
        let m = Markkit::builder().max_output_size(500).build();
        assert!(limit(run(&m, &sheet())) == LimitKind::MaxOutputSize);
        assert!(run(&Markkit::builder().build(), &sheet()).is_ok());
    }

    #[test]
    fn xlsb_zero_timeout_rejects() {
        let rows = (0..200)
            .map(|i| vec![Cell::Str(format!("row {i}")), Cell::N(i as f64)])
            .collect();
        let bytes = xlsb(&[Sheet::new("S", rows)]);
        let hint = DocumentHint::file_name("t.xlsb");
        let m = Markkit::builder().timeout(Duration::ZERO).build();
        assert_eq!(
            limit(m.extract_document_sync(&bytes, hint)),
            LimitKind::Timeout
        );
        assert!(
            Markkit::builder()
                .build()
                .extract_document_sync(&bytes, hint)
                .is_ok()
        );
    }
}
