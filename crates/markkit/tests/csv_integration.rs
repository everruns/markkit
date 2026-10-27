//! CSV / TSV → Markdown table integration and security tests.

use markkit::format::Format;
use markkit::{DocumentHint, Error, Markkit};

fn extract_with(m: &Markkit, bytes: &[u8], hint: DocumentHint<'_>) -> markkit::Extraction {
    m.extract_document_sync(bytes, hint).unwrap()
}

fn extract(bytes: &[u8], hint: DocumentHint<'_>) -> markkit::Extraction {
    extract_with(&Markkit::builder().build(), bytes, hint)
}

#[test]
fn csv_basic_table() {
    let e = extract(
        b"name,qty,note\nApple,3,\"red, sweet\"\nPear,10\n",
        DocumentHint::file_name("fruit.csv"),
    );
    assert_eq!(e.format, Format::Csv);
    assert_eq!(
        e.markdown,
        "| name | qty | note |\n| --- | --- | --- |\n| Apple | 3 | red, sweet |\n| Pear | 10 |  |\n"
    );
    assert_eq!(e.stats.rows, Some(2));
    assert_eq!(e.stats.tables, Some(1));
    assert!(e.warnings.is_empty());
}

#[test]
fn tsv_by_extension_and_content_type() {
    for hint in [
        DocumentHint::file_name("data.tsv"),
        DocumentHint::content_type("text/tab-separated-values"),
    ] {
        let e = extract(b"a\tb,c\n1\t2\n", hint);
        assert_eq!(e.format, Format::Tsv);
        assert_eq!(e.markdown, "| a | b,c |\n| --- | --- |\n| 1 | 2 |\n");
    }
}

#[test]
fn csv_content_types() {
    for ct in ["text/csv; charset=utf-8", "application/csv"] {
        let e = extract(b"a\n1", DocumentHint::content_type(ct));
        assert_eq!(e.format, Format::Csv, "{ct}");
    }
}

#[test]
fn csv_semicolon_delimiter_sniffed() {
    let e = extract(
        b"\xEF\xBB\xBFcity;price\r\nKyiv;1,5\r\n",
        DocumentHint::file_name("eu.csv"),
    );
    assert_eq!(
        e.markdown,
        "| city | price |\n| --- | --- |\n| Kyiv | 1,5 |\n"
    );
}

#[test]
fn csv_cells_escaped_and_flattened() {
    let e = extract(
        b"h\n\"a|b\nc\"\n\n,,\n  spaced  \n",
        DocumentHint::file_name("x.csv"),
    );
    assert_eq!(e.markdown, "| h |\n| --- |\n| a\\|b c |\n| spaced |\n");
}

#[test]
fn csv_empty_input() {
    for input in [&b""[..], b"\n\n", b",,,\n , \n"] {
        let e = extract(input, DocumentHint::file_name("e.csv"));
        assert_eq!(e.markdown, "");
        assert_eq!(e.stats.tables, Some(0));
        assert_eq!(e.stats.rows, Some(0));
    }
}

#[test]
fn csv_never_sniffed_without_hint() {
    let err = Markkit::builder()
        .build()
        .extract_document_sync(b"a,b\n1,2\n", DocumentHint::default())
        .unwrap_err();
    assert!(matches!(err, Error::UnsupportedFormat(_)), "{err:?}");
}

// ─── Security (threat model T7) ─────────────────────────────────────

#[test]
fn security_row_cap_stops_reading() {
    let m = Markkit::builder().max_sheet_rows(3).build();
    let input = "h\n".to_string() + &"r\n".repeat(1_000_000);
    let e = extract_with(&m, input.as_bytes(), DocumentHint::file_name("a.csv"));
    assert_eq!(e.stats.rows, Some(3));
    assert_eq!(e.markdown.lines().count(), 5);
    assert_eq!(
        e.warnings,
        vec!["truncated_table: table truncated to 3 rows"]
    );
}

#[test]
fn security_column_cap_never_stores_extra_fields() {
    let m = Markkit::builder().max_sheet_cols(2).build();
    let input = ",".repeat(5_000_000);
    let e = extract_with(
        &m,
        format!("a{input}x").as_bytes(),
        DocumentHint::file_name("a.csv"),
    );
    assert_eq!(e.markdown, "| a |  |\n| --- | --- |\n");
    assert_eq!(
        e.warnings,
        vec!["truncated_table: table truncated to 2 columns"]
    );
}

#[test]
fn security_default_caps_bound_output() {
    // 300 columns x 20 000 rows: defaults keep 256 columns and 10 000 rows.
    let row = "x,".repeat(299) + "x\n";
    let input = row.repeat(20_001);
    let e = extract(input.as_bytes(), DocumentHint::file_name("big.csv"));
    assert_eq!(e.stats.rows, Some(10_000));
    let header = e.markdown.lines().next().unwrap();
    assert_eq!(header.matches(" x ").count(), 256);
    assert_eq!(e.warnings.len(), 2);
}

#[test]
fn security_unterminated_quote_warns() {
    let input = format!("a,\"{}", "x\n".repeat(100_000));
    let e = extract(input.as_bytes(), DocumentHint::file_name("a.csv"));
    assert_eq!(e.markdown.lines().count(), 2);
    assert_eq!(e.warnings, vec!["malformed_csv: unterminated quoted field"]);
}

#[test]
fn security_input_size_limit() {
    let m = Markkit::builder().max_input_size(4).build();
    let err = m
        .extract_document_sync(b"a,b,c", DocumentHint::file_name("a.csv"))
        .unwrap_err();
    assert!(matches!(err, Error::ResourceLimit(_)), "{err:?}");
}

#[test]
fn security_control_chars_and_invalid_utf8() {
    let e = extract(
        b"h\x1b[2J\x00\n\xff\x07v\n",
        DocumentHint::file_name("a.tsv"),
    );
    assert_eq!(e.markdown, "| h\\[2J |\n| --- |\n| \u{FFFD}v |\n");
    assert!(e.warnings[0].starts_with("invalid_utf8:"));
}

#[test]
fn security_arbitrary_bytes_never_panic() {
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    for _ in 0..300 {
        let len = (seed % 512) as usize;
        let bytes: Vec<u8> = (0..len)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                const ALPHABET: &[u8] = b",;\t|\"\n\r x\\\x00\xff";
                ALPHABET[(seed % ALPHABET.len() as u64) as usize]
            })
            .collect();
        for name in ["a.csv", "a.tsv"] {
            let e = extract(&bytes, DocumentHint::file_name(name));
            // Every table line is a well-formed row.
            assert!(
                e.markdown
                    .lines()
                    .all(|l| l.starts_with('|') && l.ends_with('|')),
                "{name}: {bytes:?} -> {:?}",
                e.markdown
            );
        }
    }
}

#[test]
fn security_cell_markup_is_escaped() {
    let e = extract(
        b"h\n<img src=x onerror=alert(1)>\n[x](javascript:alert(1))\n",
        DocumentHint::file_name("a.csv"),
    );
    assert!(e.markdown.contains("\\<img"), "{}", e.markdown);
    assert!(e.markdown.contains("\\[x\\](javascript"), "{}", e.markdown);
}

#[test]
fn security_timeout_applies() {
    let m = Markkit::builder()
        .timeout(std::time::Duration::ZERO)
        .build();
    let r = m.extract_document_sync(b"a,b\n1,2\n", DocumentHint::file_name("a.csv"));
    assert!(
        matches!(&r, Err(markkit::Error::ResourceLimit(msg)) if msg.contains("timeout")),
        "{r:?}"
    );
}
