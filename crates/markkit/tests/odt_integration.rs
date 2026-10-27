//! ODT → Markdown integration and security tests (fixtures generated in-memory).

#![cfg(feature = "odt")]

mod common;

use common::*;
use markkit::{DocumentHint, Error, Markkit, format::Format};

fn try_extract(bytes: &[u8]) -> markkit::Result<markkit::Extraction> {
    Markkit::builder()
        .build()
        .extract_document_sync(bytes, DocumentHint::file_name("doc.odt"))
}

fn extract(bytes: &[u8]) -> markkit::Extraction {
    try_extract(bytes).unwrap()
}

fn md(body: &str) -> String {
    extract(&odt(body)).markdown
}

const AUTO: &str = r#"
<style:style style:name="B" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>
<style:style style:name="I" style:family="text"><style:text-properties fo:font-style="italic"/></style:style>
<style:style style:name="S" style:family="text"><style:text-properties style:text-line-through-style="solid"/></style:style>
<style:style style:name="BI" style:family="text" style:parent-style-name="B"><style:text-properties fo:font-style="italic"/></style:style>
<style:style style:name="P1" style:family="paragraph" style:parent-style-name="Heading_20_2"/>
<text:list-style style:name="Num"><text:list-level-style-number text:level="1"/><text:list-level-style-bullet text:level="2"/></text:list-style>
<text:list-style style:name="Bul"><text:list-level-style-bullet text:level="1"/><text:list-level-style-number text:level="2"/></text:list-style>
"#;

const STYLES: &str = r#"
<style:style style:name="Title" style:display-name="Title" style:family="paragraph"/>
<style:style style:name="Heading_20_2" style:display-name="Heading 2" style:family="paragraph" style:default-outline-level="2"/>
"#;

#[test]
fn headings_paragraphs_and_inline_formatting() {
    let body = r#"
<text:p text:style-name="Title">The Title</text:p>
<text:h text:outline-level="1">Intro</text:h>
<text:p>Plain <text:span text:style-name="B">bold </text:span><text:span text:style-name="B">run</text:span> and <text:span text:style-name="I">it</text:span>, <text:span text:style-name="S">gone</text:span>, <text:span text:style-name="BI">both</text:span>.</text:p>
<text:p text:style-name="P1">Styled heading</text:p>
<text:p/>
<text:h text:outline-level="9">Deep</text:h>
"#;
    let e = extract(&odt_with(AUTO, body, Some(STYLES), None));
    assert_eq!(e.format, Format::Odt);
    assert_eq!(e.title.as_deref(), Some("The Title"));
    assert_eq!(
        e.markdown,
        "# The Title\n\n# Intro\n\nPlain **bold run** and *it*, ~~gone~~, ***both***.\n\n## Styled heading\n\n###### Deep\n"
    );
    assert_eq!(e.stats.paragraphs, Some(5));
}

#[test]
fn meta_title_wins() {
    let e = extract(&odt_with(
        "",
        "<text:p>x</text:p>",
        None,
        Some("Meta  Title"),
    ));
    assert_eq!(e.title.as_deref(), Some("Meta Title"));
}

#[test]
fn links_spaces_tabs_breaks() {
    assert_eq!(
        md(
            r#"<text:p>See <text:a xlink:href="https://e.com/a b">the  site</text:a>.<text:line-break/>a<text:s text:c="3"/>b<text:tab/>c</text:p>"#
        ),
        "See [the site](https://e.com/a%20b).  \na   b c\n"
    );
}

#[test]
fn lists_ordered_and_nested() {
    let body = r#"
<text:list text:style-name="Num">
  <text:list-item><text:p>one</text:p>
    <text:list><text:list-item><text:p>sub</text:p></text:list-item></text:list>
  </text:list-item>
  <text:list-item><text:p>two</text:p></text:list-item>
</text:list>
<text:list text:style-name="Bul"><text:list-item><text:p>dot</text:p></text:list-item></text:list>
"#;
    let e = extract(&odt_with(AUTO, body, None, None));
    assert_eq!(e.markdown, "1. one\n   - sub\n2. two\n\n- dot\n");
}

#[test]
fn tables_with_repeats_and_padding() {
    let body = r#"
<table:table table:name="T">
  <table:table-column table:number-columns-repeated="1024"/>
  <table:table-header-rows>
    <table:table-row><table:table-cell><text:p>A</text:p></table:table-cell><table:table-cell><text:p>B|C</text:p></table:table-cell><table:table-cell table:number-columns-repeated="1022"/></table:table-row>
  </table:table-header-rows>
  <table:table-row><table:table-cell table:number-columns-repeated="2"><text:p>x</text:p><text:p>y</text:p></table:table-cell></table:table-row>
  <table:table-row><table:covered-table-cell/><table:table-cell><text:p>z</text:p></table:table-cell></table:table-row>
  <table:table-row table:number-rows-repeated="1048000"><table:table-cell table:number-columns-repeated="1024"/></table:table-row>
</table:table>
"#;
    let e = extract(&odt(body));
    assert_eq!(
        e.markdown,
        "| A | B\\|C |\n| --- | --- |\n| x y | x y |\n|  | z |\n"
    );
    assert_eq!(e.stats.tables, Some(1));
    assert!(e.warnings.is_empty(), "{:?}", e.warnings);
}

#[test]
fn skipped_content() {
    let body = r#"
<text:tracked-changes><text:changed-region><text:deletion><text:p>deleted</text:p></text:deletion></text:changed-region></text:tracked-changes>
<text:table-of-content><text:index-body><text:p>TOC entry</text:p></text:index-body></text:table-of-content>
<text:p>Body<text:note><text:note-citation>1</text:note-citation><text:note-body><text:p>footnote</text:p></text:note-body></text:note><office:annotation><text:p>comment</text:p></office:annotation> text<text:bookmark text:name="x"/>.</text:p>
<text:section><text:p>In section</text:p></text:section>
"#;
    assert_eq!(md(body), "Body text.\n\nIn section\n");
}

#[test]
fn detection() {
    assert_eq!(Format::from_extension("odt"), Format::Odt);
    assert_eq!(
        Format::from_content_type("application/vnd.oasis.opendocument.text"),
        Format::Odt
    );
    assert_eq!(Format::sniff(&odt("<text:p>x</text:p>")), Format::Odt);
    let e = Markkit::builder()
        .build()
        .extract_document_sync(&odt("<text:p>sniffed</text:p>"), DocumentHint::default())
        .unwrap();
    assert_eq!(e.markdown, "sniffed\n");
}

// ─── Security (threat model T6) ─────────────────────────────────────

#[test]
fn security_wrong_package_and_missing_content() {
    let ods = ods("");
    assert!(matches!(try_extract(&ods), Err(Error::OdtExtract(_))));
    let no_content = zip_bytes(&[("mimetype", ODT_MIME.as_bytes())]);
    assert!(matches!(
        try_extract(&no_content),
        Err(Error::OdtExtract(_))
    ));
    assert!(matches!(
        try_extract(b"not a zip"),
        Err(Error::OdtExtract(_))
    ));
}

#[test]
fn security_zip_bomb_bounded() {
    let big = vec![b' '; 2 * 1024 * 1024];
    let bytes = zip_bytes(&[("mimetype", ODT_MIME.as_bytes()), ("content.xml", &big)]);
    let err = Markkit::builder()
        .max_archive_entry_size(1024 * 1024)
        .build()
        .extract_document_sync(&bytes, DocumentHint::file_name("a.odt"))
        .unwrap_err();
    assert!(matches!(err, Error::ResourceLimit(_)), "{err:?}");
}

#[test]
fn security_entities_not_expanded() {
    let content = r#"<?xml version="1.0"?><!DOCTYPE d [<!ENTITY a "aaaaaaaaaa"><!ENTITY b "&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;"><!ENTITY xxe SYSTEM "file:///etc/passwd">]><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><office:body><office:text><text:p>x&b;&xxe;y</text:p></office:text></office:body></office:document-content>"#;
    let bytes = zip_bytes(&[
        ("mimetype", ODT_MIME.as_bytes()),
        ("content.xml", content.as_bytes()),
    ]);
    match try_extract(&bytes) {
        Ok(e) => {
            assert!(
                !e.markdown.contains("aaaa") && !e.markdown.contains("root:"),
                "{}",
                e.markdown
            )
        }
        Err(Error::OdtExtract(_)) => {}
        Err(e) => panic!("{e:?}"),
    }
}

#[test]
fn security_deep_nesting_is_limited() {
    let depth = 10_000;
    let body = format!(
        "{}<text:p>x</text:p>{}",
        "<text:list><text:list-item>".repeat(depth),
        "</text:list-item></text:list>".repeat(depth)
    );
    match try_extract(&odt(&body)) {
        Err(Error::ResourceLimit(_)) => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn security_repeat_counts_capped() {
    let m = Markkit::builder()
        .max_sheet_rows(5)
        .max_sheet_cols(3)
        .build();
    let body = r#"<table:table>
<table:table-row><table:table-cell table:number-columns-repeated="4000000000"><text:p>h</text:p></table:table-cell></table:table-row>
<table:table-row table:number-rows-repeated="4000000000"><table:table-cell><text:p>r</text:p></table:table-cell></table:table-row>
</table:table>
<text:p>a<text:s text:c="999999999"/>b</text:p>"#;
    let e = m
        .extract_document_sync(&odt(body), DocumentHint::file_name("a.odt"))
        .unwrap();
    assert_eq!(e.markdown.lines().filter(|l| l.starts_with('|')).count(), 7);
    assert!(e.markdown.lines().next().unwrap().matches('h').count() == 3);
    assert!(e.markdown.contains(&format!("a{}b", " ".repeat(64))));
    assert_eq!(e.warnings.len(), 1, "{:?}", e.warnings);
}

#[test]
fn security_style_cycle_terminates() {
    let auto = r#"<style:style style:name="A" style:family="text" style:parent-style-name="B"/><style:style style:name="B" style:family="text" style:parent-style-name="A"/>"#;
    let e = extract(&odt_with(
        auto,
        r#"<text:p><text:span text:style-name="A">loop</text:span></text:p>"#,
        None,
        None,
    ));
    assert_eq!(e.markdown, "loop\n");
}

#[test]
fn security_truncated_archive_never_panics() {
    let full = odt_with(
        AUTO,
        "<text:p>hello <text:span text:style-name=\"B\">x</text:span></text:p>",
        Some(STYLES),
        Some("t"),
    );
    for cut in (0..full.len()).step_by(7) {
        let _ = try_extract(&full[..cut]);
    }
}

const HIDDEN_STYLES: &str = r#"
<style:style style:name="H" style:family="text"><style:text-properties text:display="none"/></style:style>
<style:style style:name="H2" style:family="text" style:parent-style-name="H"/>
<style:style style:name="HP" style:family="paragraph"><style:text-properties text:display="none"/></style:style>
"#;

const HIDDEN_BODY: &str = r#"
<text:p>shown <text:span text:style-name="H">INJECT1</text:span><text:span text:style-name="H2">INJECT2</text:span></text:p>
<text:p text:style-name="HP">INJECT3</text:p>
<text:section text:name="S" text:display="none"><text:p>INJECT4</text:p></text:section>
<text:p><text:hidden-text text:string-value="INJECT5" text:is-hidden="true"/>tail</text:p>
"#;

fn extract_hidden(include: bool) -> markkit::Extraction {
    Markkit::builder()
        .include_hidden_content(include)
        .build()
        .extract_document_sync(
            &odt_with(HIDDEN_STYLES, HIDDEN_BODY, None, None),
            DocumentHint::file_name("doc.odt"),
        )
        .unwrap()
}

#[test]
fn security_hidden_content_skipped_by_default() {
    let e = extract_hidden(false);
    assert!(!e.markdown.contains("INJECT"), "{}", e.markdown);
    assert!(e.markdown.contains("shown") && e.markdown.contains("tail"));
    assert!(
        e.warnings
            .iter()
            .any(|w| w.code == markkit::WarningCode::HiddenContentSkipped),
        "{:?}",
        e.warnings
    );
}

#[test]
fn hidden_content_kept_when_opted_in() {
    let e = extract_hidden(true);
    for s in ["INJECT1", "INJECT2", "INJECT3", "INJECT4"] {
        assert!(e.markdown.contains(s), "{s}: {}", e.markdown);
    }
    assert!(e.warnings.is_empty(), "{:?}", e.warnings);
}

#[test]
fn security_limits_apply() {
    let bytes = odt(&"<text:p>a</text:p>".repeat(500));
    let r = Markkit::builder()
        .max_nodes(100)
        .build()
        .extract_document_sync(&bytes, DocumentHint::file_name("doc.odt"));
    assert!(
        matches!(&r, Err(e) if e.limit_kind() == Some(markkit::LimitKind::MaxNodes)),
        "{r:?}"
    );
    let r = Markkit::builder()
        .timeout(std::time::Duration::ZERO)
        .build()
        .extract_document_sync(&bytes, DocumentHint::file_name("doc.odt"));
    assert!(
        matches!(&r, Err(e) if e.limit_kind() == Some(markkit::LimitKind::Timeout)),
        "{r:?}"
    );
}

#[test]
fn security_markup_in_text_is_escaped() {
    let e = extract(&odt(
        "<text:p>&lt;script&gt;x&lt;/script&gt; [a](javascript:b)</text:p>",
    ));
    assert!(e.markdown.contains("\\<script>"), "{}", e.markdown);
    assert!(e.markdown.contains("\\[a\\](javascript"), "{}", e.markdown);
}
