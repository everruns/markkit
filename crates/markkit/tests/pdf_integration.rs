//! PDF text-layer extraction: positive mapping tests on generated fixtures.

#![cfg(feature = "pdf")]

mod common;

use common::*;
use markkit::{DocumentHint, Format, Markkit};

fn extract(bytes: &[u8]) -> markkit::Extraction {
    Markkit::builder()
        .build()
        .extract_document_sync(bytes, DocumentHint::content_type("application/pdf"))
        .unwrap()
}

#[test]
fn paragraphs_and_heading_from_font_size() {
    let pdf = pdf_pages(&[concat!(
        "BT /F1 24 Tf 72 720 Td (Annual Report) Tj ET\n",
        "BT /F1 11 Tf 13 TL 72 680 Td (First paragraph line one) Tj T* (continues here.) Tj ET\n",
        "BT /F1 11 Tf 13 TL 72 640 Td (Second paragraph.) Tj T* (Also second.) Tj ET\n",
    )]);
    let e = extract(&pdf);
    assert_eq!(e.format, Format::Pdf);
    assert_eq!(
        e.markdown.trim(),
        "# Annual Report\n\nFirst paragraph line one continues here.\n\nSecond paragraph. Also second."
    );
    assert_eq!(e.stats.pages, Some(1));
    assert!(e.warnings.is_empty(), "{:?}", e.warnings);
}

#[test]
fn detected_by_magic_bytes_without_hint() {
    let pdf = pdf_pages(&["BT /F1 12 Tf 72 700 Td (Sniffed) Tj ET"]);
    let e = Markkit::builder()
        .build()
        .extract_document_sync(&pdf, DocumentHint::default())
        .unwrap();
    assert_eq!(e.format, Format::Pdf);
    assert_eq!(e.markdown.trim(), "Sniffed");
}

#[test]
fn extract_bytes_returns_markdown() {
    let pdf = pdf_pages(&["BT /F1 12 Tf 72 700 Td (Via bytes) Tj ET"]);
    let rt = tokio::runtime::Runtime::new().unwrap();
    let md = rt
        .block_on(
            Markkit::builder()
                .build()
                .extract_bytes(&pdf, Some("application/pdf")),
        )
        .unwrap();
    assert_eq!(md.trim(), "Via bytes");
}

#[test]
fn title_from_info_dictionary() {
    let content = "BT /F1 12 Tf 72 700 Td (Body) Tj ET";
    let objs = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [4 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>"
            .to_vec(),
        pdf_stream("", content.as_bytes()),
        b"<< /Title (\xFE\xFF\x00Q\x00 \x04\x1A) >>".to_vec(),
    ];
    let e = extract(&pdf_bytes_with_trailer(&objs, "/Info 6 0 R"));
    assert_eq!(e.title.as_deref(), Some("Q К"));
}

#[test]
fn flate_compressed_content_and_win_ansi_high_bytes() {
    let content = b"BT /F1 12 Tf 72 700 Td (Caf\xE9 \x93quoted\x94 \x97 done) Tj ET";
    let objs = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [4 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>"
            .to_vec(),
        pdf_stream("/Filter /FlateDecode", &zlib(content)),
    ];
    assert_eq!(
        extract(&pdf_bytes(&objs)).markdown.trim(),
        "Café “quoted” — done"
    );
}

#[test]
fn differences_encoding_and_glyph_names() {
    let font = "<< /Type /Font /Subtype /Type1 /BaseFont /Custom /Encoding << /BaseEncoding /WinAnsiEncoding /Differences [65 /Eacute /uni0416 /f_i] >> >>";
    let pdf = pdf_pages_with(
        &["BT /F2 12 Tf 72 700 Td (ABC x) Tj ET"],
        "/F2 7 0 R",
        &[b"<< >>".to_vec(), font.as_bytes().to_vec()],
    );
    assert_eq!(extract(&pdf).markdown.trim(), "ÉЖfi x");
}

#[test]
fn type0_font_with_to_unicode_cmap() {
    let cmap = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
2 beginbfchar <0001> <041F> <0002> <0440> endbfchar\n\
1 beginbfrange <0003> <0005> <0438> endbfrange\n\
endcmap end end";
    let font = "<< /Type /Font /Subtype /Type0 /BaseFont /Sub+Font /Encoding /Identity-H /DescendantFonts [9 0 R] /ToUnicode 8 0 R >>";
    let desc = "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Sub+Font /DW 600 /W [1 [700 500] 3 5 550] >>";
    let pdf = pdf_pages_with(
        &["BT /F2 12 Tf 72 700 Td <00010002000300040005> Tj ET"],
        "/F2 7 0 R",
        &[
            b"<< >>".to_vec(),
            font.as_bytes().to_vec(),
            pdf_stream("", cmap),
            desc.as_bytes().to_vec(),
        ],
    );
    assert_eq!(extract(&pdf).markdown.trim(), "Прийк");
}

#[test]
fn tj_kerning_gap_becomes_space() {
    let pdf = pdf_pages(&["BT /F1 12 Tf 72 700 Td [(Hello) -600 (world) -20 (!)] TJ ET"]);
    assert_eq!(extract(&pdf).markdown.trim(), "Hello world!");
}

#[test]
fn separate_show_ops_on_one_baseline_join_with_spaces() {
    let pdf = pdf_pages(&[concat!(
        "BT /F1 12 Tf 72 700 Td (Left) Tj ET\n",
        "BT /F1 12 Tf 200 700 Td (Right) Tj ET\n",
    )]);
    assert_eq!(extract(&pdf).markdown.trim(), "Left Right");
}

#[test]
fn text_matrix_and_ctm_scaling() {
    // 2x CTM scale on 9pt text = 18pt rendered, so it is a heading over 9pt body.
    let pdf = pdf_pages(&[concat!(
        "q 2 0 0 2 0 0 cm BT /F1 9 Tf 36 360 Td (Scaled) Tj ET Q\n",
        "BT /F1 9 Tf 1 0 0 1 72 600 Tm (body text here) Tj ET\n",
    )]);
    assert_eq!(extract(&pdf).markdown.trim(), "# Scaled\n\nbody text here");
}

#[test]
fn bullets_become_list() {
    let pdf = pdf_pages(&[
        "BT /F1 12 Tf 14 TL 72 700 Td (Intro:) Tj T* (\\225 first) Tj T* (\\225 second) Tj ET",
    ]);
    assert_eq!(extract(&pdf).markdown.trim(), "Intro:\n\n- first\n- second");
}

#[test]
fn hyphenated_line_break_is_joined() {
    let pdf = pdf_pages(&["BT /F1 12 Tf 14 TL 72 700 Td (extrac-) Tj T* (tion works) Tj ET"]);
    assert_eq!(extract(&pdf).markdown.trim(), "extraction works");
}

#[test]
fn form_xobject_text_is_extracted() {
    let form = pdf_stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 600 800] /Resources << /Font << /F1 3 0 R >> >>",
        b"BT /F1 12 Tf 72 700 Td (From a form) Tj ET",
    );
    let pdf = pdf_pages_with(&["/X1 Do"], "/F1 3 0 R", &[form]);
    assert_eq!(extract(&pdf).markdown.trim(), "From a form");
}

#[test]
fn multiple_pages_and_page_cap() {
    let pdf = pdf_pages(&[
        "BT /F1 12 Tf 72 700 Td (Page one) Tj ET",
        "BT /F1 12 Tf 72 700 Td (Page two) Tj ET",
        "BT /F1 12 Tf 72 700 Td (Page three) Tj ET",
    ]);
    let e = extract(&pdf);
    assert_eq!(e.markdown.trim(), "Page one\n\nPage two\n\nPage three");
    assert_eq!(e.stats.pages, Some(3));

    let e = Markkit::builder()
        .max_pdf_pages(1)
        .build()
        .extract_document_sync(&pdf, DocumentHint::default())
        .unwrap();
    assert_eq!(e.markdown.trim(), "Page one");
    assert_eq!(e.stats.pages, Some(1));
    assert!(
        e.warnings[0].starts_with("truncated_pdf:"),
        "{:?}",
        e.warnings
    );
}

#[test]
fn contents_array_is_one_stream() {
    let objs = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [4 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 3 0 R >> >> /Contents [5 0 R 6 0 R] >>"
            .to_vec(),
        pdf_stream("", b"BT /F1 12 Tf 72 700 Td (Split"),
        pdf_stream("", b") Tj ET"),
    ];
    // A string split across content streams is malformed but must not panic.
    let _ = extract(&pdf_bytes(&objs));

    let objs2 = vec![
        objs[0].clone(),
        objs[1].clone(),
        objs[2].clone(),
        objs[3].clone(),
        pdf_stream("", b"BT /F1 12 Tf 72 700 Td"),
        pdf_stream("", b"(Joined) Tj ET"),
    ];
    assert_eq!(extract(&pdf_bytes(&objs2)).markdown.trim(), "Joined");
}

#[test]
fn unmapped_composite_font_is_skipped_with_warning() {
    let font = "<< /Type /Font /Subtype /Type0 /BaseFont /NoMap /Encoding /Identity-H /DescendantFonts [8 0 R] >>";
    let desc = "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /NoMap >>";
    let pdf = pdf_pages_with(
        &["BT /F2 12 Tf 72 700 Td <00010002> Tj ET BT /F1 12 Tf 72 650 Td (Visible) Tj ET"],
        "/F1 3 0 R /F2 7 0 R",
        &[
            b"<< >>".to_vec(),
            font.as_bytes().to_vec(),
            desc.as_bytes().to_vec(),
        ],
    );
    let e = extract(&pdf);
    assert_eq!(e.markdown.trim(), "Visible");
    assert_eq!(e.warnings.len(), 1, "{:?}", e.warnings);
    assert!(e.warnings[0].starts_with("pdf_unmapped_font: "));
    assert!(e.warnings[0].contains("NoMap"));
}

#[test]
fn page_without_text_warns() {
    let e = extract(&pdf_pages(&["0 0 m 100 100 l S"]));
    assert_eq!(e.markdown.trim(), "");
    assert!(
        e.warnings[0].starts_with("pdf_no_text:"),
        "{:?}",
        e.warnings
    );
}

#[test]
fn missing_font_resource_falls_back_to_win_ansi() {
    let pdf = pdf_pages(&["BT /Nope 12 Tf 72 700 Td (Still here) Tj ET"]);
    assert_eq!(extract(&pdf).markdown.trim(), "Still here");
}
