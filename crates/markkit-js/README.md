# @everruns/markkit

[![npm](https://img.shields.io/npm/v/@everruns/markkit.svg)](https://www.npmjs.com/package/@everruns/markkit)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/everruns/markkit/blob/main/LICENSE)

**Extract clean Markdown from any document, safely.** Native Node.js bindings
for [Markkit](https://github.com/everruns/markkit), written in Rust.

Convert HTML, PDF, Word, PowerPoint, Excel/ODS spreadsheets, ODT, EPUB, email
(EML), Jupyter notebooks, CSV/TSV, Markdown and plain text into well-structured
Markdown for LLMs, RAG pipelines and agents. Built for untrusted input: nothing
is executed, and every resource a document can drive is bounded.

- One call for every format, detected from file name, content type or bytes
- Title, metadata, stats and structured warnings with every result
- Main-content detection for web pages, CSS include/exclude selectors
- PDF reading order, columns, tables, optional layout geometry and OCR
- URL fetching with SSRF protection
- TypeScript declarations included; async functions run off the event loop

## Install

```bash
npm install @everruns/markkit
```

The package ships a prebuilt native addon for **Linux x64 (glibc)**. macOS,
Windows, ARM64 and musl are not published yet; for those, or for browsers and
edge runtimes, use [`@everruns/markkit-wasm`](https://www.npmjs.com/package/@everruns/markkit-wasm).
CI verifies a clean install and extraction on Node.js 24.

## Quick start

```js
const fs = require('node:fs');
const { extractHtmlSync, extractDocument } = require('@everruns/markkit');

console.log(extractHtmlSync('<h1>Hello</h1><p>World</p>'));
// # Hello
//
// World

const doc = await extractDocument(fs.readFileSync('report.docx'), { fileName: 'report.docx' });
console.log(doc.format, doc.title, doc.stats); // docx "Q3 Report" { paragraphs: 7, tables: 1 }
console.log(doc.markdown);
```

ES modules work too: `import { extractHtml } from '@everruns/markkit';`.

## API

Every function takes an optional trailing `options` object.

| Function | Returns | Use it for |
|---|---|---|
| `extractDocument(data: Buffer, hint?, options?)` | `Promise<Extraction>` | Any format, with metadata |
| `extractDocumentSync(data, hint?, options?)` | `Extraction` | Same, synchronous |
| `extractHtml(html: string, options?)` | `Promise<string>` | HTML string to Markdown |
| `extractHtmlSync(html, options?)` | `string` | Same, synchronous |
| `extractBytes(data: Buffer, contentType?, options?)` | `Promise<string>` | Bytes to Markdown only |
| `extractUrl(url: string, options?)` | `Promise<string>` | Fetch and convert a URL |

`hint` is `{ contentType?: string, fileName?: string }`. The format is detected
from `contentType`, then the extension of `fileName`, then magic bytes. The
sync variants are CPU-bound and block the event loop while they run.

`Extraction`:

| Field | Type | Contents |
|---|---|---|
| `markdown` | `string` | The Markdown |
| `title` | `string \| undefined` | Title when the source declares one |
| `format` | `string` | `html`, `pdf`, `docx`, `pptx`, `xlsx`, `xlsb`, `xls`, `ods`, `odt`, `epub`, `eml`, `ipynb`, `csv`, `tsv`, `markdown`, `text` |
| `warnings` | `{ code, detail }[]` | e.g. `{ code: 'truncated_sheet', detail: '...' }`; match on `code` |
| `stats` | `ExtractionStats` | `pages`, `sheets`, `rows`, `paragraphs`, `tables` |
| `metadata` | `Metadata` | HTML `description`, `author`, `published`, `language`, `canonicalUrl`, `siteName`, `image` |
| `layout` | `PdfLayout \| undefined` | PDF geometry, only with `pdfLayout: true` |

## Options

```js
const { extractHtml, extractUrl } = require('@everruns/markkit');

await extractHtml(html, {
  mainContent: true,              // keep the article, drop sidebars and comments
  includeSelectors: ['main'],     // keep only matches
  excludeSelectors: ['.ad'],      // drop matches (on top of nav/footer/script/...)
  baseUrl: 'https://example.com/blog/', // resolve relative links and images
  preserveImages: false,
});

// URLs: loopback, private, link-local and metadata addresses are refused,
// also after DNS resolution and on every redirect.
await extractUrl('https://docs.example.com/page', { allowedHosts: ['docs.example.com'], maxRedirects: 2 });
```

| Option | Default | Meaning |
|---|---|---|
| `mainContent` | `false` | Keep only the detected article body |
| `includeSelectors` / `excludeSelectors` | `[]` | CSS selectors to keep / drop |
| `baseUrl` | none | Base for relative links and images |
| `preserveLinks` / `preserveImages` | `true` | Keep `[text](url)` / `![alt](src)` |
| `includeHiddenSheets` | `false` | Render hidden spreadsheet sheets |
| `pdfLayout` | `false` | Add `layout` geometry for PDFs |
| `pdfRemoveRepeatedHeaders` | `false` | Drop running PDF headers/footers |
| `pdfOcr`, `pdfOcrLanguage` | `false`, `"eng"` | OCR textless PDF pages with Tesseract |
| `pdfOcrBackend`, `pdfOcrDetectionModel`, `pdfOcrRecognitionModel` | `"tesseract"` | Rust `ocrs` backend (source builds) |
| `allowPrivateNetworks` | `false` | Let `extractUrl` reach internal hosts (trusted URLs only) |
| `allowedHosts` | `[]` | Host allowlist for `extractUrl`, checked on every redirect |

### Limits and errors

| Option | Default | Bounds |
|---|---|---|
| `maxInputSize` | 50 MiB | Input bytes, fetched body |
| `maxOutputSize` | 128 MiB | Markdown output bytes |
| `timeout` | 60 | Seconds per call, fractions allowed (`maxProcessingSecs` is the whole-second form) |
| `maxNodes` | 5 000 000 | Parsed nodes, retained PDF geometry |
| `maxNestingDepth` | 256 | Element nesting (hard max 512) |
| `maxArchiveEntries` | 10 000 | ZIP entries |
| `maxArchiveEntrySize` / `maxArchiveTotalSize` | 64 / 256 MiB | Decompressed ZIP bytes |
| `maxSheetRows` / `maxSheetCols` | 10 000 / 256 | Rendered per sheet (truncated with a warning) |
| `maxSpreadsheetCells` | 4 000 000 | XLS/ODS grid before parsing |
| `maxRedirects` | 5 | `extractUrl` redirects |
| `maxPdfPages` | 10 000 | PDF pages (truncated with a warning) |
| `maxPdfStreamSize` / `maxPdfDecodedSize` | 64 / 256 MiB | Decoded PDF bytes |

Failures throw an `Error` whose message names the cause:

```js
try {
  extractDocumentSync(bytes, { fileName: 'big.xlsx' }, { maxOutputSize: 1 << 20, timeout: 10 });
} catch (err) {
  console.error(err.message); // "Resource limit exceeded: output exceeds max_output_size 1048576"
}
```

## PDF layout and OCR

```js
const doc = extractDocumentSync(pdfBytes, { fileName: 'report.pdf' }, { pdfLayout: true });
for (const block of doc.layout.pages[0].blocks) {
  console.log(block.kind, block.bbox); // reading order; bbox is [left, top, right, bottom]
}
```

PDF extraction rebuilds reading order and columns, ruled and borderless tables
(with merged and wrapped cells), emphasis, numbered lists and tagged headings.
`layout` is `{ version: 1, pages }`. Each page has source `spans` (text runs
with `bbox`, `baseline`, `font`, `size`, `words`) and semantic `blocks`
(`kind`, `bbox`, `spanIds`, `lines`, table `cells`). Coordinates are points
from the page's top-left, x right and y down. Layout never changes the
Markdown.

Scanned pages: install Tesseract and language data, then pass
`{ pdfOcr: true, pdfOcrLanguage: 'eng', timeout: 300 }`. Only pages without
text are recognized. Source builds can enable the Rust `ocrs` backend (Cargo
feature `pdf-ocr-ocrs`) with local ONNX model paths; extraction never
downloads models. See the
[PDF guide](https://github.com/everruns/markkit/blob/main/docs/pdf.md).

## Examples

Runnable scripts in
[`examples/node/`](https://github.com/everruns/markkit/tree/main/examples/node):
`quickstart.cjs` (HTML, options, bytes) and `documents.cjs` (DOCX, XLSX, PDF
with stats, limits and layout; pass your own files as arguments).

## License

MIT. See [LICENSE](https://github.com/everruns/markkit/blob/main/LICENSE) and
[NOTICE](https://github.com/everruns/markkit/blob/main/NOTICE).
