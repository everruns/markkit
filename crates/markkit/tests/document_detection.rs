//! Format detection and bytes-in API tests (feature-independent).

use markkit::format::Format;
use markkit::{DocumentHint, Error, Markkit};

const PK: &[u8] = b"PK\x03\x04";

fn fake_zip(names: &[&str]) -> Vec<u8> {
    // Only names matter for sniffing; content need not be a valid archive.
    let mut v = PK.to_vec();
    for n in names {
        v.extend_from_slice(b"\0\0\0\0");
        v.extend_from_slice(n.as_bytes());
    }
    v
}

#[test]
fn content_type_detection() {
    let cases = [
        ("text/html; charset=utf-8", Format::Html),
        ("application/pdf", Format::Pdf),
        (
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            Format::Docx,
        ),
        ("application/msword", Format::Doc),
        (
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            Format::Xlsx,
        ),
        (
            "application/vnd.ms-excel.sheet.macroEnabled.12",
            Format::Xlsx,
        ),
        ("application/vnd.ms-excel", Format::Xls),
        (
            "application/vnd.oasis.opendocument.spreadsheet",
            Format::Ods,
        ),
        (
            "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            Format::Pptx,
        ),
        ("APPLICATION/PDF", Format::Pdf),
        ("application/octet-stream", Format::Unknown),
    ];
    for (ct, f) in cases {
        assert_eq!(Format::from_content_type(ct), f, "{ct}");
    }
}

#[test]
fn extension_detection() {
    let cases = [
        ("docx", Format::Docx),
        ("DOCM", Format::Docx),
        ("doc", Format::Doc),
        ("xlsx", Format::Xlsx),
        ("xlsm", Format::Xlsx),
        ("xls", Format::Xls),
        ("ods", Format::Ods),
        ("pptx", Format::Pptx),
        ("pdf", Format::Pdf),
    ];
    for (ext, f) in cases {
        assert_eq!(Format::from_extension(ext), f, "{ext}");
    }
}

#[test]
fn magic_byte_sniffing() {
    assert_eq!(Format::sniff(b"%PDF-1.7\n..."), Format::Pdf);
    assert_eq!(
        Format::sniff(&fake_zip(&["[Content_Types].xml", "word/document.xml"])),
        Format::Docx
    );
    assert_eq!(
        Format::sniff(&fake_zip(&["[Content_Types].xml", "xl/workbook.xml"])),
        Format::Xlsx
    );
    assert_eq!(
        Format::sniff(&fake_zip(&["[Content_Types].xml", "ppt/presentation.xml"])),
        Format::Pptx
    );
    assert_eq!(
        Format::sniff(&fake_zip(&[
            "mimetypeapplication/vnd.oasis.opendocument.spreadsheet"
        ])),
        Format::Ods
    );
    assert_eq!(Format::sniff(&fake_zip(&["foo.txt"])), Format::Unknown);
    assert_eq!(
        Format::sniff(b"\xEF\xBB\xBF  <!DOCTYPE html><p>x"),
        Format::Html
    );
    assert_eq!(Format::sniff(b"<html><body>"), Format::Html);
    assert_eq!(Format::sniff(b"just text"), Format::Unknown);
    assert_eq!(Format::sniff(b""), Format::Unknown);
}

#[test]
fn ole2_sniffing_distinguishes_doc_and_xls() {
    let mut cfb = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1".to_vec();
    cfb.extend_from_slice(&[0u8; 100]);
    let utf16 = |s: &str| {
        s.encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<u8>>()
    };

    let mut xls = cfb.clone();
    xls.extend(utf16("Workbook"));
    assert_eq!(Format::sniff(&xls), Format::Xls);

    let mut doc = cfb.clone();
    doc.extend(utf16("WordDocument"));
    assert_eq!(Format::sniff(&doc), Format::Doc);

    assert_eq!(Format::sniff(&cfb), Format::Unknown);
}

#[test]
fn detect_priority_content_type_then_extension_then_magic() {
    let pdf = b"%PDF-1.4";
    // Content type wins.
    assert_eq!(
        Format::detect(
            pdf,
            DocumentHint {
                content_type: Some("text/html"),
                file_name: Some("a.docx"),
            }
        ),
        Format::Html
    );
    // Generic content type falls through to extension.
    assert_eq!(
        Format::detect(
            pdf,
            DocumentHint {
                content_type: Some("application/octet-stream"),
                file_name: Some("/files/report.docx?download=1#x"),
            }
        ),
        Format::Docx
    );
    // Unknown extension falls through to magic bytes.
    assert_eq!(
        Format::detect(pdf, DocumentHint::file_name("blob.bin")),
        Format::Pdf
    );
    assert_eq!(Format::detect(pdf, DocumentHint::default()), Format::Pdf);
}

#[test]
fn format_names() {
    assert_eq!(Format::Docx.as_str(), "docx");
    assert_eq!(Format::Xlsx.to_string(), "xlsx");
    assert_eq!(Format::Ods.as_str(), "ods");
}

#[test]
fn extract_document_html() {
    let m = Markkit::builder().build();
    let res = m.extract_document_sync(
        b"<h1>Hi</h1><p>there</p>",
        DocumentHint::content_type("text/html"),
    );
    #[cfg(feature = "html")]
    {
        let e = res.unwrap();
        assert_eq!(e.format, Format::Html);
        assert!(e.markdown.contains("# Hi"));
    }
    #[cfg(not(feature = "html"))]
    assert!(matches!(res, Err(Error::UnsupportedFormat(_))));
}

#[test]
fn extract_document_unknown_is_unsupported() {
    let m = Markkit::builder().build();
    let err = m
        .extract_document_sync(b"random bytes", DocumentHint::default())
        .unwrap_err();
    assert!(matches!(err, Error::UnsupportedFormat(_)), "{err:?}");
}

#[test]
fn extract_document_legacy_doc_and_pptx_unsupported() {
    let m = Markkit::builder().build();
    for ct in [
        "application/msword",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ] {
        let err = m
            .extract_document_sync(b"whatever", DocumentHint::content_type(ct))
            .unwrap_err();
        assert!(matches!(err, Error::UnsupportedFormat(_)), "{ct}: {err:?}");
    }
}

#[test]
fn pdf_is_explicitly_unsupported_not_empty() {
    let m = Markkit::builder().build();
    let err = m
        .extract_document_sync(b"%PDF-1.4\n%%EOF", DocumentHint::default())
        .unwrap_err();
    assert!(matches!(err, Error::UnsupportedFormat(_)), "{err:?}");

    let rt = tokio::runtime::Runtime::new().unwrap();
    let err = rt
        .block_on(m.extract_bytes(b"%PDF-1.4", Some("application/pdf")))
        .unwrap_err();
    assert!(matches!(err, Error::UnsupportedFormat(_)), "{err:?}");
}

#[test]
fn max_input_size_enforced_before_detection() {
    let m = Markkit::builder().max_input_size(4).build();
    let err = m
        .extract_document_sync(b"<p>too long</p>", DocumentHint::content_type("text/html"))
        .unwrap_err();
    assert!(matches!(err, Error::ResourceLimit(_)), "{err:?}");
}

#[cfg(not(feature = "docx"))]
#[test]
fn docx_without_feature_is_unsupported() {
    let m = Markkit::builder().build();
    let err = m
        .extract_document_sync(b"PK\x03\x04", DocumentHint::file_name("a.docx"))
        .unwrap_err();
    assert!(matches!(err, Error::UnsupportedFormat(_)), "{err:?}");
}

#[cfg(not(feature = "spreadsheet"))]
#[test]
fn spreadsheet_without_feature_is_unsupported() {
    let m = Markkit::builder().build();
    let err = m
        .extract_document_sync(b"PK\x03\x04", DocumentHint::file_name("a.xlsx"))
        .unwrap_err();
    assert!(matches!(err, Error::UnsupportedFormat(_)), "{err:?}");
}
