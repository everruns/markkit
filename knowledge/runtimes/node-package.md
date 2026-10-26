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
const { extractHtml, extractHtmlSync, extractUrl, extractBytes } = require('@everruns/markkit');

const markdown = await extractHtml('<h1>Hello</h1>');
const sync = extractHtmlSync('<h1>Hello</h1>');
const fromUrl = await extractUrl('https://example.com');
const fromBuffer = await extractBytes(Buffer.from('<h1>Hello</h1>'), 'text/html');
```

Errors reject (or throw) with the Rust error's `Display` text as the reason.

## Binding parity

| Function | Python | Node.js |
|---|---|---|
| `extract_html` / `extractHtml` | yes | yes |
| `extract_html_sync` / `extractHtmlSync` | yes | yes |
| `extract_html_sync_with_options` | yes | **no** |
| `extract_url` / `extractUrl` | yes | yes |
| `extract_bytes` / `extractBytes` | yes | yes |

Neither binding exposes `extract_document` (format, title, warnings, stats) or the
office-document limits yet; both use default builder settings except the Python
options call.

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
- [Release Process](../operations/release-process.md), npm publication and tarball verification
