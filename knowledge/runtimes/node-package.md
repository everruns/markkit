---
type: Package Design
title: Node.js Package
description: NAPI-RS Node.js bindings, exported API, build, and npm distribution.
tags:
  - markkit
  - node
  - bindings
---

# Node.js Package

## Overview

`crates/markkit-js` exposes Markkit as the native npm package `@everruns/markkit`
via NAPI-RS.

## API

```javascript
const m = require('@everruns/markkit');

const markdown = await m.extractHtml('<h1>Hello</h1>', { includeSelectors: ['article'] });
const sync = m.extractHtmlSync('<h1>Hello</h1>');
const fromUrl = await m.extractUrl('https://example.com', { allowedHosts: ['example.com'] });
const fromBuffer = await m.extractBytes(Buffer.from('<h1>Hello</h1>'), 'text/html');
const doc = m.extractDocumentSync(buf, { fileName: 'report.docx' });
// { markdown, title, format, warnings, stats, metadata }
```

Every function takes an optional trailing `options` object (`MarkkitOptions`):
the camelCase builder fields shared with the [WASM Package](wasm-package.md)
(`maxInputSize`, `maxOutputSize`, `maxNodes`, `maxNestingDepth`, `baseUrl`, `mainContent`, `preserveLinks`, `preserveImages`, `includeSelectors`,
`excludeSelectors`, archive and sheet limits, `includeHiddenSheets`) plus the URL
policy fields `timeout` (seconds, float; whole call), `maxProcessingSecs`
(whole-second alias, `timeout` wins), `allowPrivateNetworks`, `allowedHosts`,
`maxRedirects`. Selectors add to the defaults. 64-bit limits arrive as JS numbers
and must be whole and non-negative. Unlike WASM, unknown keys are ignored
(napi objects do not report them).

Errors reject (or throw) with the Rust error's `Display` text as the reason.

## Binding parity

| Capability | Python | Node.js | WASM |
|---|---|---|---|
| HTML async / sync | yes | yes | sync |
| HTML with options | `extract_html_sync_with_options` (selectors, size, `base_url`, `main_content`) | all builder options | all builder options |
| Resource limits (`Limits`) | `limits=markkit.Limits(...)` on every function | flat option keys, incl. `timeout` | flat option keys, no `timeout` (no clock) |
| URL | yes, defaults only | yes, with URL policy options | no (host fetches) |
| Bytes | yes | yes | yes |
| `extract_document` (metadata) | no | yes | yes |
| Agent tool | no | no | `MarkkitTool` |

Tests: `crates/markkit-js/__test__/*.test.cjs` (`node --test`, run by `js.yml`
after the native build): options on every entry point, limits, invalid numbers,
`extractDocument`, and URL policy against a local server.

## Build

```bash
cd crates/markkit-js
napi build --release
```

## Distribution

Published to npm by `publish-js.yml` from a verified release tag; see
[Release Process](../operations/release-process.md). CI: `js.yml` on changes to
`crates/markkit-js/` or `crates/markkit/`.

## See also

- [Python Package](python-package.md), the sibling binding
- [WASM Package](wasm-package.md), the browser and edge build
- [Release Process](../operations/release-process.md), npm publication and tarball verification
