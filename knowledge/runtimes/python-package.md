---
type: Package Design
title: Python Package
description: PyO3 Python bindings, exported API, maturin build, and PyPI distribution.
tags:
  - markkit
  - python
  - bindings
---

# Python Package

## Overview

`crates/markkit-python` exposes Markkit as the `markkit` Python module via PyO3
(`#[pyo3(name = "markkit")]`, `module-name = "markkit"` in the root `pyproject.toml`).
The PyPI distribution is also named `markkit`.

## API

```python
import markkit

# Async
markdown = await markkit.extract_html("<h1>Hello</h1>")
markdown = await markkit.extract_url("https://example.com")
markdown = await markkit.extract_bytes(b"<h1>Hello</h1>", "text/html")

# Sync
markdown = markkit.extract_html_sync("<h1>Hello</h1>")
markdown = markkit.extract_html_sync_with_options(
    html,
    include_selectors=["article"],
    exclude_selectors=["nav"],
    max_input_size=1_000_000,
    base_url="https://example.com/docs/",
    main_content=True,
)

# Resource limits: every function takes keyword-only `limits=`
limits = markkit.Limits(timeout=10.0, max_output_size=8 << 20)
markdown = markkit.extract_html_sync(html, limits=limits)
```

`markkit.Limits` mirrors `markkit::Limits` field for field (`timeout` in float
seconds); unset arguments keep library defaults and are readable as attributes.
Invalid values raise `ValueError` / `OverflowError` at construction.

Errors surface as `RuntimeError` carrying the Rust error's `Display` text.

## Build

`maturin` builds from the workspace root (`pyproject.toml` points at the crate manifest):

```bash
maturin develop   # local development
maturin build     # wheel
```

The sdist uses `sdist-generator = "git"` and `locked = true` so the workspace
`Cargo.lock` (with its security patches) ships intact.

## Distribution

Published to PyPI by `publish-python.yml` from a verified release tag; see
[Release Process](../operations/release-process.md) for platform scope.
CI: `python.yml` builds and imports the module on changes to `crates/markkit-python/`
or `crates/markkit/`.

## Structured documents

`extract_document_sync(data, *, file_name=None, content_type=None, pdf_layout=False,
pdf_remove_repeated_headers=False, pdf_ocr=False, pdf_ocr_language="eng",
pdf_ocr_backend="tesseract", pdf_ocr_detection_model=None,
pdf_ocr_recognition_model=None, limits=None)`
returns a Python dictionary with Markdown, metadata, statistics and optional layout.
PDF tables/reading order are built in. Native OCR requires installed Tesseract/languages; callers can increase Limits.timeout for longer scanned files.
`tests/test_documents.py` checks layout, tables and resource errors in python.yml.

Source builds can enable Cargo feature `pdf-ocr-ocrs` and select `ocrs` with
explicit local ONNX paths; default wheels retain Tesseract. Model licensing,
Latin-only scope and cooperative inference deadlines follow
[Format Support](../formats/format-support.md).

## See also

- [Node.js Package](node-package.md), the sibling binding and parity gaps
- [Release Process](../operations/release-process.md), wheel publication and verification
