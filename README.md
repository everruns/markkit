# Markkit

Extract clean markdown from any format.

Markkit is a Rust library and CLI tool for converting HTML, PDF, DOCX, spreadsheets, and other document formats into well-structured markdown. It provides native bindings for Python and Node.js.

## Features

- **HTML** – Convert web pages and HTML documents to markdown
- **DOCX** – Convert Word documents (headings, lists, tables, links, emphasis)
- **Spreadsheets** – Convert XLSX/XLSM, XLS and ODS workbooks to markdown tables
- **PDF** – Planned (currently reported as unsupported)
- **URL fetching** – Fetch and extract from URLs directly
- **Configurable** – CSS selectors for include/exclude, resource limits
- **Safe** – Input sanitization, resource limits, no code execution

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

`Extraction` carries `markdown`, `title`, `format`, `warnings` (e.g. `truncated_sheet: …`)
and `stats` (sheets/rows/paragraphs/tables). `extract_bytes` remains as a Markdown-only
shortcut.

### CLI

```bash
# From a URL
markkit extract https://example.com

# From a file
markkit extract --file document.html

# With CSS selector filtering
markkit extract https://example.com --include "article" --exclude "nav,footer"

# JSON output with metadata
markkit extract https://example.com --json
```

### Python

```python
import markkit

markdown = await markkit.extract_html("<h1>Hello</h1>")
markdown = await markkit.extract_url("https://example.com")
```

### Node.js

```javascript
const { extractHtml, extractUrl } = require('@everruns/markkit');

const markdown = await extractHtml('<h1>Hello</h1>');
const markdown = await extractUrl('https://example.com');
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

### Node.js

```bash
npm install @everruns/markkit
```

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
| `logging` | No | Structured tracing output |

`http_client` is optional. Embedders that fetch content themselves can build without
reqwest (no HTTP/TLS stack in the dependency tree):

```toml
[dependencies]
markkit = { version = "0.1", default-features = false, features = ["docx", "spreadsheet"] }
# add "html" if you also convert HTML
```

Office-document limits are configurable on the builder: `max_archive_entries`,
`max_archive_entry_size`, `max_archive_total_size`, `max_spreadsheet_cells`,
`max_sheet_rows`, `max_sheet_cols`, `include_hidden_sheets`.

## Development

```bash
just build     # Build workspace
just test      # Run all tests
just lint      # Run clippy
just pre-pr    # Full pre-PR validation
just bench     # Run benchmarks
```

## License

MIT License. See [LICENSE](LICENSE) for details.
