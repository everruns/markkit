---
type: Playbook
title: Examples
description: Runnable Rust examples in examples/, what each demonstrates, and their requirements.
tags:
  - markkit
  - examples
---

# Examples

## Structure

```
examples/
  README.md            # Index of every example, per surface
  data/                # Sample report.docx, sales.xlsx, sample.pdf + generate.py
  basic.rs             # Default HTML to markdown extraction
  documents.rs         # extract_document_sync: detection, title, metadata, stats, warnings
  convert.rs           # DOCX/XLSX/PDF samples or CLI-supplied files (docx,spreadsheet,pdf)
  main_content.rs      # main_content(true), base_url, include_selector
  custom_selectors.rs  # include/exclude CSS selectors
  configuration.rs     # How builder options change output
  limits.rs            # Limits, Error::ResourceLimit, LimitKind dispatch
  ir_access.rs         # Parse to IR, inspect nodes, render separately
  agent_tool.rs        # LLM tool definition, system prompt, execution
  transforms.rs        # Built-in and closure IR transform passes
  python/              # quickstart.py, documents.py, limits.py (installed markkit wheel)
  node/                # quickstart.cjs, documents.cjs (MARKKIT_MODULE overrides the package)
  wasm/                # quickstart.mjs (crates/markkit-wasm/pkg by default)
```

Rust examples live at the workspace root and are registered as `[[example]]`
targets of the `markkit` crate (`crates/markkit/Cargo.toml`, `required-features`
per example). Run one with `cargo run -p markkit --example <name>`, all with
`just examples` (`scripts/run-examples.sh`).

## Sample data

`examples/data/generate.py` (stdlib only, fixed ZIP timestamps) writes the
sample DOCX (heading, bold, hyperlink, bullet list, table, `dc:title`), XLSX
(two sheets, inline strings and numbers) and PDF (one page, Helvetica text,
`/Title`). Every binding's examples read the same files. Regenerate rather than
hand-edit; commit the outputs.

## Rust examples

- `basic.rs`: `Markkit::builder().build()`, `extract_html_sync`
- `documents.rs`: CSV by file name, ipynb by content type, HTML by magic bytes
  with `Metadata`, warning codes, legacy `.doc` rejection; default features only
- `convert.rs`: `required-features = ["docx", "spreadsheet", "pdf"]`; with no
  args converts `examples/data/*`, otherwise each path argument; prints format,
  title, stats, warnings and Markdown
- `main_content.rs`: default vs `main_content(true)` + `base_url` vs
  `include_selector("article")` on one page with nav, sidebar and comments
- `custom_selectors.rs`: `include_selector("article")`, `exclude_selector(".sidebar")`
- `configuration.rs`: `preserve_links(false)`, `preserve_images(false)`, limit errors
- `limits.rs`: tight `Limits`, matches `Error::ResourceLimit` and maps each
  `LimitKind` to a service response; deep HTML flattens under default limits
- `ir_access.rs`: `parse_html()` returns the IR `Document` (see
  [IR Specification](../foundations/ir.md)); `MarkdownRenderer` renders it
- `agent_tool.rs`: `required-features = ["html", "tool"]`; OpenAI tool
  definition, system prompt, `ToolOutput::result` (see [Tool Contract](../integrations/tool-contract.md))
- `transforms.rs`: `transform(NormalizeHeadings)`, `transform::from_fn` + `Document::visit_mut`

## Binding examples

- Python (`examples/python/`): `quickstart.py` (sync, options, asyncio),
  `documents.py` (dict result, stats, warnings, PDF layout; path args),
  `limits.py` (`markkit.Limits`, `RuntimeError`)
- Node.js (`examples/node/`): `quickstart.cjs`, `documents.cjs` (path args,
  `maxOutputSize` error, `pdfLayout`). `MARKKIT_MODULE` points at the built
  binding in CI; users `require('@everruns/markkit')`
- WASM (`examples/wasm/quickstart.mjs`): `initMarkkit`, `Markkit`, documents,
  `MarkkitTool`, limit error. `MARKKIT_WASM_MODULE` overrides the default
  `crates/markkit-wasm/pkg/index.js`

## Requirements

- No network; inline strings and `examples/data/` only (path args optional)
- Output to stdout, exit 0 on success; an exception or non-zero exit fails CI
- CI runs every example: Rust in the Test job (`scripts/run-examples.sh`),
  Python in `python.yml` after the wheel install, Node.js in `js.yml` after the
  NAPI build, WASM in the `ci.yml` wasm job after the package build
- A new Rust example is added to `crates/markkit/Cargo.toml`,
  `scripts/run-examples.sh`, `examples/README.md` and this document; a binding
  example to its directory (CI globs it), `examples/README.md` and this document

## See also

- [Documentation Architecture](documentation.md), where examples fit among the doc trees
- [Testing Strategy](testing.md), the CI jobs that run them
