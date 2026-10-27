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

## See also

- [Node.js Package](node-package.md), the sibling binding and parity gaps
- [Release Process](../operations/release-process.md), wheel publication and verification
