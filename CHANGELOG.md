# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

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
