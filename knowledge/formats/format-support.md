---
type: Subsystem Design
title: Format Support
description: Supported and planned formats, bytes-in API, format detection, DOCX, spreadsheet, plain text and Markdown mapping.
tags:
  - markkit
  - formats
  - docx
  - spreadsheet
  - detection
  - markdown
---

# Format Support

## Supported Formats

### Tier 1 (Core)
- **HTML/XHTML**: Full support via html5ever + scraper (`html` feature, default)
- **Plain text**: Always on, no dependencies (`text/plain`, `.txt`, `.text`)

### Tier 2 (Feature-gated)
- **DOCX** (`docx`): Word documents via a direct `zip` + `quick-xml` reader (no docx-rs)
- **XLSX/XLSM, XLS, ODS** (`spreadsheet`): Workbooks to Markdown tables via calamine
- **Markdown** (`markdown`): Re-parsed with pulldown-cmark into the IR and rendered back (`text/markdown`, `text/x-markdown`, `.md`, `.markdown`, `.mdown`, `.mkd`, `.mkdn`)
- **PDF** (`pdf`): Planned. Returns `Error::UnsupportedFormat` (never an empty success)

### Detected, not supported
- **DOC** (Word 97-2003 binary), **PPTX**, **XLSB**: explicit `UnsupportedFormat` errors

### Tier 3 (Planned)
- **EPUB**: E-book extraction
- **RTF**: Rich text format
- **CSV/TSV**: Tabular data to markdown tables

## Bytes-in API

```rust
pub struct DocumentHint<'a> { pub content_type: Option<&'a str>, pub file_name: Option<&'a str> }

impl Markkit {
    pub async fn extract_document(&self, bytes: &[u8], hint: DocumentHint<'_>) -> Result<Extraction>;
    pub fn extract_document_sync(&self, bytes: &[u8], hint: DocumentHint<'_>) -> Result<Extraction>;
}

pub struct Extraction {           // #[non_exhaustive]
    pub markdown: String,
    pub title: Option<String>,    // HTML og:title / JSON-LD / <title>; DOCX core.xml dc:title, else Title style
    pub format: Format,
    pub warnings: Vec<String>,    // "<code>: <message>"
    pub stats: ExtractionStats,   // pages / sheets / rows / paragraphs / tables (Option each)
    pub metadata: Metadata,       // HTML page metadata, see HTML Extraction; empty otherwise
}
```

`extract_bytes(bytes, content_type)` delegates to `extract_document_sync` and returns
only the Markdown. Parsing is CPU-bound; async callers should use the sync variant in
`spawn_blocking`.

Warning codes: `truncated_sheet`, `hidden_sheet_skipped`, `sheet_error`, `invalid_utf8`.

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

Plain text and Markdown are never sniffed: they need a content type or extension, so
unlabelled bytes stay `UnsupportedFormat` rather than rendering binary as text.

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

Containers nested deeper than 200 are flattened into the deepest kept container
(explicit-stack builder, so neither the builder nor the recursive renderer can overflow).

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

## See also

- [HTML Extraction](html-extraction.md), the Tier 1 path
- [IR Specification](../foundations/ir.md), the shared output of every parser
- [Threat Model](../security/threat-model.md), T6 malicious office documents
