# @everruns/markkit

Native Node.js bindings for Markkit HTML-to-Markdown extraction.

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
