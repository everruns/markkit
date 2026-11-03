"""Convert DOCX, XLSX and PDF files with metadata, stats and warnings.

    python examples/python/documents.py                 # bundled samples
    python examples/python/documents.py my.pptx my.epub # your own files

`extract_document_sync` detects the format from the file name, content type
and bytes, and returns a dict: markdown, title, format, warnings, stats,
metadata (and layout when pdf_layout=True).
"""

import pathlib
import sys

import markkit

DATA = pathlib.Path(__file__).resolve().parent.parent / "data"
paths = [pathlib.Path(p) for p in sys.argv[1:]] or [
    DATA / "report.docx",
    DATA / "sales.xlsx",
    DATA / "sample.pdf",
]

for path in paths:
    result = markkit.extract_document_sync(path.read_bytes(), file_name=path.name)
    print(f"=== {path.name} ===")
    print("format:", result["format"])
    print("title: ", result["title"])
    stats = {k: v for k, v in result["stats"].items() if v is not None}
    print("stats: ", stats)
    for warning in result["warnings"]:
        print(f"warning [{warning['code']}]: {warning['detail']}")
    print()
    print(result["markdown"])

# PDF geometry: pages, positioned text spans and semantic blocks.
pdf = markkit.extract_document_sync(
    (DATA / "sample.pdf").read_bytes(), file_name="sample.pdf", pdf_layout=True
)
page = pdf["layout"]["pages"][0]
print(f"=== sample.pdf layout: {page['width']}x{page['height']} pt ===")
for block in page["blocks"]:
    print(f"{block['kind']:<10} bbox={[round(v) for v in block['bbox']]}")
