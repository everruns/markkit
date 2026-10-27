# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- XLSB (Excel binary workbook) support in the `spreadsheet` feature, via a new bounds-checked
  reader: values, shared strings, dates and durations, hidden-sheet skipping, row/column caps and
  the workbook title. Previously XLSB returned `UnsupportedFormat`.
- `markkit mcp`: Model Context Protocol server over stdio exposing the `markkit` tool
  (`tools/list`, `tools/call`). URL fetching keeps the SSRF guard; `--no-url` keeps it offline,
  the `extract` limit flags and `--max-output-chars` bound each call.
- CSV and TSV input (`text/csv`, `text/tab-separated-values`, `.csv`, `.tsv`), always on: one GFM
  table, RFC 4180 quoting, `;`/`|`/tab delimiter detection, bounded by `max_sheet_rows` /
  `max_sheet_cols` (`truncated_table` warning). New `Format::Tsv`.
- Plain text input (`text/plain`, `.txt`), always on: paragraphs and hard breaks through the IR,
  UTF-8/UTF-16 BOM decoding, control characters dropped, `invalid_utf8` warning on lossy decode.
- Markdown input (`markdown` feature, in `all_formats`; `text/markdown`, `.md`): re-parsed with
  pulldown-cmark and rendered back, so raw HTML never passes through. Front matter `title:` or the
  first `# ` heading becomes `Extraction::title`. Available in the CLI, Python, Node and WASM packages; `format` serializes as
  `"text"` and `"markdown"`.
- PPTX support behind the new `pptx` feature (included in `all_formats`, so in the CLI and the
  Python/Node bindings): one `## Slide N: <title>` section per slide with bullets, text boxes,
  tables, hyperlinks and speaker notes. Hidden slides are skipped with a `hidden_slide_skipped`
  warning unless `include_hidden_content(true)`. Legacy `.ppt` is detected as the new `Format::Ppt`
  and rejected explicitly. `Error::PptxExtract` reports malformed presentations.
- Nightly cargo-fuzz targets for every parser and the renderer (`crates/markkit/fuzz`, `just fuzz`,
  `.github/workflows/fuzz.yml`), each checking the output-safety oracle.
- HTML page metadata: `extract_document` fills `Extraction::title` (Open Graph, JSON-LD or
  `<title>`) and the new `Extraction::metadata` (description, author, published date, language,
  canonical URL, site name, lead image) from `<meta>`, Open Graph, `<link rel=canonical>` and
  JSON-LD. `markkit extract --json` includes `format`, `title` and `metadata`.
- IR v2: footnotes (`[^1]`), definition lists (`Term` / `:   definition`), task lists (`- [x]`),
  figures with captions, table column alignment (`:---:`) and `colspan`/`rowspan` (laid out on a
  grid; GFM has no spans). HTML maps GitHub and Pandoc footnote markup, `<dl>`, checkbox list items,
  `<figure>`/`<figcaption>` and table spans/alignment. Row-header `<th>` cells in table bodies are
  no longer dropped.
- `Document.ir_version` (`markkit::ir::IR_VERSION = 2`) is serialized with the IR JSON.
- PDF extraction (`pdf` feature, on in the CLI `all_formats` build and the Python/Node bindings):
  the text layer becomes paragraphs, font-size headings and bullet lists, with `/Title` as
  `Extraction::title` and `stats.pages`. Pure Rust (hayro-syntax), no OCR. Fonts map through
  `ToUnicode` CMaps, simple-font encodings and `/Differences`. New warnings `truncated_pdf`,
  `pdf_unmapped_font`, `pdf_no_text`. Encrypted PDFs return `UnsupportedFormat`.
- `Limits` fields `max_pdf_pages` (10 000), `max_pdf_stream_size` (64 MiB) and
  `max_pdf_decoded_size` (256 MiB), also on the builder, as CLI flags (`--max-pdf-pages`, ...),
  Python `Limits(...)` arguments and Node options (`maxPdfPages`, ...). PDF extraction also
  honours `timeout` and `max_output_size`.
- Relative URL resolution: links, images and metadata URLs resolve against the new
  `MarkkitBuilder::base_url` (CLI `--base-url`) and the page's `<base href>`; `extract_url` uses
  the final URL after redirects. Absolute and fragment-only URLs are left unchanged.
- Math: `` $`tex`$ `` and ` ```math ` blocks from MathML (`application/x-tex` annotation), KaTeX and MathJax output.
- IR transforms: `MarkkitBuilder::transform(..)` runs `Transform` passes after parsing and before
  rendering, for every format. Built-ins `transform::NormalizeHeadings` and `transform::DropImages`;
  `transform::from_fn` for closures; `Document::visit` / `visit_mut` to walk the tree.
- Declarative HTML element rules: `MarkkitBuilder::html_rule(".note", ElementAction::BlockQuote { .. })`
  maps elements by CSS selector to skip / unwrap / heading / paragraph / block quote (with title) /
  code / emphasis / text, before the built-in mapping. CLI: `markkit extract --rules rules.json`.
- Pluggable parsers: `MarkkitBuilder::parser(..)` registers a `registry::FormatParser` that handles
  detected formats (e.g. PDF or CSV) or claims inputs by content, before the built-ins. Its IR goes
  through transforms and the shared renderer; panics become the new `Error::Parser`.
- JSON Schema for the IR at `schema/ir.schema.json`; `json-schema` feature adds `markkit::ir::json_schema()`.
- Per-block provenance: `MarkkitBuilder::track_sources(true)` records a `Source` on each IR block
  (HTML element XPath, DOCX body block index, spreadsheet sheet and A1 range) for citations.
- `Markkit::parse_document` / `parse_document_sync` return the IR plus metadata (`ParsedDocument`)
  for any supported format; `ParsedDocument::render()` produces the `Extraction`.
- `tool` feature: `markkit::tool::MarkkitTool`, an LLM agent tool following the Everruns
  toolkit-library contract (builder, OpenAI tool definition, input/output schemas, `tower::Service`,
  single-use `ToolExecution`, `en-US`/`uk-UA` text). One call converts a `url`, `html` or base64
  document to Markdown, with output capped by `max_output_chars`. Example: `examples/agent_tool.rs`.
- `@everruns/markkit-wasm`: browser / edge WebAssembly package (`crates/markkit-wasm`), single-threaded,
  no special headers. `Markkit` class (HTML, DOCX, XLSX/XLS/ODS with validated camelCase options) and
  `MarkkitTool`. Built by `crates/markkit-wasm/scripts/build.sh`, published by `publish-wasm.yml`.
- Node (`@everruns/markkit`): every function takes an optional trailing `options` object with all
  builder settings (selectors, links/images, input, archive and sheet limits) and the URL policy
  (`allowPrivateNetworks`, `allowedHosts`, `maxRedirects`, `maxProcessingSecs`). New
  `extractDocument` / `extractDocumentSync` return Markdown with title, format, warnings, stats and
  page metadata.
- `Markkit::extract_url_document`: `extract_url` with format, title, warnings and stats.
- Main-content detection: `MarkkitBuilder::main_content(true)` (CLI `--main-content`) keeps only
  the article body of a web page, Readability-style, dropping navigation, sidebars, comments, share
  bars and related-link blocks. Off by default; include selectors take precedence; falls back to the
  full page when no article-like block is found.
- Every resource limit is configurable outside Rust: CLI flags (`--max-output-size`, `--timeout`,
  `--max-nesting-depth`, `--max-nodes`, archive / sheet / redirect flags; `--max-size` kept as an
  alias of `--max-input-size`), Python `markkit.Limits(...)` passed as `limits=` to every function,
  and flat Node.js and WASM option keys (`maxOutputSize`, `maxNodes`, `maxNestingDepth`, plus
  `timeout` in Node.js).
- Node.js and WASM options gain `baseUrl` and `mainContent`; Python `extract_html_sync_with_options`
  gains `base_url=` and `main_content=`, matching the core builder.

### Security

- Resource limits on every aspect, in one `markkit::Limits` struct (`MarkkitBuilder::limits(..)` or
  per-field setters). New: `timeout` (default 60 s) now bounds parsing and rendering, not just URL
  fetch; `max_output_size` (128 MiB) stops output amplification (e.g. one wide table row padding
  thousands of rows); `max_nodes` (5 000 000) caps parsed HTML DOM / DOCX XML nodes;
  `max_nesting_depth` (256) is configurable, with a hard ceiling of 512 that keeps deep input off
  the host stack. Violations return `Error::ResourceLimit` naming the limit.
- PDF decompression bombs: every stream in the file is decoded under the PDF size limits before
  the parser runs (object streams included), interpreted content is budgeted so reused form
  XObjects cannot multiply work, and stream filters that cannot be verified are refused.

- `extract_url` (CLI, Python and Node `extract_url` too) now refuses non-public destinations by default:
  loopback, private, link-local / cloud metadata and other reserved addresses, checked on the URL,
  after DNS resolution and on every redirect. Only `http`/`https` are fetched. Redirects are capped
  (`max_redirects`, default 5), the body is bounded by `max_input_size` while streaming, and the whole
  fetch by `max_processing_secs`. New builder options `allow_private_networks(true)` and `allow_host(..)`;
  refusals return the new `Error::UrlBlocked`.
- Markdown output is now escaped: text from documents can no longer emit raw HTML (`<img onerror>`),
  links, emphasis, headings or list markers; code blocks use a fence longer than any backtick run
  inside. Links with schemes other than `http`, `https`, `mailto`, `tel`, `ftp` or relative URLs
  (e.g. `javascript:`) and images other than `http`/`https`/relative/raster `data:image/*` are
  dropped, keeping their text. URLs percent-encode `( ) < > \` and spaces.
- Markdown structure from documents can no longer break out of its container: code blocks in table
  cells and headings are flattened to one line, list item bodies are indented by the marker width
  (nested content under `1.` items now nests correctly), and nested links render as text.
- The JSON IR of DOCX and spreadsheet tables now holds raw text (pipes were pre-escaped before).
- Math TeX (taken from page markup) renders as code, GitHub's `` $`tex`$ `` and ` ```math `
  forms, instead of `$tex$` / `$$`, so it can no longer emit raw HTML or links in plain GFM.
- XLS files with truncated or inconsistent BIFF records (found by fuzzing) are rejected with an
  error instead of aborting the process inside calamine. Formula token streams are ignored
  (cached values were already the only thing rendered).
- Hidden content is skipped by default as a prompt-injection defence: HTML elements with `hidden`,
  `aria-hidden="true"` or inline `display:none` / `visibility:hidden` / `opacity:0` / `font-size:0`,
  `<template>`, and DOCX hidden (`w:vanish`) runs (warning `hidden_content_skipped`). Opt back in
  with `include_hidden_content(true)`.

### Added

- `markkit-bench` harness (`just bench`): per-case latency, ops/s, throughput, heap allocations and
  peak heap, plus process peak RSS, over a generated HTML/DOCX/XLSX/ODS corpus. Runs are saved as
  JSON + Markdown in `crates/markkit-bench/results/` and committed.

### Changed

- **Breaking:** `Error::ResourceLimit` carries a `LimitExceeded { kind: LimitKind, detail }` instead of
  a `String`. `LimitKind` names the `Limits` field (`MaxInputSize`, `Timeout`, `MaxArchiveEntrySize`,
  ...); `Error::limit_kind()` returns it. `ResourceLimit(_)` patterns and the error message are unchanged.
- **Breaking**: limit fields moved from `MarkkitBuilder` to `MarkkitBuilder::limits`
  (`config().max_input_size` → `config().limits.max_input_size`); `max_processing_secs` field is
  now `limits.timeout: Duration`. Builder setters are unchanged, plus new `timeout`,
  `max_output_size`, `max_nodes`, `max_nesting_depth`, `limits`. HTML nesting now flattens at depth
  256 (was 200).
- **Breaking (features):** `http_client` is no longer a default feature; the default build pulls in
  no HTTP/TLS stack. Library users calling `extract_url` add `features = ["http_client"]`. The CLI and
  the Python/Node packages still enable it, so their `extract_url` is unchanged.
- **Breaking:** `Error` is `#[non_exhaustive]` (new `Error::Transform`); matches need a `_` arm.
- **Breaking:** `Extraction::warnings` and `ParsedDocument::warnings` are `Vec<Warning>`
  (`code: WarningCode` + `detail: String`) instead of `"code: detail"` strings, so callers can match
  on the code. `Warning`'s `Display` keeps the old string form; JSON serializes
  `{"code": "truncated_sheet", "detail": "..."}`. The agent tool output is unchanged.
  Node and WASM `extractDocument` return `warnings` as `{ code, detail }` objects too.
- `Markkit` implements `Clone` and `Debug`.
- **Breaking (IR API):** `Node`, `Document`, `ListItem` and the new IR structs are `#[non_exhaustive]`;
  matches need a `_` arm and structs are built with constructors. `Node::Table` cells are now
  `TableCell` (`children`, `colspan`, `rowspan`) with an `alignments` field. Block variants gain a
  `source` field (use `..` in patterns, `Node::paragraph`/`Node::heading` to construct). DOCX `gridSpan` is
  carried as `colspan`.
- `markkit-eval` is now a [mira](https://github.com/everruns/mira) eval study. `just eval` runs
  `markkit_extract`, an offline extraction-quality eval over ten realistic HTML pages scored for
  completeness, accuracy, formatting and noise (needs `cargo install mira-cli`). A `mode` axis
  compares default extraction with `main_content`; dated results are committed in
  `crates/markkit-eval/results/`.
- Design docs moved from `specs/` to an OKF v0.2 knowledge bundle in `knowledge/`, validated in CI.
- CI workflows declare read-only default permissions and do not persist checkout credentials.
- CI runs every example; criterion benchmarks run with `just bench-criterion`.
- README, SECURITY and binding docs corrected: Python module is `markkit`.
- Dependencies: drop unused `async-trait`, `futures`, `regex`, `unicode-segmentation`, `encoding_rs`,
  `base64`, `mime`, `lol_html` (core) and `tracing-subscriber`, plus unused deps of the CLI, bindings
  and stub crates. Upgrade scraper 0.29, html5ever 0.40, quick-xml 0.42 (now a single version shared
  with calamine), pyo3 0.30, criterion 0.8. The `logging` feature is reserved and enables only `tracing`.

### Fixed

- HTML: deeply nested input no longer costs quadratic time. 687 KiB of nested `<div>`s took
  12.6 s and 292 KiB of unclosed nested tables ran past the 60 s timeout; tags nested past
  `max_nesting_depth` are now dropped (text kept) before html5ever parses, so both take
  milliseconds.
- HTML: rows of nested tables are no longer copied into every enclosing table (output grew 4x
  per nesting level).
- HTML: text flattened past `max_nesting_depth` no longer includes `<script>`, `<style>`,
  `<template>` and other non-content element bodies.
- Spreadsheets: a sheet with a single row now renders that row as data under Excel column letters
  (`| A | B |`) instead of as a header with no rows.
- Spreadsheets: `Extraction.title` is filled from document properties (XLSX `docProps/core.xml`,
  ODS `meta.xml`, XLS `SummaryInformation`).
- `Format::from_content_type` detects XLSB (`application/vnd.ms-excel.sheet.binary.macroEnabled.12`)
  as the new `Format::Xlsb` instead of `Xlsx`; `.xlsb` files and `xl/workbook.bin` containers too.
- XLSX packages missing `_rels/.rels` no longer fail inside calamine.
- Release builds use `panic = "unwind"`, so a parser panic becomes an error instead of aborting the
  CLI or the host Python/Node process.
- Text that only looks like a block marker is no longer escaped: `4.6 out of 5` stays `4.6`, not
  `4\.6`; same for `#tag` and `+1`. Markers followed by whitespace (`1. `, `# `, `+ `) are still
  escaped.
- Exclude selectors now apply to `<li>`, `<tr>`, `<th>` and `<td>`; they were ignored there.
- Indented HTML (most real web pages) now keeps blank lines between blocks; previously
  headings, paragraphs, lists and tables after the first ran together with a leading space
  (`# A\n B` instead of `# A\n\nB`), which also broke headings and tables as markdown.

## [0.1.1] - 2026-10-22

### Security

- Upgrade the HTTP, spreadsheet and ZIP dependency graph to patched TLS and archive releases.
- Track the workspace lockfile so CI audits the same resolution used by CLI and binding builds.
  Library consumers must refresh their application lockfiles to receive transitive TLS fixes;
  a dependency library's lockfile is not inherited.
- Run JavaScript build and packaging workflows on Node.js 24.22.1.
- Include the generated JavaScript loader and native binary in npm packages and verify installed package loading.
- Scope npm native artifacts explicitly to Linux x64/glibc; other platforms are not included in this release.
- Verify Python wheel metadata and native extraction before upload; the wheel publisher targets Linux x64/CPython 3.12.

### Changed

- **Breaking**: limit fields moved from `MarkkitBuilder` to `MarkkitBuilder::limits`
  (`config().max_input_size` → `config().limits.max_input_size`); `max_processing_secs` field is
  now `limits.timeout: Duration`. Builder setters are unchanged, plus new `timeout`,
  `max_output_size`, `max_nodes`, `max_nesting_depth`, `limits`. HTML nesting now flattens at depth
  256 (was 200).

- Release automation: merging `chore(release): prepare vX.Y.Z` to main tags, creates the
  GitHub Release and publishes to crates.io (idempotent); manual dispatch from main also works

## [0.1.0] - 2026-10-12

### Added

- Initial project structure
- Core markdown extraction library (`markkit` crate)
- CLI tool (`markkit-cli` crate)
- Python bindings (`markkit-python` crate)
- Node.js bindings (`markkit-js` crate)
- Benchmark harness (`markkit-bench` crate)
- LLM evaluation harness (`markkit-eval` crate)
- **HTML → Markdown pipeline** via canonical IR (`ir.rs`, `html/parser.rs`, `render/markdown.rs`)
  - Full element mapping: headings, paragraphs, lists (nested), blockquotes, code blocks, tables (GFM), inline formatting, links, images, thematic breaks, line breaks
  - Configurable CSS include/exclude selectors
  - Depth limiting (MAX_DEPTH=200) to prevent stack overflow
  - Whitespace normalization with inter-element space preservation
  - Non-content element stripping (script, style, iframe, form, svg, etc.)
  - Resource limit enforcement (max input size)
- **Canonical IR** with serde JSON serialization support
- **Sync API**: `extract_html_sync()` and `parse_html()` for non-async usage
- **CLI stdin support**: `--stdin` flag and `-` source convention
- **Python bindings**: `extract_html_sync()` and `extract_html_sync_with_options()` with keyword args
- **JS bindings**: `extract_html_sync()` synchronous function
- **Specs**: IR specification, threat model, comprehensive HTML extraction spec, examples spec
- **Examples**: `basic.rs`, `custom_selectors.rs`, `ir_access.rs`, `configuration.rs`
- **Test fixtures**: 9 paired input/output HTML→Markdown fixture files
- **Bytes-in document API**: `Markkit::extract_document` / `extract_document_sync` taking a
  `DocumentHint { content_type, file_name }` and returning `Extraction { markdown, title,
  format, warnings, stats }`; `extract_bytes` now delegates to it
- **Format detection**: `Format::detect` (content type → extension → magic bytes),
  `Format::sniff` (ZIP part names, OLE2 streams, `%PDF`, HTML), `Format::from_file_name`;
  new `Format::Doc`, `Xls`, `Ods`, `Pptx` variants (`Format` is now `#[non_exhaustive]`)
- **DOCX → Markdown** (`docx` feature): headings (Title/HeadingN/outline level), emphasis,
  hyperlinks, bulleted/numbered lists via numbering.xml, tables, line breaks, title from
  core properties
- **Spreadsheets → Markdown** (`spreadsheet` feature): XLSX/XLSM, XLS, ODS via calamine; one
  `## Sheet` section + table per non-empty sheet, row/column caps with `truncated_sheet`
  warnings, hidden sheets skipped by default
- **Office-document safety limits**: ZIP entry count, per-entry and total decompressed size,
  spreadsheet grid pre-scan (`max_spreadsheet_cells`), XML depth cap; fuzz-style tests

### Changed

- **Breaking**: limit fields moved from `MarkkitBuilder` to `MarkkitBuilder::limits`
  (`config().max_input_size` → `config().limits.max_input_size`); `max_processing_secs` field is
  now `limits.timeout: Duration`. Builder setters are unchanged, plus new `timeout`,
  `max_output_size`, `max_nodes`, `max_nesting_depth`, `limits`. HTML nesting now flattens at depth
  256 (was 200).

- `docx` feature now uses `zip` + `quick-xml` directly; `docx-rs` dependency removed
- `calamine` upgraded to 0.36 (pulls quick-xml 0.41, clearing RUSTSEC-2026-0194/0195); `zip` upgraded to 8 (deflate only), shared with calamine
- PDF extraction returns `Error::UnsupportedFormat` instead of an empty string
- `pdf` feature is now a placeholder with no dependencies: unused `pdf-extract` removed
  (cleared RUSTSEC-2026-0187 in lopdf and unmaintained `ttf-parser`)
- `pyo3`/`pyo3-async-runtimes` upgraded to 0.29 (RUSTSEC-2026-0176/0177); bench `tabled`
  upgraded to 0.22 (drops unmaintained `proc-macro-error2`)
- CLI `--file` detects the format from the extension and file contents
- HTML-only tests and examples are gated on the `html` feature so non-default feature
  sets (e.g. `--no-default-features --features docx,spreadsheet`) build and test cleanly
