# @everruns/markkit

Native Node.js bindings for Markkit: HTML, DOCX and spreadsheets to Markdown.

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

// Documents with metadata (HTML, DOCX, XLSX/XLS/ODS).
const doc = extractDocumentSync(fs.readFileSync('report.docx'), { fileName: 'report.docx' }, { maxSheetRows: 500 });
console.log(doc.title, doc.format, doc.warnings, doc.markdown);
```

Fields: `maxInputSize`, `maxOutputSize`, `timeout` (seconds, covers fetch, parsing and
rendering; `maxProcessingSecs` is the whole-second form), `maxNodes`, `maxNestingDepth`,
`baseUrl`, `mainContent`, `preserveLinks`, `preserveImages`,
`includeSelectors`, `excludeSelectors` (added to the defaults), `maxArchiveEntries`,
`maxArchiveEntrySize`, `maxArchiveTotalSize`, `maxSheetRows`, `maxSheetCols`,
`maxSpreadsheetCells`, `includeHiddenSheets`, `allowPrivateNetworks` (trusted URLs only),
`allowedHosts`, `maxRedirects`.

For browsers and edge runtimes use [`@everruns/markkit-wasm`](https://www.npmjs.com/package/@everruns/markkit-wasm).
