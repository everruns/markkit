# Markkit for Python

Native Python bindings for Markkit HTML-to-Markdown extraction.

The PyPI distribution and Python import are both named `markkit`
(`pip install markkit`, then `import markkit`). The Rust crate remains
`markkit-python`.

The current wheel publisher builds for **Linux x64 and CPython 3.12**. It does not
publish macOS, Windows, ARM64 or other Python-version wheels. A source
distribution is also published; source builds require Rust and native build
tools and are validated with Rust 1.99. Other wheel targets require a separate
future release; Rust and npm availability is independent.

```python
import markkit

print(markkit.extract_html_sync("<h1>Hello</h1>"))
# # Hello

# Resource limits (all optional, generous defaults); breaches raise RuntimeError
limits = markkit.Limits(timeout=10.0, max_output_size=8 << 20, max_nesting_depth=128)
markkit.extract_html_sync("<h1>Hello</h1>", limits=limits)
```

`Limits` fields: `max_input_size`, `max_output_size`, `timeout` (seconds),
`max_nesting_depth`, `max_nodes`, `max_archive_entries`, `max_archive_entry_size`,
`max_archive_total_size`, `max_sheet_rows`, `max_sheet_cols`,
`max_spreadsheet_cells`, `max_redirects`, `max_pdf_pages`, `max_pdf_stream_size`,
`max_pdf_decoded_size`. Every function accepts `limits=`.

Publication verifies wheel metadata, installs the actual wheel into a clean
virtual environment and exercises native extraction before upload.

The manual publisher uses PyPI Trusted Publishing (OIDC), without a stored
PyPI API token. A dry run builds and verifies distributions without publishing.

Python packaging is configured by the workspace-root `pyproject.toml`. Source
archives retain the complete Cargo workspace and its unchanged security lockfile.

## Structured documents, PDF layout and OCR

```python
from markkit import extract_document_sync, Limits
result = extract_document_sync(
    pdf_bytes, file_name="report.pdf", pdf_layout=True,
    pdf_remove_repeated_headers=True,
)
print(result["markdown"], result["stats"]["tables"])
print(result["layout"]["pages"][0]["blocks"])
```

PDF extraction reconstructs reading order, columns and ruled/borderless tables,
with merged/wrapped cells, emphasis, numbered lists and tagged headings/ActualText.
Layout includes source runs/words and semantic block/line/cell geometry. Keys are
snake_case; coordinates are display points from the cropped/rotated top-left.

For image-only pages, install Tesseract and your language data:
`extract_document_sync(pdf_bytes, file_name="scan.pdf", pdf_ocr=True,
pdf_ocr_language="eng", pdf_layout=True, limits=Limits(timeout=300))`.
OCR is opt-in and only processes pages without text. Resource limits also bound
geometry, grid expansion, raster dimensions/helper files and process deadlines.

## Optional Rust OCR backend

Source builds can enable Cargo feature `pdf-ocr-ocrs` (for example,
`maturin build --features pdf-ocr-ocrs`). Default wheels retain Tesseract.
Prepare the repository's pinned model pair with
`python3 scripts/prepare_ocr_models.py`, then use:

```python
markkit.extract_document_sync(
    data, file_name="scan.pdf", pdf_ocr=True, pdf_ocr_backend="ocrs",
    pdf_ocr_detection_model="/models/text-detection.onnx",
    pdf_ocr_recognition_model="/models/text-recognition.onnx",
    limits=markkit.Limits(timeout=300),
)
```

Extraction never downloads models. The pinned weights are CC-BY-SA 4.0 assets
with attribution in the setup script's NOTICE.json. The current Latin model
accepts `pdf_ocr_language="eng"` only. Model files are trusted deployment assets;
inference deadlines are cooperative and hard CPU/memory limits require host
quotas. Invalid backend names raise ValueError; missing features/models and
unsupported languages raise RuntimeError.
