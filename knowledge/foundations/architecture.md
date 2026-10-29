---
type: Architecture
title: Markkit Architecture
description: Extraction pipeline, design principles, crate structure, and builder configuration.
tags:
  - markkit
  - architecture
  - pipeline
---

# Architecture

## Overview

Markkit is a Rust library for extracting clean markdown from any document format.
It follows a pipeline architecture: **detect → parse → normalize → convert → output**.

## Design Principles

1. **Format-agnostic API**: One entry point regardless of input format
2. **Streaming where possible**: Process large documents without loading entirely into memory
3. **Configurable output**: Users control what gets included/excluded
4. **Safe by default**: All input is treated as untrusted; no code execution
5. **Feature-gated formats**: Only compile what you need. `tokio` is optional (`http_client`, `tool`). `serde_json` stays required: always-on Jupyter notebook input parses JSON

## Pipeline

```
Input (bytes/URL/file)
  → Format Detection (MIME, magic bytes, extension)
  → Parser (custom registry first, then HTML, DOCX, spreadsheets)
  → Normalized AST (intermediate representation)
  → Transforms (embedder IR passes, in registration order)
  → Markdown Converter (AST → markdown string)
  → Output (String / streaming)
```

### Transforms

`MarkkitBuilder::transform(t)` appends a `transform::Transform`
(`name()`, `apply(&mut Document) -> Result<()>`) to `config.transforms`, a
`TransformChain` of `Arc`s so the builder stays `Clone`. Every parse path runs
the chain: `parse_html`, `parse_document`, and so every `extract_*`. Passes
are trusted embedder code; the first error aborts and is returned as is
(`Error::Transform` exists for passes to use). Built-ins:
`NormalizeHeadings` (shallowest heading becomes `#`), `DropImages`;
`transform::from_fn(name, closure)` wraps a closure. `Document::visit` /
`visit_mut` and `Node::for_each_child_list[_ref]` walk the tree.

## Crate Structure

| Crate | Purpose |
|-------|---------|
| `markkit` | Core library – parsing, conversion, API |
| `markkit-cli` | Command-line tool |
| `markkit-bench` | Benchmark harness, results committed in `results/` |
| `markkit-eval` | Extraction-quality eval study (mira) |
| `markkit-python` | Python bindings (PyO3) |
| `markkit-js` | Node.js bindings (NAPI-RS) |

## Configuration

Via `MarkkitBuilder`:
- Resource limits: one `Limits` struct (`limits.rs`, set whole via `.limits(..)` or per
  field via builder setters): input/output size, timeout, nesting depth (hard max 512),
  node count, archive, spreadsheet and redirect caps. Violations are
  `Error::ResourceLimit`; see [Threat Model](../security/threat-model.md) Resource Limits
- CSS selectors for include/exclude
- Link and image preservation
- Office-document limits (archive entries/sizes, sheet rows/cols, grid cells, hidden sheets)
- Hidden content (`include_hidden_content`, default off): HTML hidden elements, DOCX hidden runs
- URL fetch policy (`allow_private_networks`, `allow_host`, `max_redirects`)
- Base URL for relative links, images and metadata URLs (`base_url`)
- PDF source layout (`pdf_layout`, off by default): `ParsedDocument::layout` and
  `Extraction::layout` hold positioned source text separately from semantic IR.
  Layout includes source runs/words and semantic blocks/lines/table cells;
  extraction reconstructs columns and tables even when layout JSON is off.
  Native OCR (Hayro plus default Tesseract or optional ocrs/RTen with local ONNX
  model paths) and repeated margin
  removal are explicit options. Rendering has cooperative deadlines; deployment
  isolation provides hard process budgets. See [Native Dependencies](../security/native-dependencies.md).
  Exhaustive Rust `MarkkitBuilder` literals must supply the new PDF fields; prefer
  builder methods or struct update syntax with `Default` for extensibility.
- Provenance (`track_sources`): per-block `Source` in the IR (off by default)
- Main-content detection for HTML (`main_content`)

## Public entry points

- `extract_*` return Markdown (`extract_document` adds metadata as `Extraction`).
- `parse_html` and `parse_document` return the IR (`ParsedDocument` = `Document`
  + title, format, warnings, stats); `ParsedDocument::render()` gives the
  `Extraction`. `extract_document` is `parse_document` + `render`.

## See also

- [IR Specification](ir.md), the normalized AST every parser emits
- [Format Support](../formats/format-support.md), detection and per-format parsers
- [HTML Extraction](../formats/html-extraction.md), the primary conversion path
- [Threat Model](../security/threat-model.md), why every input is untrusted
