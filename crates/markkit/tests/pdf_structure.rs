//! Reading-order and table regressions, including ambiguous prose negatives.
#![cfg(feature = "pdf")]
mod common;
use common::*;
use markkit::ir::Node;
use markkit::{DocumentHint, Markkit};
fn extract(content: &str) -> markkit::Extraction {
    Markkit::builder()
        .build()
        .extract_document_sync(&pdf_pages(&[content]), DocumentHint::default())
        .unwrap()
}
fn row(y: u32, left: &str, right: &str) -> String {
    format!("BT /F1 10 Tf 72 {y} Td ({left}) Tj ET\nBT /F1 10 Tf 300 {y} Td ({right}) Tj ET\n")
}
#[test]
fn columns_are_read_top_to_bottom_before_moving_right() {
    let c = row(700, "Left first", "Right first") + &row(686, "Left second", "Right second");
    assert_eq!(
        extract(&c).markdown,
        "Left first Left second\n\nRight first Right second\n"
    );
}
#[test]
fn shuffled_single_column_is_spatially_ordered() {
    assert_eq!(
        extract(
            "BT /F1 10 Tf 72 686 Td (Second line) Tj ET\nBT /F1 10 Tf 72 700 Td (First line) Tj ET"
        )
        .markdown,
        "First line Second line\n"
    );
}
#[test]
fn ruled_table_uses_cells_not_paragraphs() {
    let c = "72 660 400 60 re S\n72 680 m 472 680 l S\n72 700 m 472 700 l S\n272 660 m 272 720 l S\nBT /F1 10 Tf 80 705 Td (Item) Tj ET\nBT /F1 10 Tf 280 705 Td (Qty) Tj ET\nBT /F1 10 Tf 80 685 Td (Apples) Tj ET\nBT /F1 10 Tf 280 685 Td (3) Tj ET\nBT /F1 10 Tf 80 665 Td (Pears) Tj ET\nBT /F1 10 Tf 280 665 Td (2) Tj ET";
    let e = extract(c);
    assert_eq!(
        e.markdown,
        "| Item | Qty |\n| --- | --- |\n| Apples | 3 |\n| Pears | 2 |\n"
    );
    assert_eq!(e.stats.tables, Some(1));
}
#[test]
fn borderless_table_aligns_rows_and_numeric_columns() {
    let c = row(700, "Item", "Qty") + &row(684, "Apples", "3") + &row(668, "Pears", "2");
    assert_eq!(
        extract(&c).markdown,
        "| Item | Qty |\n| --- | --- |\n| Apples | 3 |\n| Pears | 2 |\n"
    );
}
#[test]
fn prose_columns_are_not_borderless_tables() {
    let c = row(
        700,
        "The first sentence begins here.",
        "Another sentence starts here.",
    ) + &row(
        686,
        "It carries on in this column.",
        "This is its next line of prose.",
    ) + &row(
        672,
        "And here it comes to an end.",
        "The second column finishes here.",
    );
    assert!(!extract(&c).markdown.contains('|'));
}
#[test]
fn merged_ruled_header_keeps_colspan_in_ir() {
    let c = "72 660 400 60 re S\n72 680 m 472 680 l S\n72 700 m 472 700 l S\n272 660 m 272 700 l S\nBT /F1 10 Tf 80 705 Td (Inventory) Tj ET\nBT /F1 10 Tf 80 685 Td (Apples) Tj ET\nBT /F1 10 Tf 280 685 Td (3) Tj ET\nBT /F1 10 Tf 80 665 Td (Pears) Tj ET\nBT /F1 10 Tf 280 665 Td (2) Tj ET";
    let parsed = Markkit::builder()
        .pdf_layout(true)
        .build()
        .parse_document_sync(&pdf_pages(&[c]), DocumentHint::default())
        .unwrap();
    assert!(
        parsed.document.nodes.iter().any(
            |n| matches!(n,Node::Table{headers,..} if headers.len()==1 && headers[0].colspan==2)
        )
    );
    let block = &parsed.layout.unwrap().pages[0].blocks[0];
    assert_eq!(block.kind, "table");
    assert_eq!(block.cells[0].colspan, 2);
}
#[test]
fn tagged_actual_text_replaces_glyph_content_once() {
    let c = "BT /F1 10 Tf 72 700 Td /Span << /ActualText (Accessible text) >> BDC (junk) Tj (junk2) Tj EMC ET";
    assert_eq!(extract(c).markdown, "Accessible text\n");
}
#[test]
fn numbered_lists_preserve_start() {
    assert_eq!(
        extract("BT /F1 10 Tf 14 TL 72 700 Td (3. first) Tj T* (4. second) Tj ET").markdown,
        "3. first\n4. second\n"
    );
}
#[test]
fn repeated_margin_headers_can_be_removed() {
    let bytes = pdf_pages(&[
        "BT /F1 10 Tf 72 770 Td (Company report) Tj ET\nBT /F1 10 Tf 72 700 Td (Page one body) Tj ET",
        "BT /F1 10 Tf 72 770 Td (Company report) Tj ET\nBT /F1 10 Tf 72 700 Td (Page two body) Tj ET",
    ]);
    let e = Markkit::builder()
        .pdf_remove_repeated_headers(true)
        .build()
        .extract_document_sync(&bytes, DocumentHint::default())
        .unwrap();
    assert!(!e.markdown.contains("Company report"));
    assert!(e.markdown.contains("Page one body") && e.markdown.contains("Page two body"));
}
#[test]
fn layout_has_ordered_blocks_linked_to_semantic_nodes() {
    let bytes = pdf_pages(&[
        &(row(700, "Left first", "Right first") + &row(686, "Left second", "Right second"))
    ]);
    let p = Markkit::builder()
        .pdf_layout(true)
        .build()
        .parse_document_sync(&bytes, DocumentHint::default())
        .unwrap();
    let page = &p.layout.as_ref().unwrap().pages[0];
    assert_eq!(page.blocks.len(), 2);
    for (i, n) in p.document.nodes.iter().enumerate() {
        assert_eq!(n.source().unwrap().layout_block, Some(i));
        assert_eq!(page.blocks[i].id, i);
        assert!(!page.blocks[i].lines.is_empty());
    }
}

#[test]
fn wrapped_borderless_cells_stay_in_their_row() {
    let c = row(700, "Item", "Qty")
        + &row(684, "Long description", "3")
        + "BT /F1 10 Tf 72 672 Td (continued) Tj ET\n"
        + &row(652, "Pears", "2");
    let e = extract(&c);
    assert!(
        e.markdown.contains("| Long description continued | 3 |"),
        "{}",
        e.markdown
    );
    assert!(e.markdown.contains("| Pears | 2 |"));
}
#[test]
fn prose_after_a_table_remains_a_paragraph() {
    let c = row(700, "Item", "Qty")
        + &row(684, "Apples", "3")
        + &row(668, "Pears", "2")
        + "BT /F1 10 Tf 72 620 Td (A paragraph below the table.) Tj ET";
    assert!(
        extract(&c)
            .markdown
            .ends_with("\n\nA paragraph below the table.\n")
    );
}
#[test]
fn bold_and_italic_font_runs_are_preserved() {
    let bytes = pdf_pages_with(
        &["BT /F2 10 Tf 72 700 Td (Bold words) Tj ET\nBT /F3 10 Tf 72 680 Td (Italic words) Tj ET"],
        "/F2 6 0 R /F3 7 0 R",
        &[
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>".to_vec(),
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Oblique >>".to_vec(),
        ],
    );
    let e = Markkit::builder()
        .build()
        .extract_document_sync(&bytes, DocumentHint::default())
        .unwrap();
    assert_eq!(e.markdown, "**Bold words**\n\n*Italic words*\n");
}
#[test]
fn tagged_heading_level_takes_precedence_over_font_size() {
    assert_eq!(extract("/H2 BMC BT /F1 10 Tf 72 700 Td (Section) Tj ET EMC\nBT /F1 10 Tf 72 670 Td (Body) Tj ET").markdown,"## Section\n\nBody\n");
}
#[test]
fn security_table_grid_caps_apply_before_expansion() {
    let c = "72 660 400 60 re S\n72 680 m 472 680 l S\n72 700 m 472 700 l S\n272 660 m 272 720 l S\nBT /F1 10 Tf 80 705 Td (Item) Tj ET\nBT /F1 10 Tf 280 685 Td (3) Tj ET";
    let err = Markkit::builder()
        .max_spreadsheet_cells(2)
        .build()
        .parse_document_sync(&pdf_pages(&[c]), DocumentHint::default())
        .unwrap_err();
    assert!(
        matches!(err,markkit::Error::ResourceLimit(ref e) if e.kind==markkit::limits::LimitKind::MaxSpreadsheetCells)
    );
}

#[test]
fn narrow_gutter_and_offset_baselines_do_not_interleave_columns() {
    let c = "BT /F1 10 Tf 72 700 Td (Long left line filling the space completely.) Tj ET\nBT /F1 10 Tf 302 703 Td (Right first) Tj ET\nBT /F1 10 Tf 72 686 Td (Another left line filling the space fully.) Tj ET\nBT /F1 10 Tf 302 689 Td (Right second) Tj ET";
    let e = extract(c);
    assert!(
        e.markdown.find("Another left").unwrap() < e.markdown.find("Right first").unwrap(),
        "{}",
        e.markdown
    );
}
#[test]
fn structure_tree_mcid_and_rolemap_supply_heading_role() {
    let bytes = pdf_bytes(&[
        b"<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 6 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [4 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>"
            .to_vec(),
        pdf_stream(
            "",
            b"/Span << /MCID 0 >> BDC BT /F1 10 Tf 72 700 Td (Section) Tj ET EMC",
        ),
        b"<< /Type /StructTreeRoot /RoleMap << /SectionTitle /H2 >> /K [7 0 R] >>".to_vec(),
        b"<< /Type /StructElem /S /SectionTitle /Pg 4 0 R /K 0 >>".to_vec(),
    ]);
    assert_eq!(
        Markkit::builder()
            .build()
            .extract_document_sync(&bytes, DocumentHint::default())
            .unwrap()
            .markdown,
        "## Section\n"
    );
}

#[test]
fn security_marked_content_depth_and_actual_text_storage_are_bounded() {
    let deep = "/Span BMC\n".repeat(257) + "BT /F1 10 Tf 72 700 Td (X) Tj ET";
    assert!(
        Markkit::builder()
            .build()
            .parse_document_sync(&pdf_pages(&[&deep]), DocumentHint::default())
            .is_err()
    );
    let replacement = "A".repeat(800);
    let c =
        format!("/Span << /ActualText ({replacement}) >> BDC BT /F1 10 Tf 72 700 Td (X) Tj ET EMC");
    let err = Markkit::builder()
        .max_pdf_decoded_size(1200)
        .build()
        .parse_document_sync(&pdf_pages(&[&c]), DocumentHint::default())
        .unwrap_err();
    assert!(
        matches!(err,markkit::Error::ResourceLimit(ref e) if e.kind==markkit::limits::LimitKind::MaxPdfDecodedSize)
    );
}

#[test]
fn security_many_fragments_on_one_baseline_stay_one_line() {
    let content = (0..300)
        .map(|i| format!("BT /F1 10 Tf {} 700 Td (X) Tj ET\n", 72 + i * 40))
        .collect::<String>();
    let parsed = Markkit::builder()
        .max_pdf_decoded_size(1_000_000)
        .build()
        .parse_document_sync(&pdf_pages(&[&content]), DocumentHint::default())
        .unwrap();
    assert_eq!(parsed.document.nodes.len(), 1);
    assert_eq!(parsed.render().markdown.split_whitespace().count(), 300);
}
