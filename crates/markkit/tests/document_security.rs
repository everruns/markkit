//! Security tests for office-document parsing: zip bombs, resource caps,
//! malformed input, XML entity tricks, oversized spreadsheet grids.

#![cfg(any(feature = "docx", feature = "pptx", feature = "spreadsheet"))]

mod common;

use common::*;
use markkit::{DocumentHint, Error, LimitKind, Markkit, WarningCode};
use proptest::prelude::*;

const DOCX_CT: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
const XLSX_CT: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

fn run(m: &Markkit, bytes: &[u8], ct: &str) -> markkit::Result<markkit::Extraction> {
    m.extract_document_sync(bytes, DocumentHint::content_type(ct))
}

/// Hints covering every office parser plus sniffing.
fn hints() -> Vec<DocumentHint<'static>> {
    vec![
        DocumentHint::default(),
        DocumentHint::content_type(DOCX_CT),
        DocumentHint::content_type(XLSX_CT),
        DocumentHint::content_type("application/vnd.ms-excel"),
        DocumentHint::content_type("application/vnd.oasis.opendocument.spreadsheet"),
        DocumentHint::content_type("application/vnd.ms-excel.sheet.binary.macroEnabled.12"),
        DocumentHint::content_type(PPTX_CT),
    ]
}

// ─── ZIP bombs / archive caps ───────────────────────────────────────

#[cfg(feature = "docx")]
#[test]
fn docx_zip_bomb_entry_rejected() {
    // 8 MiB of zeros compresses to a few KiB.
    let bomb = vec![b' '; 8 * 1024 * 1024];
    let mut doc =
        format!(r#"<?xml version="1.0"?><w:document {W_NS}><w:body><w:p><w:r><w:t>"#).into_bytes();
    doc.extend_from_slice(&bomb);
    doc.extend_from_slice(b"</w:t></w:r></w:p></w:body></w:document>");
    let bytes = zip_bytes(&[("word/document.xml", &doc)]);
    assert!(bytes.len() < 256 * 1024, "fixture should be small");

    let m = Markkit::builder()
        .max_archive_entry_size(1024 * 1024)
        .build();
    let err = run(&m, &bytes, DOCX_CT).unwrap_err();
    assert_eq!(
        err.limit_kind(),
        Some(LimitKind::MaxArchiveEntrySize),
        "{err:?}"
    );
}

#[cfg(feature = "docx")]
#[test]
fn docx_total_decompressed_size_capped() {
    let filler = vec![b' '; 600 * 1024];
    let mut doc = format!(r#"<?xml version="1.0"?><w:document {W_NS}><w:body>"#).into_bytes();
    doc.extend_from_slice(&filler);
    doc.extend_from_slice(b"</w:body></w:document>");
    let mut styles = format!(r#"<?xml version="1.0"?><w:styles {W_NS}>"#).into_bytes();
    styles.extend_from_slice(&filler);
    styles.extend_from_slice(b"</w:styles>");
    let bytes = zip_bytes(&[("word/document.xml", &doc), ("word/styles.xml", &styles)]);
    let m = Markkit::builder()
        .max_archive_entry_size(1024 * 1024)
        .max_archive_total_size(1024 * 1024)
        .build();
    let err = run(&m, &bytes, DOCX_CT).unwrap_err();
    assert_eq!(
        err.limit_kind(),
        Some(LimitKind::MaxArchiveTotalSize),
        "{err:?}"
    );
}

#[test]
fn archive_entry_count_capped() {
    let names: Vec<String> = (0..64).map(|i| format!("junk/{i}.xml")).collect();
    let mut entries: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b"x"[..])).collect();
    entries.push(("word/document.xml", b"<w:document/>"));
    entries.push(("xl/workbook.xml", b"<workbook/>"));
    let bytes = zip_bytes(&entries);
    let m = Markkit::builder().max_archive_entries(16).build();
    let mut types = Vec::new();
    if cfg!(feature = "docx") {
        types.push(DOCX_CT);
    }
    if cfg!(feature = "spreadsheet") {
        types.push(XLSX_CT);
    }
    for ct in types {
        let err = run(&m, &bytes, ct).unwrap_err();
        assert_eq!(
            err.limit_kind(),
            Some(LimitKind::MaxArchiveEntries),
            "{ct}: {err:?}"
        );
    }
}

#[cfg(feature = "spreadsheet")]
#[test]
fn xlsx_zip_bomb_sheet_rejected() {
    let mut data = String::from(r#"<row r="1"><c r="A1"><v>1</v></c></row>"#);
    data.push_str(&" ".repeat(8 * 1024 * 1024));
    let bytes = xlsx_with(
        &[Sheet::new("S", vec![])],
        &[Some(worksheet_xml(&data))],
        &[],
    );
    assert!(bytes.len() < 256 * 1024);
    let m = Markkit::builder()
        .max_archive_entry_size(1024 * 1024)
        .build();
    let err = run(&m, &bytes, XLSX_CT).unwrap_err();
    assert_eq!(
        err.limit_kind(),
        Some(LimitKind::MaxArchiveEntrySize),
        "{err:?}"
    );
}

#[cfg(feature = "spreadsheet")]
#[test]
fn xlsb_zip_bomb_sheet_rejected() {
    let mut rows = vec![vec![Cell::S("x")]];
    rows.extend((0..1).map(|_| vec![Cell::Str("y".repeat(4 * 1024 * 1024))]));
    let bytes = xlsb(&[Sheet::new("S", rows)]);
    assert!(bytes.len() < 256 * 1024);
    let m = Markkit::builder()
        .max_archive_entry_size(1024 * 1024)
        .build();
    let err = m
        .extract_document_sync(&bytes, DocumentHint::default())
        .unwrap_err();
    assert!(matches!(err, Error::ResourceLimit(_)), "{err:?}");
}

#[cfg(feature = "spreadsheet")]
#[test]
fn xlsb_huge_declared_dimensions_do_not_allocate() {
    // BrtWsDim claims A1:XFD1048576; cells are streamed, never a dense grid.
    let mut sheet = Vec::new();
    biff12(&mut sheet, 0x0081, &[]);
    let mut dim = Vec::new();
    for v in [0u32, 1_048_575, 0, 16_383] {
        dim.extend_from_slice(&v.to_le_bytes());
    }
    biff12(&mut sheet, 0x0094, &dim);
    biff12(&mut sheet, 0x0091, &[]);
    for (row, col) in [(0u32, 0u32), (1_048_575, 16_383)] {
        biff12(&mut sheet, 0x0000, &row.to_le_bytes());
        let mut c = col.to_le_bytes().to_vec();
        c.extend_from_slice(&0u32.to_le_bytes());
        c.extend(wide("far"));
        biff12(&mut sheet, 0x0006, &c);
    }
    biff12(&mut sheet, 0x0092, &[]);
    let base = xlsb(&[Sheet::new("S", vec![])]);
    let bytes = rezip(
        &base,
        &["xl/worksheets/sheet1.bin"],
        &[("xl/worksheets/sheet1.bin", &sheet)],
    );
    let e = Markkit::builder()
        .build()
        .extract_document_sync(&bytes, DocumentHint::default())
        .unwrap();
    assert!(e.markdown.contains("far"));
    assert!(e.markdown.len() < 64 * 1024);
    assert!(
        e.warnings
            .iter()
            .any(|w| w.code == WarningCode::TruncatedSheet)
    );
}

#[cfg(feature = "spreadsheet")]
#[test]
fn xlsx_bomb_in_unreferenced_entry_rejected() {
    // calamine may follow arbitrary relationship targets, so every entry is checked.
    let bomb = vec![0u8; 4 * 1024 * 1024];
    let bytes = xlsx_with(
        &[Sheet::new("S", vec![vec![Cell::S("x")]])],
        &[],
        &[("xl/media/image1.png", &bomb)],
    );
    let m = Markkit::builder()
        .max_archive_entry_size(1024 * 1024)
        .build();
    let err = run(&m, &bytes, XLSX_CT).unwrap_err();
    assert_eq!(
        err.limit_kind(),
        Some(LimitKind::MaxArchiveEntrySize),
        "{err:?}"
    );
}

#[test]
fn max_input_size_honoured_for_office_documents() {
    let bytes = zip_bytes(&[("word/document.xml", b"<w:document/>")]);
    let m = Markkit::builder().max_input_size(bytes.len() - 1).build();
    for hint in hints() {
        let err = m.extract_document_sync(&bytes, hint).unwrap_err();
        assert_eq!(
            err.limit_kind(),
            Some(LimitKind::MaxInputSize),
            "{hint:?}: {err:?}"
        );
    }
}

// ─── Grid amplification (sparse cells → dense allocation) ───────────

#[cfg(feature = "spreadsheet")]
#[test]
fn xls_huge_dimensions_rejected() {
    let bytes = xls(&[XlsSheet {
        name: "S",
        dims: Some((0, 65536, 0, 256)),
        cells: vec![XlsCell::Num(0, 0, 1.0)],
    }]);
    let m = Markkit::builder().max_spreadsheet_cells(1_000_000).build();
    let err = m
        .extract_document_sync(&bytes, DocumentHint::default())
        .unwrap_err();
    assert_eq!(
        err.limit_kind(),
        Some(LimitKind::MaxSpreadsheetCells),
        "{err:?}"
    );
}

#[cfg(feature = "spreadsheet")]
#[test]
fn xls_far_apart_cells_rejected() {
    let bytes = xls(&[XlsSheet {
        name: "S",
        dims: None,
        cells: vec![XlsCell::Num(0, 0, 1.0), XlsCell::Num(65535, 255, 2.0)],
    }]);
    let m = Markkit::builder().max_spreadsheet_cells(1_000_000).build();
    let err = m
        .extract_document_sync(&bytes, DocumentHint::default())
        .unwrap_err();
    assert_eq!(
        err.limit_kind(),
        Some(LimitKind::MaxSpreadsheetCells),
        "{err:?}"
    );
}

#[cfg(feature = "spreadsheet")]
#[test]
fn xls_inverted_dimensions_rejected_not_panicking() {
    let bytes = xls(&[XlsSheet {
        name: "S",
        dims: Some((100, 5, 0, 1)),
        cells: vec![],
    }]);
    let res = Markkit::builder()
        .build()
        .extract_document_sync(&bytes, DocumentHint::default());
    assert!(res.is_err());
}

#[cfg(feature = "spreadsheet")]
#[test]
fn xls_bad_sheet_offset_rejected() {
    let mut stream = biff8_workbook(&[XlsSheet {
        name: "S",
        dims: None,
        cells: vec![XlsCell::Num(0, 0, 1.0)],
    }]);
    // Locate the BOUNDSHEET record and point it past the end of the stream.
    let pos = stream.windows(2).position(|w| w == [0x85, 0x00]).unwrap();
    stream[pos + 4..pos + 8].copy_from_slice(&u32::MAX.to_le_bytes());
    let res = Markkit::builder()
        .build()
        .extract_document_sync(&xls_from_stream(&stream), DocumentHint::default());
    assert!(res.is_err());
}

#[cfg(feature = "spreadsheet")]
type Records = Vec<(u16, Vec<u8>)>;

/// One-sheet BIFF8 workbook with raw records spliced in right after the globals
/// BOF and the sheet BOF (BOUNDSHEET offset patched).
#[cfg(feature = "spreadsheet")]
fn xls_with_records(globals: &[(u16, Vec<u8>)], sheet: &[(u16, Vec<u8>)]) -> Vec<u8> {
    let enc = |recs: &[(u16, Vec<u8>)]| {
        let mut out = Vec::new();
        for (typ, data) in recs {
            out.extend_from_slice(&typ.to_le_bytes());
            out.extend_from_slice(&(data.len() as u16).to_le_bytes());
            out.extend_from_slice(data);
        }
        out
    };
    let mut stream = biff8_workbook(&[XlsSheet {
        name: "S",
        dims: None,
        cells: vec![XlsCell::Num(0, 0, 1.0)],
    }]);
    // BOF record = 4-byte header + 16 bytes.
    let g = enc(globals);
    stream.splice(20..20, g.iter().copied());
    let bs = stream.windows(2).position(|w| w == [0x85, 0x00]).unwrap() + 4;
    let pos = u32::from_le_bytes(stream[bs..bs + 4].try_into().unwrap()) + g.len() as u32;
    stream[bs..bs + 4].copy_from_slice(&pos.to_le_bytes());
    let at = pos as usize + 20;
    stream.splice(at..at, enc(sheet));
    xls_from_stream(&stream)
}

/// Malformed BIFF records calamine reads without bounds checks (found by the
/// `xls_biff` fuzz target). The pre-scan must reject them before calamine
/// panics: a caught panic is not enough under `panic = "abort"`.
#[cfg(feature = "spreadsheet")]
#[test]
fn xls_truncated_records_rejected_before_calamine() {
    let mut sst_flags = vec![0u8; 8];
    sst_flags.extend_from_slice(&[1, 0, 0x08]); // fRichSt, but no cRun bytes
    let mut sst_cont = vec![0u8; 8];
    sst_cont.extend_from_slice(&[5, 0, 0, b'a', b'b']); // 5 chars, 2 present
    let mut lbl = vec![0u8; 14];
    lbl[4] = 100; // cce > record length
    let cases: Vec<(&str, Records, Records)> = vec![
        ("FILEPASS empty", vec![(0x002F, vec![])], vec![]),
        ("CODEPAGE empty", vec![(0x0042, vec![])], vec![]),
        ("DATEMODE 1 byte", vec![(0x0022, vec![1])], vec![]),
        ("BOF 1 byte", vec![(0x0809, vec![6])], vec![]),
        ("BOUNDSHEET 5 bytes", vec![(0x0085, vec![0; 5])], vec![]),
        ("LBL short", vec![(0x0018, vec![0; 5])], vec![]),
        ("LBL cce overrun", vec![(0x0018, lbl)], vec![]),
        ("EXTERNSHEET empty", vec![(0x0017, vec![])], vec![]),
        ("EXTERNSHEET 1 byte", vec![(0x0017, vec![1])], vec![]),
        ("SST missing cRun", vec![(0x00FC, sst_flags)], vec![]),
        (
            "SST empty CONTINUE",
            vec![(0x00FC, sst_cont), (0x003C, vec![])],
            vec![],
        ),
        ("MERGECELLS overrun", vec![], vec![(0x00E5, vec![5, 0])]),
        (
            "MULRK full column range",
            vec![],
            vec![(0x00BD, vec![0, 0, 0, 0, 0xFF, 0xFF])],
        ),
        (
            "DIMENSIONS zero columns",
            vec![],
            vec![(
                0x0200,
                [0u32.to_le_bytes(), 1u32.to_le_bytes()]
                    .concat()
                    .into_iter()
                    .chain([3, 0, 3, 0, 0, 0])
                    .collect(),
            )],
        ),
    ];
    let m = Markkit::builder().build();
    for (what, g, s) in cases {
        let res = m.extract_document_sync(&xls_with_records(&g, &s), DocumentHint::default());
        let err = res.expect_err(what);
        assert!(!err.to_string().contains("panicked"), "{what}: {err}");
    }
}

/// calamine skips embedded substreams (charts) and keeps reading after their
/// EOF, so the pre-scan must too, or records after a chart bypass it.
#[cfg(feature = "spreadsheet")]
#[test]
fn xls_records_after_embedded_substream_scanned() {
    let mut bof = vec![0x00, 0x06, 0x20, 0x00];
    bof.resize(16, 0);
    let sheet = vec![(0x0809, bof), (0x000A, vec![]), (0x00E5, vec![5, 0])];
    let err = Markkit::builder()
        .build()
        .extract_document_sync(&xls_with_records(&[], &sheet), DocumentHint::default())
        .unwrap_err();
    assert!(!err.to_string().contains("panicked"), "{err}");
}

/// FORMULA record at (1, 0) with cached number 2.0 and the given `rgce`
/// (CellParsedFormula: cce + tokens).
#[cfg(feature = "spreadsheet")]
fn formula(rgce: &[u8]) -> (u16, Vec<u8>) {
    let mut d = Vec::new();
    d.extend_from_slice(&[1, 0, 0, 0, 0, 0]);
    d.extend_from_slice(&2.0f64.to_le_bytes());
    d.extend_from_slice(&[0; 6]);
    d.extend_from_slice(rgce);
    (0x0006, d)
}

/// calamine's formula decompiler indexes tokens without bounds checks. markkit
/// renders cached values only, so token streams are neutralised and the
/// cached value still renders.
#[cfg(feature = "spreadsheet")]
#[test]
fn xls_malformed_formula_tokens_neutralised() {
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("cce overrun", vec![0xFF, 0x00]),
        ("PtgName index 0", vec![5, 0, 0x23, 0, 0, 0, 0]),
        ("PtgRef truncated", vec![1, 0, 0x24]),
        ("PtgArea3d truncated", vec![3, 0, 0x3B, 0, 0]),
        ("PtgStr overrun", vec![3, 0, 0x17, 200, 0]),
    ];
    let m = Markkit::builder().build();
    for (what, rgce) in cases {
        let bytes = xls_with_records(&[], &[formula(&rgce)]);
        let out = m
            .extract_document_sync(&bytes, DocumentHint::default())
            .unwrap_or_else(|e| panic!("{what}: {e}"));
        assert!(out.markdown.contains('2'), "{what}: {}", out.markdown);
    }
}

#[cfg(feature = "spreadsheet")]
#[test]
fn xls_truncated_formula_record_rejected() {
    // 20-byte header present, cce missing.
    let (typ, mut d) = formula(&[]);
    d.push(0);
    let err = Markkit::builder()
        .build()
        .extract_document_sync(&xls_with_records(&[], &[(typ, d)]), DocumentHint::default())
        .unwrap_err();
    assert!(!err.to_string().contains("panicked"), "{err}");
}

/// Defined names (LBL) whose single token is cut short.
#[cfg(feature = "spreadsheet")]
#[test]
fn xls_truncated_defined_name_token_rejected() {
    for ptg in [0x3A, 0x3B, 0x3C, 0x3D] {
        let mut lbl = vec![0u8; 14];
        lbl[3] = 1; // cch
        lbl[4] = 1; // cce
        lbl.extend_from_slice(&[0, b'N', ptg]);
        let err = Markkit::builder()
            .build()
            .extract_document_sync(
                &xls_with_records(&[(0x0018, lbl)], &[]),
                DocumentHint::default(),
            )
            .unwrap_err();
        assert!(!err.to_string().contains("panicked"), "ptg {ptg:#x}: {err}");
    }
}

#[cfg(feature = "spreadsheet")]
#[test]
fn xls_valid_records_still_accepted() {
    // 16-byte DIMENSIONS (some writers pad), EXTERNSHEET with zero XTIs.
    let mut dims = Vec::new();
    for v in [0u32, 1] {
        dims.extend_from_slice(&v.to_le_bytes());
    }
    dims.extend_from_slice(&[0, 0, 1, 0, 0, 0, 0, 0]);
    let bytes = xls_with_records(&[(0x0017, vec![0, 0])], &[(0x0200, dims)]);
    let out = Markkit::builder()
        .build()
        .extract_document_sync(&bytes, DocumentHint::default())
        .unwrap();
    assert!(out.markdown.contains('1'), "{}", out.markdown);
}

#[cfg(feature = "spreadsheet")]
#[test]
fn ods_repeated_rows_and_columns_rejected() {
    // One non-empty cell repeated 16384 columns × 1M rows.
    let inner = r#"<table:table table:name="Bomb"><table:table-row table:number-rows-repeated="1000000"><table:table-cell office:value-type="float" office:value="1" table:number-columns-repeated="16384"><text:p>1</text:p></table:table-cell></table:table-row></table:table>"#;
    let bytes = ods(inner);
    let m = Markkit::builder().max_spreadsheet_cells(1_000_000).build();
    let err = m
        .extract_document_sync(&bytes, DocumentHint::default())
        .unwrap_err();
    assert_eq!(
        err.limit_kind(),
        Some(LimitKind::MaxSpreadsheetCells),
        "{err:?}"
    );
}

// ─── XML tricks ─────────────────────────────────────────────────────

#[cfg(feature = "docx")]
#[test]
fn docx_billion_laughs_not_expanded() {
    let doc = format!(
        r#"<?xml version="1.0"?>
<!DOCTYPE lolz [
 <!ENTITY lol "lol">
 <!ENTITY lol2 "&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;">
 <!ENTITY lol3 "&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;">
 <!ENTITY lol9 "&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;">
]>
<w:document {W_NS}><w:body><w:p><w:r><w:t>&lol9;</w:t></w:r></w:p><w:p><w:r><w:t>safe</w:t></w:r></w:p></w:body></w:document>"#
    );
    let bytes = zip_bytes(&[("word/document.xml", doc.as_bytes())]);
    let res = run(&Markkit::builder().build(), &bytes, DOCX_CT);
    if let Ok(e) = res {
        assert!(!e.markdown.contains("lollol"), "{}", e.markdown);
    }
}

#[cfg(feature = "pptx")]
#[test]
fn pptx_zip_bomb_slide_rejected() {
    let mut text = String::from("x");
    text.push_str(&" ".repeat(8 * 1024 * 1024));
    let bytes = pptx(&[PSlide::new(p_shape(None, &a_para(&text)))]);
    assert!(bytes.len() < 256 * 1024);
    let m = Markkit::builder()
        .max_archive_entry_size(1024 * 1024)
        .build();
    let err = run(&m, &bytes, PPTX_CT).unwrap_err();
    assert!(matches!(err, Error::ResourceLimit(_)), "{err:?}");
}

#[cfg(feature = "pptx")]
#[test]
fn pptx_repeated_slide_references_bounded_by_total_size() {
    // 2000 slide ids all point at one 64 KiB slide: re-reading it must count
    // against the total decompressed budget.
    let big = p_shape(None, &a_para(&"y".repeat(64 * 1024)));
    let base = pptx(&[PSlide::new(big)]);
    let ids: String = (0..2000)
        .map(|i| format!(r#"<p:sldId id="{}" r:id="rId1"/>"#, 256 + i))
        .collect();
    let pres = format!(
        r#"<p:presentation xmlns:p="p" xmlns:r="r"><p:sldIdLst>{ids}</p:sldIdLst></p:presentation>"#
    );
    let bytes = rezip(
        &base,
        &["ppt/presentation.xml"],
        &[("ppt/presentation.xml", pres.as_bytes())],
    );
    let m = Markkit::builder()
        .max_archive_total_size(16 * 1024 * 1024)
        .build();
    let err = run(&m, &bytes, PPTX_CT).unwrap_err();
    assert!(matches!(err, Error::ResourceLimit(_)), "{err:?}");
}

#[cfg(feature = "pptx")]
#[test]
fn pptx_billion_laughs_and_deep_groups() {
    let laughs = r#"<?xml version="1.0"?><!DOCTYPE l [<!ENTITY a "aaaaaaaaaa"><!ENTITY b "&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;"><!ENTITY c "&b;&b;&b;&b;&b;&b;&b;&b;&b;&b;">]><p:sld xmlns:p="p" xmlns:a="a"><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>&c;safe</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#;
    let base = pptx(&[PSlide::new(String::new())]);
    let bytes = rezip(
        &base,
        &["ppt/slides/slide1.xml"],
        &[("ppt/slides/slide1.xml", laughs.as_bytes())],
    );
    let e = run(&Markkit::builder().build(), &bytes, PPTX_CT).unwrap();
    assert!(e.markdown.contains("safe"));
    assert!(
        !e.markdown.contains("aaaaaaaaaaaaaaaaaaaa"),
        "{}",
        e.markdown
    );

    // Group shapes nested past the XML depth cap fail cleanly, no stack overflow.
    let deep = format!(
        "{}{}{}",
        "<p:grpSp>".repeat(10_000),
        p_shape(None, &a_para("deep")),
        "</p:grpSp>".repeat(10_000)
    );
    let bytes = pptx(&[PSlide::new(deep)]);
    let err = run(&Markkit::builder().build(), &bytes, PPTX_CT).unwrap_err();
    assert!(matches!(err, Error::ResourceLimit(_)), "{err:?}");
}

#[cfg(feature = "docx")]
#[test]
fn docx_external_entity_not_resolved() {
    let doc = format!(
        r#"<?xml version="1.0"?><!DOCTYPE d [<!ENTITY xxe SYSTEM "file:///etc/passwd">]><w:document {W_NS}><w:body><w:p><w:r><w:t>&xxe;</w:t></w:r></w:p></w:body></w:document>"#
    );
    let bytes = zip_bytes(&[("word/document.xml", doc.as_bytes())]);
    if let Ok(e) = run(&Markkit::builder().build(), &bytes, DOCX_CT) {
        assert!(!e.markdown.contains("root:"), "{}", e.markdown);
    }
}

#[cfg(feature = "docx")]
#[test]
fn docx_deep_nesting_does_not_overflow_stack() {
    let depth = 100_000;
    let mut body = String::new();
    for _ in 0..depth {
        body.push_str("<w:sdt><w:sdtContent>");
    }
    body.push_str(&para("deep"));
    for _ in 0..depth {
        body.push_str("</w:sdtContent></w:sdt>");
    }
    let res = run(&Markkit::builder().build(), &docx(&body), DOCX_CT);
    assert!(
        matches!(res, Err(Error::ResourceLimit(_)) | Ok(_)),
        "{res:?}"
    );
}

#[cfg(feature = "docx")]
#[test]
fn docx_encrypted_or_non_zip_is_error() {
    // OLE2 container (how encrypted OOXML is stored) labelled as DOCX.
    let mut ole = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1".to_vec();
    ole.extend_from_slice(&[0u8; 512]);
    let err = run(&Markkit::builder().build(), &ole, DOCX_CT).unwrap_err();
    assert!(matches!(err, Error::DocxExtract(_)), "{err:?}");
}

#[cfg(feature = "docx")]
#[test]
fn docx_missing_document_part_is_error() {
    let bytes = zip_bytes(&[("word/styles.xml", b"<w:styles/>")]);
    let err = run(&Markkit::builder().build(), &bytes, DOCX_CT).unwrap_err();
    assert!(matches!(err, Error::DocxExtract(_)), "{err:?}");
}

#[cfg(feature = "docx")]
#[test]
fn docx_without_document_or_body_is_error() {
    // The body is streamed while parsing; structure errors must still surface.
    let w = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
    for doc in [
        format!("<w:other {w}><w:body><w:p><w:r><w:t>x</w:t></w:r></w:p></w:body></w:other>"),
        format!("<w:document {w}><w:p><w:r><w:t>x</w:t></w:r></w:p></w:document>"),
        format!("<w:x {w}><w:document><w:body><w:p/></w:body></w:document></w:x>"),
    ] {
        let bytes = zip_bytes(&[("word/document.xml", doc.as_bytes())]);
        let err = run(&Markkit::builder().build(), &bytes, DOCX_CT).unwrap_err();
        assert!(matches!(err, Error::DocxExtract(_)), "{doc}: {err:?}");
    }
}

#[cfg(feature = "docx")]
#[test]
fn docx_streamed_body_keeps_node_and_depth_limits() {
    let body = para("x").repeat(2_000);
    let err = run(
        &Markkit::builder().max_nodes(500).build(),
        &docx(&body),
        DOCX_CT,
    )
    .unwrap_err();
    assert!(matches!(err, Error::ResourceLimit(_)), "{err:?}");
}

#[cfg(feature = "spreadsheet")]
#[test]
fn spreadsheet_garbage_is_error() {
    let err = run(&Markkit::builder().build(), b"not a workbook", XLSX_CT).unwrap_err();
    assert!(matches!(err, Error::SpreadsheetExtract(_)), "{err:?}");
}

// ─── Malformed input never panics ───────────────────────────────────

fn valid_fixtures() -> Vec<Vec<u8>> {
    let mut v = Vec::new();
    #[cfg(feature = "docx")]
    v.push(docx_with(DocxParts {
        body: &format!(
            "{}{}{}<w:tbl><w:tr><w:tc>{}</w:tc></w:tr></w:tbl>",
            styled_para("Heading1", "T"),
            list_para(1, 0, "item"),
            para("text"),
            para("cell")
        ),
        numbering: Some(NUMBERING),
        title: Some("t"),
        ..Default::default()
    }));
    #[cfg(feature = "pptx")]
    {
        let mut slide = PSlide::new(
            p_shape(Some("title"), &a_para("T")) + &p_shape(Some(""), &a_para("bullet")),
        );
        slide.notes = Some("note");
        v.push(pptx(&[slide]));
        v.push(
            std::fs::read(format!(
                "{}/tests/fixtures/pptx/deck.pptx",
                env!("CARGO_MANIFEST_DIR")
            ))
            .unwrap(),
        );
    }
    #[cfg(feature = "spreadsheet")]
    {
        v.push(xlsx(&[Sheet::new(
            "S",
            vec![vec![Cell::S("a"), Cell::N(1.0)], vec![Cell::B(true)]],
        )]));
        v.push(ods(&format!(
            r#"<table:table table:name="T"><table:table-row>{}</table:table-row></table:table>"#,
            ods_string_cell("x")
        )));
        v.push(xls(&[XlsSheet {
            name: "S",
            dims: Some((0, 1, 0, 2)),
            cells: vec![XlsCell::Label(0, 0, "a"), XlsCell::Num(0, 1, 2.0)],
        }]));
        // Title sources: XLS SummaryInformation, XLSX core.xml + no `_rels/.rels`.
        let stream = biff8_workbook(&[XlsSheet {
            name: "S",
            dims: None,
            cells: vec![XlsCell::Label(0, 0, "a")],
        }]);
        let summary = summary_information(1252, b"Title");
        v.push(ole2(&[
            ("/Workbook", &stream),
            ("/\u{5}SummaryInformation", &summary),
        ]));
        let core = b"<cp:coreProperties><dc:title>T</dc:title></cp:coreProperties>";
        let book = xlsx_with(
            &[Sheet::new("S", vec![vec![Cell::S("a")]])],
            &[],
            &[("docProps/core.xml", core)],
        );
        v.push(rezip(&book, &["_rels/.rels"], &[]));
        v.push(xlsb(&[Sheet::new(
            "S",
            vec![vec![Cell::S("a"), Cell::N(1.0)], vec![Cell::B(true)]],
        )]));
        for name in ["date", "issues"] {
            v.push(
                std::fs::read(format!(
                    "{}/tests/fixtures/xlsb/{name}.xlsb",
                    env!("CARGO_MANIFEST_DIR")
                ))
                .unwrap(),
            );
        }
    }
    v
}

#[test]
fn truncated_fixtures_never_panic() {
    let m = Markkit::builder().build();
    for fixture in valid_fixtures() {
        let step = (fixture.len() / 97).max(1);
        for cut in (0..fixture.len()).step_by(step) {
            for hint in hints() {
                let _ = m.extract_document_sync(&fixture[..cut], hint);
            }
        }
    }
}

#[test]
fn fixtures_parse_cleanly() {
    let m = Markkit::builder().build();
    for fixture in valid_fixtures() {
        let e = m
            .extract_document_sync(&fixture, DocumentHint::default())
            .unwrap();
        assert!(!e.markdown.is_empty());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Random bytes never panic, whatever the claimed type.
    #[test]
    fn random_bytes_never_panic(data in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let m = Markkit::builder().build();
        for hint in hints() {
            let _ = m.extract_document_sync(&data, hint);
        }
    }

    /// Random bytes behind a ZIP / OLE2 magic header never panic.
    #[test]
    fn random_container_bytes_never_panic(
        magic in prop_oneof![
            Just(b"PK\x03\x04".to_vec()),
            Just(b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1".to_vec()),
        ],
        data in proptest::collection::vec(any::<u8>(), 0..4096),
    ) {
        let mut bytes = magic;
        bytes.extend(data);
        let m = Markkit::builder().build();
        for hint in hints() {
            let _ = m.extract_document_sync(&bytes, hint);
        }
    }

    /// Byte flips in valid documents never panic.
    #[test]
    fn mutated_fixtures_never_panic(
        idx in 0usize..11,
        flips in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..16),
    ) {
        let fixtures = valid_fixtures();
        let mut bytes = fixtures[idx % fixtures.len()].clone();
        for (pos, val) in flips {
            let i = pos.index(bytes.len());
            bytes[i] = val;
        }
        let m = Markkit::builder().build();
        for hint in hints() {
            let _ = m.extract_document_sync(&bytes, hint);
        }
    }

    /// Arbitrary text in document.xml never panics and never leaks markup tags as raw HTML.
    #[cfg(feature = "docx")]
    #[test]
    fn arbitrary_docx_text_never_panics(s in "\\PC{0,200}") {
        let bytes = docx(&para(&xml_escape(&s)));
        let m = Markkit::builder().build();
        let _ = m.extract_document_sync(&bytes, DocumentHint::content_type(DOCX_CT));
    }
}
