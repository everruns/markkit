# @everruns/markkit-wasm

[![npm](https://img.shields.io/npm/v/@everruns/markkit-wasm.svg)](https://www.npmjs.com/package/@everruns/markkit-wasm)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/everruns/markkit/blob/main/LICENSE)

**Extract clean Markdown from documents in the browser and at the edge.**
[Markkit](https://github.com/everruns/markkit) compiled to WebAssembly.

Convert HTML, PDF (text and layout), DOCX, spreadsheets (XLSX/XLSM/XLSB, XLS,
ODS), CSV/TSV, Jupyter notebooks, Markdown and plain text to Markdown in any
JavaScript runtime: browsers, Cloudflare Workers, Vercel Edge, Deno Deploy,
Deno, Bun and Node.js. Includes an LLM agent tool.

- Single-threaded `wasm-bindgen` build: no `SharedArrayBuffer`, no
  `COOP`/`COEP` headers, no bundler required
- Untrusted input is safe: nothing executes, sizes, nodes and nesting are bounded
- No network access: the host fetches and passes bytes in, so network policy
  stays with the host
- TypeScript declarations included

For a native Node.js addon (all formats, OCR, URL fetching), use
[`@everruns/markkit`](https://www.npmjs.com/package/@everruns/markkit).

## Install

```bash
npm install @everruns/markkit-wasm
```

## Quick start

```js
import { initMarkkit, Markkit } from "@everruns/markkit-wasm";

await initMarkkit(); // load the .wasm once, before first use

const mk = new Markkit({ mainContent: true, preserveImages: false });
console.log(mk.extractHtml("<article><h1>Hello</h1><p>World</p></article>"));
// # Hello
//
// World

const res = await fetch("/report.docx");
const doc = mk.extractDocument(new Uint8Array(await res.arrayBuffer()), {
  contentType: res.headers.get("content-type") ?? undefined,
  fileName: "report.docx",
});
console.log(doc.format, doc.title, doc.stats, doc.markdown);
```

### Loading the module

`initMarkkit()` is idempotent. Browsers and edge runtimes fetch
`markkit_wasm_bg.wasm` next to the module; Node reads it from disk. Pass your
own source when the bundler or runtime needs it:

```js
// Cloudflare Workers / bundlers that import .wasm as a module
import wasm from "@everruns/markkit-wasm/markkit_wasm_bg.wasm";
await initMarkkit(wasm);

// Or any URL, Response, BufferSource or WebAssembly.Module
await initMarkkit(new URL("/assets/markkit_wasm_bg.wasm", location.href));
```

## API

| Member | Returns | Use it for |
|---|---|---|
| `initMarkkit(input?)` | `Promise<void>` | Load the module once |
| `version()` | `string` | Library version |
| `new Markkit(options?)` | `Markkit` | Extractor with fixed options; reuse it |
| `mk.extractDocument(bytes: Uint8Array, hint?)` | `Extraction` | Any supported format, with metadata |
| `mk.extractHtml(html: string)` | `string` | HTML string to Markdown |
| `mk.extractBytes(bytes, contentType?)` | `string` | Bytes to Markdown only |
| `mk.free()` | | Release wasm memory early (optional) |
| `new MarkkitTool(options?)` | `MarkkitTool` | LLM agent tool, see below |

`hint` is `{ contentType?, fileName? }`; detection uses the content type, then
the file extension, then magic bytes. Methods are synchronous; run them in a
Web Worker to keep a page responsive on large files.

`Extraction` is `{ markdown, title, format, warnings, stats, metadata, layout? }`.
`warnings` are `{ code, detail }` with stable codes such as `truncated_sheet`.
`metadata` holds HTML `description`, `author`, `published`, `language`,
`canonicalUrl`, `siteName` and `image`.

## Options

| Option | Default | Meaning |
|---|---|---|
| `mainContent` | `false` | Keep only the detected article body of HTML |
| `includeSelectors` / `excludeSelectors` | `[]` | CSS selectors to keep / drop (drops add to nav/footer/script/...) |
| `baseUrl` | none | Base for relative links and images |
| `preserveLinks` / `preserveImages` | `true` | Keep `[text](url)` / `![alt](src)` |
| `includeHiddenSheets` | `false` | Render hidden spreadsheet sheets |
| `pdfLayout` | `false` | Add PDF `layout` geometry |
| `pdfRemoveRepeatedHeaders` | `false` | Drop running PDF headers/footers |
| `maxInputSize` | 50 MiB | Input bytes |
| `maxOutputSize` | 128 MiB | Markdown output bytes |
| `maxNodes` | 5 000 000 | Parsed nodes, retained PDF geometry |
| `maxNestingDepth` | 256 | Element nesting (hard max 512) |
| `maxArchiveEntries` | 10 000 | ZIP entries |
| `maxArchiveEntrySize` / `maxArchiveTotalSize` | 64 / 256 MiB | Decompressed ZIP bytes |
| `maxSheetRows` / `maxSheetCols` | 10 000 / 256 | Rendered per sheet (truncated with a warning) |
| `maxSpreadsheetCells` | 4 000 000 | XLS/ODS grid before parsing |
| `maxPdfPages` | 10 000 | PDF pages (truncated with a warning) |
| `maxPdfStreamSize` / `maxPdfDecodedSize` | 64 / 256 MiB | Decoded PDF bytes |

Unknown keys throw. There is no `timeout`: WebAssembly has no clock here, so
bound time in the host (a Worker timeout, an edge CPU limit, or
`worker.terminate()`). Failures throw an `Error` whose message names the
cause, for example `Resource limit exceeded: output exceeds max_output_size 8`.

## Agent tool

`MarkkitTool` follows the Everruns toolkit contract: name, descriptions, input
and output JSON schemas, a system prompt, an OpenAI-compatible tool definition
and a validated `execute`. Sources are `html` or `content_base64` (with an
optional `file_name` / `content_type`).

```js
import { initMarkkit, MarkkitTool } from "@everruns/markkit-wasm";
await initMarkkit();

const tool = new MarkkitTool({ maxOutputChars: 20000 });
const tools = [tool.toolDefinition()]; // send with the model request
const system = tool.systemPrompt();

try {
  const result = await tool.execute({ html: "<h1>Title</h1>" });
  // { markdown, title, format, warnings, truncated }
} catch (e) {
  if (e.userFacing) { /* safe to return e.message to the model */ }
}
```

## PDF layout

`pdfLayout: true` adds `layout: { version: 1, pages }` to PDF results. Each
page has source `spans` (text runs with `bbox`, `baseline`, `font`, `size`,
`words`) and semantic `blocks` in reading order (`kind`, `bbox`, `spanIds`,
`lines`, table `cells` with row/column spans). Coordinates are points from the
page's top-left, x right and y down. Layout never changes the Markdown.

PDF extraction rebuilds reading order, columns and tables from the text layer.
OCR for scanned pages runs only in the native Rust, Node.js and Python
packages. See the [PDF guide](https://github.com/everruns/markkit/blob/main/docs/pdf.md).

## Example

[`examples/wasm/quickstart.mjs`](https://github.com/everruns/markkit/blob/main/examples/wasm/quickstart.mjs)
converts HTML, DOCX, XLSX and PDF and runs the agent tool; it runs in CI
against the freshly built package.

## License

MIT. See [LICENSE](https://github.com/everruns/markkit/blob/main/LICENSE); third-party notices are in
[NOTICE](https://github.com/everruns/markkit/blob/main/NOTICE).
