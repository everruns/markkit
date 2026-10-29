# @everruns/markkit

Native Node.js bindings for Markkit: HTML, PDF, DOCX and spreadsheets to Markdown.

This npm release supports **Linux x64 with glibc**. It does not include binaries for
macOS, Windows, ARM64 or musl Linux. Other platforms require a separate future
native release; Rust and Python package availability is independent.

The package contains its JavaScript loader and native binary. CI verifies a clean
installation and extraction smoke on Node.js 24.22.1 before publication. Use a
currently supported Node.js release with its security patches installed.

```js
const { extractHtmlSync } = require('@everruns/markkit');
console.log(extractHtmlSync('<h1>Hello</h1>'));
// # Hello
```

## Options

Every function takes an optional trailing options object:

```js
const { extractHtml, extractUrl, extractDocumentSync } = require('@everruns/markkit');

await extractHtml(html, { includeSelectors: ['article'], excludeSelectors: ['.ad'], preserveImages: false });

// URLs: non-public addresses are blocked by default.
await extractUrl('https://docs.example.com/page', { allowedHosts: ['docs.example.com'], maxRedirects: 2 });

// Documents with metadata (HTML, PDF, DOCX, PPTX, XLSX/XLSB/XLS/ODS). `warnings` are `{ code, detail }`.
const doc = extractDocumentSync(fs.readFileSync('report.docx'), { fileName: 'report.docx' }, { maxSheetRows: 500 });
console.log(doc.title, doc.format, doc.warnings, doc.markdown);
```

Fields: `maxInputSize`, `maxOutputSize`, `timeout` (seconds, covers fetch, parsing and
rendering; `maxProcessingSecs` is the whole-second form), `maxNodes`, `maxNestingDepth`,
`baseUrl`, `mainContent`, `pdfLayout`, `preserveLinks`, `preserveImages`,
`includeSelectors`, `excludeSelectors` (added to the defaults), `maxArchiveEntries`,
`maxArchiveEntrySize`, `maxArchiveTotalSize`, `maxSheetRows`, `maxSheetCols`,
`maxSpreadsheetCells`, `includeHiddenSheets`, `allowPrivateNetworks` (trusted URLs only),
`allowedHosts`, `maxRedirects`, `maxPdfPages`, `maxPdfStreamSize`, `maxPdfDecodedSize`.

For browsers and edge runtimes use [`@everruns/markkit-wasm`](https://www.npmjs.com/package/@everruns/markkit-wasm).

## PDF layout and tables

PDF text extraction reconstructs columns, ruled and conservative borderless tables,
including merged and wrapped cells. Tagged headings/ActualText, bold/italic and
numbered lists are preserved. Set `pdfRemoveRepeatedHeaders: true` to suppress
repeated margin headers/footers.

`pdfLayout: true` adds `layout: { version: 1, pages }` to document results. Source
`spans` retain drawing order and include glyph-positioned `words`. Semantic `blocks`
are in reading order, with `id`, `kind`, `bbox`, `spanIds`, `lines` and table `cells`.
Cells have row/column positions, rowspan/colspan and source references. Coordinates
are display points from the cropped/rotated top-left, x right and y down. Font
bounds approximate an em box. Layout is optional and does not change Markdown.

Node and WASM use camelCase keys. `maxNodes` bounds primitives and geometry;
`maxPdfDecodedSize` bounds retained geometry and analysis work; table row/column/
grid limits apply. `maxOutputSize` independently bounds compact layout JSON and
Markdown, excluding JSON wrapper/pretty-print overhead.

Native OCR: install Tesseract plus language data. Use
`{ pdfOcr: true, pdfOcrLanguage: "eng", timeout: 300 }` on `extractDocumentSync` /
`extractDocument`. Only textless pages are recognized. OCR word bounds are returned
in layout; helpers use private files, bounded dimensions/output and the deadline.

## Optional Rust OCR backend

Source builds can enable `markkit-js`'s Cargo feature `pdf-ocr-ocrs`. Default npm
builds retain Tesseract. Prepare the pinned models from the repository with
`python3 scripts/prepare_ocr_models.py`, then configure:

```js
extractDocumentSync(bytes, { fileName: "scan.pdf" }, {
  pdfOcr: true, pdfOcrBackend: "ocrs", timeout: 300,
  pdfOcrDetectionModel: "/models/text-detection.onnx",
  pdfOcrRecognitionModel: "/models/text-recognition.onnx"
});
```

Models load from explicit local paths only. The pinned weights use CC-BY-SA 4.0
and carry attribution in the setup script's NOTICE.json. The current Latin
model accepts `pdfOcrLanguage: "eng"`. Missing build features/models and other
languages fail explicitly. Model files and inference need trusted deployment
configuration and host CPU/memory quotas; inference deadlines are cooperative.
