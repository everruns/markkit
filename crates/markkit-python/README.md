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
