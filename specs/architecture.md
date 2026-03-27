# Architecture

## Overview

Markkit is a Rust library for extracting clean markdown from any document format.
It follows a pipeline architecture: **detect → parse → normalize → convert → output**.

## Design Principles

1. **Format-agnostic API**: One entry point regardless of input format
2. **Streaming where possible**: Process large documents without loading entirely into memory
3. **Configurable output**: Users control what gets included/excluded
4. **Safe by default**: All input is treated as untrusted; no code execution
5. **Feature-gated formats**: Only compile what you need

## Pipeline

```
Input (bytes/URL/file)
  → Format Detection (MIME, magic bytes, extension)
  → Parser (HTML, PDF, DOCX, etc.)
  → Normalized AST (intermediate representation)
  → Markdown Converter (AST → markdown string)
  → Output (String / streaming)
```

## Crate Structure

| Crate | Purpose |
|-------|---------|
| `markkit` | Core library – parsing, conversion, API |
| `markkit-cli` | Command-line tool |
| `markkit-bench` | Performance benchmarks |
| `markkit-eval` | LLM-based quality evaluation |
| `markkit-python` | Python bindings (PyO3) |
| `markkit-js` | Node.js bindings (NAPI-RS) |

## Configuration

Via `MarkkitBuilder`:
- Input size limits
- Processing time limits
- CSS selectors for include/exclude
- Link and image preservation
- Code block extraction
