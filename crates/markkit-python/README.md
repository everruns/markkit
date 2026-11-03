# markkit for Python

[![PyPI](https://img.shields.io/pypi/v/markkit.svg)](https://pypi.org/project/markkit/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/everruns/markkit/blob/main/LICENSE)

**Extract clean Markdown from any document, safely.** Native Python bindings
for [Markkit](https://github.com/everruns/markkit), written in Rust.

Convert HTML, PDF, Word, PowerPoint, Excel/ODS spreadsheets, ODT, EPUB, email
(EML), Jupyter notebooks, CSV/TSV, Markdown and plain text into well-structured
Markdown for LLMs, RAG pipelines and agents. Markkit is built for untrusted
input: nothing is executed, and every resource a document can drive (size,
time, nesting, nodes, archives, PDF streams) is bounded.

- One call for every format, detected from file name, content type or bytes
- Title, metadata, stats and structured warnings with every result
- Main-content detection for web pages, CSS include/exclude selectors
- PDF reading order, columns, tables, optional layout geometry and OCR
- URL fetching with SSRF protection (private and metadata addresses refused)
- Fully typed (`.pyi` stubs + `py.typed`), sync and `asyncio` APIs

## Install

```bash
pip install markkit
```

Wheels: Linux x86_64 (manylinux2014), CPython 3.12. Other platforms install
from the source distribution, which needs a Rust toolchain (1.99+). OCR additionally needs
[Tesseract](https://tesseract-ocr.github.io/) and language data installed.

## Quick start

```python
import markkit

print(markkit.extract_html_sync("<h1>Hello</h1><p>World</p>"))
# # Hello
#
# World

with open("report.docx", "rb") as f:
    result = markkit.extract_document_sync(f.read(), file_name="report.docx")

print(result["format"])    # "docx"
print(result["title"])     # "Q3 Report"
print(result["stats"])     # {"paragraphs": 7, "tables": 1, ...}
print(result["markdown"])
```

## API

| Function | Returns | Use it for |
|---|---|---|
| `extract_document_sync(data, *, file_name=None, content_type=None, pdf_layout=False, pdf_remove_repeated_headers=False, pdf_ocr=False, pdf_ocr_language="eng", pdf_ocr_backend="tesseract", pdf_ocr_detection_model=None, pdf_ocr_recognition_model=None, limits=None)` | `dict` | Any format, with metadata |
| `extract_html_sync(html, *, limits=None)` | `str` | HTML string to Markdown |
| `extract_html_sync_with_options(html, *, include_selectors=None, exclude_selectors=None, max_input_size=None, base_url=None, main_content=False, limits=None)` | `str` | HTML with selectors, link resolution, main content |
| `await extract_html(html, *, limits=None)` | `str` | Async HTML |
| `await extract_bytes(data, content_type=None, *, limits=None)` | `str` | Async bytes to Markdown only |
| `await extract_url(url, *, limits=None)` | `str` | Fetch and convert a URL |
| `Limits(...)` | `Limits` | Resource limits for any call |

### Documents

`extract_document_sync` detects the format from `content_type`, then the
extension of `file_name`, then magic bytes, and returns a dict:

| Key | Type | Contents |
|---|---|---|
| `markdown` | `str` | The Markdown |
| `title` | `str \| None` | Title when the source declares one |
| `format` | `str` | `html`, `pdf`, `docx`, `pptx`, `xlsx`, `xlsb`, `xls`, `ods`, `odt`, `epub`, `eml`, `ipynb`, `csv`, `tsv`, `markdown`, `text` |
| `warnings` | `list[dict]` | `{"code": "truncated_sheet", "detail": "..."}`; match on `code` |
| `stats` | `dict` | `pages`, `sheets`, `rows`, `paragraphs`, `tables`, ... (`None` when not applicable) |
| `metadata` | `dict` | HTML `description`, `author`, `published`, `language`, `canonical_url`, `site_name`, `image` |
| `layout` | `dict` | PDF geometry, only with `pdf_layout=True` |

```python
import markkit

def convert(path: str) -> str:
    with open(path, "rb") as f:
        result = markkit.extract_document_sync(f.read(), file_name=path)
    for w in result["warnings"]:
        print(f"warning [{w['code']}]: {w['detail']}")
    return result["markdown"]
```

From an HTTP response, pass the content type:
`extract_document_sync(resp.content, content_type=resp.headers["content-type"])`.

### Web pages

```python
md = markkit.extract_html_sync_with_options(
    html,
    main_content=True,                  # keep the article, drop sidebars and comments
    include_selectors=["main"],         # keep only matches
    exclude_selectors=[".newsletter"],  # drop matches (on top of nav/footer/script/...)
    base_url="https://example.com/blog/",  # resolve relative links and images
)
```

### Async

The async functions return awaitables backed by a native Tokio runtime, so
they do not block the event loop.

```python
import asyncio
import markkit

async def main():
    md = await markkit.extract_url("https://example.com")
    md2 = await markkit.extract_bytes(b"a,b\n1,2\n", "text/csv")

asyncio.run(main())
```

`extract_url` accepts only http and https. It refuses loopback, private,
link-local and cloud-metadata addresses, also after DNS resolution and on every
redirect.

## Limits and errors

Every function accepts `limits=markkit.Limits(...)`. All fields are optional
keyword arguments with generous defaults:

| Field | Default | Bounds |
|---|---|---|
| `max_input_size` | 50 MiB | Input bytes, fetched body |
| `max_output_size` | 128 MiB | Markdown output bytes |
| `timeout` | 60.0 | Seconds per call (fetch + parse + render) |
| `max_nesting_depth` | 256 | Element nesting (deeper HTML is flattened; hard max 512) |
| `max_nodes` | 5 000 000 | Parsed nodes, retained PDF geometry |
| `max_archive_entries` | 10 000 | ZIP entries (DOCX, PPTX, XLSX, ODT, EPUB) |
| `max_archive_entry_size` / `max_archive_total_size` | 64 / 256 MiB | Decompressed ZIP bytes |
| `max_sheet_rows` / `max_sheet_cols` | 10 000 / 256 | Rendered per sheet (truncated with a warning) |
| `max_spreadsheet_cells` | 4 000 000 | XLS/ODS grid before parsing |
| `max_redirects` | 5 | `extract_url` redirects |
| `max_pdf_pages` | 10 000 | PDF pages (truncated with a warning) |
| `max_pdf_stream_size` / `max_pdf_decoded_size` | 64 / 256 MiB | Decoded PDF bytes |

```python
limits = markkit.Limits(max_input_size=10 << 20, max_output_size=1 << 20, timeout=10.0)
try:
    md = markkit.extract_html_sync(html, limits=limits)
except RuntimeError as err:
    print(err)  # "Resource limit exceeded: output exceeds max_output_size 1048576"
```

Errors raise `RuntimeError` with a message naming the cause: an exceeded limit,
an unsupported format (legacy `.doc`/`.ppt`, encrypted PDF), a parse failure,
a blocked URL or an HTTP failure. Invalid arguments (for example an unknown OCR
backend) raise `ValueError`.

## PDF layout and OCR

```python
result = markkit.extract_document_sync(
    pdf_bytes, file_name="report.pdf",
    pdf_layout=True,                   # add page geometry
    pdf_remove_repeated_headers=True,  # drop running headers/footers
)
page = result["layout"]["pages"][0]
for block in page["blocks"]:           # reading order
    print(block["kind"], block["bbox"])  # "table", [left, top, right, bottom]
```

PDF extraction rebuilds reading order and columns, ruled and borderless tables
(with merged and wrapped cells), emphasis, numbered lists and tagged headings.
Layout adds source `spans` (text runs with `bbox`, `baseline`, `font`, `size`,
`words`) and semantic `blocks` (`kind`, `bbox`, `span_ids`, `lines`, table
`cells`). Coordinates are points from the page's top-left, x right and y down.

Scanned pages, with Tesseract and language data installed:

```python
result = markkit.extract_document_sync(
    pdf_bytes, file_name="scan.pdf",
    pdf_ocr=True, pdf_ocr_language="eng+deu",
    limits=markkit.Limits(timeout=300),
)
```

OCR is opt-in and only processes pages without text. Source builds can also
enable the Rust `ocrs` backend (`maturin build --features pdf-ocr-ocrs`) with
`pdf_ocr_backend="ocrs"` plus local `pdf_ocr_detection_model` and
`pdf_ocr_recognition_model` paths; extraction never downloads models. See the
[PDF guide](https://github.com/everruns/markkit/blob/main/docs/pdf.md).

## Examples

Runnable scripts in
[`examples/python/`](https://github.com/everruns/markkit/tree/main/examples/python):
`quickstart.py` (HTML, options, async), `documents.py` (DOCX, XLSX, PDF with
stats and layout; pass your own files as arguments) and `limits.py`.

## Packaging

The PyPI distribution and import are both `markkit`. Wheels are built with
maturin and published with PyPI Trusted Publishing (OIDC). Before upload, the
publisher verifies wheel metadata, installs the wheel into a clean virtual
environment and runs a native extraction. The source distribution keeps the
full Cargo workspace and lockfile.

## Links

- [Source and issues](https://github.com/everruns/markkit)
- [Changelog](https://github.com/everruns/markkit/blob/main/CHANGELOG.md)
- [Security model](https://github.com/everruns/markkit/blob/main/knowledge/security/threat-model.md)
- Also available as a [Rust crate](https://crates.io/crates/markkit), a
  [CLI](https://crates.io/crates/markkit-cli), a native
  [npm package](https://www.npmjs.com/package/@everruns/markkit) and a
  [WebAssembly package](https://www.npmjs.com/package/@everruns/markkit-wasm)

## License

MIT. See [LICENSE](https://github.com/everruns/markkit/blob/main/LICENSE) and
[NOTICE](https://github.com/everruns/markkit/blob/main/NOTICE). Optional OCR
model weights are separate CC-BY-SA 4.0 assets, never bundled.
