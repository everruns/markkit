//! Benchmark corpus.
//!
//! Inputs are generated deterministically in code (no network, no committed
//! binaries) so every run on every machine measures the same bytes. Sizes stay
//! under the default builder limits: the harness measures the happy path, the
//! security limits are covered by tests.

use std::io::{Cursor, Write};

use markkit::{DocumentHint, Format, Markkit};
use zip::write::{SimpleFileOptions, ZipWriter};

/// What a case feeds to markkit.
pub enum Input {
    /// `Markkit::extract_html_sync`.
    Html(String),
    /// `Markkit::extract_document_sync` with a file-name hint.
    Document {
        bytes: Vec<u8>,
        file_name: &'static str,
    },
    /// `Format::detect` on raw bytes with no hint (magic-byte sniffing).
    Detect(Vec<u8>),
}

pub struct Case {
    pub name: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    pub input: Input,
}

impl Case {
    pub fn input_bytes(&self) -> usize {
        match &self.input {
            Input::Html(s) => s.len(),
            Input::Document { bytes, .. } | Input::Detect(bytes) => bytes.len(),
        }
    }

    /// Run one operation. Returns output size in bytes.
    pub fn run(&self, mk: &Markkit) -> Result<usize, String> {
        match &self.input {
            Input::Html(html) => mk
                .extract_html_sync(html)
                .map(|md| md.len())
                .map_err(|e| e.to_string()),
            Input::Document { bytes, file_name } => mk
                .extract_document_sync(
                    bytes,
                    DocumentHint {
                        content_type: None,
                        file_name: Some(file_name),
                    },
                )
                .map(|e| e.markdown.len())
                .map_err(|e| e.to_string()),
            Input::Detect(bytes) => match Format::detect(bytes, DocumentHint::default()) {
                Format::Unknown => Err("format not detected".into()),
                _ => Ok(0),
            },
        }
    }
}

pub const CATEGORIES: &[&str] = &["html", "docx", "spreadsheet", "detect"];

pub fn all() -> Vec<Case> {
    let docx_large = docx_report(400);
    vec![
        Case {
            name: "html_fixture_article",
            category: "html",
            description: "Golden fixture full_article.html",
            input: Input::Html(
                include_str!("../../markkit/tests/fixtures/html/full_article.html").into(),
            ),
        },
        Case {
            name: "html_blog_post",
            category: "html",
            description: "Article with nav/footer/scripts, 20 sections",
            input: Input::Html(html_page(20, true)),
        },
        Case {
            name: "html_docs_page",
            category: "html",
            description: "Long docs page, 400 sections, no chrome",
            input: Input::Html(html_page(400, false)),
        },
        Case {
            name: "html_tables",
            category: "html",
            description: "50 tables x 40 rows x 6 cols",
            input: Input::Html(html_tables(50, 40, 6)),
        },
        Case {
            name: "html_boilerplate",
            category: "html",
            description: "Mostly script/style/nav to strip, little content",
            input: Input::Html(html_boilerplate(400)),
        },
        Case {
            name: "html_deep_nesting",
            category: "html",
            description: "Divs nested past the depth limit (1000 levels)",
            input: Input::Html(format!(
                "{}<p>deep</p>{}",
                "<div>".repeat(1000),
                "</div>".repeat(1000)
            )),
        },
        Case {
            name: "docx_memo",
            category: "docx",
            description: "Short memo: heading, 10 paragraphs, 1 list",
            input: Input::Document {
                bytes: docx_report(1),
                file_name: "memo.docx",
            },
        },
        Case {
            name: "docx_report",
            category: "docx",
            description: "Report: 400 sections with lists and tables",
            input: Input::Document {
                bytes: docx_large.clone(),
                file_name: "report.docx",
            },
        },
        Case {
            name: "xlsx_small",
            category: "spreadsheet",
            description: "1 sheet, 50 rows x 5 cols",
            input: Input::Document {
                bytes: xlsx(1, 50, 5),
                file_name: "small.xlsx",
            },
        },
        Case {
            name: "xlsx_large",
            category: "spreadsheet",
            description: "3 sheets, 5000 rows x 10 cols each (150k cells)",
            input: Input::Document {
                bytes: xlsx(3, 5000, 10),
                file_name: "large.xlsx",
            },
        },
        Case {
            name: "ods_medium",
            category: "spreadsheet",
            description: "1 sheet, 2000 rows x 8 cols",
            input: Input::Document {
                bytes: ods(2000, 8),
                file_name: "medium.ods",
            },
        },
        Case {
            name: "detect_docx_magic",
            category: "detect",
            description: "Sniff DOCX from bytes without a hint",
            input: Input::Detect(docx_large),
        },
    ]
}

// ─── HTML ───────────────────────────────────────────────────────────

const LOREM: &str = "Markkit turns documents into clean markdown for language models. \
It keeps <strong>structure</strong>, drops <em>noise</em>, and links to \
<a href=\"https://example.com/docs\">reference docs</a> stay intact.";

fn html_page(sections: usize, chrome: bool) -> String {
    let mut s = String::from("<!doctype html><html><head><title>Bench page</title>");
    if chrome {
        s.push_str("<style>body{font:14px sans-serif}.nav{display:flex}</style>");
        s.push_str("<script>window.dataLayer=[];function track(){}</script>");
    }
    s.push_str("</head><body>");
    if chrome {
        s.push_str("<nav class=\"nav\"><a href=\"/\">Home</a><a href=\"/blog\">Blog</a></nav>");
    }
    s.push_str("<main><article><h1>Benchmark document</h1>");
    for i in 0..sections {
        s.push_str(&format!("<h2>Section {i}</h2>"));
        for _ in 0..3 {
            s.push_str(&format!("<p>{LOREM}</p>"));
        }
        s.push_str(
            "<ul><li>First point</li><li>Second <code>inline()</code></li><li>Third</li></ul>",
        );
        if i % 4 == 0 {
            s.push_str("<pre><code class=\"language-rust\">fn main() {\n    println!(\"hi\");\n}</code></pre>");
        }
        if i % 5 == 0 {
            s.push_str("<blockquote><p>A quoted remark.</p></blockquote>");
            s.push_str("<img src=\"/img/chart.png\" alt=\"Chart\">");
        }
    }
    s.push_str("</article></main>");
    if chrome {
        s.push_str("<footer><p>Copyright</p><script>track()</script></footer>");
    }
    s.push_str("</body></html>");
    s
}

fn html_tables(tables: usize, rows: usize, cols: usize) -> String {
    let mut s = String::from("<html><body>");
    for t in 0..tables {
        s.push_str(&format!("<h3>Table {t}</h3><table><thead><tr>"));
        for c in 0..cols {
            s.push_str(&format!("<th>Col {c}</th>"));
        }
        s.push_str("</tr></thead><tbody>");
        for r in 0..rows {
            s.push_str("<tr>");
            for c in 0..cols {
                s.push_str(&format!("<td>r{r}c{c} value</td>"));
            }
            s.push_str("</tr>");
        }
        s.push_str("</tbody></table>");
    }
    s.push_str("</body></html>");
    s
}

fn html_boilerplate(blocks: usize) -> String {
    let mut s = String::from("<html><head>");
    for i in 0..blocks {
        s.push_str(&format!(
            "<script>var x{i} = {{a: 1, b: [1,2,3], c: 'payload {i}'}};</script>"
        ));
        s.push_str(&format!(
            "<style>.c{i} {{ color: red; margin: {i}px; }}</style>"
        ));
    }
    s.push_str("</head><body><nav><a href=\"/\">Home</a></nav><main><h1>Title</h1>");
    s.push_str(&format!("<p>{LOREM}</p>"));
    s.push_str("</main><aside>Related</aside><footer>Footer</footer></body></html>");
    s
}

// ─── Office ─────────────────────────────────────────────────────────

fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in entries {
        let opts = if *name == "mimetype" {
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)
        } else {
            SimpleFileOptions::default()
        };
        w.start_file(*name, opts).expect("zip entry");
        w.write_all(data).expect("zip write");
    }
    w.finish().expect("zip finish").into_inner()
}

const W_NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;

const NUMBERING: &str = r#"<w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="bullet"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>"#;

fn docx_report(sections: usize) -> Vec<u8> {
    let mut body = String::new();
    for i in 0..sections {
        body.push_str(&format!(
            "<w:p><w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr><w:r><w:t>Section {i}</w:t></w:r></w:p>"
        ));
        for _ in 0..10 {
            body.push_str(
                "<w:p><w:r><w:t xml:space=\"preserve\">Plain text paragraph with </w:t></w:r>\
                 <w:r><w:rPr><w:b/></w:rPr><w:t>bold</w:t></w:r>\
                 <w:r><w:t xml:space=\"preserve\"> and more words to read.</w:t></w:r></w:p>",
            );
        }
        for item in ["Alpha", "Beta", "Gamma"] {
            body.push_str(&format!(
                "<w:p><w:pPr><w:pStyle w:val=\"ListParagraph\"/><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"1\"/></w:numPr></w:pPr><w:r><w:t>{item}</w:t></w:r></w:p>"
            ));
        }
        if sections > 1 && i % 10 == 0 {
            body.push_str("<w:tbl>");
            for r in 0..10 {
                body.push_str("<w:tr>");
                for c in 0..4 {
                    body.push_str(&format!(
                        "<w:tc><w:p><w:r><w:t>r{r}c{c}</w:t></w:r></w:p></w:tc>"
                    ));
                }
                body.push_str("</w:tr>");
            }
            body.push_str("</w:tbl>");
        }
    }
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {W_NS}><w:body>{body}</w:body></w:document>"#
    );
    let numbering =
        format!(r#"<?xml version="1.0"?><w:numbering {W_NS}>{NUMBERING}</w:numbering>"#);
    zip_bytes(&[
        (
            "[Content_Types].xml",
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        ("word/document.xml", document.as_bytes()),
        ("word/numbering.xml", numbering.as_bytes()),
    ])
}

fn col_name(mut c: usize) -> String {
    let mut s = Vec::new();
    loop {
        s.push(b'A' + (c % 26) as u8);
        if c < 26 {
            break;
        }
        c = c / 26 - 1;
    }
    s.reverse();
    String::from_utf8(s).expect("ascii")
}

fn xlsx(sheets: usize, rows: usize, cols: usize) -> Vec<u8> {
    let mut ct = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#,
    );
    let mut wb_sheets = String::new();
    let mut wb_rels = String::new();
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    for n in 1..=sheets {
        ct.push_str(&format!(r#"<Override PartName="/xl/worksheets/sheet{n}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#));
        wb_sheets.push_str(&format!(
            r#"<sheet name="Sheet{n}" sheetId="{n}" r:id="rId{n}"/>"#
        ));
        wb_rels.push_str(&format!(r#"<Relationship Id="rId{n}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{n}.xml"/>"#));
        let mut data = String::new();
        for r in 0..rows {
            data.push_str(&format!(r#"<row r="{}">"#, r + 1));
            for c in 0..cols {
                let at = format!("{}{}", col_name(c), r + 1);
                if r == 0 || c % 2 == 0 {
                    data.push_str(&format!(
                        r#"<c r="{at}" t="inlineStr"><is><t>item {r}-{c}</t></is></c>"#
                    ));
                } else {
                    data.push_str(&format!(r#"<c r="{at}"><v>{}.25</v></c>"#, r * c));
                }
            }
            data.push_str("</row>");
        }
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{data}</sheetData></worksheet>"#
        );
        parts.push((format!("xl/worksheets/sheet{n}.xml"), xml.into_bytes()));
    }
    ct.push_str("</Types>");
    let workbook = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>{wb_sheets}</sheets></workbook>"#
    );
    let rels = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{wb_rels}</Relationships>"#
    );
    let root_rels = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
    let mut entries: Vec<(&str, &[u8])> = vec![
        ("[Content_Types].xml", ct.as_bytes()),
        ("_rels/.rels", root_rels),
        ("xl/workbook.xml", workbook.as_bytes()),
        ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
    ];
    entries.extend(parts.iter().map(|(n, d)| (n.as_str(), d.as_slice())));
    zip_bytes(&entries)
}

fn ods(rows: usize, cols: usize) -> Vec<u8> {
    let mut table = String::from(r#"<table:table table:name="Data">"#);
    for r in 0..rows {
        table.push_str("<table:table-row>");
        for c in 0..cols {
            if r == 0 || c % 2 == 0 {
                table.push_str(&format!(
                    r#"<table:table-cell office:value-type="string"><text:p>item {r}-{c}</text:p></table:table-cell>"#
                ));
            } else {
                let v = r * c;
                table.push_str(&format!(
                    r#"<table:table-cell office:value-type="float" office:value="{v}"><text:p>{v}</text:p></table:table-cell>"#
                ));
            }
        }
        table.push_str("</table:table-row>");
    }
    table.push_str("</table:table>");
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" office:version="1.2"><office:body><office:spreadsheet>{table}</office:spreadsheet></office:body></office:document-content>"#
    );
    let manifest = br#"<?xml version="1.0" encoding="UTF-8"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"><manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.spreadsheet"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/></manifest:manifest>"#;
    zip_bytes(&[
        (
            "mimetype",
            b"application/vnd.oasis.opendocument.spreadsheet",
        ),
        ("content.xml", content.as_bytes()),
        ("META-INF/manifest.xml", manifest),
    ])
}
