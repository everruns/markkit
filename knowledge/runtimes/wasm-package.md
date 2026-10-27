---
type: Package Design
title: WASM Package
description: wasm-bindgen browser and edge package @everruns/markkit-wasm, its API, build, tests, and npm distribution.
tags:
  - markkit
  - wasm
  - browser
  - bindings
---

# WASM Package

## Overview

`crates/markkit-wasm` builds `@everruns/markkit-wasm`: Markkit compiled to
`wasm32-unknown-unknown` with `wasm-bindgen --target web`. It mirrors bashkit-wasm:

- **Single-threaded**: no `SharedArrayBuffer`, no `COOP`/`COEP` headers; runs in
  browsers (incl. iframes), edge workers, Node, Deno and Bun.
- **Separate from the napi package** (`@everruns/markkit`), which stays the fast
  native option for Node on Linux x64.
- **Features**: `html`, `docx`, `spreadsheet`, `tool`. No `http_client`: reqwest
  needs sockets, and the host fetching keeps network policy (SSRF) with the host.
  The tool therefore offers only `html` and `content_base64`.

## API

```js
import { initMarkkit, Markkit, MarkkitTool, version } from "@everruns/markkit-wasm";
await initMarkkit();                          // once; idempotent
const mk = new Markkit({ includeSelectors: ["article"] });
mk.extractHtml(html);                         // string
mk.extractDocument(bytes, { fileName });      // { markdown, title, format, warnings, stats, metadata }
mk.extractBytes(bytes, contentType);          // string
const tool = new MarkkitTool({ locale, maxOutputChars });
tool.toolDefinition(); await tool.execute(args);
```

Options are camelCase builder fields (`maxInputSize`, `maxOutputSize`,
`maxNodes`, `maxNestingDepth`, `baseUrl`, `mainContent`, `preserveLinks`,
`preserveImages`, `includeSelectors`, `excludeSelectors`, archive and sheet
limits, `includeHiddenSheets`). Selectors add to the defaults, as in Python.
Unknown keys and wrong types throw `invalid options`: values go through
`serde_json::Value` because serde-wasm-bindgen only reads named fields, so
`deny_unknown_fields` alone would never see a typo.

wasm32-unknown-unknown has no clock (`Instant::now` panics), so the core
`timeout` limit is not enforced here (`Budget` runs without a deadline); every
other limit applies. Hosts bound time themselves, e.g. by terminating a Worker.

Errors throw `Error` with the Rust `Display` text. `MarkkitTool.execute` rejects
with an `Error` carrying `userFacing` (see [Tool Contract](../integrations/tool-contract.md)).

`initMarkkit()` reads the `.wasm` from disk under Node (no `fetch` for `file:`
URLs) and otherwise lets the glue fetch it next to the module; callers may pass
their own bytes, `Response`, URL or `WebAssembly.Module`.

## Build

```bash
rustup target add wasm32-unknown-unknown
bash scripts/install-wasm-bindgen.sh      # CLI pinned to Cargo.lock's wasm-bindgen
bash crates/markkit-wasm/scripts/build.sh # -> crates/markkit-wasm/pkg
```

Uses the `wasm-release` profile (`opt-level = "z"`, `panic = "abort"`); runs
`wasm-opt -Oz` when binaryen is on PATH. Unoptimized output is about 1.4 MB.
With `panic = "abort"` a Rust panic traps the instance; parsers are expected to
return errors, never panic (invariant S9).

## Tests

`crates/markkit-wasm/__test__/*.test.mjs` (`node --test`) run against `pkg/`:
options and their validation, HTML, bytes, DOCX through the zip/XML stack
(fixtures built by `__test__/zip.mjs`), archive limits, malformed input, version
parity with `package.json`, and the tool (definition, locale, execution,
`userFacing` errors). CI job `wasm` in `ci.yml` also checks the core crate for
wasm32 and the npm archive contents; it gates `Check`.

## Distribution

`publish-wasm.yml` (workflow_dispatch, dry run by default) builds, tests and
publishes with npm trusted publishing (OIDC). The trusted publisher must be set up
on npmjs.com for `@everruns/markkit-wasm` before the first real publish.
`package.json` `version` must equal the workspace version (tested).

## See also

- [Node.js Package](node-package.md), the native napi sibling
- [Tool Contract](../integrations/tool-contract.md), what `MarkkitTool` implements
- [Release Process](../operations/release-process.md), where the publisher runs
