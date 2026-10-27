# @everruns/markkit-wasm

Markkit compiled to WebAssembly: extract clean Markdown from HTML, DOCX,
spreadsheets (XLSX/XLSM, XLS, ODS), plain text and Markdown in the **browser
and any JavaScript runtime**:
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
