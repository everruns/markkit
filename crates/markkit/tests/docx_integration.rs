//! DOCX → Markdown integration tests (fixtures generated in-memory).

#![cfg(feature = "docx")]

mod common;

use common::*;
use markkit::{DocumentHint, Markkit, format::Format};

const DOCX_CT: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

fn extract(bytes: &[u8]) -> markkit::Extraction {
    Markkit::builder()
        .build()
        .extract_document_sync(bytes, DocumentHint::content_type(DOCX_CT))
        .unwrap()
}

fn md(body: &str) -> String {
    extract(&docx(body)).markdown
}

#[test]
fn plain_paragraphs() {
    let out = md(&format!("{}{}", para("First paragraph."), para("Second.")));
    assert_eq!(out, "First paragraph.\n\nSecond.\n");
}

#[test]
fn format_and_stats() {
    let e = extract(&docx(&para("Hi")));
    assert_eq!(e.format, Format::Docx);
    assert_eq!(e.stats.paragraphs, Some(1));
    assert!(e.warnings.is_empty());
}

#[test]
fn builtin_heading_styles() {
    let body = format!(
        "{}{}{}{}",
        styled_para("Heading1", "Intro"),
        styled_para("Heading2", "Details"),
        styled_para("Heading6", "Deep"),
        styled_para("Heading9", "Deeper")
    );
    let out = md(&body);
    assert!(out.contains("# Intro\n"), "{out}");
    assert!(out.contains("## Details\n"), "{out}");
    assert!(out.contains("###### Deep\n"), "{out}");
    assert!(out.contains("###### Deeper\n"), "{out}");
}

#[test]
fn title_style_is_h1_and_title_fallback() {
    let e = extract(&docx(&format!(
        "{}{}",
        styled_para("Title", "My Report"),
        para("Body")
    )));
    assert!(e.markdown.starts_with("# My Report\n"), "{}", e.markdown);
    assert_eq!(e.title.as_deref(), Some("My Report"));
}

#[test]
fn title_from_core_properties() {
    let bytes = docx_with(DocxParts {
        body: &para("Body"),
        title: Some("Core &amp; Title"),
        ..Default::default()
    });
    let e = extract(&bytes);
    assert_eq!(e.title.as_deref(), Some("Core & Title"));
}

#[test]
fn localized_heading_style_resolved_via_styles_xml() {
    let styles = r#"
        <w:style w:type="paragraph" w:styleId="berschrift1"><w:name w:val="heading 1"/></w:style>
        <w:style w:type="paragraph" w:styleId="Custom"><w:name w:val="My Custom"/><w:basedOn w:val="berschrift1"/></w:style>
        <w:style w:type="paragraph" w:styleId="Outline3"><w:name w:val="Outline"/><w:pPr><w:outlineLvl w:val="2"/></w:pPr></w:style>
    "#;
    let body = format!(
        "{}{}{}",
        styled_para("berschrift1", "Einleitung"),
        styled_para("Custom", "Inherited"),
        styled_para("Outline3", "Outlined")
    );
    let out = extract(&docx_with(DocxParts {
        body: &body,
        styles: Some(styles),
        ..Default::default()
    }))
    .markdown;
    assert!(out.contains("# Einleitung\n"), "{out}");
    assert!(out.contains("# Inherited\n"), "{out}");
    assert!(out.contains("### Outlined\n"), "{out}");
}

#[test]
fn inline_formatting() {
    let body = r#"<w:p>
        <w:r><w:t xml:space="preserve">Plain </w:t></w:r>
        <w:r><w:rPr><w:b/></w:rPr><w:t>bold</w:t></w:r>
        <w:r><w:t xml:space="preserve"> and </w:t></w:r>
        <w:r><w:rPr><w:i/></w:rPr><w:t>italic</w:t></w:r>
        <w:r><w:t xml:space="preserve"> and </w:t></w:r>
        <w:r><w:rPr><w:strike/></w:rPr><w:t>gone</w:t></w:r>
        <w:r><w:t xml:space="preserve"> and </w:t></w:r>
        <w:r><w:rPr><w:b/><w:i/></w:rPr><w:t>both</w:t></w:r>
    </w:p>"#;
    let out = md(body);
    assert_eq!(
        out,
        "Plain **bold** and *italic* and ~~gone~~ and ***both***\n"
    );
}

#[test]
fn adjacent_runs_with_same_format_merge() {
    let body = r#"<w:p>
        <w:r><w:rPr><w:b/></w:rPr><w:t>Hel</w:t></w:r>
        <w:r><w:rPr><w:b/></w:rPr><w:t>lo</w:t></w:r>
    </w:p>"#;
    assert_eq!(md(body), "**Hello**\n");
}

#[test]
fn explicitly_disabled_bold_is_plain() {
    let body = r#"<w:p><w:r><w:rPr><w:b w:val="0"/><w:i w:val="false"/></w:rPr><w:t>plain</w:t></w:r></w:p>"#;
    assert_eq!(md(body), "plain\n");
}

#[test]
fn whitespace_moved_outside_emphasis() {
    let body = r#"<w:p><w:r><w:t xml:space="preserve">a</w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t xml:space="preserve"> bold </w:t></w:r><w:r><w:t>b</w:t></w:r></w:p>"#;
    assert_eq!(md(body), "a **bold** b\n");
}

#[test]
fn hyperlinks_external_and_anchor() {
    let rels = r#"<Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/a b" TargetMode="External"/>"#;
    let body = r#"<w:p>
        <w:r><w:t xml:space="preserve">See </w:t></w:r>
        <w:hyperlink r:id="rId9"><w:r><w:rPr><w:rStyle w:val="Hyperlink"/></w:rPr><w:t>Example</w:t></w:r></w:hyperlink>
        <w:r><w:t xml:space="preserve"> or </w:t></w:r>
        <w:hyperlink w:anchor="sec2"><w:r><w:t>section 2</w:t></w:r></w:hyperlink>
        <w:hyperlink r:id="rIdMissing"><w:r><w:t xml:space="preserve"> dangling</w:t></w:r></w:hyperlink>
    </w:p>"#;
    let out = extract(&docx_with(DocxParts {
        body,
        rels,
        ..Default::default()
    }))
    .markdown;
    assert_eq!(
        out,
        "See [Example](https://example.com/a%20b) or [section 2](#sec2) dangling\n"
    );
}

#[test]
fn line_breaks_and_tabs() {
    let body = r#"<w:p><w:r><w:t>line1</w:t><w:br/><w:t>line2</w:t><w:tab/><w:t>after tab</w:t></w:r></w:p>"#;
    assert_eq!(md(body), "line1  \nline2 after tab\n");
}

#[test]
fn bullet_and_numbered_lists() {
    let body = format!(
        "{}{}{}{}{}{}",
        list_para(1, 0, "apple"),
        list_para(1, 1, "green"),
        list_para(1, 0, "banana"),
        para("Between"),
        list_para(2, 0, "first"),
        list_para(2, 0, "second"),
    );
    let out = extract(&docx_with(DocxParts {
        body: &body,
        numbering: Some(NUMBERING),
        ..Default::default()
    }))
    .markdown;
    assert!(
        out.contains("- apple\n  - green\n- banana\n"),
        "unexpected list output:\n{out}"
    );
    assert!(out.contains("Between"), "{out}");
    assert!(out.contains("1. first\n2. second\n"), "{out}");
}

#[test]
fn list_without_numbering_part_is_bulleted() {
    let out = md(&format!("{}{}", list_para(7, 0, "x"), list_para(7, 0, "y")));
    assert_eq!(out, "- x\n- y\n");
}

#[test]
fn tables_first_row_header_and_pipes_escaped() {
    let cell = |t: &str| format!("<w:tc><w:p><w:r><w:t>{t}</w:t></w:r></w:p></w:tc>");
    let body = format!(
        "<w:tbl><w:tr>{}{}</w:tr><w:tr>{}{}</w:tr><w:tr>{}<w:tc><w:p><w:r><w:t>two</w:t></w:r></w:p><w:p><w:r><w:t>paras</w:t></w:r></w:p></w:tc></w:tr></w:tbl>",
        cell("Name"),
        cell("Age"),
        cell("Ann"),
        cell("a | b"),
        cell("Bob"),
    );
    let e = extract(&docx(&body));
    assert_eq!(
        e.markdown,
        "| Name | Age |\n| --- | --- |\n| Ann | a \\| b |\n| Bob | two paras |\n"
    );
    assert_eq!(e.stats.tables, Some(1));
}

#[test]
fn table_grid_span_pads_cells() {
    let body = r#"<w:tbl>
        <w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>C</w:t></w:r></w:p></w:tc></w:tr>
        <w:tr><w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>wide</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>c</w:t></w:r></w:p></w:tc></w:tr>
    </w:tbl>"#;
    assert_eq!(
        md(body),
        "| A | B | C |\n| --- | --- | --- |\n| wide |  | c |\n"
    );
}

#[test]
fn tracked_changes_insertions_kept_deletions_dropped() {
    let body = r#"<w:p>
        <w:r><w:t xml:space="preserve">Keep </w:t></w:r>
        <w:ins w:id="1"><w:r><w:t xml:space="preserve">inserted </w:t></w:r></w:ins>
        <w:del w:id="2"><w:r><w:delText>deleted </w:delText></w:r></w:del>
        <w:r><w:t>end</w:t></w:r>
    </w:p>"#;
    assert_eq!(md(body), "Keep inserted end\n");
}

#[test]
fn field_instructions_hidden_results_kept() {
    let body = r#"<w:p>
        <w:r><w:fldChar w:fldCharType="begin"/></w:r>
        <w:r><w:instrText xml:space="preserve"> HYPERLINK "https://x.test" </w:instrText></w:r>
        <w:r><w:fldChar w:fldCharType="separate"/></w:r>
        <w:r><w:t>result</w:t></w:r>
        <w:r><w:fldChar w:fldCharType="end"/></w:r>
    </w:p>"#;
    assert_eq!(md(body), "result\n");
}

#[test]
fn content_controls_are_unwrapped() {
    let body = format!(
        "<w:sdt><w:sdtPr/><w:sdtContent>{}</w:sdtContent></w:sdt>",
        para("Inside control")
    );
    assert_eq!(md(&body), "Inside control\n");
}

#[test]
fn empty_paragraphs_skipped() {
    let body = format!("<w:p/><w:p><w:r><w:t></w:t></w:r></w:p>{}", para("x"));
    assert_eq!(md(&body), "x\n");
}

#[test]
fn xml_entities_decoded() {
    assert_eq!(md(&para("a &lt;b&gt; &amp; &#x41;&#66;")), "a <b> & AB\n");
}

#[test]
fn extract_bytes_delegates_for_docx() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let out = rt
        .block_on(
            Markkit::builder()
                .build()
                .extract_bytes(&docx(&styled_para("Heading1", "Hi")), Some(DOCX_CT)),
        )
        .unwrap();
    assert_eq!(out, "# Hi\n");
}

#[tokio::test]
async fn async_extract_document() {
    let e = Markkit::builder()
        .build()
        .extract_document(
            &docx(&para("async")),
            DocumentHint {
                content_type: None,
                file_name: Some("report.docx"),
            },
        )
        .await
        .unwrap();
    assert_eq!(e.markdown, "async\n");
}

#[test]
fn detection_by_magic_bytes() {
    let e = Markkit::builder()
        .build()
        .extract_document_sync(&docx(&para("sniffed")), DocumentHint::default())
        .unwrap();
    assert_eq!(e.format, Format::Docx);
    assert_eq!(e.markdown, "sniffed\n");
}

#[test]
fn main_part_resolved_via_package_relationships() {
    // Main document stored under a non-default name.
    let doc = format!(
        r#"<?xml version="1.0"?><w:document {W_NS}><w:body>{}</w:body></w:document>"#,
        para("relocated")
    );
    let rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="/word/main.xml"/></Relationships>"#;
    let bytes = zip_bytes(&[
        ("[Content_Types].xml", b"<Types/>"),
        ("_rels/.rels", rels.as_bytes()),
        ("word/main.xml", doc.as_bytes()),
    ]);
    let e = Markkit::builder()
        .build()
        .extract_document_sync(&bytes, DocumentHint::content_type(DOCX_CT))
        .unwrap();
    assert_eq!(e.markdown, "relocated\n");
}

#[test]
fn list_styles_resolve_numbering_and_switch_list_type() {
    // python-docx / Word default template: numbering lives on the paragraph styles.
    let styles = r#"
        <w:style w:type="paragraph" w:styleId="ListBullet"><w:name w:val="List Bullet"/><w:pPr><w:numPr><w:numId w:val="1"/></w:numPr></w:pPr></w:style>
        <w:style w:type="paragraph" w:styleId="ListBullet2"><w:name w:val="List Bullet 2"/><w:pPr><w:numPr><w:numId w:val="1"/></w:numPr></w:pPr></w:style>
        <w:style w:type="paragraph" w:styleId="ListNumber"><w:name w:val="List Number"/><w:pPr><w:numPr><w:numId w:val="2"/></w:numPr></w:pPr></w:style>
    "#;
    let body = format!(
        "{}{}{}{}{}",
        styled_para("ListBullet", "one"),
        styled_para("ListBullet2", "nested"),
        styled_para("ListBullet", "two"),
        styled_para("ListNumber", "step a"),
        styled_para("ListNumber", "step b"),
    );
    let out = extract(&docx_with(DocxParts {
        body: &body,
        styles: Some(styles),
        numbering: Some(NUMBERING),
        ..Default::default()
    }))
    .markdown;
    assert_eq!(out, "- one\n  - nested\n- two\n\n1. step a\n2. step b\n");
}

// ─── Provenance ──────────────────────────────────────────────────────

#[test]
fn sources_record_body_block_index() {
    let body = format!(
        "{}{}{}{}{}",
        styled_para("Heading1", "Title"),
        para(""),
        list_para(1, 0, "one"),
        list_para(1, 0, "two"),
        para("After"),
    );
    let m = Markkit::builder().track_sources(true).build();
    let p = m
        .parse_document_sync(&docx(&body), DocumentHint::content_type(DOCX_CT))
        .unwrap();
    let idx: Vec<Option<usize>> = p
        .document
        .nodes
        .iter()
        .map(|n| n.source().and_then(|s| s.index))
        .collect();
    // Blank paragraph 1 is skipped but still counted.
    assert_eq!(idx, vec![Some(0), Some(2), Some(4)]);
    assert_eq!(p.render().markdown, md(&body));
}

#[test]
fn transforms_apply_to_docx() {
    let m = Markkit::builder()
        .transform(markkit::transform::NormalizeHeadings)
        .build();
    let out = m
        .extract_document_sync(
            &docx(&styled_para("Heading3", "Deep")),
            DocumentHint::content_type(DOCX_CT),
        )
        .unwrap();
    assert_eq!(out.markdown, "# Deep\n");
}
