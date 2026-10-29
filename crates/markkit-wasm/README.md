# @everruns/markkit-wasm

Markkit compiled to WebAssembly: extract clean Markdown from HTML, PDF text/layout, DOCX,
spreadsheets (XLSX/XLSM, XLS, ODS), CSV/TSV, Jupyter notebooks, plain text and
Markdown in the **browser and any JavaScript runtime**:
edge workers (Cloudflare Workers, Vercel Edge, Deno Deploy), Node, Deno and Bun.

Single-threaded `wasm-bindgen` build: no `SharedArrayBuffer`, no `COOP`/`COEP`
headers. For a native Node.js addon use
[`@everruns/markkit`](https://www.npmjs.com/package/@everruns/markkit).

No URL fetching: the host fetches and passes bytes in, so network policy stays
with the host.

## Install

```bash
npm install @everruns/markkit-wasm
```

## Quick start

```js
import { initMarkkit, Markkit } from "@everruns/markkit-wasm";

await initMarkkit(); // once

const mk = new Markkit({ includeSelectors: ["article"], preserveImages: false });
console.log(mk.extractHtml("<article><h1>Hello</h1></article>")); // # Hello

const res = await fetch("/report.docx");
const doc = mk.extractDocument(new Uint8Array(await res.arrayBuffer()), {
  contentType: res.headers.get("content-type") ?? undefined,
  fileName: "report.docx",
});
console.log(doc.title, doc.format, doc.markdown);
```

## Options

`maxInputSize`, `maxOutputSize`, `maxNodes`, `maxNestingDepth`, `baseUrl`, `mainContent`, `preserveLinks`, `preserveImages`, `includeSelectors`,
`excludeSelectors` (added to the defaults), `maxArchiveEntries`,
`maxArchiveEntrySize`, `maxArchiveTotalSize`, `maxSheetRows`, `maxSheetCols`,
`maxSpreadsheetCells`, `includeHiddenSheets`. Unknown keys throw. There is no
`timeout`: wasm has no clock, so bound time in the host (e.g. a Worker timeout).

## Agent tool

`MarkkitTool` follows the Everruns toolkit contract: tool definition, schemas,
system prompt and a validated `execute`. Sources are `html` or `content_base64`.

```js
import { initMarkkit, MarkkitTool } from "@everruns/markkit-wasm";
await initMarkkit();

const tool = new MarkkitTool({ maxOutputChars: 20000 });
const tools = [tool.toolDefinition()]; // send with the model request

try {
  const result = await tool.execute({ html: "<h1>Title</h1>" });
  // { markdown, title, format, warnings, truncated }
} catch (e) {
  if (e.userFacing) { /* safe to return e.message to the model */ }
}
```

## License

MIT

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

WASM reads existing PDF text layers. Native process OCR runs through the Rust,
Node or Python packages with installed Tesseract and language data.
