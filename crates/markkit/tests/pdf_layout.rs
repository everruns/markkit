//! Positioned PDF text is opt-in, deterministic and independent of Markdown.
#![cfg(feature = "pdf")]
mod common;
use common::*;
use markkit::{DocumentHint, Markkit};

fn extract(bytes: &[u8]) -> markkit::Extraction {
    Markkit::builder()
        .pdf_layout(true)
        .build()
        .extract_document_sync(bytes, DocumentHint::default())
        .unwrap()
}

fn page_with(attrs: &str, content: &str) -> Vec<u8> {
    pdf_bytes(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [4 0 R] /Count 1 /MediaBox [0 0 612 792] >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        format!("<< /Type /Page /Parent 2 0 R {attrs} /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>").into_bytes(),
        pdf_stream("", content.as_bytes()),
    ])
}

fn near(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 0.001, "{actual} != {expected}");
}

#[test]
fn layout_is_opt_in_and_survives_rendering() {
    let bytes = pdf_pages(&["BT /F1 10 Tf 72 700 Td (Hello) Tj ET"]);
    let plain = Markkit::builder()
        .build()
        .extract_document_sync(&bytes, DocumentHint::default())
        .unwrap();
    assert!(plain.layout.is_none());
    assert!(
        serde_json::to_value(&plain)
            .unwrap()
            .get("layout")
            .is_none()
    );
    let parsed = Markkit::builder()
        .pdf_layout(true)
        .build()
        .parse_document_sync(&bytes, DocumentHint::default())
        .unwrap();
    let layout = parsed.layout.clone().unwrap();
    let e = parsed.render();
    assert_eq!(e.markdown, plain.markdown);
    assert_eq!(e.layout.as_ref(), Some(&layout));
    assert_eq!(layout.version, 1);
    let p = &layout.pages[0];
    assert_eq!(
        (p.page, p.width, p.height, p.rotation),
        (1, 612.0, 792.0, 0)
    );
    let s = &p.spans[0];
    assert_eq!(
        (s.id, s.text.as_str(), s.font.as_deref()),
        (0, "Hello", Some("Helvetica"))
    );
    assert_eq!(s.baseline, [[72.0, 92.0], [97.0, 92.0]]);
    assert_eq!(s.bbox, [72.0, 84.0, 97.0, 94.0]);
    near(s.size, 10.0);
    near(s.angle, 0.0);
    let json = serde_json::to_string(&layout).unwrap();
    assert_eq!(
        serde_json::from_str::<markkit::layout::PdfLayout>(&json).unwrap(),
        layout
    );
}

#[test]
fn coordinates_account_for_crop_rotation_and_user_unit() {
    for (rotation, width, height, start, end, angle) in [
        (0, 800.0, 1200.0, [100.0, 1000.0], [150.0, 1000.0], 0.0),
        (90, 1200.0, 800.0, [200.0, 100.0], [200.0, 150.0], 90.0),
        (180, 800.0, 1200.0, [700.0, 200.0], [650.0, 200.0], 180.0),
        (270, 1200.0, 800.0, [1000.0, 700.0], [1000.0, 650.0], 270.0),
    ] {
        let bytes = page_with(
            &format!("/CropBox [50 100 450 700] /Rotate {rotation} /UserUnit 2"),
            "BT /F1 10 Tf 100 200 Td (Hello) Tj ET",
        );
        let p = extract(&bytes).layout.unwrap().pages.remove(0);
        assert_eq!((p.width, p.height, p.rotation), (width, height, rotation));
        assert_eq!(p.spans[0].baseline, [start, end]);
        near(p.spans[0].size, 20.0);
        near(p.spans[0].angle, angle);
    }
}

#[test]
fn rotated_and_reflected_runs_have_nonempty_bounds() {
    let bytes = pdf_pages(&[
        "BT /F1 10 Tf 0 1 -1 0 100 200 Tm (Hello) Tj ET\nBT /F1 10 Tf -1 0 0 1 200 300 Tm (Hello) Tj ET",
    ]);
    let p = extract(&bytes).layout.unwrap().pages.remove(0);
    assert_eq!(p.spans[0].baseline, [[100.0, 592.0], [100.0, 567.0]]);
    near(p.spans[0].angle, 270.0);
    assert_eq!(p.spans[1].baseline, [[200.0, 492.0], [175.0, 492.0]]);
    for s in p.spans {
        assert!(s.bbox[0] < s.bbox[2] && s.bbox[1] < s.bbox[3]);
    }
}

#[test]
fn inherited_page_boxes_and_negative_rotation_are_normalized() {
    let bytes = pdf_bytes(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [4 0 R] /Count 1 /MediaBox [10 20 610 820] /CropBox [20 40 500 700] /Rotate -90 >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>".to_vec(),
        pdf_stream("", b"BT /F1 10 Tf 100 200 Td (Hello) Tj ET"),
    ]);
    let page = extract(&bytes).layout.unwrap().pages.remove(0);
    assert_eq!(
        (page.width, page.height, page.rotation),
        (660.0, 480.0, 270)
    );
    assert_eq!(page.spans[0].baseline, [[500.0, 400.0], [500.0, 375.0]]);
}

#[test]
fn page_ids_empty_pages_and_page_cap() {
    let bytes = pdf_pages(&[
        "BT /F1 10 Tf 72 700 Td (one) Tj (two) Tj ET",
        "",
        "BT /F1 10 Tf 72 700 Td (three) Tj ET",
    ]);
    let p = extract(&bytes).layout.unwrap().pages;
    assert_eq!(p.len(), 3);
    assert_eq!(p[0].spans.iter().map(|s| s.id).collect::<Vec<_>>(), [0, 1]);
    assert!(p[1].spans.is_empty());
    assert_eq!((p[2].page, p[2].spans[0].id), (3, 0));
    let capped = Markkit::builder()
        .pdf_layout(true)
        .max_pdf_pages(1)
        .build()
        .extract_document_sync(&bytes, DocumentHint::default())
        .unwrap();
    assert_eq!(capped.layout.unwrap().pages.len(), 1);
    assert_eq!(capped.warnings[0].code, markkit::WarningCode::TruncatedPdf);
    assert_eq!(extract(&bytes), extract(&bytes));
}

#[test]
fn form_matrix_and_tj_adjustments_preserve_individual_runs() {
    let form = pdf_stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 600 800] /Matrix [2 0 0 2 10 20]",
        b"BT /F1 10 Tf 10 100 Td [(Hi) -500 (there)] TJ ET",
    );
    let bytes = pdf_pages_with(&["/X1 Do"], "/F1 3 0 R", &[form]);
    let p = extract(&bytes).layout.unwrap().pages.remove(0);
    assert_eq!(p.spans.len(), 2);
    assert_eq!(p.spans[0].text, "Hi");
    assert_eq!(p.spans[1].text, "there");
    assert_eq!(p.spans[0].baseline, [[30.0, 572.0], [50.0, 572.0]]);
    assert_eq!(p.spans[1].baseline[0], [60.0, 572.0]);
    assert_eq!(extract(&bytes).markdown.trim(), "Hi there");
}

#[test]
fn layout_is_absent_for_non_pdf_inputs() {
    let e = Markkit::builder()
        .pdf_layout(true)
        .build()
        .extract_document_sync(b"plain text", DocumentHint::file_name("notes.txt"))
        .unwrap();
    assert!(e.layout.is_none());
}

#[test]
fn security_layout_limits_apply_to_parse_only_callers() {
    let bytes = pdf_pages(&["BT /F1 10 Tf 72 700 Td (Hello) Tj (world) Tj ET"]);
    for (builder, kind) in [
        (
            Markkit::builder().max_nodes(2),
            markkit::limits::LimitKind::MaxNodes,
        ),
        (
            Markkit::builder().max_output_size(100),
            markkit::limits::LimitKind::MaxOutputSize,
        ),
        (
            Markkit::builder().max_pdf_decoded_size(300),
            markkit::limits::LimitKind::MaxPdfDecodedSize,
        ),
        (
            Markkit::builder().timeout(std::time::Duration::ZERO),
            markkit::limits::LimitKind::Timeout,
        ),
    ] {
        let err = builder
            .pdf_layout(true)
            .build()
            .parse_document_sync(&bytes, DocumentHint::default())
            .unwrap_err();
        assert!(
            matches!(err, markkit::Error::ResourceLimit(ref e) if e.kind == kind),
            "{err:?}"
        );
    }
    // The compact JSON cap is exact, including escaping, page metadata and IDs.
    let size = serde_json::to_vec(&extract(&bytes).layout.unwrap())
        .unwrap()
        .len();
    assert!(
        Markkit::builder()
            .pdf_layout(true)
            .max_output_size(size)
            .build()
            .parse_document_sync(&bytes, DocumentHint::default())
            .is_ok()
    );
    assert!(
        Markkit::builder()
            .pdf_layout(true)
            .max_output_size(size - 1)
            .build()
            .parse_document_sync(&bytes, DocumentHint::default())
            .is_err()
    );
}

#[test]
fn security_invalid_page_dimensions_are_rejected() {
    for attrs in [
        "/UserUnit -1",
        "/UserUnit 0",
        "/UserUnit 75001",
        "/CropBox [0 0 0 0]",
        "/CropBox [700 800 900 1000]",
    ] {
        let bytes = page_with(attrs, "BT /F1 10 Tf 72 700 Td (Hello) Tj ET");
        let err = Markkit::builder()
            .pdf_layout(true)
            .build()
            .parse_document_sync(&bytes, DocumentHint::default())
            .unwrap_err();
        assert!(
            matches!(err, markkit::Error::PdfExtract(_)),
            "{attrs}: {err:?}"
        );
    }
}

#[test]
fn security_extreme_transforms_never_publish_nonfinite_coordinates() {
    let bytes = page_with(
        "/UserUnit 75000",
        "q 1e30 0 0 1e30 0 0 cm BT /F1 10 Tf 72 700 Td (Huge) Tj ET Q",
    );
    // Malformed or overflowing operators may be skipped or fail, but a
    // successful extraction must always serialize finite numeric geometry.
    if let Ok(e) = Markkit::builder()
        .pdf_layout(true)
        .build()
        .extract_document_sync(&bytes, DocumentHint::default())
    {
        let value = serde_json::to_value(e.layout.unwrap()).unwrap();
        for p in value["pages"].as_array().unwrap() {
            assert!(p["width"].is_number() && p["height"].is_number());
            for s in p["spans"].as_array().unwrap() {
                assert!(
                    s["bbox"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(serde_json::Value::is_number)
                );
            }
        }
    }
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(64))]
    #[test]
    fn security_arbitrary_layout_streams_never_panic(content in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..2048)) {
        let bytes = page_with("", &String::from_utf8_lossy(&content));
        let _ = Markkit::builder().pdf_layout(true).max_nodes(1000).max_output_size(65536).build().extract_document_sync(&bytes, DocumentHint::default());
    }
}
