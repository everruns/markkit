//! Native OCR integration; requires installed Tesseract and language data.
#![cfg(all(feature = "pdf-ocr", not(target_arch = "wasm32")))]
mod common;
use common::*;
use markkit::{DocumentHint, Markkit};
fn scanned_pdf() -> Vec<u8> {
    // Generate a scanned page with the Rust renderer; no PDF executable needed.
    let source = pdf_pages(&["BT /F1 36 Tf 72 700 Td (MARKKIT OCR TEST) Tj ET"]);
    let pdf = hayro_syntax::Pdf::new(source).unwrap();
    let raster = hayro::render(
        &pdf.pages()[0],
        &hayro::RenderCache::new(),
        &Default::default(),
        &Default::default(),
        &hayro::PixmapSettings {
            x_scale: 2.0,
            y_scale: 2.0,
            bg_color: hayro::vello_cpu::color::palette::css::WHITE,
        },
    );
    let (width, height) = (1224, 1584);
    let pixels = raster
        .data_as_u8_slice()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| p[0])
        .collect::<Vec<_>>();
    assert_eq!(pixels.len(), width * height);
    pdf_bytes(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /XObject << /Im 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
        pdf_stream("",b"q 612 0 0 792 0 0 cm /Im Do Q"),
        pdf_stream(&format!("/Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode"),&zlib(&pixels)),
    ])
}

#[test]
#[ignore = "requires local tesseract and eng language data; run explicitly for OCR changes"]
fn image_only_pdf_is_recognized_with_positioned_words() {
    let bytes = scanned_pdf();
    let plain = Markkit::builder()
        .build()
        .extract_document_sync(&bytes, DocumentHint::default())
        .unwrap();
    assert!(plain.markdown.is_empty());
    let e = Markkit::builder()
        .pdf_ocr(true)
        .pdf_layout(true)
        .max_processing_secs(180)
        .build()
        .extract_document_sync(&bytes, DocumentHint::default())
        .unwrap();
    assert!(
        e.markdown.contains("MARKKIT") && e.markdown.contains("OCR") && e.markdown.contains("TEST"),
        "{}",
        e.markdown
    );
    assert!(
        e.layout.unwrap().pages[0]
            .spans
            .iter()
            .all(|s| s.ocr && !s.words.is_empty())
    );
}
#[test]
fn invalid_ocr_language_is_rejected_without_spawning_helpers() {
    let bytes = pdf_pages(&[""]);
    let err = Markkit::builder()
        .pdf_ocr(true)
        .pdf_ocr_language("eng;touch unsafe")
        .build()
        .extract_document_sync(&bytes, DocumentHint::default())
        .unwrap_err();
    assert!(matches!(err, markkit::Error::PdfExtract(_)));
}

#[test]
fn raster_budget_is_checked_before_helper_execution() {
    let error = Markkit::builder()
        .pdf_ocr(true)
        .max_pdf_stream_size(1024)
        .build()
        .extract_document_sync(&pdf_pages(&[""]), DocumentHint::default())
        .unwrap_err();
    match error {
        markkit::Error::ResourceLimit(limit) => {
            assert_eq!(limit.kind, markkit::LimitKind::MaxPdfStreamSize)
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn text_pages_skip_ocr_renderer_preflight() {
    let bytes = pdf_bytes(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_vec(),
        pdf_stream("", b"BT /F1 12 Tf 72 700 Td (Existing text) Tj ET"),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        pdf_stream("/Subtype /Image /Width 1 /Height 1 /Filter /JBIG2Decode", b"unused"),
    ]);
    let result = Markkit::builder()
        .pdf_ocr(true)
        .build()
        .extract_document_sync(&bytes, DocumentHint::default())
        .unwrap();
    assert_eq!(result.markdown.trim(), "Existing text");
}

#[test]
fn unknown_ocr_backend_is_rejected() {
    assert!("mystery".parse::<markkit::PdfOcrBackend>().is_err());
    assert_eq!(
        "ocrs".parse::<markkit::PdfOcrBackend>().unwrap(),
        markkit::PdfOcrBackend::Ocrs
    );
}

#[cfg(not(feature = "pdf-ocr-ocrs"))]
#[test]
fn ocrs_requires_compiled_backend() {
    let error = Markkit::builder()
        .pdf_ocr(true)
        .pdf_ocr_backend(markkit::PdfOcrBackend::Ocrs)
        .build()
        .extract_document_sync(&pdf_pages(&[""]), DocumentHint::default())
        .unwrap_err();
    assert!(error.to_string().contains("pdf-ocr-ocrs"), "{error}");
}

#[cfg(feature = "pdf-ocr-ocrs")]
#[test]
fn ocrs_requires_explicit_models_and_supported_language() {
    let bytes = pdf_pages(&[""]);
    let error = Markkit::builder()
        .pdf_ocr(true)
        .pdf_ocr_backend(markkit::PdfOcrBackend::Ocrs)
        .build()
        .extract_document_sync(&bytes, DocumentHint::default())
        .unwrap_err();
    assert!(error.to_string().contains("model"), "{error}");
    let error = Markkit::builder()
        .pdf_ocr(true)
        .pdf_ocr_backend(markkit::PdfOcrBackend::Ocrs)
        .pdf_ocr_language("eng+deu")
        .build()
        .extract_document_sync(&bytes, DocumentHint::default())
        .unwrap_err();
    assert!(error.to_string().contains("eng"), "{error}");
}

#[cfg(feature = "pdf-ocr-ocrs")]
#[test]
#[ignore = "requires pinned local ONNX models; set MARKKIT_OCR_MODEL_DIR"]
fn ocrs_recognizes_scanned_text_with_source_word_geometry() {
    let models = std::path::PathBuf::from(
        std::env::var_os("MARKKIT_OCR_MODEL_DIR").expect("set MARKKIT_OCR_MODEL_DIR"),
    );
    let result = Markkit::builder()
        .pdf_ocr(true)
        .pdf_ocr_backend(markkit::PdfOcrBackend::Ocrs)
        .pdf_ocr_detection_model(models.join("text-detection.onnx"))
        .pdf_ocr_recognition_model(models.join("text-recognition.onnx"))
        .pdf_layout(true)
        .max_processing_secs(180)
        .build()
        .extract_document_sync(&scanned_pdf(), DocumentHint::default())
        .unwrap();
    assert!(
        result.markdown.contains("MARKKIT")
            && result.markdown.contains("OCR")
            && result.markdown.contains("TEST"),
        "{}",
        result.markdown
    );
    let layout = result.layout.unwrap();
    let spans = &layout.pages[0].spans;
    assert!(spans.len() >= 3);
    for (id, span) in spans.iter().enumerate() {
        assert_eq!(span.id, id);
        assert!(span.ocr && !span.words.is_empty());
        // Authored text is at x=72,y=700 (PDF bottom origin), in 36pt type.
        // Independent bounds enclose the known rasterized line after display transform.
        assert!(
            span.bbox[0] >= 65.0 && span.bbox[2] <= 500.0,
            "{:?}",
            span.bbox
        );
        assert!(
            span.bbox[1] >= 50.0 && span.bbox[3] <= 105.0,
            "{:?}",
            span.bbox
        );
    }
}

#[cfg(feature = "pdf-ocr-ocrs")]
#[test]
fn ocrs_model_size_and_invalid_data_fail_explicitly() {
    let dir = tempfile::tempdir().unwrap();
    let model = dir.path().join("model.onnx");
    std::fs::write(&model, vec![0_u8; 1024 * 1024 + 1]).unwrap();
    let builder = Markkit::builder()
        .pdf_ocr(true)
        .pdf_ocr_backend(markkit::PdfOcrBackend::Ocrs)
        .pdf_ocr_detection_model(&model)
        .pdf_ocr_recognition_model(&model)
        .max_pdf_decoded_size(1024 * 1024);
    let error = builder
        .clone()
        .build()
        .extract_document_sync(&pdf_pages(&[""]), DocumentHint::default())
        .unwrap_err();
    assert!(
        matches!(error, markkit::Error::ResourceLimit(ref limit) if limit.kind == markkit::LimitKind::MaxPdfDecodedSize)
    );
    std::fs::write(&model, b"not an ONNX model").unwrap();
    let error = builder
        .build()
        .extract_document_sync(&pdf_pages(&[""]), DocumentHint::default())
        .unwrap_err();
    assert!(matches!(error, markkit::Error::PdfExtract(_)), "{error}");
}
