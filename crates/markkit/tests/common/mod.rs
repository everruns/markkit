//! Programmatic fixture builders for office-document tests.
//!
//! Fixtures are generated in-memory (zip + minimal OOXML/ODF/BIFF8) instead of
//! committing binaries, so each test documents exactly the structure it exercises.

#![allow(dead_code)]

use std::io::{Cursor, Write};

use zip::CompressionMethod;
use zip::write::{SimpleFileOptions, ZipWriter};

/// Build a ZIP archive from `(name, bytes)` entries (deflated).
pub fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    zip_bytes_with(entries, CompressionMethod::Deflated)
}

pub fn zip_bytes_with(entries: &[(&str, &[u8])], method: CompressionMethod) -> Vec<u8> {
    let mut w = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in entries {
        let opts = if *name == "mimetype" {
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored)
        } else {
            SimpleFileOptions::default()
                .compression_method(method)
                .large_file(data.len() > u32::MAX as usize / 2)
        };
        w.start_file(*name, opts).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

// ─── DOCX ───────────────────────────────────────────────────────────

pub const W_NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;

const DOCX_CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;

const DOCX_ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>
</Relationships>"#;

/// Optional DOCX parts.
#[derive(Default)]
pub struct DocxParts<'a> {
    /// `<w:body>` inner XML.
    pub body: &'a str,
    /// Extra `<Relationship>` elements for `word/_rels/document.xml.rels`.
    pub rels: &'a str,
    /// Inner XML of `<w:styles>`.
    pub styles: Option<&'a str>,
    /// Inner XML of `<w:numbering>`.
    pub numbering: Option<&'a str>,
    /// `dc:title` in `docProps/core.xml`.
    pub title: Option<&'a str>,
    /// Additional raw entries.
    pub extra: Vec<(&'a str, Vec<u8>)>,
}

pub fn docx(body: &str) -> Vec<u8> {
    docx_with(DocxParts {
        body,
        ..Default::default()
    })
}

pub fn docx_with(p: DocxParts<'_>) -> Vec<u8> {
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {W_NS}><w:body>{}</w:body></w:document>"#,
        p.body
    );
    let rels = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{}</Relationships>"#,
        p.rels
    );
    let mut entries: Vec<(String, Vec<u8>)> = vec![
        ("[Content_Types].xml".into(), DOCX_CONTENT_TYPES.into()),
        ("_rels/.rels".into(), DOCX_ROOT_RELS.into()),
        ("word/document.xml".into(), document.into_bytes()),
        ("word/_rels/document.xml.rels".into(), rels.into_bytes()),
    ];
    if let Some(styles) = p.styles {
        entries.push((
            "word/styles.xml".into(),
            format!(r#"<?xml version="1.0"?><w:styles {W_NS}>{styles}</w:styles>"#).into_bytes(),
        ));
    }
    if let Some(numbering) = p.numbering {
        entries.push((
            "word/numbering.xml".into(),
            format!(r#"<?xml version="1.0"?><w:numbering {W_NS}>{numbering}</w:numbering>"#)
                .into_bytes(),
        ));
    }
    if let Some(title) = p.title {
        entries.push((
            "docProps/core.xml".into(),
            format!(
                r#"<?xml version="1.0"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>{title}</dc:title></cp:coreProperties>"#
            )
            .into_bytes(),
        ));
    }
    for (name, data) in p.extra {
        entries.push((name.to_string(), data));
    }
    let refs: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(n, d)| (n.as_str(), d.as_slice()))
        .collect();
    zip_bytes(&refs)
}

/// `<w:p>` with a single plain run.
pub fn para(text: &str) -> String {
    format!("<w:p><w:r><w:t xml:space=\"preserve\">{text}</w:t></w:r></w:p>")
}

/// `<w:p>` with a paragraph style.
pub fn styled_para(style: &str, text: &str) -> String {
    format!("<w:p><w:pPr><w:pStyle w:val=\"{style}\"/></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>")
}

/// `<w:p>` that is a list item (numId / ilvl).
pub fn list_para(num_id: u32, ilvl: u32, text: &str) -> String {
    format!(
        "<w:p><w:pPr><w:pStyle w:val=\"ListParagraph\"/><w:numPr><w:ilvl w:val=\"{ilvl}\"/><w:numId w:val=\"{num_id}\"/></w:numPr></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>"
    )
}

/// numbering.xml: numId 1 = bullets, numId 2 = decimal.
pub const NUMBERING: &str = r#"
<w:abstractNum w:abstractNumId="0">
  <w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="bullet"/></w:lvl>
  <w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="bullet"/></w:lvl>
</w:abstractNum>
<w:abstractNum w:abstractNumId="1">
  <w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/></w:lvl>
  <w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="lowerLetter"/></w:lvl>
</w:abstractNum>
<w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
<w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num>
"#;

// ─── XLSX ───────────────────────────────────────────────────────────

/// A cell value for generated sheets.
#[derive(Clone, Debug)]
pub enum Cell {
    S(&'static str),
    Str(String),
    N(f64),
    B(bool),
    Empty,
}

pub struct Sheet {
    pub name: String,
    pub rows: Vec<Vec<Cell>>,
    pub hidden: bool,
}

impl Sheet {
    pub fn new(name: &str, rows: Vec<Vec<Cell>>) -> Self {
        Self {
            name: name.into(),
            rows,
            hidden: false,
        }
    }
}

pub fn col_name(mut c: usize) -> String {
    let mut s = Vec::new();
    loop {
        s.push(b'A' + (c % 26) as u8);
        if c < 26 {
            break;
        }
        c = c / 26 - 1;
    }
    s.reverse();
    String::from_utf8(s).unwrap()
}

pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn cell_xml(r: usize, c: usize, cell: &Cell) -> String {
    let at = format!("{}{}", col_name(c), r + 1);
    match cell {
        Cell::S(s) => format!(
            r#"<c r="{at}" t="inlineStr"><is><t xml:space="preserve">{}</t></is></c>"#,
            xml_escape(s)
        ),
        Cell::Str(s) => format!(
            r#"<c r="{at}" t="inlineStr"><is><t xml:space="preserve">{}</t></is></c>"#,
            xml_escape(s)
        ),
        Cell::N(n) => format!(r#"<c r="{at}"><v>{n}</v></c>"#),
        Cell::B(b) => format!(r#"<c r="{at}" t="b"><v>{}</v></c>"#, u8::from(*b)),
        Cell::Empty => String::new(),
    }
}

/// Raw worksheet XML from `<sheetData>` inner XML.
pub fn worksheet_xml(sheet_data: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheetData>{sheet_data}</sheetData></worksheet>"#
    )
}

pub fn sheet_data(rows: &[Vec<Cell>]) -> String {
    let mut out = String::new();
    for (r, row) in rows.iter().enumerate() {
        out.push_str(&format!(r#"<row r="{}">"#, r + 1));
        for (c, cell) in row.iter().enumerate() {
            out.push_str(&cell_xml(r, c, cell));
        }
        out.push_str("</row>");
    }
    out
}

/// Build an XLSX from sheets, plus extra raw entries.
pub fn xlsx_with(
    sheets: &[Sheet],
    raw_sheet_xml: &[Option<String>],
    extra: &[(&str, &[u8])],
) -> Vec<u8> {
    let mut ct = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#,
    );
    let mut wb_sheets = String::new();
    let mut wb_rels = String::new();
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for (i, s) in sheets.iter().enumerate() {
        let n = i + 1;
        ct.push_str(&format!(r#"<Override PartName="/xl/worksheets/sheet{n}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#));
        let state = if s.hidden { r#" state="hidden""# } else { "" };
        wb_sheets.push_str(&format!(
            r#"<sheet name="{}" sheetId="{n}"{state} r:id="rId{n}"/>"#,
            xml_escape(&s.name)
        ));
        wb_rels.push_str(&format!(r#"<Relationship Id="rId{n}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{n}.xml"/>"#));
        let xml = raw_sheet_xml
            .get(i)
            .cloned()
            .flatten()
            .unwrap_or_else(|| worksheet_xml(&sheet_data(&s.rows)));
        entries.push((format!("xl/worksheets/sheet{n}.xml"), xml.into_bytes()));
    }
    ct.push_str("</Types>");
    let workbook = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>{wb_sheets}</sheets></workbook>"#
    );
    let rels = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{wb_rels}</Relationships>"#
    );
    let root_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
    let mut all: Vec<(String, Vec<u8>)> = vec![
        ("[Content_Types].xml".into(), ct.into_bytes()),
        ("_rels/.rels".into(), root_rels.as_bytes().to_vec()),
        ("xl/workbook.xml".into(), workbook.into_bytes()),
        ("xl/_rels/workbook.xml.rels".into(), rels.into_bytes()),
    ];
    all.extend(entries);
    for (n, d) in extra {
        all.push((n.to_string(), d.to_vec()));
    }
    let refs: Vec<(&str, &[u8])> = all
        .iter()
        .map(|(n, d)| (n.as_str(), d.as_slice()))
        .collect();
    zip_bytes(&refs)
}

pub fn xlsx(sheets: &[Sheet]) -> Vec<u8> {
    xlsx_with(sheets, &[], &[])
}

// ─── ODS ────────────────────────────────────────────────────────────

pub const ODS_MIME: &str = "application/vnd.oasis.opendocument.spreadsheet";

/// Build an ODS from `<office:spreadsheet>` inner XML.
pub fn ods(spreadsheet_inner: &str) -> Vec<u8> {
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" office:version="1.2"><office:body><office:spreadsheet>{spreadsheet_inner}</office:spreadsheet></office:body></office:document-content>"#
    );
    let manifest = r#"<?xml version="1.0" encoding="UTF-8"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"><manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.spreadsheet"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/></manifest:manifest>"#;
    zip_bytes(&[
        ("mimetype", ODS_MIME.as_bytes()),
        ("content.xml", content.as_bytes()),
        ("META-INF/manifest.xml", manifest.as_bytes()),
    ])
}

pub fn ods_string_cell(s: &str) -> String {
    format!(
        r#"<table:table-cell office:value-type="string"><text:p>{}</text:p></table:table-cell>"#,
        xml_escape(s)
    )
}

pub fn ods_float_cell(v: f64) -> String {
    format!(
        r#"<table:table-cell office:value-type="float" office:value="{v}"><text:p>{v}</text:p></table:table-cell>"#
    )
}

// ─── ODT ────────────────────────────────────────────────────────────

pub const ODT_MIME: &str = "application/vnd.oasis.opendocument.text";

const ODF_NS: &str = r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:meta="urn:oasis:names:tc:opendocument:xmlns:meta:1.0""#;

/// ODT with `automatic` styles + `body` in content.xml, optional styles.xml
/// `<office:styles>` inner XML and meta.xml title.
pub fn odt_with(automatic: &str, body: &str, styles: Option<&str>, title: Option<&str>) -> Vec<u8> {
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content {ODF_NS} office:version="1.3"><office:automatic-styles>{automatic}</office:automatic-styles><office:body><office:text>{body}</office:text></office:body></office:document-content>"#
    );
    let styles_xml = styles.map(|s| {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><office:document-styles {ODF_NS} office:version="1.3"><office:styles>{s}</office:styles></office:document-styles>"#
        )
    });
    let meta = title.map(|t| {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><office:document-meta {ODF_NS} office:version="1.3"><office:meta><dc:title>{}</dc:title></office:meta></office:document-meta>"#,
            xml_escape(t)
        )
    });
    let mut entries: Vec<(&str, &[u8])> = vec![
        ("mimetype", ODT_MIME.as_bytes()),
        ("content.xml", content.as_bytes()),
    ];
    if let Some(s) = &styles_xml {
        entries.push(("styles.xml", s.as_bytes()));
    }
    if let Some(m) = &meta {
        entries.push(("meta.xml", m.as_bytes()));
    }
    zip_bytes(&entries)
}

pub fn odt(body: &str) -> Vec<u8> {
    odt_with("", body, None, None)
}

// ─── EPUB ───────────────────────────────────────────────────────────

pub const EPUB_MIME: &str = "application/epub+zip";

pub const EPUB_CONTAINER: &str = r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;

/// OPF with `title`, manifest `(id, href, media-type)` and spine `(idref, linear)`.
pub fn epub_opf(title: &str, manifest: &[(&str, &str, &str)], spine: &[(&str, bool)]) -> String {
    let items: String = manifest
        .iter()
        .map(|(id, href, mt)| format!(r#"<item id="{id}" href="{href}" media-type="{mt}"/>"#))
        .collect();
    let refs: String = spine
        .iter()
        .map(|(id, linear)| {
            if *linear {
                format!(r#"<itemref idref="{id}"/>"#)
            } else {
                format!(r#"<itemref idref="{id}" linear="no"/>"#)
            }
        })
        .collect();
    format!(
        r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>{}</dc:title></metadata><manifest>{items}</manifest><spine>{refs}</spine></package>"#,
        xml_escape(title)
    )
}

pub fn xhtml(body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title></head><body>{body}</body></html>"#
    )
}

/// EPUB from extra `(path, bytes)` entries plus mimetype and container.
pub fn epub(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut all: Vec<(&str, &[u8])> = vec![
        ("mimetype", EPUB_MIME.as_bytes()),
        ("META-INF/container.xml", EPUB_CONTAINER.as_bytes()),
    ];
    all.extend_from_slice(entries);
    zip_bytes(&all)
}

// ─── XLS (BIFF8 in an OLE2 compound file) ───────────────────────────

fn record(out: &mut Vec<u8>, typ: u16, data: &[u8]) {
    out.extend_from_slice(&typ.to_le_bytes());
    out.extend_from_slice(&(data.len() as u16).to_le_bytes());
    out.extend_from_slice(data);
}

fn bof(dt: u16) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&0x0600u16.to_le_bytes()); // BIFF8
    d.extend_from_slice(&dt.to_le_bytes());
    d.extend_from_slice(&0x0DBBu16.to_le_bytes());
    d.extend_from_slice(&0x07CCu16.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&0x0006u32.to_le_bytes());
    d
}

/// XLS cell for generated workbooks.
pub enum XlsCell {
    Num(u16, u16, f64),
    Label(u16, u16, &'static str),
}

/// A raw BIFF8 sheet: optional DIMENSIONS (rowFirst, rowLastPlus1, colFirst, colLastPlus1) and cells.
pub struct XlsSheet {
    pub name: &'static str,
    pub dims: Option<(u32, u32, u16, u16)>,
    pub cells: Vec<XlsCell>,
}

/// Build the BIFF8 `Workbook` stream.
pub fn biff8_workbook(sheets: &[XlsSheet]) -> Vec<u8> {
    // Globals: BOF, CODEPAGE, BOUNDSHEET*, EOF. BOUNDSHEET offsets patched afterwards.
    let mut globals = Vec::new();
    record(&mut globals, 0x0809, &bof(0x0005));
    record(&mut globals, 0x0042, &1200u16.to_le_bytes());
    let mut patch_at = Vec::new();
    for s in sheets {
        let mut d = Vec::new();
        d.extend_from_slice(&0u32.to_le_bytes()); // placeholder
        d.push(0); // visible
        d.push(0); // worksheet
        d.push(s.name.len() as u8);
        d.push(0); // compressed 8-bit chars
        d.extend_from_slice(s.name.as_bytes());
        patch_at.push(globals.len() + 4);
        record(&mut globals, 0x0085, &d);
    }
    record(&mut globals, 0x000A, &[]);

    let mut stream = globals;
    for (i, s) in sheets.iter().enumerate() {
        let pos = stream.len() as u32;
        stream[patch_at[i]..patch_at[i] + 4].copy_from_slice(&pos.to_le_bytes());
        record(&mut stream, 0x0809, &bof(0x0010));
        if let Some((rf, rl, cf, cl)) = s.dims {
            let mut d = Vec::new();
            d.extend_from_slice(&rf.to_le_bytes());
            d.extend_from_slice(&rl.to_le_bytes());
            d.extend_from_slice(&cf.to_le_bytes());
            d.extend_from_slice(&cl.to_le_bytes());
            d.extend_from_slice(&0u16.to_le_bytes());
            record(&mut stream, 0x0200, &d);
        }
        for c in &s.cells {
            match c {
                XlsCell::Num(r, col, v) => {
                    let mut d = Vec::new();
                    d.extend_from_slice(&r.to_le_bytes());
                    d.extend_from_slice(&col.to_le_bytes());
                    d.extend_from_slice(&0u16.to_le_bytes());
                    d.extend_from_slice(&v.to_le_bytes());
                    record(&mut stream, 0x0203, &d);
                }
                XlsCell::Label(r, col, text) => {
                    let mut d = Vec::new();
                    d.extend_from_slice(&r.to_le_bytes());
                    d.extend_from_slice(&col.to_le_bytes());
                    d.extend_from_slice(&0u16.to_le_bytes());
                    d.extend_from_slice(&(text.len() as u16).to_le_bytes());
                    d.push(0);
                    d.extend_from_slice(text.as_bytes());
                    record(&mut stream, 0x0204, &d);
                }
            }
        }
        record(&mut stream, 0x000A, &[]);
    }
    stream
}

/// Wrap a workbook stream into an OLE2 compound file.
pub fn xls_from_stream(stream: &[u8]) -> Vec<u8> {
    ole2(&[("/Workbook", stream)])
}

/// Build an OLE2 compound file from `(path, data)` streams.
pub fn ole2(streams: &[(&str, &[u8])]) -> Vec<u8> {
    let mut comp = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    for (path, data) in streams {
        let mut s = comp.create_stream(path).unwrap();
        s.write_all(data).unwrap();
    }
    comp.flush().unwrap();
    comp.into_inner().into_inner()
}

/// `\u{5}SummaryInformation` property set with a code page and a VT_LPSTR title.
pub fn summary_information(codepage: u16, title: &[u8]) -> Vec<u8> {
    let mut value = title.to_vec();
    value.push(0);
    let mut set = Vec::new();
    for v in [
        0u32,
        2,
        1,
        24,
        2,
        32,
        2,
        u32::from(codepage),
        0x1E,
        value.len() as u32,
    ] {
        set.extend_from_slice(&v.to_le_bytes());
    }
    set.extend_from_slice(&value);
    let mut d = vec![0u8; 48];
    d[0..2].copy_from_slice(&0xFFFEu16.to_le_bytes());
    d[24..28].copy_from_slice(&1u32.to_le_bytes());
    d[44..48].copy_from_slice(&48u32.to_le_bytes());
    d.extend_from_slice(&set);
    d
}

/// Rebuild a ZIP without the `drop` entries and with `add` appended.
pub fn rezip(bytes: &[u8], drop: &[&str], add: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut entries = Vec::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        let name = f.name().unwrap().to_string();
        if drop.contains(&name.as_str()) {
            continue;
        }
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut data).unwrap();
        entries.push((name, data));
    }
    let mut refs: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(n, d)| (n.as_str(), d.as_slice()))
        .collect();
    refs.extend_from_slice(add);
    zip_bytes(&refs)
}

pub fn xls(sheets: &[XlsSheet]) -> Vec<u8> {
    xls_from_stream(&biff8_workbook(sheets))
}

// ─── XLSB (BIFF12 records in an OPC ZIP) ────────────────────────────

/// Append a BIFF12 record (1–2 byte type, 7-bit varint size).
pub fn biff12(out: &mut Vec<u8>, typ: u16, data: &[u8]) {
    if typ < 0x80 {
        out.push(typ as u8);
    } else {
        out.push((typ & 0x7F) as u8 | 0x80);
        out.push((typ >> 7) as u8);
    }
    let mut n = data.len();
    loop {
        let b = (n & 0x7F) as u8;
        n >>= 7;
        if n == 0 {
            out.push(b);
            break;
        }
        out.push(b | 0x80);
    }
    out.extend_from_slice(data);
}

/// `XLWideString`.
pub fn wide(s: &str) -> Vec<u8> {
    let units: Vec<u16> = s.encode_utf16().collect();
    let mut v = (units.len() as u32).to_le_bytes().to_vec();
    v.extend(units.iter().flat_map(|u| u.to_le_bytes()));
    v
}

/// Worksheet part with inline strings (`BrtCellSt`) and numbers (`BrtCellReal`).
pub fn xlsb_sheet(rows: &[Vec<Cell>]) -> Vec<u8> {
    let mut d = Vec::new();
    biff12(&mut d, 0x0081, &[]); // BrtBeginSheet
    biff12(&mut d, 0x0094, &[0; 16]); // BrtWsDim
    biff12(&mut d, 0x0091, &[]); // BrtBeginSheetData
    for (r, row) in rows.iter().enumerate() {
        biff12(&mut d, 0x0000, &(r as u32).to_le_bytes());
        for (c, cell) in row.iter().enumerate() {
            let mut rec = (c as u32).to_le_bytes().to_vec();
            rec.extend_from_slice(&0u32.to_le_bytes());
            match cell {
                Cell::S(s) => {
                    rec.extend(wide(s));
                    biff12(&mut d, 0x0006, &rec);
                }
                Cell::Str(s) => {
                    rec.extend(wide(s));
                    biff12(&mut d, 0x0006, &rec);
                }
                Cell::N(n) => {
                    rec.extend_from_slice(&n.to_le_bytes());
                    biff12(&mut d, 0x0005, &rec);
                }
                Cell::B(b) => {
                    rec.push(u8::from(*b));
                    biff12(&mut d, 0x0004, &rec);
                }
                Cell::Empty => {}
            }
        }
    }
    biff12(&mut d, 0x0092, &[]); // BrtEndSheetData
    biff12(&mut d, 0x0082, &[]); // BrtEndSheet
    d
}

/// Build an XLSB from sheets (hidden sheets honoured), plus extra raw entries.
pub fn xlsb_with(sheets: &[Sheet], extra: &[(&str, &[u8])]) -> Vec<u8> {
    let mut wb = Vec::new();
    biff12(&mut wb, 0x0083, &[]); // BrtBeginBook
    biff12(&mut wb, 0x008F, &[]); // BrtBeginBundleShs
    let mut rels = String::new();
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for (i, s) in sheets.iter().enumerate() {
        let n = i + 1;
        let mut rec = u32::from(s.hidden).to_le_bytes().to_vec();
        rec.extend_from_slice(&(n as u32).to_le_bytes());
        rec.extend(wide(&format!("rId{n}")));
        rec.extend(wide(&s.name));
        biff12(&mut wb, 0x009C, &rec);
        rels.push_str(&format!(r#"<Relationship Id="rId{n}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{n}.bin"/>"#));
        entries.push((format!("xl/worksheets/sheet{n}.bin"), xlsb_sheet(&s.rows)));
    }
    biff12(&mut wb, 0x0090, &[]); // BrtEndBundleShs
    biff12(&mut wb, 0x0084, &[]); // BrtEndBook
    let rels = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}</Relationships>"#
    );
    let root_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.bin"/></Relationships>"#;
    let mut all: Vec<(String, Vec<u8>)> = vec![
        ("_rels/.rels".into(), root_rels.as_bytes().to_vec()),
        ("xl/workbook.bin".into(), wb),
        ("xl/_rels/workbook.bin.rels".into(), rels.into_bytes()),
    ];
    all.extend(entries);
    for (n, d) in extra {
        all.push((n.to_string(), d.to_vec()));
    }
    let refs: Vec<(&str, &[u8])> = all
        .iter()
        .map(|(n, d)| (n.as_str(), d.as_slice()))
        .collect();
    zip_bytes(&refs)
}

pub fn xlsb(sheets: &[Sheet]) -> Vec<u8> {
    xlsb_with(sheets, &[])
}

// ─── PDF ────────────────────────────────────────────────────────────

/// Zlib-compress (for `/FlateDecode` streams).
pub fn zlib(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(data, 9)
}

/// A stream object body: `<< dict /Length n >> stream … endstream`.
pub fn pdf_stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// Assemble a PDF from object bodies (object `i + 1` = `objects[i]`) with a
/// correct xref table. Object 1 must be the catalog.
pub fn pdf_bytes_with_trailer(objects: &[Vec<u8>], trailer_extra: &str) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", i + 1).bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
    for off in offsets {
        out.extend(format!("{off:010} 00000 n \n").bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R {trailer_extra} >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .bytes(),
    );
    out
}

pub fn pdf_bytes(objects: &[Vec<u8>]) -> Vec<u8> {
    pdf_bytes_with_trailer(objects, "")
}

/// Single- or multi-page PDF; each page's content uses `/F1` = Helvetica
/// (WinAnsi). Objects: 1 catalog, 2 pages, 3 font, then (page, content) pairs.
pub fn pdf_pages(contents: &[&str]) -> Vec<u8> {
    pdf_pages_with(contents, "/F1 3 0 R", &[])
}

/// Like [`pdf_pages`] with a custom font resource dict body and extra objects
/// appended after the pages (numbered from `4 + 2 * contents.len()`).
pub fn pdf_pages_with(contents: &[&str], fonts: &str, extra: &[Vec<u8>]) -> Vec<u8> {
    let n = contents.len();
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 4 + 2 * i)).collect();
    let mut objs = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")).into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_vec(),
    ];
    for (i, c) in contents.iter().enumerate() {
        objs.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << {fonts} >> /XObject << /X1 {} 0 R >> >> /Contents {} 0 R >>",
                4 + 2 * n,
                5 + 2 * i
            )
            .into_bytes(),
        );
        objs.push(pdf_stream("", c.as_bytes()));
    }
    objs.extend(extra.iter().cloned());
    pdf_bytes(&objs)
}

// ─── PPTX ───────────────────────────────────────────────────────────

pub const PPTX_CT: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presentation";

const P_NS: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;
const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// A generated slide: `spTree` inner XML, optional notes text, hidden flag,
/// extra slide relationships (`Id`, `Target`) as external hyperlinks.
#[derive(Default)]
pub struct PSlide<'a> {
    pub shapes: String,
    pub notes: Option<&'a str>,
    pub hidden: bool,
    pub links: Vec<(&'a str, &'a str)>,
}

impl PSlide<'_> {
    pub fn new(shapes: String) -> Self {
        Self {
            shapes,
            ..Default::default()
        }
    }
}

/// `<p:sp>` with an optional placeholder type (`Some("")` = untyped content placeholder).
pub fn p_shape(ph: Option<&str>, paragraphs: &str) -> String {
    let ph = match ph {
        None => String::new(),
        Some("") => r#"<p:ph idx="1"/>"#.to_string(),
        Some(t) => format!(r#"<p:ph type="{t}"/>"#),
    };
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="2" name="s"/><p:cNvSpPr/><p:nvPr>{ph}</p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/>{paragraphs}</p:txBody></p:sp>"#
    )
}

/// `<a:p>` with one plain run.
pub fn a_para(text: &str) -> String {
    format!("<a:p><a:r><a:t>{}</a:t></a:r></a:p>", xml_escape(text))
}

pub fn pptx_with(slides: &[PSlide<'_>], extra: &[(&str, &[u8])]) -> Vec<u8> {
    let mut ids = String::new();
    let mut rels = String::new();
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for (i, s) in slides.iter().enumerate() {
        let n = i + 1;
        ids.push_str(&format!(r#"<p:sldId id="{}" r:id="rId{n}"/>"#, 255 + n));
        rels.push_str(&format!(
            r#"<Relationship Id="rId{n}" Type="{REL_NS}/slide" Target="slides/slide{n}.xml"/>"#
        ));
        let show = if s.hidden { r#" show="0""# } else { "" };
        entries.push((
            format!("ppt/slides/slide{n}.xml"),
            format!(r#"<?xml version="1.0" encoding="UTF-8"?><p:sld {P_NS}{show}><p:cSld><p:spTree>{}</p:spTree></p:cSld></p:sld>"#, s.shapes).into_bytes(),
        ));
        let mut srels = String::new();
        for (id, target) in &s.links {
            srels.push_str(&format!(r#"<Relationship Id="{id}" Type="{REL_NS}/hyperlink" Target="{}" TargetMode="External"/>"#, xml_escape(target)));
        }
        if let Some(notes) = s.notes {
            srels.push_str(&format!(r#"<Relationship Id="rIdN" Type="{REL_NS}/notesSlide" Target="../notesSlides/notesSlide{n}.xml"/>"#));
            entries.push((
                format!("ppt/notesSlides/notesSlide{n}.xml"),
                format!(
                    r#"<p:notes {P_NS}><p:cSld><p:spTree>{}{}</p:spTree></p:cSld></p:notes>"#,
                    p_shape(Some("sldImg"), ""),
                    p_shape(Some("body"), &a_para(notes))
                )
                .into_bytes(),
            ));
        }
        entries.push((
            format!("ppt/slides/_rels/slide{n}.xml.rels"),
            format!(r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{srels}</Relationships>"#).into_bytes(),
        ));
    }
    let pres = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><p:presentation {P_NS}><p:sldIdLst>{ids}</p:sldIdLst></p:presentation>"#
    );
    let pres_rels = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}</Relationships>"#
    );
    let root_rels = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{REL_NS}/officeDocument" Target="ppt/presentation.xml"/></Relationships>"#
    );
    let mut all: Vec<(String, Vec<u8>)> = vec![
        ("_rels/.rels".into(), root_rels.into_bytes()),
        ("ppt/presentation.xml".into(), pres.into_bytes()),
        (
            "ppt/_rels/presentation.xml.rels".into(),
            pres_rels.into_bytes(),
        ),
    ];
    all.extend(entries);
    for (n, d) in extra {
        all.push((n.to_string(), d.to_vec()));
    }
    let refs: Vec<(&str, &[u8])> = all
        .iter()
        .map(|(n, d)| (n.as_str(), d.as_slice()))
        .collect();
    zip_bytes(&refs)
}

pub fn pptx(slides: &[PSlide<'_>]) -> Vec<u8> {
    pptx_with(slides, &[])
}
