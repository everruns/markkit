# Python Package

## Overview

The `markkit-python` crate exposes Markkit as a Python module via PyO3.

## API

```python
import markkit_python

# Async extraction from HTML
markdown = await markkit_python.extract_html("<h1>Hello</h1>")

# Async extraction from URL
markdown = await markkit_python.extract_url("https://example.com")

# Async extraction from bytes
markdown = await markkit_python.extract_bytes(b"<h1>Hello</h1>", "text/html")
```

## Build

Uses `maturin` for building Python wheels:

```bash
cd crates/markkit-python
maturin develop  # for local development
maturin build    # for distribution
```

## Distribution

Published to PyPI via GitHub Actions workflow.
