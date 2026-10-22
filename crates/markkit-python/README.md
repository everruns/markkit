# markkit-python

Native Python bindings for Markkit HTML-to-Markdown extraction.

The current wheel publisher builds for **Linux x64 and CPython 3.12**. It does not
publish macOS, Windows, ARM64 or other Python-version wheels. Other wheel targets
require a separate future release; Rust and npm availability is independent.

```python
import markkit_python

print(markkit_python.extract_html_sync("<h1>Hello</h1>"))
# # Hello
```

Publication verifies wheel metadata, installs the actual wheel into a clean
virtual environment and exercises native extraction before upload.
