# Markkit

[![CI](https://github.com/everruns/markkit/actions/workflows/ci.yml/badge.svg)](https://github.com/everruns/markkit/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/markkit.svg)](https://crates.io/crates/markkit)
[![docs.rs](https://img.shields.io/docsrs/markkit)](https://docs.rs/markkit)
[![PyPI](https://img.shields.io/pypi/v/markkit.svg)](https://pypi.org/project/markkit/)
[![npm](https://img.shields.io/npm/v/@everruns/markkit.svg)](https://www.npmjs.com/package/@everruns/markkit)
[![npm wasm](https://img.shields.io/npm/v/@everruns/markkit-wasm.svg?label=npm%20wasm)](https://www.npmjs.com/package/@everruns/markkit-wasm)
[![Repo: Agent Friendly](https://img.shields.io/badge/Repo-Agent%20Friendly-blue)](https://github.com/everruns/markkit/blob/main/AGENTS.md)

**Extract clean Markdown from any document, safely.** Written in Rust.

Markkit turns web pages, PDFs, Word and PowerPoint files, spreadsheets,
e-books, email and notebooks into well-structured Markdown that LLMs and agents
can read. It is built for **untrusted input**: nothing is executed, every
resource a document can drive is bounded, and the default build has no network
stack.

One engine, every runtime:

| Surface | Install | Docs |
|---|---|---|
| Rust library | `cargo add markkit` | [docs.rs/markkit](https://docs.rs/markkit) |
| CLI + MCP server | `cargo install markkit-cli` | [CLI](#cli) |
| Python | `pip install markkit` | [Python README](crates/markkit-python/README.md) |
| Node.js (native) | `npm install @everruns/markkit` | [Node.js README](crates/markkit-js/README.md) |
| Browser / edge (WebAssembly) | `npm install @everruns/markkit-wasm` | [WASM README](crates/markkit-wasm/README.md) |

## Why Markkit

- **Many formats, one API.** Bytes in, Markdown out. The format is detected
  from the content type, file extension or magic bytes.
- **Safe by default.** No scripts, macros or formulas run. XML entities are
  never expanded. Archives and PDF streams are decompressed under measured
  limits. Hidden text (a prompt-injection vector) is dropped. URL fetching
  refuses private and cloud-metadata addresses. Every mitigation in the
  [threat model](knowledge/security/threat-model.md) has a test.
- **Bounded.** Input and output size, time, nesting depth, node count, archive
  and spreadsheet sizes and PDF streams all have limits. A breach is a typed
  error, never a hang or an out-of-memory crash.
- **Agent-ready.** Main-content detection strips sidebars and comment
  threads. Results carry title, metadata, stats and structured warnings. A
  built-in LLM tool and an MCP server are included.
- **Fast.** HTML extracts at 14–22 MiB/s and format detection takes
  microseconds. See [Performance](#performance).

## Supported formats

| Format | Rust feature | Notes |
|---|---|---|
| HTML / XHTML | `html` (default) | Nav, header, footer, scripts, styles, forms stripped; optional main-content detection |
| PDF | `pdf` | Text layer, reading order, columns, ruled and borderless tables, optional [layout geometry](docs/pdf.md) |
| PDF OCR | `pdf-ocr` | Textless pages via installed Tesseract; optional Rust `ocrs` backend |
| DOCX | `docx` | Headings, lists, tables, links, emphasis, footnotes |
| PPTX, PPTM, PPSX | `pptx` | One section per slide: titles, bullets, tables, links, speaker notes |
| XLSX, XLSM, XLSB, XLS, ODS | `spreadsheet` | One Markdown table per sheet; hidden sheets skipped |
| ODT | `odt` | OpenDocument text |
| EPUB | `epub` | Chapters in reading order, title from package metadata |
| Email (EML) | `email` | Headers, body (plain preferred, HTML converted), attachment list |
| Markdown | `markdown` | Normalized through the same IR; raw HTML never passed through |
| Jupyter notebooks | always on | Markdown and code cells, text outputs, tracebacks |
| CSV / TSV | always on | `,` `;` `\|` and tab delimiters, RFC 4180 quoting |
| Plain text | always on | Paragraphs |
| DOC, PPT (legacy binary) | – | Detected and rejected with a clear error |

`all_formats` enables every format feature. The Python, Node.js and CLI
packages ship with all formats, OCR and URL fetching. The WASM package covers
HTML, PDF text and layout, DOCX, spreadsheets, Markdown, CSV/TSV, notebooks and
text, without URL fetching or OCR.

## Quick start

### Rust

```toml
[dependencies]
markkit = { version = "0.3", features = ["pdf", "docx", "spreadsheet"] }
```

```rust,no_run
use markkit::{DocumentHint, Markkit};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let markkit = Markkit::builder().build();

    // HTML string to Markdown
    let md = markkit.extract_html_sync("<h1>Hello</h1><p>World</p>")?;
    assert_eq!(md, "# Hello\n\nWorld\n");

    // Any supported file: format, title, warnings and stats come back too
    let bytes = std::fs::read("report.docx")?;
    let doc = markkit.extract_document_sync(&bytes, DocumentHint::file_name("report.docx"))?;
    println!("{} {:?} {:?}", doc.format, doc.title, doc.stats);
    println!("{}", doc.markdown);
    Ok(())
}
```

Extraction is CPU-bound. In async code, call the `_sync` methods from
`tokio::task::spawn_blocking`. Async `extract_html`, `extract_document` and
`extract_url` (feature `http_client`) are also available.

### CLI

```bash
cargo install markkit-cli

markkit extract https://example.com                    # URL
markkit extract --file report.docx                     # any supported file
markkit extract --file report.pdf --json               # JSON: markdown, format, title, warnings, stats, metadata
markkit extract https://example.com/post --main-content  # article body only
markkit extract https://example.com -i article -e .ads # CSS include / exclude
markkit extract --file page.html --base-url https://example.com/blog/
cat page.html | markkit extract -                      # stdin
markkit extract --file scan.pdf --ocr --ocr-language eng --timeout 300
markkit extract --file page.html --rules rules.json    # site-specific element rules
```

Every resource limit has a flag (`--max-input-size`, `--timeout`,
`--max-nodes`, ...); see `markkit extract --help`.

**MCP server.** `markkit mcp` serves the `markkit` tool over stdio
(`--no-url` keeps it offline):

```json
{ "mcpServers": { "markkit": { "command": "markkit", "args": ["mcp"] } } }
```

### Python

```python
import markkit

print(markkit.extract_html_sync("<h1>Hello</h1>"))  # "# Hello\n"

with open("report.pdf", "rb") as f:
    result = markkit.extract_document_sync(f.read(), file_name="report.pdf")
print(result["format"], result["title"], result["stats"])
print(result["markdown"])

md = await markkit.extract_url("https://example.com")  # async
```

Typed (ships `.pyi` stubs). Full API: [Python README](crates/markkit-python/README.md).

### Node.js

```js
const fs = require('node:fs');
const { extractHtmlSync, extractDocument, extractUrl } = require('@everruns/markkit');

console.log(extractHtmlSync('<h1>Hello</h1>')); // "# Hello\n"

const doc = await extractDocument(fs.readFileSync('report.docx'), { fileName: 'report.docx' });
console.log(doc.format, doc.title, doc.markdown);

const md = await extractUrl('https://example.com', { mainContent: true });
```

TypeScript types included. Full API: [Node.js README](crates/markkit-js/README.md).

### Browser and edge (WebAssembly)

```js
import { initMarkkit, Markkit } from '@everruns/markkit-wasm';

await initMarkkit(); // once
const mk = new Markkit({ mainContent: true });
console.log(mk.extractHtml('<h1>Hello</h1>'));

const res = await fetch('/report.pdf');
const doc = mk.extractDocument(new Uint8Array(await res.arrayBuffer()), { fileName: 'report.pdf' });
```

Runs in browsers, Cloudflare Workers, Vercel Edge, Deno, Bun and Node with no
special headers. Full API: [WASM README](crates/markkit-wasm/README.md).

## Examples

Runnable examples for every surface live in [`examples/`](examples/), use the
bundled sample DOCX, XLSX and PDF in [`examples/data/`](examples/data/), and run
in CI on every change.

```bash
cargo run -p markkit --example basic
cargo run -p markkit --features all_formats --example convert -- path/to/file.pptx
python examples/python/documents.py
node examples/node/documents.cjs
node examples/wasm/quickstart.mjs
```

See [examples/README.md](examples/README.md) for the full list.

## Configuration

```rust
use std::time::Duration;
use markkit::{Limits, Markkit};

let markkit = Markkit::builder()
    .main_content(true)                       // keep the article, drop sidebars and comments
    .include_selector("article")              // CSS selectors to keep
    .exclude_selector(".newsletter")          // and to drop (added to nav/footer/script/...)
    .base_url("https://example.com/blog/")    // resolve relative links and images
    .preserve_images(false)
    .track_sources(true)                      // per-block provenance in the IR
    .limits(Limits::default().timeout(Duration::from_secs(10)).max_output_size(8 << 20))
    .build();
```

A built `Markkit` is immutable and can be shared across threads. Bindings use
the same options: snake_case keyword arguments in Python, flat camelCase keys
in Node.js and WASM.

### Resource limits

Every resource an input can drive is bounded by `markkit::Limits`. Defaults are
generous. A breach returns `Error::ResourceLimit`, and `err.limit_kind()` names
the limit (Python raises `RuntimeError`, JS throws `Error`, with the limit name
in the message).

| Limit | Default | Bounds |
|-------|---------|--------|
| `max_input_size` | 50 MiB | Input bytes, fetched URL body |
| `max_output_size` | 128 MiB | Markdown output |
| `timeout` | 60 s | Fetch + parse + render, per call (not enforced in WASM: no clock) |
| `max_nesting_depth` | 256 (hard max 512) | HTML flattens deeper content; deeper DOCX XML is rejected |
| `max_nodes` | 5 000 000 | HTML DOM nodes, XML nodes, retained PDF layout |
| `max_archive_entries` | 10 000 | ZIP entries (DOCX/PPTX/XLSX/ODT/EPUB) |
| `max_archive_entry_size` | 64 MiB | Decompressed bytes per ZIP entry |
| `max_archive_total_size` | 256 MiB | Decompressed bytes per ZIP |
| `max_spreadsheet_cells` | 4 000 000 | XLS/ODS grid before parsing |
| `max_sheet_rows` / `max_sheet_cols` | 10 000 / 256 | Rendered per sheet (truncated with a warning) |
| `max_pdf_pages` | 10 000 | PDF pages rendered (truncated with a warning) |
| `max_pdf_stream_size` | 64 MiB | Decoded bytes per PDF stream |
| `max_pdf_decoded_size` | 256 MiB | Decoded PDF bytes in total |
| `max_redirects` | 5 | `extract_url` redirects |

### Cargo features

| Feature | Default | Description |
|---------|---------|-------------|
| `html` | Yes | HTML parsing and conversion |
| `pdf`, `docx`, `pptx`, `spreadsheet`, `odt`, `epub`, `email`, `markdown` | No | Format support, see [Supported formats](#supported-formats) |
| `all_formats` | No | All of the above |
| `pdf-ocr` | No | OCR for textless PDF pages: Hayro rendering + installed Tesseract |
| `pdf-ocr-ocrs` | No | Rust OCR via ocrs/RTen with caller-supplied local ONNX models |
| `http_client` | No | `extract_url` / `extract_url_document` via reqwest, with SSRF protection |
| `tool` | No | `markkit::tool::MarkkitTool` LLM agent tool (Everruns toolkit contract) |
| `json-schema` | No | `markkit::ir::json_schema()` for the serialized IR |
| `logging` | No | Reserved: pulls in `tracing`, emits no events yet |

`http_client` is opt-in: the default build has no HTTP/TLS stack and no tokio.
Office formats only, no HTML, no network:

```toml
markkit = { version = "0.3", default-features = false, features = ["docx", "pptx", "spreadsheet"] }
```

## PDF

PDF extraction reads the text layer, rebuilds reading order across columns,
reconstructs ruled and borderless tables (with merged and wrapped cells), keeps
bold, italic and numbered lists, and uses tagged-PDF headings and
`ActualText`. Opt-in extras:

- **Layout geometry**: `pdf_layout(true)` / `--layout` / `pdfLayout: true` adds
  pages, positioned text spans and semantic blocks with bounding boxes.
- **Repeated headers**: `pdf_remove_repeated_headers(true)` /
  `--remove-pdf-headers` drops running headers and footers.
- **OCR**: `pdf_ocr(true)` / `--ocr` recognizes pages without text using
  installed Tesseract, or the Rust `ocrs` backend with local ONNX models.

Encrypted PDFs are rejected. Details, coordinates, OCR setup and limits:
[docs/pdf.md](docs/pdf.md).

## Agent tool

With the `tool` feature, `markkit::tool::MarkkitTool` is an LLM tool with
input/output JSON schemas, an OpenAI-style definition, a system prompt and a
validated `execute` (also a `tower::Service`). The same tool is available as
`MarkkitTool` in the WASM package and over MCP via `markkit mcp`. See
[`examples/agent_tool.rs`](examples/agent_tool.rs) and the
[tool contract](knowledge/integrations/tool-contract.md).

## Performance

Measured with `just bench` (in-process, release build, 20 iterations after 3 warmup) on a
4-CPU Linux x86_64 VM. Allocations and peak heap are per single extraction; the last column
is the 0.1.1 baseline. Full report: [`bench-vm-linux-x86_64-1793070312.md`](https://github.com/everruns/markkit/blob/main/crates/markkit-bench/results/bench-vm-linux-x86_64-1793070312.md); history in
[`crates/markkit-bench/results/`](https://github.com/everruns/markkit/blob/main/crates/markkit-bench/results/).

| Case | Input | Mean | Ops/s | Throughput | Allocs/op | Peak heap | Allocs / peak in 0.1.1 |
|------|------:|-----:|------:|-----------:|----------:|----------:|------:|
| HTML article (fixture) | 881 B | 54 µs | 18,375 | 15 MiB/s | 158 | 22 KiB | 213 / 28 KiB |
| HTML blog post with nav/scripts | 15 KiB | 690 µs | 1,449 | 22 MiB/s | 2,505 | 241 KiB | 2,427 / 304 KiB |
| HTML long docs page | 300 KiB | 15.1 ms | 66 | 19 MiB/s | 49,342 | 4.7 MiB | 47,578 / 7.5 MiB |
| HTML 50 tables | 257 KiB | 18.3 ms | 55 | 14 MiB/s | 33,635 | 5.1 MiB | 50,349 / 8.6 MiB |
| HTML mostly boilerplate | 45 KiB | 704 µs | 1,420 | 63 MiB/s | 66 | 252 KiB | 81 / 305 KiB |
| HTML 1000 nested divs | 11 KiB | 512 µs | 1,953 | 20 MiB/s | 551 | 52 KiB | 442 / 147 KiB |
| DOCX memo | 1.4 KiB | 75 µs | 13,401 | 18 MiB/s | 499 | 59 KiB | 733 / 93 KiB |
| DOCX 400-section report | 9 KiB (262 KiB out) | 22.3 ms | 45 | 0.4 MiB/s | 136,866 | 3.9 MiB | 265,791 / 18.5 MiB |
| XLSX 50×5 | 2.8 KiB | 301 µs | 3,321 | 9 MiB/s | 1,165 | 129 KiB | 1,668 / 129 KiB |
| XLSX 3 sheets, 150k cells | 766 KiB | 172 ms | 6 | 4 MiB/s | 512,786 | 21.5 MiB | 782,800 / 24.9 MiB |
| ODS 2000×8 | 70 KiB | 24.5 ms | 41 | 3 MiB/s | 72,498 | 3.0 MiB | 92,515 / 3.0 MiB |
| Format sniffing (DOCX) | 9 KiB | 0.9 µs | 1,076,774 | n/a | 0 | 0 B | 0 / 0 B |

Takeaways:

- **HTML** extracts at 14–22 MiB/s with peak heap 16–20x the input (was ~25x).
- **Deep nesting is linear**: 1000 nested `<div>`s take 0.5 ms (was 3.6 ms), and 64k nested
  `<div>`s take 2.4 ms (was 12.6 s).
- **Boilerplate-heavy pages are cheap**: scripts, styles and nav are dropped at ~60 MiB/s.
- **DOCX** streams the document body: a 400-section report peaks at 3.9 MiB (was 18.5 MiB) with
  half the allocations.
- **Spreadsheets** cost about 3.4 allocations per cell (was 5.2); one is calamine's and two are
  the cell's text and its table node, so peak heap now tracks the size of the output table.
- **Format detection** is effectively free (microseconds, no allocations).

## Security

Markkit treats every input as untrusted. HTML scripts, styles, forms, iframes
and SVG are stripped. Office containers are decompressed under measured byte
limits. Spreadsheet grids are pre-scanned so sparse cells cannot force huge
allocations. XML entities are never expanded. Every PDF stream is checked
against size limits before decoding. Macros and formulas are never executed.
Hidden sheets and hidden text are skipped by default.

`extract_url` accepts only http and https. It refuses loopback, private,
link-local and cloud-metadata addresses (also after DNS resolution and on
every redirect), caps redirects, and bounds the body by `max_input_size` and
the fetch by `timeout`. Opt in to internal hosts with
`allow_private_networks(true)`; restrict hosts with `allow_host(..)`.

See the [threat model](knowledge/security/threat-model.md). To report a
vulnerability, follow the [security policy](SECURITY.md).

## Documentation

- [Getting started](docs/getting-started.md): install, extract, configure, IR and provenance
- [PDF guide](docs/pdf.md): layout geometry, tables, OCR
- API reference: [docs.rs/markkit](https://docs.rs/markkit)
- Bindings: [Python](crates/markkit-python/README.md), [Node.js](crates/markkit-js/README.md), [WASM](crates/markkit-wasm/README.md)
- [Examples](examples/README.md)
- Design knowledge (architecture, IR, formats, threat model, processes): [`knowledge/`](knowledge/index.md)
- [Changelog](CHANGELOG.md)

## Development

```bash
just build      # Build workspace
just test       # Run all tests
just lint       # Run clippy
just examples   # Run every Rust example
just check      # fmt, clippy, tests, knowledge, examples
just pre-pr     # Full pre-PR validation
just bench      # Benchmarks, results saved to crates/markkit-bench/results
just eval-public-setup   # Pinned Mira host + isolated scorer environment
just eval-public-prepare # Download and verify WCXB + AgentDocBench inputs
just eval-public        # Release quality/timing run and regression gate
```

Public benchmark selections, scoring and baseline policy:
[`scripts/public_benchmarks/README.md`](scripts/public_benchmarks/README.md).

Coding agents: start with [AGENTS.md](AGENTS.md).

## Contributing

Bug reports, feature requests and questions are welcome as
[issues](https://github.com/everruns/markkit/issues). For code, see
[CONTRIBUTING.md](CONTRIBUTING.md) and the [code of conduct](CODE_OF_CONDUCT.md).

## Ecosystem

Markkit is part of [Everruns](https://everruns.com/projects/), a free, non-commercial
community ecosystem of open-source AI projects by
[Mykhailo Chalyi](https://chaliy.name) and contributors. Questions, ideas and
research discussion are welcome on [Discord](https://everruns.com/discord).

## License

MIT License. See [LICENSE](LICENSE). Third-party attributions, including data
embedded by PDF rendering, are in [NOTICE](NOTICE). Optional OCR model weights
fetched by `scripts/prepare_ocr_models.py` are separate CC-BY-SA 4.0 assets,
never bundled. Report vulnerabilities privately per [SECURITY.md](SECURITY.md).
