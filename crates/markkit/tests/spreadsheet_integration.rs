//! Spreadsheet (XLSX/XLS/ODS) → Markdown integration tests.

#![cfg(feature = "spreadsheet")]

mod common;

use common::Cell::*;
use common::*;
use markkit::{DocumentHint, Markkit, format::Format};

const XLSX_CT: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

fn extract_with(m: &Markkit, bytes: &[u8]) -> markkit::Extraction {
    m.extract_document_sync(bytes, DocumentHint::default())
        .unwrap()
}

fn extract(bytes: &[u8]) -> markkit::Extraction {
    extract_with(&Markkit::builder().build(), bytes)
}

#[test]
fn xlsx_single_sheet_table() {
    let bytes = xlsx(&[Sheet::new(
        "People",
        vec![
            vec![S("Name"), S("Age"), S("Member")],
            vec![S("Ann"), N(30.0), B(true)],
            vec![S("Bob"), N(41.5), B(false)],
        ],
    )]);
    let e = extract(&bytes);
    assert_eq!(e.format, Format::Xlsx);
    assert_eq!(
        e.markdown,
        "## People\n\n| Name | Age | Member |\n| --- | --- | --- |\n| Ann | 30 | true |\n| Bob | 41.5 | false |\n"
    );
    assert_eq!(e.stats.sheets, Some(1));
    assert_eq!(e.stats.rows, Some(2));
    assert!(e.warnings.is_empty(), "{:?}", e.warnings);
}

#[test]
fn xlsx_multiple_sheets_and_empty_sheet_skipped() {
    let bytes = xlsx(&[
        Sheet::new("A", vec![vec![S("h")], vec![S("1")]]),
        Sheet::new("Empty", vec![]),
        Sheet::new("B", vec![vec![S("x"), S("y")]]),
    ]);
    let e = extract(&bytes);
    assert_eq!(
        e.markdown,
        "## A\n\n| h |\n| --- |\n| 1 |\n\n## B\n\n| A | B |\n| --- | --- |\n| x | y |\n"
    );
    assert_eq!(e.stats.sheets, Some(2));
    assert_eq!(e.stats.rows, Some(2));
}

#[test]
fn xlsx_header_is_first_non_empty_row_and_offset_trimmed() {
    // Data starts at C3 – leading empty rows/columns are trimmed.
    let bytes = xlsx(&[Sheet::new(
        "S",
        vec![
            vec![],
            vec![],
            vec![Empty, Empty, S("k"), S("v")],
            vec![],
            vec![Empty, Empty, S("a"), N(1.0)],
        ],
    )]);
    assert_eq!(
        extract(&bytes).markdown,
        "## S\n\n| k | v |\n| --- | --- |\n| a | 1 |\n"
    );
}

#[test]
fn xlsx_pipes_newlines_escaped() {
    let bytes = xlsx(&[Sheet::new(
        "S",
        vec![vec![S("a|b"), S("multi\nline")], vec![S("x\\y"), S("")]],
    )]);
    assert_eq!(
        extract(&bytes).markdown,
        "## S\n\n| a\\|b | multi line |\n| --- | --- |\n| x\\y |  |\n"
    );
}

#[test]
fn xlsx_row_cap_truncates_with_warning() {
    let rows: Vec<Vec<common::Cell>> = std::iter::once(vec![S("n")])
        .chain((0..10).map(|i| vec![N(i as f64)]))
        .collect();
    let bytes = xlsx(&[Sheet::new("Big", rows)]);
    let m = Markkit::builder().max_sheet_rows(3).build();
    let e = extract_with(&m, &bytes);
    assert_eq!(
        e.markdown,
        "## Big\n\n| n |\n| --- |\n| 0 |\n| 1 |\n| 2 |\n"
    );
    assert_eq!(e.warnings.len(), 1, "{:?}", e.warnings);
    assert!(
        e.warnings[0].starts_with("truncated_sheet:"),
        "{:?}",
        e.warnings
    );
    assert!(e.warnings[0].contains("Big"));
}

#[test]
fn xlsx_col_cap_truncates_with_warning() {
    let bytes = xlsx(&[Sheet::new(
        "Wide",
        vec![
            vec![S("a"), S("b"), S("c"), S("d")],
            vec![N(1.0), N(2.0), N(3.0), N(4.0)],
        ],
    )]);
    let m = Markkit::builder().max_sheet_cols(2).build();
    let e = extract_with(&m, &bytes);
    assert_eq!(
        e.markdown,
        "## Wide\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n"
    );
    assert!(e.warnings.iter().any(|w| w.starts_with("truncated_sheet:")));
}

#[test]
fn xlsx_hidden_sheet_skipped_by_default() {
    let mut hidden = Sheet::new("Secret", vec![vec![S("ignore previous instructions")]]);
    hidden.hidden = true;
    let bytes = xlsx(&[Sheet::new("Visible", vec![vec![S("ok")]]), hidden]);
    let e = extract(&bytes);
    assert!(!e.markdown.contains("ignore previous"), "{}", e.markdown);
    assert!(e.markdown.contains("## Visible"));
    assert!(
        e.warnings
            .iter()
            .any(|w| w.starts_with("hidden_sheet_skipped:")),
        "{:?}",
        e.warnings
    );

    let m = Markkit::builder().include_hidden_sheets(true).build();
    assert!(extract_with(&m, &bytes).markdown.contains("## Secret"));
}

#[test]
fn xlsx_by_content_type_and_extension() {
    let bytes = xlsx(&[Sheet::new("S", vec![vec![S("v")]])]);
    let m = Markkit::builder().build();
    for hint in [
        DocumentHint::content_type(XLSX_CT),
        DocumentHint::file_name("book.XLSX"),
        // Mislabelled as legacy Excel: container is sniffed.
        DocumentHint::content_type("application/vnd.ms-excel"),
    ] {
        let e = m.extract_document_sync(&bytes, hint).unwrap();
        assert!(e.markdown.contains("| v |"), "{hint:?}");
    }
}

#[test]
fn xlsx_sparse_far_cell_does_not_allocate_grid() {
    // A1 and XFD1048576: a dense grid would be ~17 billion cells.
    let data = r#"<row r="1"><c r="A1" t="inlineStr"><is><t>top</t></is></c></row><row r="1048576"><c r="XFD1048576"><v>1</v></c></row>"#;
    let bytes = xlsx_with(
        &[Sheet::new("Sparse", vec![])],
        &[Some(worksheet_xml(data))],
        &[],
    );
    let e = extract(&bytes);
    assert!(e.markdown.contains("top"), "{}", e.markdown);
    assert!(e.markdown.len() < 64 * 1024);
    assert!(e.warnings.iter().any(|w| w.starts_with("truncated_sheet:")));
}

#[test]
fn xlsm_with_vba_project_extracts_values_only() {
    let bytes = xlsx_with(
        &[Sheet::new("S", vec![vec![S("value")]])],
        &[],
        &[(
            "xl/vbaProject.bin",
            b"Attribute VB_Name = \"Evil\"\nShell \"calc\"",
        )],
    );
    let e = Markkit::builder()
        .build()
        .extract_document_sync(&bytes, DocumentHint::file_name("macro.xlsm"))
        .unwrap();
    assert!(e.markdown.contains("value"));
    assert!(!e.markdown.contains("Shell"));
}

#[tokio::test]
async fn extract_bytes_delegates_for_xlsx() {
    let bytes = xlsx(&[Sheet::new("S", vec![vec![S("v")]])]);
    let md = Markkit::builder()
        .build()
        .extract_bytes(&bytes, Some(XLSX_CT))
        .await
        .unwrap();
    assert!(md.contains("## S"));
}

// ─── ODS ────────────────────────────────────────────────────────────

#[test]
fn ods_basic() {
    let inner = format!(
        r#"<table:table table:name="Inventory"><table:table-row>{}{}</table:table-row><table:table-row>{}{}</table:table-row><table:table-row table:number-rows-repeated="1048570"><table:table-cell table:number-columns-repeated="1024"/></table:table-row></table:table><table:table table:name="Blank"><table:table-row><table:table-cell/></table:table-row></table:table>"#,
        ods_string_cell("Item"),
        ods_string_cell("Qty"),
        ods_string_cell("Bolt"),
        ods_float_cell(12.0),
    );
    let bytes = ods(&inner);
    let e = extract(&bytes);
    assert_eq!(e.format, Format::Ods);
    assert_eq!(
        e.markdown,
        "## Inventory\n\n| Item | Qty |\n| --- | --- |\n| Bolt | 12 |\n"
    );
}

#[test]
fn ods_by_extension() {
    let inner = format!(
        r#"<table:table table:name="T"><table:table-row>{}</table:table-row></table:table>"#,
        ods_string_cell("x")
    );
    let e = Markkit::builder()
        .build()
        .extract_document_sync(&ods(&inner), DocumentHint::file_name("a.ods"))
        .unwrap();
    assert!(e.markdown.contains("| x |"));
}

// ─── XLS ────────────────────────────────────────────────────────────

#[test]
fn xls_basic() {
    let bytes = xls(&[XlsSheet {
        name: "Legacy",
        dims: Some((0, 2, 0, 2)),
        cells: vec![
            XlsCell::Label(0, 0, "Name"),
            XlsCell::Label(0, 1, "Score"),
            XlsCell::Label(1, 0, "Ann"),
            XlsCell::Num(1, 1, 9.5),
        ],
    }]);
    let e = extract(&bytes);
    assert_eq!(e.format, Format::Xls);
    assert_eq!(
        e.markdown,
        "## Legacy\n\n| Name | Score |\n| --- | --- |\n| Ann | 9.5 |\n"
    );
}

#[test]
fn xls_by_content_type() {
    let bytes = xls(&[XlsSheet {
        name: "S",
        dims: None,
        cells: vec![XlsCell::Label(0, 0, "x")],
    }]);
    let e = Markkit::builder()
        .build()
        .extract_document_sync(
            &bytes,
            DocumentHint::content_type("application/vnd.ms-excel"),
        )
        .unwrap();
    assert!(e.markdown.contains("| x |"), "{}", e.markdown);
}

// ─── Single-row sheets, titles, package quirks ──────────────────────

#[test]
fn single_row_sheet_renders_as_data_under_column_letters() {
    // One row starting at column C: header is the real column letters.
    let data = r#"<row r="2"><c r="C2" t="inlineStr"><is><t>only</t></is></c><c r="D2"><v>7</v></c></row>"#;
    let bytes = xlsx_with(
        &[Sheet::new("One", vec![])],
        &[Some(worksheet_xml(data))],
        &[],
    );
    let e = extract(&bytes);
    assert_eq!(
        e.markdown,
        "## One\n\n| C | D |\n| --- | --- |\n| only | 7 |\n"
    );
    assert_eq!(e.stats.rows, Some(1));

    // Two rows: the first is still the header.
    let bytes = xlsx(&[Sheet::new("Two", vec![vec![S("h")], vec![S("v")]])]);
    assert_eq!(
        extract(&bytes).markdown,
        "## Two\n\n| h |\n| --- |\n| v |\n"
    );
}

#[test]
fn xlsx_title_from_core_properties() {
    let core = br#"<?xml version="1.0" encoding="UTF-8"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Quarterly &amp; Annual</dc:title></cp:coreProperties>"#;
    let bytes = xlsx_with(
        &[Sheet::new("S", vec![vec![S("h")], vec![S("v")]])],
        &[],
        &[("docProps/core.xml", core)],
    );
    assert_eq!(extract(&bytes).title.as_deref(), Some("Quarterly & Annual"));
    // No core properties: no title.
    let bytes = xlsx(&[Sheet::new("S", vec![vec![S("v")]])]);
    assert_eq!(extract(&bytes).title, None);
}

#[test]
fn xlsx_malformed_core_properties_do_not_fail_extraction() {
    let bytes = xlsx_with(
        &[Sheet::new("S", vec![vec![S("h")], vec![S("v")]])],
        &[],
        &[(
            "docProps/core.xml",
            b"<cp:coreProperties><dc:title>x</dc:titl",
        )],
    );
    let e = extract(&bytes);
    assert!(e.markdown.contains("| v |"));
    let deep = format!("{}{}", "<a>".repeat(10_000), "<title>t</title>");
    let bytes = xlsx_with(
        &[Sheet::new("S", vec![vec![S("v")]])],
        &[],
        &[("docProps/core.xml", deep.as_bytes())],
    );
    assert_eq!(extract(&bytes).title, None);
}

#[test]
fn ods_title_from_meta() {
    let content = format!(
        r#"<table:table table:name="T"><table:table-row>{}</table:table-row></table:table>"#,
        ods_string_cell("v")
    );
    let meta = br#"<?xml version="1.0" encoding="UTF-8"?><office:document-meta xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:dc="http://purl.org/dc/elements/1.1/"><office:meta><dc:title>ODS Report</dc:title></office:meta></office:document-meta>"#;
    let bytes = rezip(&ods(&content), &[], &[("meta.xml", meta)]);
    let e = extract(&bytes);
    assert_eq!(e.format, Format::Ods);
    assert_eq!(e.title.as_deref(), Some("ODS Report"));
}

#[test]
fn xls_title_from_summary_information() {
    let stream = biff8_workbook(&[XlsSheet {
        name: "S",
        dims: None,
        cells: vec![XlsCell::Label(0, 0, "h"), XlsCell::Label(1, 0, "v")],
    }]);
    let summary = summary_information(1252, b"Legacy Budget");
    let bytes = ole2(&[
        ("/Workbook", &stream),
        ("/\u{5}SummaryInformation", &summary),
    ]);
    let e = extract(&bytes);
    assert_eq!(e.format, Format::Xls);
    assert_eq!(e.title.as_deref(), Some("Legacy Budget"));

    // Garbage property set: no title, extraction still succeeds.
    let bytes = ole2(&[
        ("/Workbook", &stream),
        ("/\u{5}SummaryInformation", b"\xFE\xFFjunk"),
    ]);
    let e = extract(&bytes);
    assert_eq!(e.title, None);
    assert!(e.markdown.contains("| v |"));
}

#[test]
fn xlsx_without_package_relationships_still_parses() {
    let bytes = xlsx(&[Sheet::new("S", vec![vec![S("h")], vec![S("v")]])]);
    let bytes = rezip(&bytes, &["_rels/.rels"], &[]);
    let e = extract(&bytes);
    assert_eq!(e.markdown, "## S\n\n| h |\n| --- |\n| v |\n");
}

// ─── XLSB ───────────────────────────────────────────────────────────

const XLSB_CT: &str = "application/vnd.ms-excel.sheet.binary.macroEnabled.12";

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

#[test]
fn xlsb_content_type_is_not_xlsx() {
    assert_eq!(Format::from_content_type(XLSB_CT), Format::Xlsb);
    assert_eq!(Format::from_file_name("book.xlsb"), Format::Xlsb);
}

#[test]
fn xlsb_generated_workbook() {
    let bytes = xlsb(&[
        Sheet::new(
            "People",
            vec![
                vec![S("Name"), S("Age"), S("Member")],
                vec![S("Ann"), N(30.0), B(true)],
                vec![S("Bob|Jr"), N(41.5), B(false)],
            ],
        ),
        Sheet::new("Empty", vec![]),
    ]);
    for hint in [
        DocumentHint::default(),
        DocumentHint::content_type(XLSB_CT),
        DocumentHint::file_name("book.xlsb"),
        // Mislabelled: the container is sniffed.
        DocumentHint::content_type(XLSX_CT),
    ] {
        let e = Markkit::builder()
            .build()
            .extract_document_sync(&bytes, hint)
            .unwrap();
        assert_eq!(e.format, Format::Xlsb, "{hint:?}");
        assert_eq!(
            e.markdown,
            "## People\n\n| Name | Age | Member |\n| --- | --- | --- |\n| Ann | 30 | true |\n| Bob\\|Jr | 41.5 | false |\n"
        );
        assert_eq!(e.stats.sheets, Some(1));
        assert_eq!(e.stats.rows, Some(2));
    }
}

#[test]
fn xlsb_real_workbook_with_dates_and_duration() {
    let e = extract(&fixture("xlsb/date.xlsb"));
    assert_eq!(e.format, Format::Xlsb);
    assert_eq!(
        e.markdown,
        "## Sheet1\n\n| 2021-01-01 | 15 |\n| --- | --- |\n| 2021-01-02 | 16 |\n| 255:10:10 | 17 |\n"
    );
}

#[test]
fn xlsb_real_workbook_mixed_types_and_shared_strings() {
    let e = extract(&fixture("xlsb/issues.xlsb"));
    assert!(e.warnings.is_empty(), "{:?}", e.warnings);
    assert!(e.markdown.starts_with(
        "## datatypes\n\n| 1 |\n| --- |\n| 1.5 |\n| ab |\n| false |\n| test |\n| 2016-10-20 |\n"
    ));
    assert!(
        e.markdown
            .contains("## issue2\n\n| 1 | a |\n| --- | --- |\n| 2 | b |\n| 3 | c |\n")
    );
    assert!(e.markdown.contains("| ☺ |"));
    assert!(e.markdown.contains("| àâéêèçöïî«» |"));
    // VBA project is present in the file but never rendered.
    assert!(!e.markdown.to_lowercase().contains("vba"));
}

#[test]
fn xlsb_hidden_sheet_skipped_by_default() {
    let mut hidden = Sheet::new("Secret", vec![vec![S("h")], vec![S("pw")]]);
    hidden.hidden = true;
    let bytes = xlsb(&[Sheet::new("Open", vec![vec![S("h")], vec![S("v")]]), hidden]);
    let e = extract(&bytes);
    assert!(!e.markdown.contains("Secret"), "{}", e.markdown);
    assert!(
        e.warnings
            .iter()
            .any(|w| w.starts_with("hidden_sheet_skipped:"))
    );
    let m = Markkit::builder().include_hidden_sheets(true).build();
    assert!(extract_with(&m, &bytes).markdown.contains("## Secret"));
}

#[test]
fn xlsb_caps_and_title() {
    let rows: Vec<Vec<Cell>> = (0..20).map(|i| vec![N(f64::from(i)); 10]).collect();
    let core = b"<cp:coreProperties><dc:title>Binary Book</dc:title></cp:coreProperties>";
    let bytes = xlsb_with(&[Sheet::new("Big", rows)], &[("docProps/core.xml", core)]);
    let m = Markkit::builder()
        .max_sheet_rows(3)
        .max_sheet_cols(2)
        .build();
    let e = extract_with(&m, &bytes);
    assert_eq!(e.title.as_deref(), Some("Binary Book"));
    assert_eq!(e.stats.rows, Some(3));
    assert_eq!(
        e.warnings
            .iter()
            .filter(|w| w.starts_with("truncated_sheet:"))
            .count(),
        2,
        "{:?}",
        e.warnings
    );
}

#[test]
fn xlsb_missing_and_truncated_sheet_parts_warn() {
    let bytes = xlsb(&[Sheet::new("S", vec![vec![S("h")], vec![S("v")]])]);
    // Sheet part removed.
    let gone = rezip(&bytes, &["xl/worksheets/sheet1.bin"], &[]);
    let e = extract(&gone);
    assert!(
        e.warnings.iter().any(|w| w.starts_with("sheet_error:")),
        "{:?}",
        e.warnings
    );
    // Sheet part cut mid-record (the end records take 6 bytes).
    let sheet = xlsb_sheet(&[vec![S("h")], vec![S("value")]]);
    let cut = rezip(
        &bytes,
        &["xl/worksheets/sheet1.bin"],
        &[("xl/worksheets/sheet1.bin", &sheet[..sheet.len() - 8])],
    );
    let e = extract(&cut);
    assert!(
        e.warnings.iter().any(|w| w.starts_with("sheet_error:")),
        "{:?}",
        e.warnings
    );
}

// ─── Provenance ──────────────────────────────────────────────────────

#[test]
fn xlsx_sources_record_sheet_and_range() {
    let bytes = xlsx(&[Sheet::new(
        "Data",
        vec![
            vec![Empty, Empty],
            vec![Empty, S("h1"), S("h2")],
            vec![Empty, S("a"), S("b")],
        ],
    )]);
    let m = Markkit::builder().track_sources(true).build();
    let p = m
        .parse_document_sync(&bytes, DocumentHint::default())
        .unwrap();
    let nodes = &p.document.nodes;
    let heading = nodes[0].source().unwrap();
    assert_eq!(heading.sheet.as_deref(), Some("Data"));
    assert_eq!(heading.range, None);
    let table = nodes[1].source().unwrap();
    assert_eq!(table.range.as_deref(), Some("B2:C3"));
    // Off by default.
    let p = Markkit::builder()
        .build()
        .parse_document_sync(&bytes, DocumentHint::default())
        .unwrap();
    assert!(p.document.nodes.iter().all(|n| n.source().is_none()));
}
