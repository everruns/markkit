---
type: Subsystem Design
title: Format Support
description: Supported and planned formats, bytes-in API, format detection, DOCX, spreadsheet and PDF mapping.
tags:
  - markkit
  - formats
  - docx
  - spreadsheet
  - pdf
  - detection
---

# Format Support

## Supported Formats

### Tier 1 (Core)
- **HTML/XHTML**: Full support via html5ever + scraper (`html` feature, default)
- **Plain text**: Planned pass-through (currently `UnsupportedFormat` via the bytes API)

### Tier 2 (Feature-gated)
- **DOCX** (`docx`): Word documents via a direct `zip` + `quick-xml` reader (no docx-rs)
- **XLSX/XLSM, XLS, ODS** (`spreadsheet`): Workbooks to Markdown tables via calamine
- **PDF** (`pdf`): Text layer via hayro-syntax (pure Rust); no OCR, encrypted PDFs rejected

### Detected, not supported
- **DOC** (Word 97-2003 binary), **PPTX**, **XLSB**: explicit `UnsupportedFormat` errors

### Tier 3 (Planned)
- **EPUB**: E-book extraction
- **RTF**: Rich text format
- **CSV/TSV**: Tabular data to markdown tables
- **Markdown**: Normalize/clean existing markdown

## Bytes-in API

```rust
pub struct DocumentHint<'a> { pub content_type: Option<&'a str>, pub file_name: Option<&'a str> }

impl Markkit {
    pub async fn extract_document(&self, bytes: &[u8], hint: DocumentHint<'_>) -> Result<Extraction>;
    pub fn extract_document_sync(&self, bytes: &[u8], hint: DocumentHint<'_>) -> Result<Extraction>;
}

pub struct Extraction {           // #[non_exhaustive]
    pub markdown: String,
    pub title: Option<String>,    // HTML og:title / JSON-LD / <title>; DOCX dc:title / Title style; PDF /Info /Title
    pub format: Format,
    pub warnings: Vec<String>,    // "<code>: <message>"
    pub stats: ExtractionStats,   // pages / sheets / rows / paragraphs / tables (Option each)
    pub metadata: Metadata,       // HTML page metadata, see HTML Extraction; empty otherwise
}
```

`extract_bytes(bytes, content_type)` delegates to `extract_document_sync` and returns
only the Markdown. Parsing is CPU-bound; async callers should use the sync variant in
`spawn_blocking`.

Warning codes: `truncated_sheet`, `hidden_sheet_skipped`, `sheet_error`, `truncated_pdf`,
`pdf_unmapped_font`, `pdf_no_text`.

## Format Detection

Priority order (`Format::detect`):
1. Content type (`DocumentHint::content_type`; parameters ignored). Generic types such as
   `application/octet-stream` map to `Unknown` and fall through.
2. File extension (`DocumentHint::file_name`; URL query/fragment ignored).
3. Magic bytes (`Format::sniff`):
   - `PK\x03\x04` ZIP: ODF `mimetype` entry → ODS; `word/document*` → DOCX;
     `xl/workbook.*` → XLSX; `ppt/presentation.*` → PPTX (part names are stored
     uncompressed, so no archive parsing is needed for detection).
   - OLE2 (`D0 CF 11 E0`): UTF-16 `WordDocument` stream → DOC; `Workbook`/`Book` → XLS.
   - `%PDF-` within the first 1 KiB → PDF.
   - `<!doctype html`, `<html`, `<head`, `<body` (after BOM/whitespace) → HTML.

Spreadsheet parsing re-sniffs the container (XLSX vs XLS vs ODS), so a workbook
mislabelled as another Excel type still parses.

## DOCX Mapping

| WordprocessingML | Markdown |
|---|---|
| `Title` style / `HeadingN` (id or name `heading n`, via `basedOn`) / `outlineLvl` | `#`..`######` (levels > 6 clamp to 6) |
| Paragraph | Paragraph (empty paragraphs dropped) |
| `w:b`, `w:i`, `w:strike`/`w:dstrike` (`w:val="0"/"false"` = off) | `**`, `*`, `~~`; adjacent identical runs merged, edge whitespace moved outside |
| `w:hyperlink` (`r:id` relationship target, or `w:anchor`) | `[text](url)` / `[text](#anchor)`; spaces → `%20` |
| `w:numPr` (direct or via style) + numbering.xml `numFmt` | `-` for `bullet`/`none`, `1.` otherwise; nested by `ilvl`; no numbering part → bullets |
| `w:tbl` | GFM table, first row = header, `gridSpan` pads empty cells, nested tables flattened, pipes escaped |
| `w:br` / `w:cr` | Hard line break (page/column breaks ignored); `w:tab` → space |
| `w:ins`, `w:sdt`, `w:smartTag`, `w:customXml`, `w:fldSimple` | Unwrapped |
| `w:del`, `w:instrText`, drawings/text boxes, comments, headers/footers, footnotes | Skipped |

The main part is located through `_rels/.rels` (fallback `word/document.xml`); styles and
numbering through the main part's relationships.

## Spreadsheet Mapping

- One `## <Sheet name>` section per non-empty worksheet, in workbook order, followed by a
  GFM table. The first non-empty row is the header; fully empty rows and leading empty
  columns are dropped.
- Cells: numbers without trailing `.0` for integers; booleans `true`/`false`; dates as
  `YYYY-MM-DD[ HH:MM:SS]`, durations `H:MM:SS`; errors as Excel shows them (`#DIV/0!`).
  Pipes are escaped (`\|`), line breaks flattened to spaces.
- Caps: `max_sheet_rows` (10 000 data rows) and `max_sheet_cols` (256); exceeding either
  emits a `truncated_sheet` warning.
- Hidden / very hidden sheets are skipped with `hidden_sheet_skipped` unless
  `include_hidden_sheets(true)`. Chart, dialog and macro sheets are skipped.
- Only cached cell values are read: formulas are not evaluated, VBA/XLM macros are never
  executed or rendered.

## PDF Mapping

Text layer only (`crates/markkit/src/pdf/`). Layout is reconstructed from glyph positions:

| PDF | Markdown |
|---|---|
| `Tj` / `TJ` / `'` / `"` runs on one baseline, moving right | One line; gaps > 0.15 em and `TJ` adjustments < −200 become spaces |
| Consecutive lines | One paragraph, unless the vertical step exceeds 1.3× the page's most common line pitch, the text moves up (new column), or the size changes by > 15% |
| `letter-` at line end + lowercase next line; soft hyphen | Joined without the hyphen |
| Paragraph ≤ 3 lines, ≤ 200 chars, size ≥ 1.2× body size (size with most characters) | Heading; distinct heading sizes, largest first, map to `#`..`######` |
| Line starting `•`, `●`, `▪`, `◦`, `‣`, `∙`, `■` or `–` + space | Unordered list item (consecutive items merge) |
| Form XObjects (`Do`) | Followed with their matrix and resources; depth 8, cycles skipped |
| Pages | Paragraphs never cross pages; no page separators |
| `/Info /Title` | `Extraction::title` (UTF-16BE / PDFDocEncoding decoded) |
| Images, annotations, outlines, form fields | Skipped |

Unicode per font, in order: `ToUnicode` CMap (`bfchar`, `bfrange` incl. arrays, code
lengths from `codespacerange`); simple fonts: `/Encoding` (WinAnsi, MacRoman, Standard)
+ `/Differences` glyph names (AGL subset, `uniXXXX`, `uXXXX`, `a_b` ligatures, `.suffix`
stripped); otherwise WinAnsi. Type0 fonts without `ToUnicode` are skipped with
`pdf_unmapped_font` (no CID guessing). Ligature presentation forms are expanded
(`ﬁ` → `fi`); control characters are dropped. Text render mode is ignored, so invisible
OCR layers (mode 3) are extracted. Encrypted documents (an `/Encrypt` key anywhere)
return `UnsupportedFormat`; a page set with no text emits `pdf_no_text`.

Known gaps (follow-ups): multi-column reading order relies on content-stream order,
no table detection, no repeated header/footer removal, bold/italic not mapped, numbered
lists stay paragraphs, rotated text is not re-oriented.

## Resource Limits

All formats enforce:
- Maximum input size (default: 50MB), checked before detection
- Maximum processing time (default: 60s) – configuration only, enforcement TBD
- Maximum output size (prevent markdown explosion) – bounded by the caps below

Office formats additionally enforce (see [Threat Model](../security/threat-model.md) T6):
- `max_archive_entries` (10 000), `max_archive_entry_size` (64 MiB decompressed),
  `max_archive_total_size` (256 MiB decompressed)
- `max_spreadsheet_cells` (4 000 000) for the dense grid calamine builds for XLS/ODS
- XML nesting depth 256 for DOCX parts

PDF additionally enforces (T7): `max_pdf_pages` (10 000; truncate + `truncated_pdf`),
`max_pdf_stream_size` (64 MiB per decoded stream), `max_pdf_decoded_size` (256 MiB total
decoded in the pre-scan, and separately the total content interpreted, form reuse included).

## See also

- [HTML Extraction](html-extraction.md), the Tier 1 path
- [IR Specification](../foundations/ir.md), the shared output of every parser
- [Threat Model](../security/threat-model.md), T6 malicious office documents, T7 malicious PDFs
