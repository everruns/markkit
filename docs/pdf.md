# PDF guide

Markkit's `pdf` feature extracts the text layer of PDF files with a pure-Rust
parser (Hayro). This guide covers what it reconstructs, the optional layout
geometry, OCR for scanned pages, and the limits that bound it. The Python,
Node.js and CLI packages include PDF and OCR support. The WASM package reads
text layers and layout, but has no OCR.

## Text, reading order and tables

PDF pages are positioned glyphs, not paragraphs. Markkit rebuilds:

- **Reading order** across columns
- **Tables**: ruled and conservative borderless tables, including merged and
  wrapped cells. Ambiguous alignments stay prose instead of becoming false
  tables.
- **Emphasis and lists**: bold, italic and numbered lists
- **Headings** from tagged-PDF roles, and `ActualText` replacements
- **Repeated headers and footers** (opt-in): running margin text repeated on
  every page is dropped

| Surface | Remove repeated headers |
|---|---|
| Rust | `Markkit::builder().pdf_remove_repeated_headers(true)` |
| CLI | `markkit extract --file report.pdf --remove-pdf-headers` |
| Python | `extract_document_sync(data, file_name="report.pdf", pdf_remove_repeated_headers=True)` |
| Node.js / WASM | `{ pdfRemoveRepeatedHeaders: true }` |

Encrypted PDFs are rejected with `Error::UnsupportedFormat`. A page without a
text layer produces a `pdf_no_text` warning, unless OCR is enabled.

## Layout geometry

Layout is an optional sidecar for citation, highlighting and custom chunking.
It never changes the Markdown.

```rust
use markkit::{DocumentHint, Markkit};

fn page_geometry(pdf_bytes: &[u8]) -> markkit::Result<()> {
    let markkit = Markkit::builder().pdf_layout(true).build();
    let doc = markkit.extract_document_sync(pdf_bytes, DocumentHint::file_name("report.pdf"))?;
    let layout = doc.layout.as_ref().expect("PDF input with pdf_layout(true)");
    for page in &layout.pages {
        println!("page {}: {}x{} pt", page.page, page.width, page.height);
        for block in &page.blocks {
            println!("  {} at {:?}", block.kind, block.bbox); // [left, top, right, bottom]
        }
    }
    Ok(())
}
```

| Surface | Enable layout |
|---|---|
| Rust | `.pdf_layout(true)`, then `Extraction::layout` / `ParsedDocument::layout` |
| CLI | `markkit extract --file report.pdf --json --layout` |
| Python | `extract_document_sync(data, file_name="report.pdf", pdf_layout=True)["layout"]` |
| Node.js | `extractDocumentSync(bytes, { fileName: "report.pdf" }, { pdfLayout: true }).layout` |
| WASM | `new Markkit({ pdfLayout: true }).extractDocument(bytes, { fileName: "report.pdf" }).layout` |

Layout v1 (`{ version: 1, pages: [...] }`) has, for each page:

- `page`, `width`, `height`, `rotation`
- `spans`: source text runs in drawing order, with `id`, `text`, `bbox`
  (`[left, top, right, bottom]`), `baseline`, `font`, `size`, `angle`, `bold`,
  `italic`, tagged `role`, an `ocr` flag and glyph-positioned `words`
- `blocks`: semantic blocks in reading order, with `id`, `kind` (`heading`,
  `paragraph`, `table`, ...), `bbox`, source `span_ids` (`spanIds` in JS),
  `lines`, and table `cells` with row/column positions, rowspan/colspan and
  bounds

Coordinates are display points from the cropped and rotated page's top-left
corner, x right and y down. Font bounds approximate an em box; OCR bounds come
from the recognizer. With `track_sources(true)`, IR blocks carry
`Source.page` and `Source.layout_block`, which link Markdown blocks back to
layout blocks. Python keys are snake_case; Node.js and WASM keys are camelCase.

Try it on the bundled sample: `python examples/python/documents.py` or
`node examples/node/documents.cjs` print the block geometry of
`examples/data/sample.pdf`.

## OCR for scanned pages

OCR is opt-in and only processes pages that have no text layer. Pages are
rendered in Rust (Hayro) and recognized locally. Extraction never downloads
anything.

### Tesseract (default)

Install Tesseract and the language data you need (for example
`apt install tesseract-ocr tesseract-ocr-deu`).

```bash
markkit extract --file scanned.pdf --ocr --ocr-language eng --timeout 300
```

| Surface | Enable OCR |
|---|---|
| Rust | feature `pdf-ocr`; `.pdf_ocr(true).pdf_ocr_language("eng+deu")` |
| CLI | `--ocr --ocr-language eng` |
| Python | `extract_document_sync(data, file_name="scan.pdf", pdf_ocr=True, pdf_ocr_language="eng", limits=markkit.Limits(timeout=300))` |
| Node.js | `{ pdfOcr: true, pdfOcrLanguage: "eng", timeout: 300 }` |

OCR is slow compared to text extraction: raise `timeout` for scanned
documents. OCR word bounds are included in layout output.

### Rust `ocrs` backend (optional)

For deployments without Tesseract, build with the `pdf-ocr-ocrs` feature and
supply local ONNX models. Default packages keep Tesseract.

```bash
python3 scripts/prepare_ocr_models.py  # fetch pinned weights, verify sizes and SHA-256
cargo build --release -p markkit-cli --features pdf-ocr-ocrs
target/release/markkit extract --file scanned.pdf --ocr --ocr-backend ocrs \
  --ocr-detection-model target/ocr-models/text-detection.onnx \
  --ocr-recognition-model target/ocr-models/text-recognition.onnx --timeout 300
```

Rust: `.pdf_ocr_backend(markkit::PdfOcrBackend::Ocrs)` plus
`.pdf_ocr_detection_model(path)` and `.pdf_ocr_recognition_model(path)`.
Python and Node.js source builds accept the same settings
(`pdf_ocr_backend="ocrs"` / `pdfOcrBackend: "ocrs"` and model paths).

The pinned weights are CC-BY-SA 4.0, separate from Markkit's MIT code. The
setup script writes their attribution and license to
`target/ocr-models/NOTICE.json`. The default model supports the Latin
alphabet, and `ocrs` currently accepts `eng` only. Models are trusted
deployment assets. `RTEN_NUM_THREADS` and `RAYON_NUM_THREADS` size the
inference thread pools at process start.

## Limits

PDF work is bounded like every other format:

| Limit | Default | Bounds |
|---|---|---|
| `max_pdf_pages` | 10 000 | Pages rendered; later pages are truncated with a `truncated_pdf` warning |
| `max_pdf_stream_size` | 64 MiB | Decoded bytes per stream, checked before decoding |
| `max_pdf_decoded_size` | 256 MiB | Decoded bytes in total, PDF content interpreted, retained geometry and analysis work |
| `max_nodes` | 5 000 000 | Geometry and rule primitives, retained layout |
| `max_output_size` | 128 MiB | Markdown and compact layout JSON, each bounded on its own |
| `timeout` | 60 s | Whole call, including OCR |

Table grids also obey the row, column and grid limits. Hayro checks image and
raster budgets before rendering, and the deadline before and after its
non-interruptible render step. Tesseract runs with private temp files, output
caps and a killable deadline, without a shell. OCR currently supports JPEG,
JPEG2000 and generic-filter images without DecodeParms. Inline, JBIG2 and CCITT
images are refused until bounded decoding is available. For hard CPU and
memory quotas, isolate the host process.

See the [threat model](../knowledge/security/threat-model.md) (T8) and
[native dependencies](../knowledge/security/native-dependencies.md) for
provenance and licensing.
