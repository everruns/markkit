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
a Rust library, a CLI, and native bindings for Python and Node.js.

## Features

- **HTML** – Web pages and HTML documents; nav, header, footer, scripts, styles and forms stripped by default
- **DOCX** – Headings, lists, tables, links, emphasis
- **Spreadsheets** – XLSX/XLSM, XLS and ODS workbooks to markdown tables, one section per sheet
- **Format detection** – Content type, then file extension, then magic bytes
- **URL fetching** – Fetch and extract in one call (`http_client` feature)
- **Configurable** – CSS include/exclude selectors, link/image preservation, resource limits
- **Safe by default** – No code execution (scripts, macros, formulas), bounded decompression,
  depth and size limits, [threat model](knowledge/security/threat-model.md) with a test per mitigation
- **PDF** – Planned (currently an explicit `UnsupportedFormat` error, never an empty success)

| Format | Status | Feature |
|---|---|---|
| HTML / XHTML | Supported | `html` (default) |
| DOCX | Supported | `docx` |
| XLSX, XLSM, XLS, ODS | Supported | `spreadsheet` |
| PDF | Planned | `pdf` |
| DOC, PPTX, XLSB | Detected, rejected explicitly | – |

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

`Extraction` carries `markdown`, `title`, `format`, `warnings` (e.g. `truncated_sheet: …`),
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

# JSON output; files and stdin also carry format, title and page metadata
markkit extract --file page.html --json

# From stdin
cat page.html | markkit extract -
```

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
| `http_client` | Yes | URL fetching via reqwest (`extract_url`) |
| `docx` | No | DOCX conversion (zip + quick-xml) |
| `spreadsheet` | No | XLSX/XLSM, XLS, ODS conversion (calamine) |
| `pdf` | No | Placeholder; PDF currently returns `UnsupportedFormat` |
| `all_formats` | No | `html`, `pdf`, `docx`, `spreadsheet` |
| `logging` | No | Reserved: pulls in `tracing`, emits no events yet |

`http_client` is optional. Embedders that fetch content themselves can build without
reqwest (no HTTP/TLS stack in the dependency tree):

```toml
[dependencies]
markkit = { version = "0.1", default-features = false, features = ["docx", "spreadsheet"] }
# add "html" if you also convert HTML
```

Office-document limits are configurable on the builder: `max_archive_entries`,
`max_archive_entry_size`, `max_archive_total_size`, `max_spreadsheet_cells`,
`max_sheet_rows`, `max_sheet_cols`, `include_hidden_sheets`. URL fetching:
`allow_private_networks`, `allow_host`, `max_redirects`.

## Performance

Measured with `just bench` (in-process, release build, 20 iterations after 3 warmup) on a
4-CPU Linux x86_64 VM, markkit 0.1.1. Allocations and peak heap are per single extraction.
Full report: [`bench-vm-linux-x86_64-1793058767.md`](https://github.com/everruns/markkit/blob/main/crates/markkit-bench/results/bench-vm-linux-x86_64-1793058767.md); history in
[`crates/markkit-bench/results/`](https://github.com/everruns/markkit/blob/main/crates/markkit-bench/results/).

| Case | Input | Mean | Ops/s | Throughput | Allocs/op | Peak heap |
|------|------:|-----:|------:|-----------:|----------:|----------:|
| HTML article (fixture) | 881 B | 49 µs | 20,365 | 17 MiB/s | 213 | 28 KiB |
| HTML blog post with nav/scripts | 15 KiB | 641 µs | 1,561 | 23 MiB/s | 2,427 | 304 KiB |
| HTML long docs page | 300 KiB | 13.0 ms | 77 | 22 MiB/s | 47,578 | 7.5 MiB |
| HTML 50 tables | 257 KiB | 15.0 ms | 67 | 17 MiB/s | 50,349 | 8.6 MiB |
| HTML mostly boilerplate | 45 KiB | 706 µs | 1,417 | 63 MiB/s | 81 | 305 KiB |
| HTML 1000 nested divs | 11 KiB | 3.6 ms | 282 | 3 MiB/s | 442 | 147 KiB |
| DOCX memo | 1.4 KiB | 87 µs | 11,508 | 16 MiB/s | 733 | 93 KiB |
| DOCX 400-section report | 9 KiB (262 KiB out) | 35.7 ms | 28 | 0.2 MiB/s | 265,791 | 18.5 MiB |
| XLSX 50×5 | 2.8 KiB | 291 µs | 3,435 | 9 MiB/s | 1,668 | 129 KiB |
| XLSX 3 sheets, 150k cells | 766 KiB | 172 ms | 6 | 4 MiB/s | 782,800 | 24.9 MiB |
| ODS 2000×8 | 70 KiB | 23.2 ms | 43 | 3 MiB/s | 92,515 | 3.0 MiB |
| Format sniffing (DOCX) | 9 KiB | 0.9 µs | 1,141,227 | n/a | 0 | 0 B |

Takeaways:

- **HTML** extracts at 17–25 MiB/s; peak heap is ~25x the input size.
- **Boilerplate-heavy pages are cheap**: scripts, styles and nav are dropped at ~60 MiB/s.
- **Deep nesting is the slow outlier**: 1000 nested `<div>`s run at 3 MiB/s with few
  allocations, so the cost is CPU in tree building, not memory.
- **Office formats allocate most**: a 150k-cell XLSX takes 172 ms and ~5 allocations per cell;
  a 9 KiB compressed DOCX peaks at 18.5 MiB of heap (~2000x its compressed size).
- **Format detection** is effectively free (under 1 µs, no allocations).

Timings on a shared VM are noisy (stddev up to ~70% on the smallest cases); allocation counts
are deterministic and the better regression signal. Compare runs from the same machine only.

## Security

Markkit treats every input as untrusted. HTML scripts, styles, forms, iframes and
SVG are stripped; office containers are decompressed under measured byte limits;
spreadsheet grids are pre-scanned so sparse cells cannot force huge allocations;
XML entities are never expanded; macros and formulas are never executed; hidden
sheets are skipped by default. `extract_url` accepts only http/https, refuses
loopback, private, link-local and cloud-metadata addresses (also after DNS
resolution and on every redirect), caps redirects, and bounds the body by
`max_input_size` and the fetch by `max_processing_secs`. Opt in to internal
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
```

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
