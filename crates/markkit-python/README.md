# markkit

Extract clean Markdown from HTML, PDF, Office documents, spreadsheets, e-books,
email and notebooks. Built for feeding untrusted documents to LLMs and agents.
Native Rust core, no runtime dependencies.

```bash
pip install markkit
```

```python
import markkit

markkit.extract_html_sync("<h1>Hello</h1><p>World</p>")
# '# Hello\n\nWorld\n'
```

## Supported formats

HTML, PDF (text, layout, tables, optional OCR), DOCX, PPTX, ODT, EPUB,
XLSX/XLSM/XLSB/XLS/ODS, CSV/TSV, Jupyter notebooks, email (EML), Markdown and
plain text. The format is detected from the content type, then the file name,
then the bytes.

## Documents

`extract_document_sync` takes raw bytes and returns a dict with the Markdown and
metadata:

```python
import markkit

with open("report.docx", "rb") as f:
    doc = markkit.extract_document_sync(f.read(), file_name="report.docx")

doc["markdown"]  # extracted Markdown
doc["title"]     # document title, or None
doc["format"]    # detected format, e.g. "docx"
doc["warnings"]  # list of {"code": ..., "detail": ...}
```

## HTML options

```python
markkit.extract_html_sync_with_options(
    html,
    include_selectors=["article"],    # keep only these
    exclude_selectors=[".ad", "nav"], # drop these (added to the defaults)
    main_content=True,                # keep only the detected article body
    base_url="https://example.com/",  # resolve relative links and images
)
```

## Async

`extract_html`, `extract_bytes` and `extract_url` are coroutines:

```python
import asyncio
import markkit

async def main():
    md = await markkit.extract_url("https://example.com")
    pdf = await markkit.extract_bytes(open("paper.pdf", "rb").read(), "application/pdf")

asyncio.run(main())
```

`extract_url` blocks private and loopback addresses and bounds redirects, body
size and time.

## Resource limits

All inputs are treated as untrusted. Every function accepts `limits=`, and every
limit has a safe default. A breach raises `RuntimeError`.

```python
limits = markkit.Limits(
    timeout=10.0,                    # seconds
    max_output_size=8 * 1024 * 1024, # bytes
    max_nesting_depth=128,
)
markkit.extract_html_sync(html, limits=limits)
```

Fields: `max_input_size`, `max_output_size`, `timeout`, `max_nesting_depth`,
`max_nodes`, `max_archive_entries`, `max_archive_entry_size`,
`max_archive_total_size`, `max_sheet_rows`, `max_sheet_cols`,
`max_spreadsheet_cells`, `max_redirects`, `max_pdf_pages`,
`max_pdf_stream_size`, `max_pdf_decoded_size`.

## PDF layout and OCR

```python
result = markkit.extract_document_sync(
    pdf_bytes, file_name="report.pdf",
    pdf_layout=True, pdf_remove_repeated_headers=True,
)
result["markdown"]
result["layout"]["pages"][0]["blocks"]  # positioned blocks, lines and table cells
```

PDF extraction reconstructs reading order, columns and tables (ruled and
borderless, with merged cells). Layout keys are snake_case; coordinates are
points from the top-left of the cropped, rotated page.

For scanned pages, install [Tesseract](https://github.com/tesseract-ocr/tesseract)
with your language data and pass `pdf_ocr=True`:

```python
markkit.extract_document_sync(
    pdf_bytes, file_name="scan.pdf",
    pdf_ocr=True, pdf_ocr_language="eng",
    limits=markkit.Limits(timeout=300),
)
```

OCR is opt-in, runs locally and only touches pages without text.

<details>
<summary>Rust OCR backend (source builds)</summary>

Build from source with the `pdf-ocr-ocrs` Cargo feature
(`maturin build --features pdf-ocr-ocrs`), prepare the pinned model pair with
`python3 scripts/prepare_ocr_models.py`, then pass
`pdf_ocr_backend="ocrs"`, `pdf_ocr_detection_model=...` and
`pdf_ocr_recognition_model=...`. Extraction never downloads models. The pinned
weights are CC-BY-SA 4.0; only `pdf_ocr_language="eng"` is supported. An unknown
backend raises `ValueError`; a missing feature or model raises `RuntimeError`.

</details>

## Platforms

Wheels are published for **Linux x86_64 (manylinux2014), CPython 3.12**. On other
platforms pip builds from the source distribution, which needs a Rust toolchain
(1.99+).

## Links

- [Source and issues](https://github.com/everruns/markkit)
- [Changelog](https://github.com/everruns/markkit/blob/main/CHANGELOG.md)
- [Security model](https://github.com/everruns/markkit/blob/main/knowledge/security/threat-model.md)
- Also available as a [Rust crate](https://crates.io/crates/markkit), a
  [CLI](https://crates.io/crates/markkit-cli) and an
  [npm package](https://www.npmjs.com/package/@everruns/markkit)

MIT licensed; see [LICENSE](https://github.com/everruns/markkit/blob/main/LICENSE). Third-party notices are in
[NOTICE](https://github.com/everruns/markkit/blob/main/NOTICE).
