---
type: Subsystem Design
title: Format Support
description: Supported and planned formats, bytes-in API, format detection, DOCX, spreadsheet, PDF, plain text, Markdown, CSV/TSV, Jupyter notebook, email, ODT and EPUB mapping.
tags:
  - markkit
  - formats
  - docx
  - spreadsheet
  - pdf
  - detection
  - markdown
  - csv
  - notebook
  - email
  - odt
  - epub
---

# Format Support

## Supported Formats

### Tier 1 (Core)
- **HTML/XHTML**: Full support via html5ever + scraper (`html` feature, default)
- **Plain text**: Always on, no dependencies (`text/plain`, `.txt`, `.text`)
- **Email** (`email`): `.eml` / `message/rfc822` via mail-parser 0.11 (no default features)
- **Jupyter notebooks**: Always on, serde_json (`application/x-ipynb+json`, `.ipynb`; nbformat 4)
- **CSV / TSV**: Always on, no dependencies (`text/csv`, `application/csv`, `.csv`; `text/tab-separated-values`, `.tsv`, `.tab`)

### Tier 2 (Feature-gated)
- **DOCX** (`docx`): Word documents via a direct `zip` + `quick-xml` reader (no docx-rs)
- **PPTX/PPTM/PPSX/POTX** (`pptx`): presentations via the same direct reader
- **EPUB** (`epub`, implies `html`): EPUB 2/3 chapters through the HTML parser
- **ODT** (`odt`): OpenDocument text via the same bounded `zip` + `quick-xml` reader as DOCX
- **XLSX/XLSM, XLS, ODS** (`spreadsheet`): Workbooks to Markdown tables via calamine
- **XLSB** (`spreadsheet`): Excel binary workbooks via markkit's own bounds-checked BIFF12
  reader (calamine's XLSB reader panics on malformed records)
- **Markdown** (`markdown`): Re-parsed with pulldown-cmark into the IR and rendered back (`text/markdown`, `text/x-markdown`, `.md`, `.markdown`, `.mdown`, `.mkd`, `.mkdn`)
- **PDF** (`pdf`): Text/layout, reading order and tables via hayro-syntax (pure Rust); encrypted PDFs rejected. Native OCR is opt-in with `pdf-ocr` and installed helpers

### Detected, not supported
- **DOC** (Word 97-2003 binary), **PPT** (PowerPoint 97-2003 binary): explicit `UnsupportedFormat` errors

### Tier 3 (Planned)
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
    pub warnings: Vec<Warning>,   // { code: WarningCode, detail: String }; Display "<code>: <detail>"
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
| PPTX | `docProps/core.xml` `dc:title`, else the first slide title |
| XLS | `\u{5}SummaryInformation` property set, `PIDSI_TITLE` (code page aware: 1200 UTF-16, UTF-8, else Latin-1) |
| PDF | `/Info /Title` (UTF-16BE with BOM, else PDFDocEncoding) |

Whitespace is collapsed, titles are capped at 1024 characters, and a missing or
malformed property part yields `None` without failing the extraction.

`extract_bytes(bytes, content_type)` delegates to `extract_document_sync` and returns
only the Markdown. Parsing is CPU-bound; async callers should use the sync variant in
`spawn_blocking`.

Warning codes (`WarningCode`, serialized `snake_case`): `truncated_sheet`,
`hidden_sheet_skipped`, `sheet_error`, `hidden_slide_skipped`, `slide_error`, `truncated_pdf`,
`pdf_unmapped_font`, `pdf_no_text`, `invalid_utf8`, `hidden_content_skipped`, `malformed_csv`,
`truncated_table`, `encoding_problem`, `missing_chapter`, `truncated` (agent tool only) and `other` (custom parsers). Callers match on
`code`; `detail` is for humans and may change.

`Markkit` is `Clone + Debug + Send + Sync` and holds only its configuration, so embedders can
clone it per task instead of wrapping it in an `Arc`.

## Format Detection

Priority order (`Format::detect`):
1. Content type (`DocumentHint::content_type`; parameters ignored). XLSB
   (`application/vnd.ms-excel.sheet.binary.macroEnabled.12`) is matched before the
   generic `ms-excel.sheet` → XLSX rule. `application/vnd.ms-powerpoint.*.macroEnabled.12`
   is PPTX; bare `application/vnd.ms-powerpoint` is legacy PPT. Generic types such as
   `application/octet-stream` map to `Unknown` and fall through.
2. File extension (`DocumentHint::file_name`; URL query/fragment ignored).
3. Magic bytes (`Format::sniff`):
   - `PK\x03\x04` ZIP: ODF/EPUB `mimetype` entry → ODS, ODT or EPUB; `word/document*` → DOCX;
     `xl/workbook.bin` → XLSB; `xl/workbook.*` → XLSX; `ppt/presentation.*` → PPTX (part names are stored
     uncompressed, so no archive parsing is needed for detection).
   - OLE2 (`D0 CF 11 E0`): UTF-16 `WordDocument` stream → DOC; `PowerPoint Document` →
     PPT; `Workbook`/`Book` → XLS.
   - `%PDF-` within the first 1 KiB → PDF.
   - `<!doctype html`, `<html`, `<head`, `<body` (after BOM/whitespace) → HTML.

Plain text, Markdown, CSV/TSV, notebooks and email are never sniffed: they need a content type or extension, so
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
numbering through the main part's relationships. Body blocks (`w:p`, `w:tbl`, ...) are
converted while the main part is parsed and each block's XML subtree is freed right after
(`xml::parse_split`), so peak memory follows the largest block, not the document. XML
trees intern element names and attribute values per part.

## PPTX Mapping

| PresentationML | Markdown |
|---|---|
| `p:sldIdLst` order (via `ppt/_rels/presentation.xml.rels`) | One `## Slide N: <title>` section per slide (`## Slide N` without a title) |
| Title / centered-title placeholder | Slide heading text (multi-paragraph titles joined with spaces) |
| Body / content placeholder (`ph` with `type="body"`, `obj` or no type) | Bulleted list, nested by `a:pPr@lvl` (0–8); `a:buNone` paragraphs stay paragraphs |
| Text box / other placeholders (subtitle, footer, ...) | Paragraphs; paragraphs with `a:buChar`/`a:buBlip` become bullets |
| `a:buAutoNum` (`startAt`) | Ordered list |
| `a:rPr` `b`, `i`, `strike` (≠ `noStrike`) | `**`, `*`, `~~` (shared run merging with DOCX) |
| `a:hlinkClick r:id` → external relationship | `[text](url)`; internal jumps (slide links) stay plain text |
| `a:br` | Hard line break |
| `a:tbl` in a graphic frame | GFM table, first row = header, `gridSpan`/`rowSpan` kept, `hMerge`/`vMerge` continuation cells dropped |
| `p:grpSp`, `mc:AlternateContent` | Walked in document order (first branch) |
| Notes slide body placeholder | `### Notes` section after the slide |
| `p:sld@show="0"` | Skipped with `hidden_slide_skipped` unless `include_hidden_content(true)` |
| Pictures, charts, SmartArt, media, comments, masters/layouts | Skipped |

A slide whose relationship is dangling or whose part is missing emits `slide_error`; the
other slides still render. `stats.pages` counts rendered slides, `stats.tables` tables.

## ODT Mapping

| OpenDocument text | Markdown |
|---|---|
| `text:h` (`outline-level`, default 1) / paragraph style with `default-outline-level` | `#`..`######` (clamped) |
| Paragraph style `Title` (name or display name, via parents) | `#`, and the title fallback after `meta.xml` `dc:title` |
| `text:p` | Paragraph (empty dropped) |
| `text:span` style: `font-weight` bold/≥600, `font-style` italic/oblique, `text-line-through-style` ≠ none | `**`, `*`, `~~`; adjacent equal runs merged, edge whitespace outside |
| `text:a xlink:href` | `[text](url)`, spaces → `%20` |
| `text:list` + `list-level-style-number` / `-bullet` | `1.` / `-`, nested by level |
| `table:table` (incl. header rows, row groups) | GFM table, first row header, covered cells empty, nested tables flattened, pipes escaped |
| `text:s c=N` (≤ 64), `text:tab`, `text:line-break` | Spaces, space, hard break |
| `text:section`, fields | Unwrapped |
| Notes, annotations, tracked changes, frames/drawings, table of contents, bookmarks | Skipped |
| Hidden: style chain with `text:display="none"`, `text:section text:display="none"`, `text:hidden-text`, `text:hidden-paragraph` | Skipped with `hidden_content_skipped` unless `include_hidden_content(true)` |

Styles come from `styles.xml` and both automatic-style containers; parent chains stop after
16 hops (cycles terminate). Repeated columns/rows are capped by `max_sheet_cols` /
`max_sheet_rows`; trailing empty cells and rows (producers pad to 1024 columns and ~1M rows)
are dropped, and `truncated_table` is emitted only when a non-empty cell was cut. Text is
kept raw in the IR and escaped by the renderer (S17).

## EPUB Mapping

- Package: `META-INF/container.xml` first rootfile, else the only `.opf` entry; OPF
  `dc:title` is the title.
- Spine order, `linear="no"` items (covers, notes) skipped, each document once (a spine
  repeating one item cannot multiply output); only (X)HTML manifest items are read.
- Each chapter goes through the HTML parser (same stripping and config as web pages);
  chapters are separated by `---`. `stats.chapters` counts rendered chapters. Package XML
  and all chapters share one `Limits` budget (nodes, timeout, depth); hidden-content
  rules (S20) apply per chapter.
- hrefs resolve against the OPF directory, percent-decoded; paths escaping the archive
  root are ignored. Missing manifest items or entries emit `missing_chapter`.
- A spine document listed in `META-INF/encryption.xml` (DRM) is `Error::EpubExtract`;
  font obfuscation alone is fine. Images, CSS, fonts, NCX/nav documents outside the spine
  are not read.

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

Text and spatial structure (`crates/markkit/src/pdf/`); optional native OCR. Base mappings:

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

Spatial extraction (`pdf/analyze.rs`) uses glyph-derived words normalized to the
page's dominant orientation. Repeated gutters define XY cuts; spanning headings
and tables divide bands, with columns read top-to-bottom, left-to-right. Single
rows without column evidence remain lines. Lines form paragraphs, styled inline
content and numbered/bullet lists. Heading sizes rank levels; tagged heading roles
(including structure-tree MCIDs/RoleMap) take precedence. Marked-content ActualText
replaces enclosed glyph text once; recursion, role lengths and retained bytes are
bounded. Optional `pdf_remove_repeated_headers` removes text repeated in the outer
8% margins of at least two pages and 60% of the extracted page set.

Tables: axis-aligned vector rulings form connected grids. Missing internal borders
infer rectangular merged cells (row/column spans); nonrectangular ambiguous merges
fall back to text. Borderless candidates need >=3 aligned logical rows and numeric
data or explicit bold/TH headers; wrapped first-column continuations stay in their
cell. Ordinary prose columns remain prose. The first logical row becomes the
header; bounds/row/column/span metadata are retained. IR uses the existing Table /
TableCell representation. `stats.tables` counts detected tables.

`pdf_layout(true)` returns a version-1 `layout::PdfLayout` sidecar on parse and
extraction results; otherwise omitted. Pages contain source runs/words in drawing
order and semantic blocks/lines/cells in reading order. Every block has a page-local
ID, type, bounds, source span IDs and line geometry; table cells include row/column,
rowspan/colspan and bounds. IR Source.page + Source.layout_block link back to blocks
when layout is requested (independent of track_sources). Geometry remains source
data if IR transforms rewrite nodes. Coordinates are display points from the
intersected crop/media top-left, x right, y down after rotation/UserUnit. Font bounds
use 0.8 em ascent / 0.2 em descent, not outlines; OCR uses recognized word bounds.
Off-page text remains. Invalid/non-finite geometry fails explicitly. JSON remains
optional; `--layout` requires `--json`. Node/WASM recursively expose camelCase keys;
Python/Rust/CLI serialize snake_case keys.

Native `pdf-ocr` adds opt-in `pdf_ocr(true)` (CLI --ocr, Node pdfOcr, Python
pdf_ocr). Only pages without text are recognized, so text layers are never doubled.
Hayro renders the displayed crop box in Rust at max 1800×1800 pixels onto white,
then exports bounded grayscale PGM. Installed Tesseract emits TSV words using one
OpenMP thread. Languages are validated installed codes, default eng; callers can
set pdf_ocr_language / CLI --ocr-language. Tesseract uses private temporary files,
direct argv, size/deadline polling and kill+reap on expiry. Hayro rendering is a
non-interruptible third-party step: deadlines are checked before and after it.
Hosts must provide CPU/memory isolation for hostile documents. OCR can need longer
deadlines, e.g. --timeout 300. WASM has no native process OCR.

Optional `pdf-ocr-ocrs` adds `PdfOcrBackend::Ocrs`, selected by
pdf_ocr_backend / CLI --ocr-backend / Node pdfOcrBackend. Tesseract remains the
default; missing compiled support fails explicitly. Rust, Node and Python accept
pdf_ocr_detection_model / pdf_ocr_recognition_model (camelCase in Node): explicit
local ONNX paths. Both are required only when a textless page needs recognition.
Models load once per document; input file bytes together obey max_pdf_decoded_size.
The core never fetches models or falls back to a different engine. The default
Latin model currently requires language eng; alternate language codes fail.

`prepare_ocr_models.py` verifies the pinned official example pair from
`scripts/ocr_models.json`, including size/SHA256, and records CC-BY-SA 4.0 model
attribution beside the files. These weights are not embedded in packages. RTen
and ocrs execute in Rust and emit character-derived word rectangles, converted
to the same OCR spans/IR/layout as Tesseract. Output storage/nodes obey the same
limits. Deadline checks surround model loading, preprocessing, detection, line
analysis and recognition batches (max 20 lines); individual inference operations
are non-interruptible. Trusted models and host CPU/memory quotas remain required.
RTEN_NUM_THREADS and RAYON_NUM_THREADS control process-level pools. Native bindings
expose the knobs; their optional Cargo feature must be enabled when building.
Markkit WASM still exposes text-layer PDF extraction rather than OCR.

OCR rendering conservatively supports JPEG, header-bounded JPEG2000 and
generic-filter images without DecodeParms. Inline images, JBIG2 and CCITT are refused explicitly until their
internal allocations can be bounded. Per-image working storage is estimated at
16 bytes per source pixel against the stream cap; cumulative RGBA storage is
charged at four bytes per pixel against the decoded cap. Huge or indirect
dimensions fail closed. PGM has no DPI metadata, so Tesseract receives the displayed
page resolution explicitly. This is a working-storage estimate, not a hard
heap quota on Hayro. See [Native Dependencies](../security/native-dependencies.md)
for provenance and remaining C/C++ components.

PDF source geometry/rules count against max_nodes; retained layout storage and
pairwise analysis work are separately bounded by max_pdf_decoded_size. Grids obey
max_sheet_rows/max_sheet_cols/max_spreadsheet_cells before expansion. Layout JSON is
counted without allocating a second JSON copy against max_output_size, including
parse-only callers. Markdown has its own cap at that same setting, excluding JSON
wrapper/pretty-print overhead. OCR raster/TSV bytes use PDF stream/decoded caps.

Heuristic limits: irregular/nonrectangular tables, nested tables, vertical scripts,
rotated individual cells and incomplete tagging may fall back to text; OCR accuracy
depends on source resolution and installed languages. Images are not exported.

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

## Jupyter Notebook Mapping

| nbformat 4 | Markdown |
|---|---|
| Markdown cell | Markdown parser (`markdown` feature), else plain text paragraphs |
| Code cell | Fenced block tagged with `kernelspec.language` / `language_info.name` (sanitized to `[A-Za-z0-9+-_#.]`) |
| Raw cell | Untagged fenced block |
| `stream`, `text/plain` result | ```` ```text ```` block |
| `text/markdown` result | Markdown parser path |
| `error` output | ```` ```text ```` block: `ename: evalue` + traceback |
| Images, HTML, widgets, attachments | Skipped |

Text values may be strings or line arrays. ANSI escapes are stripped before the shared
control-character pass. Title: `metadata.title`, else the first level-1 heading.
`stats.cells` counts cells that rendered. nbformat < 4, invalid JSON and missing `cells`
are `Error::NotebookExtract`; serde_json's 128-level nesting limit turns hostile JSON into
an error. Unexpected value types are ignored.

## Email Mapping

```
# <Subject>

**From:** Name <addr>
**To:** a, b          (groups flattened)
**Cc:** …
**Date:** RFC 3339

<body>

## Attachments

- report.pdf (application/pdf, 1.2 MB)
```

- Body = RFC 8621 `textBody`: the text/plain alternative when present, else the HTML one.
  Plain parts use the plain-text path; HTML parts the HTML parser (`html` feature) or
  mail-parser's HTML-to-text conversion.
- Attachments (including nested `message/rfc822`, shown by subject) are listed, never
  parsed. `stats.attachments` counts them.
- Header values: RFC 2047 decoded by mail-parser, then control characters dropped and all
  whitespace (including encoded newlines) folded to single spaces, so headers cannot inject
  lines. `encoding_problem` when a body part could not be fully decoded.
- Legacy multi-byte charsets (Shift-JIS, GB2312, …) need mail-parser's `full_encoding`,
  which is not enabled; such parts decode lossily.

## Resource Limits

All formats enforce:
- Maximum input size (default: 50MB), checked before detection
- Maximum processing time (default: 60s) – configuration only, enforcement TBD
- Maximum output size (prevent markdown explosion) – bounded by the caps below

Office formats additionally enforce (see [Threat Model](../security/threat-model.md) T6):
- `max_archive_entries` (10 000), `max_archive_entry_size` (64 MiB decompressed),
  `max_archive_total_size` (256 MiB decompressed)
- `max_spreadsheet_cells` (4 000 000) for the dense grid calamine builds for XLS/ODS
- XML nesting depth 256 for DOCX and PPTX parts

PDF additionally enforces (T8): `max_pdf_pages` (10 000; truncate + `truncated_pdf`),
`max_pdf_stream_size` (64 MiB per decoded stream), `max_pdf_decoded_size` (256 MiB total
decoded in the pre-scan, and separately the total content interpreted, form reuse included).

## See also

- [HTML Extraction](html-extraction.md), the Tier 1 path
- [IR Specification](../foundations/ir.md), the shared output of every parser
- [Threat Model](../security/threat-model.md), T6 malicious office documents, T7 text and Markdown, T8 malicious PDFs
