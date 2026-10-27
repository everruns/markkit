# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- HTML page metadata: `extract_document` fills `Extraction::title` (Open Graph, JSON-LD or
  `<title>`) and the new `Extraction::metadata` (description, author, published date, language,
  canonical URL, site name, lead image) from `<meta>`, Open Graph, `<link rel=canonical>` and
  JSON-LD. `markkit extract --json` includes `format`, `title` and `metadata` for files and stdin.
- IR v2: footnotes (`[^1]`), definition lists (`Term` / `:   definition`), task lists (`- [x]`),
  figures with captions, table column alignment (`:---:`) and `colspan`/`rowspan` (laid out on a
  grid; GFM has no spans). HTML maps GitHub and Pandoc footnote markup, `<dl>`, checkbox list items,
  `<figure>`/`<figcaption>` and table spans/alignment. Row-header `<th>` cells in table bodies are
  no longer dropped.
- `Document.ir_version` (`markkit::ir::IR_VERSION = 2`) is serialized with the IR JSON.
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

### Security

- `extract_url` (CLI, Python and Node `extract_url` too) now refuses non-public destinations by default:
  loopback, private, link-local / cloud metadata and other reserved addresses, checked on the URL,
  after DNS resolution and on every redirect. Only `http`/`https` are fetched. Redirects are capped
  (`max_redirects`, default 5), the body is bounded by `max_input_size` while streaming, and the whole
  fetch by `max_processing_secs`. New builder options `allow_private_networks(true)` and `allow_host(..)`;
  refusals return the new `Error::UrlBlocked`.

### Changed

- **Breaking (IR API):** `Node`, `Document`, `ListItem` and the new IR structs are `#[non_exhaustive]`;
  matches need a `_` arm and structs are built with constructors. `Node::Table` cells are now
  `TableCell` (`children`, `colspan`, `rowspan`) with an `alignments` field. Block variants gain a
  `source` field (use `..` in patterns, `Node::paragraph`/`Node::heading` to construct). DOCX `gridSpan` is
  carried as `colspan`.
- Design docs moved from `specs/` to an OKF v0.2 knowledge bundle in `knowledge/`, validated in CI.
- CI workflows declare read-only default permissions and do not persist checkout credentials.
- CI runs every example; `just bench` now runs the criterion benchmarks.
- README, SECURITY and binding docs corrected: Python module is `markkit`.
- Dependencies: drop unused `async-trait`, `futures`, `regex`, `unicode-segmentation`, `encoding_rs`,
  `base64`, `mime`, `lol_html` (core) and `tracing-subscriber`, plus unused deps of the CLI, bindings
  and stub crates. Upgrade scraper 0.29, html5ever 0.40, quick-xml 0.42 (now a single version shared
  with calamine), pyo3 0.30, criterion 0.8. The `logging` feature is reserved and enables only `tracing`.

### Fixed

- Spreadsheets: a sheet with a single row now renders that row as data under Excel column letters
  (`| A | B |`) instead of as a header with no rows.
- Spreadsheets: `Extraction.title` is filled from document properties (XLSX `docProps/core.xml`,
  ODS `meta.xml`, XLS `SummaryInformation`).
- `Format::from_content_type` detects XLSB (`application/vnd.ms-excel.sheet.binary.macroEnabled.12`)
  as the new `Format::Xlsb` instead of `Xlsx`; `.xlsb` files and `xl/workbook.bin` containers too.
- XLSX packages missing `_rels/.rels` no longer fail inside calamine.
- Release builds use `panic = "unwind"`, so a parser panic becomes an error instead of aborting the
  CLI or the host Python/Node process.
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
