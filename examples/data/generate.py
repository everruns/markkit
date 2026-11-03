#!/usr/bin/env python3
"""Regenerate the sample documents in examples/data/ (deterministic, stdlib only).

    python3 examples/data/generate.py

The files are tiny, hand-built and contain no macros, scripts or external
references. Every Rust, Python, Node.js and WASM example reads them, so the
same inputs exercise every binding.
"""

import pathlib
import zipfile

HERE = pathlib.Path(__file__).resolve().parent
# Fixed timestamp keeps the archives byte-for-byte reproducible.
STAMP = (2026, 1, 1, 0, 0, 0)


def write_zip(name, entries):
    with zipfile.ZipFile(HERE / name, "w", zipfile.ZIP_DEFLATED) as z:
        for path, data in entries:
            info = zipfile.ZipInfo(path, STAMP)
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, data)


W_NS = (
    'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" '
    'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"'
)
REL_NS = "http://schemas.openxmlformats.org/package/2006/relationships"
OFFICE_REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"


def para(text, style=None, num=None):
    ppr = ""
    if style or num:
        ppr = "<w:pPr>"
        if style:
            ppr += f'<w:pStyle w:val="{style}"/>'
        if num:
            ppr += f'<w:numPr><w:ilvl w:val="0"/><w:numId w:val="{num}"/></w:numPr>'
        ppr += "</w:pPr>"
    return f'<w:p>{ppr}<w:r><w:t xml:space="preserve">{text}</w:t></w:r></w:p>'


def cell(text, bold=False):
    rpr = "<w:rPr><w:b/></w:rPr>" if bold else ""
    return f'<w:tc><w:p><w:r>{rpr}<w:t>{text}</w:t></w:r></w:p></w:tc>'


def docx():
    body = "".join(
        [
            para("Quarterly report", "Heading1"),
            '<w:p><w:r><w:t xml:space="preserve">Revenue grew </w:t></w:r>'
            '<w:r><w:rPr><w:b/></w:rPr><w:t>18%</w:t></w:r>'
            '<w:r><w:t xml:space="preserve"> quarter over quarter. Details on </w:t></w:r>'
            '<w:hyperlink r:id="rIdLink"><w:r><w:t>the dashboard</w:t></w:r></w:hyperlink>'
            "<w:r><w:t>.</w:t></w:r></w:p>",
            para("Highlights", "Heading2"),
            para("Two new enterprise customers", num=1),
            para("Churn below 2%", num=1),
            para("PDF table extraction shipped", num=1),
            para("Regional breakdown", "Heading2"),
            "<w:tbl>"
            "<w:tr>" + cell("Region", True) + cell("Revenue", True) + cell("Growth", True) + "</w:tr>"
            "<w:tr>" + cell("EMEA") + cell("$1.2M") + cell("21%") + "</w:tr>"
            "<w:tr>" + cell("Americas") + cell("$2.4M") + cell("16%") + "</w:tr>"
            "</w:tbl>",
        ]
    )
    numbering = (
        f'<w:numbering {W_NS}><w:abstractNum w:abstractNumId="0">'
        '<w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="bullet"/></w:lvl>'
        '</w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>'
    )
    write_zip(
        "report.docx",
        [
            (
                "[Content_Types].xml",
                '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
                '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
                '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
                '<Default Extension="xml" ContentType="application/xml"/>'
                '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
                "</Types>",
            ),
            (
                "_rels/.rels",
                f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="{REL_NS}">'
                f'<Relationship Id="rId1" Type="{OFFICE_REL}/officeDocument" Target="word/document.xml"/>'
                f'<Relationship Id="rId2" Type="{REL_NS}/metadata/core-properties" Target="docProps/core.xml"/>'
                "</Relationships>",
            ),
            (
                "word/document.xml",
                f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {W_NS}><w:body>{body}</w:body></w:document>',
            ),
            (
                "word/_rels/document.xml.rels",
                f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="{REL_NS}">'
                f'<Relationship Id="rIdLink" Type="{OFFICE_REL}/hyperlink" Target="https://example.com/dashboard" TargetMode="External"/>'
                "</Relationships>",
            ),
            ("word/numbering.xml", f'<?xml version="1.0"?>{numbering}'),
            (
                "docProps/core.xml",
                '<?xml version="1.0"?><cp:coreProperties '
                'xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" '
                'xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Q3 Report</dc:title></cp:coreProperties>',
            ),
        ],
    )


def col(c):
    return "ABCDEFGHIJKLMNOPQRSTUVWXYZ"[c]


def sheet_xml(rows):
    out = []
    for r, row in enumerate(rows):
        cells = []
        for c, v in enumerate(row):
            at = f"{col(c)}{r + 1}"
            if isinstance(v, (int, float)):
                cells.append(f'<c r="{at}"><v>{v}</v></c>')
            else:
                cells.append(f'<c r="{at}" t="inlineStr"><is><t>{v}</t></is></c>')
        out.append(f'<row r="{r + 1}">{"".join(cells)}</row>')
    return (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">'
        f"<sheetData>{''.join(out)}</sheetData></worksheet>"
    )


def xlsx():
    sheets = [
        ("Sales", [["Month", "Units", "Revenue"], ["Jan", 120, 2400], ["Feb", 135, 2700], ["Mar", 160, 3200]]),
        ("Team", [["Name", "Role"], ["Ada", "Engineering"], ["Grace", "Research"]]),
    ]
    ct = (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
        '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
        '<Default Extension="xml" ContentType="application/xml"/>'
        '<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>'
    )
    wb_sheets, wb_rels, entries = "", "", []
    for i, (name, rows) in enumerate(sheets, 1):
        ct += (
            f'<Override PartName="/xl/worksheets/sheet{i}.xml" '
            'ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>'
        )
        wb_sheets += f'<sheet name="{name}" sheetId="{i}" r:id="rId{i}"/>'
        wb_rels += f'<Relationship Id="rId{i}" Type="{OFFICE_REL}/worksheet" Target="worksheets/sheet{i}.xml"/>'
        entries.append((f"xl/worksheets/sheet{i}.xml", sheet_xml(rows)))
    ct += "</Types>"
    write_zip(
        "sales.xlsx",
        [
            ("[Content_Types].xml", ct),
            (
                "_rels/.rels",
                f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="{REL_NS}">'
                f'<Relationship Id="rId1" Type="{OFFICE_REL}/officeDocument" Target="xl/workbook.xml"/></Relationships>',
            ),
            (
                "xl/workbook.xml",
                '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
                '<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" '
                f'xmlns:r="{OFFICE_REL}"><sheets>{wb_sheets}</sheets></workbook>',
            ),
            (
                "xl/_rels/workbook.xml.rels",
                f'<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="{REL_NS}">{wb_rels}</Relationships>',
            ),
        ]
        + entries,
    )


def pdf():
    lines = [
        ("F2", 20, 72, 720, "Markkit sample PDF"),
        ("F1", 12, 72, 690, "Markkit reads the text layer of PDF files, rebuilds"),
        ("F1", 12, 72, 675, "reading order across columns and detects tables."),
        ("F2", 14, 72, 640, "Why it matters"),
        ("F1", 12, 72, 615, "Agents get clean Markdown instead of positioned glyphs."),
    ]
    text = "".join(f"BT /{f} {s} Tf {x} {y} Td ({t}) Tj ET\n" for f, s, x, y, t in lines)
    objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
        "/Resources << /Font << /F1 4 0 R /F2 5 0 R >> >> /Contents 6 0 R >>",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>",
        f"<< /Length {len(text)} >>\nstream\n{text}endstream",
        "<< /Title (Markkit sample) >>",
    ]
    out = b"%PDF-1.4\n"
    offsets = []
    for i, body in enumerate(objects, 1):
        offsets.append(len(out))
        out += f"{i} 0 obj\n{body}\nendobj\n".encode("latin-1")
    xref = len(out)
    out += f"xref\n0 {len(objects) + 1}\n0000000000 65535 f \n".encode()
    for off in offsets:
        out += f"{off:010d} 00000 n \n".encode()
    out += (
        f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R /Info 7 0 R >>\n"
        f"startxref\n{xref}\n%%EOF\n"
    ).encode()
    (HERE / "sample.pdf").write_bytes(out)


if __name__ == "__main__":
    docx()
    xlsx()
    pdf()
    print("wrote", ", ".join(p.name for p in sorted(HERE.glob("*.*")) if p.suffix != ".py"))
