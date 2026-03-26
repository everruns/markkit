# Markkit

Extract clean markdown from any format.

Markkit is a Rust library and CLI tool for converting HTML, PDF, DOCX, spreadsheets, and other document formats into well-structured markdown. It provides native bindings for Python and Node.js.

## Features

- **HTML** – Convert web pages and HTML documents to markdown
- **PDF** – Extract text from PDF documents
- **DOCX** – Convert Word documents
- **Spreadsheets** – Convert Excel/ODS files to markdown tables
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
import markkit_python

markdown = await markkit_python.extract_html("<h1>Hello</h1>")
markdown = await markkit_python.extract_url("https://example.com")
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
| `http_client` | Yes | URL fetching via reqwest |
| `pdf` | No | PDF text extraction |
| `docx` | No | DOCX conversion |
| `spreadsheet` | No | Excel/ODS conversion |
| `all_formats` | No | Enable all format support |
| `logging` | No | Structured tracing output |

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
