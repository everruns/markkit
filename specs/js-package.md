# JavaScript/Node.js Package

## Overview

The `markkit-js` crate exposes Markkit as a native Node.js module via NAPI-RS.

## API

```javascript
const { extractHtml, extractUrl, extractBytes } = require('@everruns/markkit');

// Async extraction from HTML
const markdown = await extractHtml('<h1>Hello</h1>');

// Async extraction from URL
const markdown = await extractUrl('https://example.com');

// Async extraction from buffer
const markdown = await extractBytes(Buffer.from('<h1>Hello</h1>'), 'text/html');
```

## Build

Uses `napi-rs` CLI for building:

```bash
cd crates/markkit-js
napi build --release
```

## Distribution

Published to npm via GitHub Actions workflow.
