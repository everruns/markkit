---
type: Subsystem Design
title: Format Support
description: Supported and planned formats, bytes-in API, format detection, DOCX, spreadsheet, PDF, plain text, Markdown and CSV/TSV mapping.
tags:
  - markkit
  - formats
  - docx
  - spreadsheet
  - pdf
  - detection
  - markdown
  - csv
---

# Format Support

## Supported Formats

### Tier 1 (Core)
- **HTML/XHTML**: Full support via html5ever + scraper (`html` feature, default)
- **Plain text**: Always on, no dependencies (`text/plain`, `.txt`, `.text`)
- **CSV / TSV**: Always on, no dependencies (`text/csv`, `application/csv`, `.csv`; `text/tab-separated-values`, `.tsv`, `.tab`)

### Tier 2 (Feature-gated)
- **DOCX** (`docx`): Word documents via a direct `zip` + `quick-xml` reader (no docx-rs)
- **XLSX/XLSM, XLS, ODS** (`spreadsheet`): Workbooks to Markdown tables via calamine
- **XLSB** (`spreadsheet`): Excel binary workbooks via markkit's own bounds-checked BIFF12
  reader (calamine's XLSB reader panics on malformed records)
- **Markdown** (`markdown`): Re-parsed with pulldown-cmark into the IR and rendered back (`text/markdown`, `text/x-markdown`, `.md`, `.markdown`, `.mdown`, `.mkd`, `.mkdn`)
- **PDF** (`pdf`): Text layer via hayro-syntax (pure Rust); no OCR, encrypted PDFs rejected

### Detected, not supported
- **DOC** (Word 97-2003 binary), **PPTX**: explicit `UnsupportedFormat` errors

### Tier 3 (Planned)
- **EPUB**: E-book extraction
- **RTF**: Rich text format

## Bytes-in API

```rust
pub struct DocumentHint<'a> { pub content_type: Option<&'a str>, pub file_name: Option<&'a str> }

impl Markkit {
    pub async fn extract_document(&self, bytes: &[u8], hint: DocumentHint<'_>) -> Result<Extraction>;
    pub fn extract_document_sync(&self, bytes: &[u8], hint: DocumentHint<'_>) -> Result<Extraction>;
}

pub struct Extraction {           // #[non_exhaustive]
    pub markdown: String,
    pub title: Option<String>,    // see "Document titles" below
    pub format: Format,
    pub warnings: Vec<String>,    // "<code>: <message>"
    pub stats: ExtractionStats,   // pages / sheets / rows / paragraphs / tables (Option each)
    pub metadata: Metadata,       // HTML page metadata, see HTML Extraction; empty otherwise
}
```

Non-empty `markdown` always ends with exactly one `\n` (POSIX text convention; outputs
concatenate cleanly); an empty document renders `""`. This is intended and stable.

### Document titles

| Format | Source |
|---|---|
| HTML | `og:title` / JSON-LD / `<title>` (see [HTML Extraction](html-extraction.md)) |
| DOCX | `docProps/core.xml` `dc:title` (via `_rels/.rels`), else the first `Title`-style paragraph |
| XLSX, XLSB | `docProps/core.xml` `dc:title` |
| ODS | `meta.xml` `dc:title` |
| XLS | `\u{5}SummaryInformation` property set, `PIDSI_TITLE` (code page aware: 1200 UTF-16, UTF-8, else Latin-1) |
| PDF | `/Info /Title` (UTF-16BE with BOM, else PDFDocEncoding) |

Whitespace is collapsed, titles are capped at 1024 characters, and a missing or
malformed property part yields `None` without failing the extraction.

`extract_bytes(bytes, content_type)` delegates to `extract_document_sync` and returns
only the Markdown. Parsing is CPU-bound; async callers should use the sync variant in
`spawn_blocking`.

Warning codes: `truncated_sheet`, `hidden_sheet_skipped`, `sheet_error`, `truncated_pdf`,
`pdf_unmapped_font`, `pdf_no_text`, `invalid_utf8`, `hidden_content_skipped`,
`malformed_csv`, `truncated_table`.

## Format Detection

Priority order (`Format::detect`):
1. Content type (`DocumentHint::content_type`; parameters ignored). XLSB
   (`application/vnd.ms-excel.sheet.binary.macroEnabled.12`) is matched before the
   generic `ms-excel.sheet` → XLSX rule. Generic types such as
   `application/octet-stream` map to `Unknown` and fall through.
2. File extension (`DocumentHint::file_name`; URL query/fragment ignored).
3. Magic bytes (`Format::sniff`):
   - `PK\x03\x04` ZIP: ODF `mimetype` entry → ODS; `word/document*` → DOCX;
     `xl/workbook.bin` → XLSB; `xl/workbook.*` → XLSX; `ppt/presentation.*` → PPTX (part names are stored
     uncompressed, so no archive parsing is needed for detection).
   - OLE2 (`D0 CF 11 E0`): UTF-16 `WordDocument` stream → DOC; `Workbook`/`Book` → XLS.
   - `%PDF-` within the first 1 KiB → PDF.
   - `<!doctype html`, `<html`, `<head`, `<body` (after BOM/whitespace) → HTML.

Plain text, Markdown and CSV/TSV are never sniffed: they need a content type or extension, so
unlabelled bytes stay `UnsupportedFormat` rather than rendering binary as text.

Spreadsheet parsing re-sniffs the container (XLSX vs XLS vs ODS), so a workbook
mislabelled as another Excel type still parses.

## Custom Parsers

`MarkkitBuilder::parser(p)` registers a `registry::FormatParser`
(`name`, `formats() -> &[Format]`, `claims(bytes, hint) -> bool`,
`parse(bytes, &MarkkitBuilder) -> Result<ParsedDocument>`). In
`parse_document` / `extract_document`, after the `max_input_size` check:

1. first registered parser whose `claims` is true (new formats without a
   `Format` variant), else
2. first registered parser listing the detected `Format` (fill PDF/CSV/... or
   replace a built-in), else
3. the built-in parser for the detected format.

A panic in a custom parser becomes `Error::Parser`; its errors pass through.
The deadline is checked when it returns and `max_output_size` bounds the render.
Transforms run on its IR and the shared renderer produces the Markdown.
Custom parsers see untrusted bytes and own their resource limits (they get
the builder, so they can honour the configured ones). `ParsedDocument::new`
is public for them.

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
| Runs with direct `w:vanish` (hidden text) | Skipped with `hidden_content_skipped` unless `include_hidden_content(true)` |

The main part is located through `_rels/.rels` (fallback `word/document.xml`); styles and
numbering through the main part's relationships.

## Spreadsheet Mapping

- One `## <Sheet name>` section per non-empty worksheet, in workbook order, followed by a
  GFM table. The first non-empty row is the header; fully empty rows and leading empty
  columns are dropped. A sheet with a single non-empty row has no header: its columns
  are labelled with their Excel letters (`| C | D |`) and the row renders as data
  (counted in `stats.rows`).
- XLSX packages without `_rels/.rels` (hand-built files) get the standard
  officeDocument relationship to `xl/workbook.xml` before calamine opens them.
- Cells: numbers without trailing `.0` for integers; booleans `true`/`false`; dates as
  `YYYY-MM-DD[ HH:MM:SS]`, durations `H:MM:SS`; errors as Excel shows them (`#DIV/0!`).
  Pipes are escaped (`\|`), line breaks flattened to spaces.
- Caps: `max_sheet_rows` (10 000 data rows) and `max_sheet_cols` (256); exceeding either
  emits a `truncated_sheet` warning.
- Hidden / very hidden sheets are skipped with `hidden_sheet_skipped` unless
  `include_hidden_sheets(true)`. Chart, dialog and macro sheets are skipped.
- XLSB: sheets from `xl/workbook.bin` (`BrtBundleSh`) resolved through
  `xl/_rels/workbook.bin.rels`; shared strings from `xl/sharedStrings.bin`; dates and
  durations from the cell XF's number format in `xl/styles.bin` (built-in date ids or custom
  codes with date/time tokens; `[h]`-style elapsed codes are durations). A missing or
  truncated sheet part emits `sheet_error` and the other sheets still render.
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
| `track_sources(true)` | `Source::page` (1-based) on each top-level block |
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

## Plain Text and Markdown

Shared decoding: UTF-8 (BOM stripped); UTF-16 LE/BE only with a BOM; otherwise lossy UTF-8
with an `invalid_utf8` warning. CRLF/CR become LF; C0 controls other than tab/newline, and
DEL, are dropped (no terminal escapes or NULs in output).

Plain text goes through the IR: blank lines separate paragraphs (`stats.paragraphs`),
single newlines become hard breaks (`  \n`), trailing whitespace is trimmed, text stays
`Node::Text`. No title.

Markdown is parsed, not passed through:

| Markdown | Output |
|---|---|
| CommonMark blocks and inlines | Same construct via the IR (`*` bullets become `-`, setext headings become ATX) |
| GFM tables (with `:---:` alignment), `~~strike~~`, task lists `[x]` | IR v2 `Table.alignments`, `Strikethrough`, `ListItem::checked` |
| Footnotes `[^n]`, definition lists (`Term` / `: def`) | IR v2 `FootnoteReference` / `FootnoteDefinition`, `DefinitionList` |
| Soft line break | Space (paragraphs reflow); hard breaks kept |
| YAML front matter | Not rendered; `title:` becomes `Extraction::title` (else first `# ` heading) |
| HTML block | Through the HTML parser when `html` is on (scripts, styles, handlers stripped), else dropped |
| Inline HTML tag | Dropped; text between tags kept |
| Math, wikilinks, other extensions | Not enabled (literal text) |

Containers nested deeper than `max_nesting_depth` (256) are flattened into the deepest kept container
(explicit-stack builder, so neither the builder nor the recursive renderer can overflow).

## CSV / TSV Mapping

- Decoded like plain text (BOM, UTF-16, control characters, `invalid_utf8`).
- One GFM table, no heading. First non-empty record is the header; fully empty records
  dropped; short rows padded to the widest row.
- Delimiter: tab for TSV. For CSV, whichever of `,` `;` `\t` `|` occurs most outside quotes
  in the first record (ties prefer `,`), so European `;` files work.
- RFC 4180 quoting: quoted fields may span lines, `""` is a literal quote; a quote inside an
  unquoted field is literal; an unterminated quote takes the rest of the input and emits
  `malformed_csv`.
- Cells trimmed and kept raw in the IR; the renderer escapes pipes and flattens line breaks
  in table context, as for spreadsheet cells. Values are
  text: no type inference, formulas (`=…`) are plain strings.
- Caps reuse `max_sheet_rows` (data rows; reading stops at the cap) and `max_sheet_cols`
  (extra fields are never stored); either emits `truncated_table`. Each record ticks the call's `timeout`.
- Stats: `rows` (data rows), `tables` (0 or 1).

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

PDF additionally enforces (T8): `max_pdf_pages` (10 000; truncate + `truncated_pdf`),
`max_pdf_stream_size` (64 MiB per decoded stream), `max_pdf_decoded_size` (256 MiB total
decoded in the pre-scan, and separately the total content interpreted, form reuse included).

## See also

- [HTML Extraction](html-extraction.md), the Tier 1 path
- [IR Specification](../foundations/ir.md), the shared output of every parser
- [Threat Model](../security/threat-model.md), T6 malicious office documents, T7 text and Markdown, T8 malicious PDFs
