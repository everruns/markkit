//! Jupyter notebook (.ipynb) → Markdown integration and security tests.

use markkit::format::Format;
use markkit::{DocumentHint, Error, Markkit};
use serde_json::json;

fn extract(nb: &serde_json::Value) -> markkit::Extraction {
    Markkit::builder()
        .build()
        .extract_document_sync(
            nb.to_string().as_bytes(),
            DocumentHint::file_name("nb.ipynb"),
        )
        .unwrap()
}

fn err(bytes: &[u8]) -> Error {
    Markkit::builder()
        .build()
        .extract_document_sync(bytes, DocumentHint::file_name("nb.ipynb"))
        .unwrap_err()
}

fn notebook(cells: serde_json::Value) -> serde_json::Value {
    json!({
        "nbformat": 4,
        "nbformat_minor": 5,
        "metadata": {"kernelspec": {"name": "python3", "language": "python"}},
        "cells": cells
    })
}

#[test]
fn cells_and_outputs() {
    let nb = notebook(json!([
        {"cell_type": "markdown", "source": ["# Analysis\n", "\n", "Load the data."]},
        {"cell_type": "code", "source": "import pandas as pd\ndf = pd.read_csv('x.csv')\ndf.shape",
         "outputs": [
            {"output_type": "stream", "name": "stdout", "text": ["loading\n"]},
            {"output_type": "execute_result", "data": {"text/plain": ["(3, 2)"], "image/png": "iVBOR"}},
            {"output_type": "display_data", "data": {"image/png": "iVBOR"}}
         ]},
        {"cell_type": "code", "source": [], "outputs": []},
        {"cell_type": "raw", "source": "raw text"}
    ]));
    let e = extract(&nb);
    assert_eq!(e.format, Format::Notebook);
    if cfg!(feature = "markdown") {
        assert_eq!(e.title.as_deref(), Some("Analysis"));
        assert!(e.markdown.starts_with("# Analysis\n\nLoad the data.\n"));
    } else {
        // Without the parser, Markdown cells are plain text paragraphs.
        assert_eq!(e.title, None);
    }
    assert_eq!(e.stats.cells, Some(3));
    assert!(e.markdown.ends_with(
        "```python\nimport pandas as pd\ndf = pd.read_csv('x.csv')\ndf.shape\n```\n\n```text\nloading\n```\n\n```text\n(3, 2)\n```\n\n```\nraw text\n```\n"
    ), "{}", e.markdown);
    assert!(!e.markdown.contains("iVBOR"));
}

#[test]
fn error_output_strips_ansi() {
    let nb = notebook(json!([
        {"cell_type": "code", "source": "1/0", "outputs": [{
            "output_type": "error", "ename": "ZeroDivisionError", "evalue": "division by zero",
            "traceback": ["\u{1b}[0;31m---------\u{1b}[0m", "\u{1b}[0;31mZeroDivisionError\u{1b}[0m: division by zero"]
        }]}
    ]));
    let e = extract(&nb);
    assert!(
        e.markdown.contains("```text\nZeroDivisionError: division by zero\n---------\nZeroDivisionError: division by zero\n```"),
        "{}",
        e.markdown
    );
    assert!(!e.markdown.contains('\u{1b}') && !e.markdown.contains("[0;31m"));
}

#[test]
fn metadata_title_and_language_info() {
    let nb = json!({
        "nbformat": 4, "nbformat_minor": 2,
        "metadata": {"title": "My NB", "language_info": {"name": "julia"}},
        "cells": [{"cell_type": "code", "source": "1 + 1", "outputs": []}]
    });
    let e = extract(&nb);
    assert_eq!(e.title.as_deref(), Some("My NB"));
    assert_eq!(e.markdown, "```julia\n1 + 1\n```\n");
}

#[test]
fn detection() {
    assert_eq!(Format::from_extension("IPYNB"), Format::Notebook);
    assert_eq!(
        Format::from_content_type("application/x-ipynb+json"),
        Format::Notebook
    );
    // Never sniffed: plain JSON bytes without a hint stay unsupported.
    let e = Markkit::builder()
        .build()
        .extract_document_sync(br#"{"nbformat":4,"cells":[]}"#, DocumentHint::default())
        .unwrap_err();
    assert!(matches!(e, Error::UnsupportedFormat(_)));
}

#[test]
fn empty_notebook() {
    let e = extract(&notebook(json!([])));
    assert_eq!(e.markdown, "");
    assert_eq!(e.stats.cells, Some(0));
}

// ─── Security (threat model T7) ─────────────────────────────────────

#[test]
fn security_malformed_json_is_error() {
    for bytes in [
        &b""[..],
        b"not json",
        b"{\"nbformat\": 4",
        b"[1,2,3]",
        b"{\"nbformat\": 4, \"cells\": 7}",
        b"{\"nbformat\": 3, \"worksheets\": []}",
        b"\xff\xfe\x00",
    ] {
        assert!(matches!(err(bytes), Error::NotebookExtract(_)), "{bytes:?}");
    }
}

#[test]
fn security_deep_json_nesting_is_error_not_overflow() {
    let deep = format!(
        "{{\"nbformat\":4,\"cells\":[],\"metadata\":{}{}}}",
        "[".repeat(100_000),
        "]".repeat(100_000)
    );
    assert!(matches!(err(deep.as_bytes()), Error::NotebookExtract(_)));
}

#[test]
fn security_wrong_types_ignored() {
    let nb = json!({
        "nbformat": 4,
        "metadata": {"kernelspec": {"language": "py`thon\n```"}, "title": 5},
        "cells": [
            {"cell_type": 1, "source": "x"},
            {"cell_type": "code", "source": {"a": 1}, "outputs": "nope"},
            {"cell_type": "code", "source": ["ok", 3, null], "outputs": [7, {"output_type": "stream", "text": 9}]},
            "not a cell"
        ]
    });
    let e = extract(&nb);
    assert_eq!(e.markdown, "```python\nok\n```\n");
    assert_eq!(e.title, None);
}

#[cfg(feature = "markdown")]
#[test]
fn security_markdown_cells_and_outputs_drop_raw_html() {
    let nb = notebook(json!([
        {"cell_type": "markdown", "source": "hi <img src=x onerror=alert(1)>\n\n<script>bad()</script>"},
        {"cell_type": "code", "source": "x", "outputs": [
            {"output_type": "display_data", "data": {"text/html": "<script>bad()</script>", "text/markdown": "**ok** <iframe src=evil>"}}
        ]}
    ]));
    let e = extract(&nb);
    for bad in ["<img", "onerror", "<script", "bad()", "<iframe"] {
        assert!(!e.markdown.contains(bad), "{bad}: {}", e.markdown);
    }
    assert!(e.markdown.contains("**ok**"));
}

#[test]
fn security_control_chars_dropped() {
    let nb = notebook(json!([
        {"cell_type": "code", "source": "a\u{0}b\u{7}\u{1b}]0;pwn\u{7}c", "outputs": [
            {"output_type": "stream", "text": "x\u{8}\u{1b}[2Jy"}
        ]}
    ]));
    let e = extract(&nb);
    assert!(
        !e.markdown
            .chars()
            .any(|c| c.is_ascii_control() && c != '\n' && c != '\t'),
        "{:?}",
        e.markdown
    );
    assert!(e.markdown.contains("abc") && e.markdown.contains("xy"));
}

#[test]
fn security_limits_apply() {
    let nb = notebook(json!([{"cell_type": "raw", "source": "x"}])).to_string();
    let m = Markkit::builder()
        .timeout(std::time::Duration::ZERO)
        .build();
    let r = m.extract_document_sync(nb.as_bytes(), DocumentHint::file_name("nb.ipynb"));
    assert!(
        matches!(&r, Err(Error::ResourceLimit(msg)) if msg.contains("timeout")),
        "{r:?}"
    );
    let cells: Vec<_> = (0..50)
        .map(|_| json!({"cell_type": "raw", "source": "x"}))
        .collect();
    let nb = notebook(json!(cells)).to_string();
    let m = Markkit::builder().max_nodes(10).build();
    let r = m.extract_document_sync(nb.as_bytes(), DocumentHint::file_name("nb.ipynb"));
    assert!(matches!(&r, Err(Error::ResourceLimit(_))), "{r:?}");
}
