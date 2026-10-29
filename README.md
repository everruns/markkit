# Markkit

[![CI](https://github.com/everruns/markkit/actions/workflows/ci.yml/badge.svg)](https://github.com/everruns/markkit/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/markkit.svg)](https://crates.io/crates/markkit)
[![docs.rs](https://img.shields.io/docsrs/markkit)](https://docs.rs/markkit)
[![PyPI](https://img.shields.io/pypi/v/markkit.svg)](https://pypi.org/project/markkit/)
[![npm](https://img.shields.io/npm/v/@everruns/markkit.svg)](https://www.npmjs.com/package/@everruns/markkit)
[![Repo: Agent Friendly](https://img.shields.io/badge/Repo-Agent%20Friendly-blue)](https://github.com/everruns/markkit/blob/main/AGENTS.md)

Extract clean markdown from any format. Written in Rust.

Markkit converts HTML, Word documents and spreadsheets into well-structured
markdown, built for feeding untrusted documents to LLMs and agents. It ships as
a Rust library, a CLI, native bindings for Python and Node.js, and a WebAssembly
package for browsers and edge runtimes.

## Features

- **HTML** – Web pages and HTML documents; nav, header, footer, scripts, styles and forms stripped by default
- **DOCX** – Headings, lists, tables, links, emphasis
- **PPTX** – One section per slide: titles, bullets, tables, links, speaker notes
- **ODT** – OpenDocument text: headings, lists, tables, links, emphasis
- **EPUB** – E-book chapters in reading order, title from the package metadata
- **Spreadsheets** – XLSX/XLSM, XLSB, XLS and ODS workbooks to markdown tables, one section per sheet
- **Email (EML)** – Subject, sender, recipients, date, body (plain preferred, HTML converted), attachment list
- **Jupyter notebooks** – Markdown cells, code cells with language, text outputs and tracebacks
- **CSV / TSV** – One markdown table; `;` and `|` delimiters detected, RFC 4180 quoting
- **Plain text / Markdown** – Text to paragraphs; Markdown normalized through the same IR (raw HTML never passed through)
- **Format detection** – Content type, then file extension, then magic bytes
- **URL fetching** – Fetch and extract in one call (`http_client` feature)
- **Configurable** – CSS include/exclude selectors, link/image preservation, resource limits
- **Safe by default** – No code execution (scripts, macros, formulas), bounded decompression,
  depth and size limits, [threat model](knowledge/security/threat-model.md) with a test per mitigation
- **PDF** – Text/layout, columns and tables; optional native OCR (encrypted PDFs rejected)

| Format | Status | Feature |
|---|---|---|
| HTML / XHTML | Supported | `html` (default) |
| DOCX | Supported | `docx` |
| PPTX, PPTM, PPSX | Supported | `pptx` |
| ODT | Supported | `odt` |
| EPUB | Supported | `epub` |
| XLSX, XLSM, XLSB, XLS, ODS | Supported | `spreadsheet` |
| Plain text | Supported | always on |
| CSV, TSV | Supported | always on |
| Jupyter notebooks (ipynb) | Supported | always on |
| Email (EML) | Supported | `email` |
| Markdown | Supported | `markdown` |
| PDF (text, layout and tables) | Supported | `pdf` |
| PDF OCR | Supported with Tesseract/language data | `pdf-ocr` |
| DOC, PPT | Detected, rejected explicitly | – |

### PDF source layout

```rust
let parsed = markkit::Markkit::builder().pdf_layout(true).build()
    .parse_document_sync(pdf_bytes, markkit::DocumentHint::file_name("report.pdf"))?;
let layout = parsed.layout.as_ref().unwrap(); // pages, dimensions, positioned text spans
```

CLI: `markkit extract --file report.pdf --json --layout`.
Node: `extractDocumentSync(bytes, { fileName: "report.pdf" }, { pdfLayout: true })`.
WASM: `new Markkit({ pdfLayout: true }).extractDocument(bytes, { fileName: "report.pdf" })`.

Layout v1 includes source runs and words, plus semantic blocks, lines and table
cells in reading order. Coordinates are display points from the cropped/rotated
page top-left (x right, y down). Source runs retain drawing order. Each block has
an ID, type, bounds and source span IDs; table cells include row/column positions,
rowspan/colspan and bounds. IR `Source.page` + `Source.layout_block` link blocks
when layout is requested. Font bounds approximate an em box; OCR bounds come from
the recognizer. Layout is omitted by default, and does not change Markdown.

PDF extraction reconstructs columns, handles ruled and borderless tables, preserves
bold/italic and numbered lists, and uses tagged-PDF heading roles and `ActualText`.
Ambiguous alignments remain prose. Enable repeated margin-header/footer suppression
with `.pdf_remove_repeated_headers(true)` or CLI `--remove-pdf-headers`.

Native OCR uses Rust Hayro rendering for pages without text. Install **Tesseract**
plus language data. Rust requires the `pdf-ocr` feature; the CLI,
Node and Python packages compile support in. WASM extracts existing text layers;
OCR helpers run in native hosts.

```bash
markkit extract --file scanned.pdf --ocr --ocr-language eng --timeout 300
```

Rust: `.pdf_ocr(true).pdf_ocr_language("eng+deu")`; Node: `{ pdfOcr: true,
pdfOcrLanguage: "eng", timeout: 300 }`. Python: `extract_document_sync(data,
file_name="report.pdf", pdf_layout=True, pdf_ocr=True, limits=Limits(timeout=300))`.
The same structured Python API returns metadata and table counts for all formats.

For Rust-only OCR, build with the optional `pdf-ocr-ocrs` feature and supply local
ONNX models. Tesseract remains the default backend:

```bash
python3 scripts/prepare_ocr_models.py  # verifies pinned model sizes and SHA-256
cargo build --release -p markkit-cli --features pdf-ocr-ocrs
target/release/markkit extract --file scanned.pdf --ocr --ocr-backend ocrs \
  --ocr-detection-model target/ocr-models/text-detection.onnx \
  --ocr-recognition-model target/ocr-models/text-recognition.onnx --timeout 300
```

Rust: `.pdf_ocr_backend(markkit::PdfOcrBackend::Ocrs)` plus
`.pdf_ocr_detection_model(path)` and `.pdf_ocr_recognition_model(path)`.
Extraction never downloads models. The pinned weights are CC-BY-SA 4.0, separate
from the MIT/Apache library code; the setup script writes their attribution and
license to `target/ocr-models/NOTICE.json`. The default model supports the Latin
alphabet; `ocrs` currently accepts `eng` only. Models are trusted deployment
assets. RTen inference checks deadlines between phases and batches; host quotas
provide hard CPU/memory limits. `RTEN_NUM_THREADS` and `RAYON_NUM_THREADS` configure
inference/postprocessing thread pools at process startup. Markkit's WASM binding
does not yet expose this native OCR path.

PDF geometry/rules count against `max_nodes`; retained geometry and analysis work
are bounded by `max_pdf_decoded_size`. Table grids obey row/column/grid limits.
Compact layout JSON and Markdown are independently bounded by `max_output_size`
(excluding JSON wrapper/pretty-print overhead), including parse-only callers.
Hayro checks image/raster budgets before rendering and the deadline before/after
its non-interruptible render step. Tesseract uses private files, output caps and
a killable deadline; no shell is used. OCR currently supports JPEG, JPEG2000 and
generic-filter images without DecodeParms. Inline, JBIG2 and CCITT OCR is refused
until bounded decoding is available. Host isolation provides hard CPU/memory quotas.

## Quick Start

### Rust Library

```rust
use markkit::Markkit;

let markkit = Markkit::builder().build();
let md = markkit.extract_html("<h1>Hello</h1><p>World</p>").await?;
```

### Documents from bytes

```rust
use markkit::{DocumentHint, Markkit};

let markkit = Markkit::builder().build();
// Detection order: content type, then file extension, then magic bytes.
let hint = DocumentHint { content_type: Some("application/octet-stream"), file_name: Some("report.docx") };
// CPU-bound: call the sync variant from `tokio::task::spawn_blocking` in async code.
let doc = markkit.extract_document_sync(&bytes, hint)?;
println!("{:?} {:?} {:?}", doc.format, doc.title, doc.warnings);
println!("{}", doc.markdown);
```

`Extraction` carries `markdown`, `title`, `format`, `warnings` (structured: `code` such as
`WarningCode::TruncatedSheet` plus a human-readable `detail`),
`stats` (sheets/rows/paragraphs/tables) and, for HTML, `metadata` (description, author,
published date, language, canonical URL, site name, lead image from `<meta>`, Open Graph
and JSON-LD). `extract_bytes` remains as a Markdown-only shortcut.

### CLI

```bash
# From a URL
markkit extract https://example.com

# From a file
markkit extract --file document.html

# With CSS selector filtering
markkit extract https://example.com --include "article" --exclude "nav,footer"

# JSON output with format, title and page metadata
markkit extract https://example.com --json

# Keep only the article body (drops sidebars, comments, share bars)
markkit extract https://example.com/post --main-content

# Resolve relative links in a saved page
markkit extract --file page.html --base-url https://example.com/blog/

# From stdin
cat page.html | markkit extract -

# Map site-specific markup with element rules (JSON file)
# [{"selector": ".note", "action": "block_quote", "title": "Note"}, {"selector": ".ad", "action": "skip"}]
markkit extract --file page.html --rules rules.json

# MCP server (stdio) exposing the `markkit` tool to agents
markkit mcp            # add --no-url to keep it offline
```

MCP client config: `{"mcpServers": {"markkit": {"command": "markkit", "args": ["mcp"]}}}`.

### Python

```python
import markkit

markdown = markkit.extract_html_sync("<h1>Hello</h1>")
markdown = await markkit.extract_url("https://example.com")
```

### Node.js

```javascript
const { extractHtml, extractUrl } = require('@everruns/markkit');

const fromHtml = await extractHtml('<h1>Hello</h1>');
const fromUrl = await extractUrl('https://example.com');
```

### Browser / edge (WebAssembly)

```javascript
import { initMarkkit, Markkit } from '@everruns/markkit-wasm';

await initMarkkit();
const md = new Markkit().extractHtml('<h1>Hello</h1>');
```

## Installation

### Rust

```toml
[dependencies]
markkit = "0.1"
```

### CLI

```bash
cargo install markkit-cli
```

### Python

```bash
pip install markkit
```

Wheels: Linux x64, CPython 3.12. Other platforms build from the sdist (needs Rust).

### Node.js

```bash
npm install @everruns/markkit
```

Native binary: Linux x64 (glibc).

### Browser / edge (WebAssembly)

```bash
npm install @everruns/markkit-wasm
```

Any JS runtime (browsers, Cloudflare Workers, Deno, Bun, Node); no special headers.
HTML, PDF text/layout, DOCX and spreadsheets; no URL fetching. See
[`crates/markkit-wasm/README.md`](crates/markkit-wasm/README.md).

## Configuration

```rust
let markkit = Markkit::builder()
    .max_input_size(10 * 1024 * 1024)  // 10MB limit
    .max_processing_secs(30)
    .preserve_links(true)
    .preserve_images(false)
    .include_selector("article")
    .exclude_selector("nav")
    .exclude_selector(".sidebar")
    .build();
```

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `html` | Yes | HTML parsing and conversion |
| `http_client` | No | URL fetching via reqwest (`extract_url`) |
| `docx` | No | DOCX conversion (zip + quick-xml) |
| `pptx` | No | PPTX conversion (zip + quick-xml) |
| `spreadsheet` | No | XLSX/XLSM, XLS, ODS conversion (calamine) |
| `pdf` | No | PDF text/layout, reading order and tables (pure Rust) |
| `pdf-ocr` | No | Native OCR: Rust Hayro rendering + installed Tesseract/language data |
| `pdf-ocr-ocrs` | No | Optional Rust OCR via ocrs/RTen and caller-supplied local ONNX models |
| `markdown` | No | Markdown normalization (pulldown-cmark) |
| `email` | No | EML conversion (mail-parser) |
| `odt` | No | ODT conversion (zip + quick-xml) |
| `epub` | No | EPUB conversion (implies `html`) |
| `all_formats` | No | `html`, `pdf`, `docx`, `pptx`, `spreadsheet`, `markdown`, `email`, `odt`, `epub` |
| `tool` | No | `markkit::tool::MarkkitTool` LLM agent tool (Everruns toolkit contract) |
| `logging` | No | Reserved: pulls in `tracing`, emits no events yet |

`http_client` is opt-in: the default build has no HTTP/TLS stack and no tokio in the dependency
tree.
Enable it to get `extract_url` (the CLI and the Python/Node bindings ship with it on):

```toml
[dependencies]
markkit = { version = "0.1", features = ["http_client"] }
# office formats only, no HTML, no network:
# markkit = { version = "0.1", default-features = false, features = ["docx", "pptx", "spreadsheet"] }
```

### Resource limits

Every resource an input can drive is bounded by `markkit::Limits`. Defaults are
generous; violations return `Error::ResourceLimit`, whose `LimitKind` (`err.limit_kind()`)
names the field that was exceeded.

| Limit | Default | Bounds |
|-------|---------|--------|
| `max_input_size` | 50 MiB | Input bytes, fetched URL body |
| `max_output_size` | 128 MiB | Markdown output |
| `timeout` | 60 s | Fetch + parse + render, per call |
| `max_nesting_depth` | 256 (hard max 512) | HTML flattens deeper content; deeper DOCX XML is rejected |
| `max_nodes` | 5 000 000 | HTML DOM nodes, DOCX XML nodes; retained PDF layout pages/runs |
| `max_archive_entries` | 10 000 | ZIP entries (DOCX/XLSX/ODS) |
| `max_archive_entry_size` | 64 MiB | Decompressed bytes per ZIP entry |
| `max_archive_total_size` | 256 MiB | Decompressed bytes per ZIP |
| `max_spreadsheet_cells` | 4 000 000 | XLS/ODS grid before parsing |
| `max_sheet_rows` / `max_sheet_cols` | 10 000 / 256 | Rendered per sheet (truncated with a warning) |
| `max_pdf_pages` | 10 000 | PDF pages rendered (truncated with a warning) |
| `max_pdf_stream_size` | 64 MiB | Decoded bytes per PDF stream |
| `max_pdf_decoded_size` | 256 MiB | Decoded PDF stream bytes in total, and PDF content interpreted |
| `max_redirects` | 5 | `extract_url` redirects |

```rust
use markkit::{Limits, Markkit};
use std::time::Duration;

let markkit = Markkit::builder()
    .limits(Limits::default().timeout(Duration::from_secs(10)).max_output_size(8 << 20))
    .build();
```

Each limit also has a builder setter (`.max_nodes(..)`, `.timeout(..)`, ...), a CLI
flag (`--max-nodes`, `--timeout`, ...; see `markkit extract --help`), a
`markkit.Limits(...)` argument in Python and flat option keys in Node.js and WASM.
Other options: `include_hidden_sheets`, `allow_private_networks`, `allow_host`.

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

Markkit treats every input as untrusted. HTML scripts, styles, forms, iframes and
SVG are stripped; office containers are decompressed under measured byte limits;
spreadsheet grids are pre-scanned so sparse cells cannot force huge allocations;
XML entities are never expanded; every PDF stream is decode-checked against size
limits before parsing; macros and formulas are never executed; hidden
sheets are skipped by default. `extract_url` accepts only http/https, refuses
loopback, private, link-local and cloud-metadata addresses (also after DNS
resolution and on every redirect), caps redirects, and bounds the body by
`max_input_size` and the fetch by `timeout`. Opt in to internal
hosts with `allow_private_networks(true)`; restrict hosts with `allow_host(..)`.

See the [threat model](knowledge/security/threat-model.md) and the
[security policy](SECURITY.md) for reporting vulnerabilities.

## Documentation

- [Getting started](docs/getting-started.md)
- API reference: [docs.rs/markkit](https://docs.rs/markkit)
- Design knowledge (architecture, IR, formats, threat model, processes): [`knowledge/`](knowledge/index.md)
- [Examples](examples/): `cargo run -p markkit --example basic`

## Development

```bash
just build      # Build workspace
just test       # Run all tests
just lint       # Run clippy
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
[CONTRIBUTING.md](CONTRIBUTING.md).

## Ecosystem

Markkit is part of the [Everruns](https://everruns.com) ecosystem, alongside
[Bashkit](https://github.com/everruns/bashkit).

## License

MIT License. See [LICENSE](LICENSE) for details.
