# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.1] - 2026-10-22

### Security

- Upgrade the HTTP, spreadsheet and ZIP dependency graph to patched TLS and archive releases.
- Track the workspace lockfile so CI audits the same resolution used by CLI and binding builds.
- Run JavaScript build and packaging workflows on Node.js 24.22.1.

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
