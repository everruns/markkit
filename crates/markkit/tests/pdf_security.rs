//! PDF security tests (threat model T8): decompression bombs, interpretation
//! amplification, unverifiable/encrypted input, malformed files never panic.

#![cfg(feature = "pdf")]

mod common;

use common::*;
use markkit::{DocumentHint, Error, Markkit};
use proptest::prelude::*;

fn run(m: &Markkit, bytes: &[u8]) -> markkit::Result<markkit::Extraction> {
    m.extract_document_sync(bytes, DocumentHint::content_type("application/pdf"))
}

fn catalog_page(contents: &str) -> Vec<Vec<u8>> {
    vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [4 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 3 0 R >> /XObject << /X1 6 0 R >> >> /Contents {contents} >>"
        )
        .into_bytes(),
    ]
}

// ─── Decompression bombs (T8a) ──────────────────────────────────────

#[test]
fn flate_bomb_content_stream_rejected() {
    // 80 MiB of spaces deflates to ~80 KiB; the per-stream cap is 64 MiB.
    let bomb = zlib(&vec![b' '; 80 * 1024 * 1024]);
    let mut objs = catalog_page("5 0 R");
    objs.push(pdf_stream("/Filter /FlateDecode", &bomb));
    let err = run(&Markkit::builder().build(), &pdf_bytes(&objs)).unwrap_err();
    assert!(matches!(err, Error::ResourceLimit(_)), "{err:?}");
}

#[test]
fn bomb_in_unreferenced_object_stream_rejected_before_parsing() {
    // hayro would decode an object stream while resolving objects; the
    // pre-scan must catch it even though nothing references it as content.
    let bomb = zlib(&vec![b'0'; 2 * 1024 * 1024]);
    let mut objs = catalog_page("5 0 R");
    objs.push(pdf_stream("", b"BT /F1 12 Tf 72 700 Td (ok) Tj ET"));
    objs.push(pdf_stream(
        "/Type /ObjStm /N 1 /First 4 /Filter /FlateDecode",
        &bomb,
    ));
    let m = Markkit::builder().max_pdf_stream_size(1024 * 1024).build();
    let err = run(&m, &pdf_bytes(&objs)).unwrap_err();
    assert!(matches!(err, Error::ResourceLimit(_)), "{err:?}");
}

#[test]
fn chained_filters_are_measured_after_every_stage() {
    // ASCIIHex wrapping a Flate bomb: the raw hex never inflates on its own.
    let z = zlib(&vec![b'a'; 4 * 1024 * 1024]);
    let hex: String = z.iter().map(|b| format!("{b:02x}")).collect();
    let mut objs = catalog_page("5 0 R");
    objs.push(pdf_stream(
        "/Filter [/AHx /Fl]",
        format!("{hex}>").as_bytes(),
    ));
    let m = Markkit::builder().max_pdf_stream_size(1024 * 1024).build();
    let err = run(&m, &pdf_bytes(&objs)).unwrap_err();
    assert!(matches!(err, Error::ResourceLimit(_)), "{err:?}");
}

#[test]
fn escaped_filter_name_cannot_evade_scan() {
    let bomb = zlib(&vec![b' '; 2 * 1024 * 1024]);
    let mut objs = catalog_page("5 0 R");
    objs.push(pdf_stream("/Filter /Fl#61teDecode", &bomb));
    let m = Markkit::builder().max_pdf_stream_size(1024 * 1024).build();
    let err = run(&m, &pdf_bytes(&objs)).unwrap_err();
    assert!(matches!(err, Error::ResourceLimit(_)), "{err:?}");
}

#[test]
fn total_decoded_size_capped_across_streams() {
    let chunk = zlib(&vec![b' '; 600 * 1024]);
    let mut objs = catalog_page("5 0 R");
    objs.push(pdf_stream("/Filter /FlateDecode", &chunk));
    objs.push(pdf_stream("/Filter /FlateDecode", &chunk));
    let m = Markkit::builder()
        .max_pdf_stream_size(1024 * 1024)
        .max_pdf_decoded_size(1024 * 1024)
        .build();
    let err = run(&m, &pdf_bytes(&objs)).unwrap_err();
    assert!(matches!(err, Error::ResourceLimit(_)), "{err:?}");
    // Each stream alone is fine.
    assert!(run(&Markkit::builder().build(), &pdf_bytes(&objs)).is_ok());
}

#[test]
fn run_length_bomb_rejected() {
    let rl: Vec<u8> = std::iter::repeat_n([129u8, b' '], 20_000)
        .flatten()
        .collect(); // 2.5 MB
    let mut objs = catalog_page("5 0 R");
    objs.push(pdf_stream("/Filter /RunLengthDecode", &rl));
    let m = Markkit::builder().max_pdf_stream_size(1024 * 1024).build();
    assert!(matches!(
        run(&m, &pdf_bytes(&objs)),
        Err(Error::ResourceLimit(_))
    ));
}

// ─── Interpretation amplification (T8b) ─────────────────────────────

#[test]
fn form_xobject_reuse_hits_interpretation_budget() {
    let form_body = "BT /F1 12 Tf 72 700 Td (form text) Tj ET\n".repeat(200);
    let page = "/X1 Do\n".repeat(5_000);
    let mut objs = catalog_page("5 0 R");
    objs.push(pdf_stream("", page.as_bytes()));
    objs.push(pdf_stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 600 800] /Resources << /Font << /F1 3 0 R >> >>",
        form_body.as_bytes(),
    ));
    let m = Markkit::builder()
        .max_pdf_decoded_size(4 * 1024 * 1024)
        .build();
    let err = run(&m, &pdf_bytes(&objs)).unwrap_err();
    assert!(matches!(err, Error::ResourceLimit(_)), "{err:?}");
}

#[test]
fn self_referencing_form_does_not_recurse_forever() {
    let mut objs = catalog_page("5 0 R");
    objs.push(pdf_stream("", b"/X1 Do BT /F1 12 Tf 72 700 Td (top) Tj ET"));
    objs.push(pdf_stream(
        "/Type /XObject /Subtype /Form /Resources << /XObject << /X1 6 0 R >> >>",
        b"/X1 Do",
    ));
    let e = run(&Markkit::builder().build(), &pdf_bytes(&objs)).unwrap();
    assert_eq!(e.markdown.trim(), "top");
}

#[test]
fn repeated_contents_reference_is_charged_each_time() {
    let content = "BT /F1 12 Tf 72 700 Td (x) Tj ET\n".repeat(1000);
    let refs = "5 0 R ".repeat(2000);
    let mut objs = catalog_page(&format!("[{refs}]"));
    objs.push(pdf_stream("", content.as_bytes()));
    let m = Markkit::builder()
        .max_pdf_decoded_size(8 * 1024 * 1024)
        .build();
    let err = run(&m, &pdf_bytes(&objs)).unwrap_err();
    assert!(matches!(err, Error::ResourceLimit(_)), "{err:?}");
}

#[test]
fn deep_save_state_nesting_and_degenerate_matrices() {
    let content = format!(
        "{}{} BT /F1 1e38 Tf 1e38 0 0 1e38 1e38 1e38 Tm (big) Tj ET 0 0 0 0 0 0 cm BT /F1 12 Tf (zero) Tj ET",
        "q ".repeat(100_000),
        "Q ".repeat(100_000)
    );
    let mut objs = catalog_page("5 0 R");
    objs.push(pdf_stream("", content.as_bytes()));
    let _ = run(&Markkit::builder().build(), &pdf_bytes(&objs)).unwrap();
}

#[test]
fn huge_cmap_range_and_width_arrays_are_cheap() {
    let cmap = b"begincodespacerange <0000> <FFFF> endcodespacerange beginbfrange <0000> <FFFF> <0041> endbfrange";
    let font = "<< /Type /Font /Subtype /Type0 /BaseFont /F /Encoding /Identity-H /DescendantFonts [8 0 R] /ToUnicode 7 0 R >>";
    let desc = "<< /Type /Font /Subtype /CIDFontType2 /W [0 4294967295 500] >>";
    let mut objs = catalog_page("5 0 R");
    objs[3] =
        b"<< /Type /Page /Parent 2 0 R /Resources << /Font << /F2 6 0 R >> >> /Contents 5 0 R >>"
            .to_vec();
    objs.push(pdf_stream(
        "",
        b"BT /F2 12 Tf 72 700 Td <00000001FFFF> Tj ET",
    ));
    objs.push(font.as_bytes().to_vec());
    objs.push(pdf_stream("", cmap));
    objs.push(desc.as_bytes().to_vec());
    let e = run(&Markkit::builder().build(), &pdf_bytes(&objs)).unwrap();
    assert!(e.markdown.trim().starts_with("AB"), "{}", e.markdown);
}

// ─── Refused input ──────────────────────────────────────────────────

#[test]
fn encrypted_pdf_is_explicitly_unsupported() {
    let mut objs = catalog_page("5 0 R");
    objs.push(pdf_stream("", b"BT /F1 12 Tf 72 700 Td (secret) Tj ET"));
    objs.push(b"<< /Filter /Standard /V 1 /R 2 /O <00> /U <00> /P -4 >>".to_vec());
    let pdf = pdf_bytes_with_trailer(&objs, "/Encrypt 6 0 R /ID [<00> <00>]");
    let err = run(&Markkit::builder().build(), &pdf).unwrap_err();
    assert!(matches!(err, Error::UnsupportedFormat(_)), "{err:?}");
}

#[test]
fn indirect_filter_is_refused_not_trusted() {
    let mut objs = catalog_page("5 0 R");
    objs.push(pdf_stream("/Filter 6 0 R", &zlib(b"BT ET")));
    objs.push(b"/FlateDecode".to_vec());
    let err = run(&Markkit::builder().build(), &pdf_bytes(&objs)).unwrap_err();
    assert!(matches!(err, Error::PdfExtract(_)), "{err:?}");
}

#[test]
fn not_a_pdf_is_an_error_not_a_panic() {
    let m = Markkit::builder().build();
    for bytes in [
        &b""[..],
        b"%PDF-",
        b"%PDF-1.7\n%%EOF",
        b"%PDF-1.4\n1 0 obj << >> endobj",
    ] {
        assert!(run(&m, bytes).is_err(), "{bytes:?}");
    }
}

#[test]
fn max_input_size_honoured_for_pdf() {
    let pdf = pdf_pages(&["BT /F1 12 Tf 72 700 Td (x) Tj ET"]);
    let m = Markkit::builder().max_input_size(pdf.len() - 1).build();
    assert!(matches!(run(&m, &pdf), Err(Error::ResourceLimit(_))));
}

// ─── Malformed input never panics (S9) ──────────────────────────────

fn fixtures() -> Vec<Vec<u8>> {
    let cmap = b"begincodespacerange <0000> <FFFF> endcodespacerange 1 beginbfchar <0001> <0041> endbfchar";
    let font = "<< /Type /Font /Subtype /Type0 /BaseFont /F /Encoding /Identity-H /DescendantFonts [<< /Subtype /CIDFontType2 /W [1 [500]] >>] /ToUnicode 7 0 R >>";
    let mut type0 = catalog_page("5 0 R");
    type0[3] =
        b"<< /Type /Page /Parent 2 0 R /Resources << /Font << /F2 6 0 R >> >> /Contents 5 0 R >>"
            .to_vec();
    type0.push(pdf_stream(
        "/Filter /FlateDecode",
        &zlib(b"BT /F2 12 Tf 72 700 Td <0001> Tj ET"),
    ));
    type0.push(font.as_bytes().to_vec());
    type0.push(pdf_stream("", cmap));

    let mut form = catalog_page("5 0 R");
    form.push(pdf_stream("", b"q 1 0 0 1 10 10 cm /X1 Do Q"));
    form.push(pdf_stream(
        "/Type /XObject /Subtype /Form /Resources << /Font << /F1 3 0 R >> >>",
        b"BT /F1 12 Tf [(a) -500 (b)] TJ ET",
    ));
    vec![
        pdf_pages(&[
            "BT /F1 18 Tf 72 720 Td (Title) Tj ET BT /F1 11 Tf 14 TL 72 680 Td (one) Tj T* (two) Tj ET",
            "BT /F1 11 Tf 72 700 Td (\\225 item) Tj ET",
        ]),
        pdf_bytes(&type0),
        pdf_bytes(&form),
    ]
}

#[test]
fn fixtures_parse_cleanly() {
    let m = Markkit::builder().build();
    for f in fixtures() {
        let e = run(&m, &f).unwrap();
        assert!(!e.markdown.trim().is_empty());
    }
}

#[test]
fn truncated_pdfs_never_panic() {
    let m = Markkit::builder().build();
    for f in fixtures() {
        for cut in 0..f.len() {
            let _ = run(&m, &f[..cut]);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn random_bytes_after_header_never_panic(data in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let mut bytes = b"%PDF-1.7\n".to_vec();
        bytes.extend(data);
        let _ = run(&Markkit::builder().build(), &bytes);
    }

    #[test]
    fn mutated_pdfs_never_panic(
        idx in 0usize..3,
        flips in proptest::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..16),
    ) {
        let mut bytes = fixtures()[idx].clone();
        for (pos, val) in flips {
            let i = pos.index(bytes.len());
            bytes[i] = val;
        }
        let _ = run(&Markkit::builder().build(), &bytes);
    }

    #[test]
    fn arbitrary_content_stream_never_panics(content in "[ -~\\n]{0,400}") {
        let _ = run(&Markkit::builder().build(), &pdf_pages(&[&content]));
    }
}

// ─── Shared limits ──────────────────────────────────────────────────

#[test]
fn zero_timeout_rejects() {
    let bytes = pdf_pages(&["BT /F1 12 Tf 72 720 Td (x) Tj ET"]);
    let m = Markkit::builder()
        .timeout(std::time::Duration::ZERO)
        .build();
    match run(&m, &bytes) {
        Err(Error::ResourceLimit(msg)) => assert!(msg.contains("timeout"), "{msg}"),
        other => panic!("expected timeout, got {other:?}"),
    }
}

#[test]
fn pdf_limits_settable_via_limits_struct() {
    let bytes = pdf_pages(&[
        "BT /F1 12 Tf 72 720 Td (one) Tj ET",
        "BT /F1 12 Tf 72 720 Td (two) Tj ET",
    ]);
    let m = Markkit::builder()
        .limits(markkit::Limits::default().max_pdf_pages(1))
        .build();
    let out = run(&m, &bytes).unwrap();
    assert!(out.markdown.contains("one") && !out.markdown.contains("two"));
}

#[test]
fn output_cap_rejects() {
    let content: String = (0..200)
        .map(|i| {
            format!(
                "BT /F1 12 Tf 72 {} Td (paragraph {i}) Tj ET\n",
                7000 - i * 30
            )
        })
        .collect();
    let bytes = pdf_pages(&[content.as_str()]);
    let m = Markkit::builder().max_output_size(500).build();
    match run(&m, &bytes) {
        Err(Error::ResourceLimit(msg)) => assert!(msg.contains("max_output_size"), "{msg}"),
        other => panic!("expected output cap, got {other:?}"),
    }
}
