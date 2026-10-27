//! EPUB → Markdown integration and security tests (fixtures generated in-memory).

#![cfg(feature = "epub")]

mod common;

use common::*;
use markkit::{DocumentHint, Error, Markkit, format::Format};

const XHTML: &str = "application/xhtml+xml";

fn try_extract(bytes: &[u8]) -> markkit::Result<markkit::Extraction> {
    Markkit::builder()
        .build()
        .extract_document_sync(bytes, DocumentHint::file_name("book.epub"))
}

fn book() -> Vec<u8> {
    let opf = epub_opf(
        "A  Small\nBook",
        &[
            ("cover", "cover.xhtml", XHTML),
            ("c1", "text/ch%201.xhtml", XHTML),
            ("c2", "text/ch2.xhtml", XHTML),
            ("css", "style.css", "text/css"),
            ("img", "img.png", "image/png"),
        ],
        &[
            ("cover", false),
            ("c1", true),
            ("c2", true),
            ("c1", true),
            ("css", true),
        ],
    );
    let ch1 =
        xhtml("<h1>Chapter 1</h1><p>It was a <em>dark</em> night.</p><script>evil()</script>");
    let ch2 = xhtml("<h1>Chapter 2</h1><ul><li>one</li><li>two</li></ul>");
    let cover = xhtml("<p>COVER</p>");
    epub(&[
        ("OEBPS/content.opf", opf.as_bytes()),
        ("OEBPS/text/ch 1.xhtml", ch1.as_bytes()),
        ("OEBPS/text/ch2.xhtml", ch2.as_bytes()),
        ("OEBPS/cover.xhtml", cover.as_bytes()),
        ("OEBPS/style.css", b"p{}"),
    ])
}

#[test]
fn chapters_in_spine_order() {
    let e = try_extract(&book()).unwrap();
    assert_eq!(e.format, Format::Epub);
    assert_eq!(e.title.as_deref(), Some("A Small Book"));
    assert_eq!(e.stats.chapters, Some(2));
    assert_eq!(
        e.markdown,
        "# Chapter 1\n\nIt was a *dark* night.\n\n---\n\n# Chapter 2\n\n- one\n- two\n"
    );
    assert!(e.warnings.is_empty(), "{:?}", e.warnings);
}

#[test]
fn detection() {
    assert_eq!(Format::from_extension("EPUB"), Format::Epub);
    assert_eq!(
        Format::from_content_type("application/epub+zip"),
        Format::Epub
    );
    assert_eq!(Format::sniff(&book()), Format::Epub);
    let e = Markkit::builder()
        .build()
        .extract_document_sync(&book(), DocumentHint::default())
        .unwrap();
    assert_eq!(e.format, Format::Epub);
}

#[test]
fn missing_container_falls_back_to_single_opf() {
    let opf = epub_opf("T", &[("c", "c.xhtml", XHTML)], &[("c", true)]);
    let ch = xhtml("<p>hi</p>");
    let bytes = zip_bytes(&[
        ("mimetype", EPUB_MIME.as_bytes()),
        ("book/pkg.opf", opf.as_bytes()),
        ("book/c.xhtml", ch.as_bytes()),
    ]);
    assert_eq!(try_extract(&bytes).unwrap().markdown, "hi\n");
}

#[test]
fn missing_chapter_warns() {
    let opf = epub_opf(
        "T",
        &[("a", "a.xhtml", XHTML), ("gone", "gone.xhtml", XHTML)],
        &[("a", true), ("gone", true), ("nope", true)],
    );
    let a = xhtml("<p>a</p>");
    let e = try_extract(&epub(&[
        ("OEBPS/content.opf", opf.as_bytes()),
        ("OEBPS/a.xhtml", a.as_bytes()),
    ]))
    .unwrap();
    assert_eq!(e.markdown, "a\n");
    assert_eq!(e.warnings.len(), 2, "{:?}", e.warnings);
    assert!(
        e.warnings
            .iter()
            .all(|w| w.code == markkit::WarningCode::MissingChapter)
    );
}

// ─── Security (threat models T1, T6) ────────────────────────────────

#[test]
fn security_scripts_stripped() {
    let e = try_extract(&book()).unwrap();
    assert!(!e.markdown.contains("evil") && !e.markdown.contains("<script"));
}

#[test]
fn security_path_traversal_ignored() {
    let opf = epub_opf(
        "T",
        &[
            ("x", "../../../etc/passwd", XHTML),
            ("ok", "ok.xhtml", XHTML),
        ],
        &[("x", true), ("ok", true)],
    );
    let ok = xhtml("<p>ok</p>");
    let e = try_extract(&epub(&[
        ("OEBPS/content.opf", opf.as_bytes()),
        ("OEBPS/ok.xhtml", ok.as_bytes()),
    ]))
    .unwrap();
    assert_eq!(e.markdown, "ok\n");
}

#[test]
fn security_drm_encrypted_is_error() {
    let opf = epub_opf("T", &[("c", "c.xhtml", XHTML)], &[("c", true)]);
    let enc = r#"<encryption xmlns="urn:oasis:names:tc:opendocument:xmlns:container" xmlns:enc="http://www.w3.org/2001/04/xmlenc#"><enc:EncryptedData><enc:CipherData><enc:CipherReference URI="OEBPS/c.xhtml"/></enc:CipherData></enc:EncryptedData></encryption>"#;
    let bytes = epub(&[
        ("META-INF/encryption.xml", enc.as_bytes()),
        ("OEBPS/content.opf", opf.as_bytes()),
        ("OEBPS/c.xhtml", b"\x8f\x01ciphertext"),
    ]);
    assert!(matches!(try_extract(&bytes), Err(Error::EpubExtract(_))));
}

#[test]
fn security_repeated_spine_reads_once() {
    // 5 000 itemrefs to one 100 KB chapter must not render it 5 000 times.
    let opf = epub_opf("T", &[("c", "c.xhtml", XHTML)], &vec![("c", true); 5_000]);
    let ch = xhtml(&format!("<p>{}</p>", "x".repeat(100_000)));
    let e = try_extract(&epub(&[
        ("OEBPS/content.opf", opf.as_bytes()),
        ("OEBPS/c.xhtml", ch.as_bytes()),
    ]))
    .unwrap();
    assert_eq!(e.stats.chapters, Some(1));
    assert!(e.markdown.len() < 110_000);
}

#[test]
fn security_zip_bomb_chapter_bounded() {
    let opf = epub_opf("T", &[("c", "c.xhtml", XHTML)], &[("c", true)]);
    let big = vec![b' '; 2 * 1024 * 1024];
    let bytes = epub(&[
        ("OEBPS/content.opf", opf.as_bytes()),
        ("OEBPS/c.xhtml", &big),
    ]);
    let err = Markkit::builder()
        .max_archive_entry_size(1024 * 1024)
        .build()
        .extract_document_sync(&bytes, DocumentHint::file_name("a.epub"))
        .unwrap_err();
    assert!(matches!(err, Error::ResourceLimit(_)), "{err:?}");
}

#[test]
fn security_malformed_packages() {
    for bytes in [
        b"not a zip".to_vec(),
        epub(&[]),
        epub(&[("OEBPS/content.opf", b"<not-a-package/>")]),
        epub(&[("OEBPS/content.opf", b"<package><spine>")]),
    ] {
        match try_extract(&bytes) {
            Err(Error::EpubExtract(_)) => {}
            Ok(e) => assert_eq!(e.markdown, ""),
            Err(e) => panic!("{e:?}"),
        }
    }
}

#[test]
fn security_truncated_archive_never_panics() {
    let full = book();
    for cut in (0..full.len()).step_by(5) {
        let _ = try_extract(&full[..cut]);
    }
}

fn one_chapter(body: &str) -> Vec<u8> {
    let opf = epub_opf("T", &[("c", "c.xhtml", XHTML)], &[("c", true)]);
    let ch = xhtml(body);
    epub(&[
        ("OEBPS/content.opf", opf.as_bytes()),
        ("OEBPS/c.xhtml", ch.as_bytes()),
    ])
}

#[test]
fn security_limits_apply() {
    let bytes = one_chapter(&"<p>a</p>".repeat(500));
    let r = Markkit::builder()
        .max_nodes(100)
        .build()
        .extract_document_sync(&bytes, DocumentHint::file_name("book.epub"));
    assert!(
        matches!(&r, Err(e) if e.limit_kind() == Some(markkit::LimitKind::MaxNodes)),
        "{r:?}"
    );
    let r = Markkit::builder()
        .timeout(std::time::Duration::ZERO)
        .build()
        .extract_document_sync(&bytes, DocumentHint::file_name("book.epub"));
    assert!(
        matches!(&r, Err(e) if e.limit_kind() == Some(markkit::LimitKind::Timeout)),
        "{r:?}"
    );
    let r = Markkit::builder()
        .max_output_size(100)
        .build()
        .extract_document_sync(&bytes, DocumentHint::file_name("book.epub"));
    assert!(
        matches!(&r, Err(e) if e.limit_kind() == Some(markkit::LimitKind::MaxOutputSize)),
        "{r:?}"
    );
}

#[test]
fn security_hidden_content_skipped_by_default() {
    let bytes = one_chapter(r#"<p>shown</p><p style="display:none">INJECT</p>"#);
    let e = try_extract(&bytes).unwrap();
    assert_eq!(e.markdown, "shown\n");
    let e = Markkit::builder()
        .include_hidden_content(true)
        .build()
        .extract_document_sync(&bytes, DocumentHint::file_name("book.epub"))
        .unwrap();
    assert!(e.markdown.contains("INJECT"), "{}", e.markdown);
}

#[test]
fn security_markup_in_text_is_escaped() {
    let e = try_extract(&one_chapter(
        "<p>&lt;img src=x onerror=y&gt; <a href=\"javascript:alert(1)\">x</a></p>",
    ))
    .unwrap();
    assert!(e.markdown.contains("\\<img"), "{}", e.markdown);
    assert!(!e.markdown.contains("javascript:"), "{}", e.markdown);
}
