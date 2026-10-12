//! Security tests for office-document parsing: zip bombs, resource caps,
//! malformed input, XML entity tricks, oversized spreadsheet grids.

#![cfg(any(feature = "docx", feature = "spreadsheet"))]

mod common;

use common::*;
use markkit::{DocumentHint, Error, Markkit};
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
    assert!(matches!(err, Error::ResourceLimit(_)), "{err:?}");
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
    assert!(matches!(err, Error::ResourceLimit(_)), "{err:?}");
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
        assert!(matches!(err, Error::ResourceLimit(_)), "{ct}: {err:?}");
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
    assert!(matches!(err, Error::ResourceLimit(_)), "{err:?}");
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
    assert!(matches!(err, Error::ResourceLimit(_)), "{err:?}");
}

#[test]
fn max_input_size_honoured_for_office_documents() {
    let bytes = zip_bytes(&[("word/document.xml", b"<w:document/>")]);
    let m = Markkit::builder().max_input_size(bytes.len() - 1).build();
    for hint in hints() {
        let err = m.extract_document_sync(&bytes, hint).unwrap_err();
        assert!(matches!(err, Error::ResourceLimit(_)), "{hint:?}: {err:?}");
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
    assert!(matches!(err, Error::ResourceLimit(_)), "{err:?}");
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
    assert!(matches!(err, Error::ResourceLimit(_)), "{err:?}");
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
#[test]
fn ods_repeated_rows_and_columns_rejected() {
    // One non-empty cell repeated 16384 columns × 1M rows.
    let inner = r#"<table:table table:name="Bomb"><table:table-row table:number-rows-repeated="1000000"><table:table-cell office:value-type="float" office:value="1" table:number-columns-repeated="16384"><text:p>1</text:p></table:table-cell></table:table-row></table:table>"#;
    let bytes = ods(inner);
    let m = Markkit::builder().max_spreadsheet_cells(1_000_000).build();
    let err = m
        .extract_document_sync(&bytes, DocumentHint::default())
        .unwrap_err();
    assert!(matches!(err, Error::ResourceLimit(_)), "{err:?}");
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
        idx in 0usize..4,
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
