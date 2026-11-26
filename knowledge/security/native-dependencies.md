---
type: Architecture
title: Native Dependencies
description: Hayro provenance review and native code in core features, bindings and development tools.
tags:
  - markkit
  - dependencies
  - security
---

# Native Dependencies

## Hayro review (2026-11-25 Chicago)

Hayro 0.9.0 is a Rust PDF renderer, licensed MIT OR Apache-2.0. Public evidence:

- [Repository](https://github.com/LaurenzV/hayro): individual account LaurenzV.
- Repository creation: 2025-04-22. The [earliest surviving default-branch commit](https://github.com/LaurenzV/hayro/commit/38efa5f7d08e68ebca2d1a209d4921c1deffeaf3)
  is “Initial commit”, authored by Laurenz Stampfl that day.
- [Maintainer profile](https://github.com/LaurenzV): Laurenz Stampfl; public location
  Vienna, Austria; company Canva at review time.
- [Crates.io ownership API](https://crates.io/api/v1/crates/hayro/owners): sole
  owner LaurenzV. Version 0.9.0 was published by that account on 2026-11-02.
- Crates.io checksum: `34d3977ae664ab6eea015be3024379c83c6a3580f67d51be6884f1992bf683ba`.

These records show no Russian organizational ownership. They cannot certify the
nationality, ancestry, residence or affiliations of every contributor, historical
contributor or transitive dependency author. No such conclusions are inferred
from names. This is a public ownership/location review, not a supply-chain audit.

Use the registry release and committed lockfile. The [0.9 release](https://github.com/LaurenzV/hayro/releases/tag/hayro-v0.9.0)
removes the SIMD opt-out: this review accepts its mandatory Rust SIMD paths,
including `fearless_simd`, `pic-scale` and `vello_cpu`. This supersedes the
0.8 decision to disable optional SIMD. Rendering remains behind `pdf-ocr`,
with existing page/pixel/stream budgets and cooperative deadlines; it is not
a process isolation boundary. `RenderContent::PAGE` explicitly excludes
annotations, preserving the prior behavior. New render settings use upstream
defaults around the existing interpolation setting. MSRV is 1.92. `embed-fonts` and `embed-cmaps` are
enabled: the binary carries PDFium/Foxit standard fonts and Adobe CMaps
(BSD-3-Clause; binary redistribution must reproduce the notices) and CC0 ICC
profiles. Root `NOTICE` reproduces them; see Licensing below. The renderer and its codec/font/
color dependencies are implemented in Rust. It replaces the Poppler executable;
Tesseract remains necessary for text recognition.

## Email parser review (2026-11-25 Chicago)

`mail-parser` 1.0.2 is a Rust MIME parser under MIT OR Apache-2.0 from
[Stalwart Labs](https://github.com/stalwartlabs/mail-parser). The
[registry ownership API](https://crates.io/api/v1/crates/mail-parser/owners)
lists sole owner [Maurus Decimus](https://github.com/mdecimus), whose profile
lists Stalwart Labs and no location. That account published 1.0.2 on
2026-11-23; checksum
`2e57f7caaeb0071026b4be388c81e9c253853d9ed072a3061d425ef1c1b1b569`.
These records do not establish every contributor's residence or affiliations.

The registry source, manifest and changelog were inspected for the 0.11-to-1.0
rewrite. It requires Rust 1.98 and retains no default features. Body decoding
is lazy; Markkit reads `text_body()` views, warns on transfer-decode problems
from `decoded_checked()` and lists nested-message attachments without
extracting their content. Header/address views preserve the prior flattening.
The parser has default caps of 64 nesting levels, 10,000 parts and three
encoded-message nesting levels; reaching an upstream cap may truncate the
parsed structure. Markkit still bounds input and charges parsed parts/body
nodes to its shared budget after the non-interruptible parse.

New unsafe Rust lives in SIMD scanning and UTF-8 span resolution. Reviewed
`scan/mod.rs` CPU dispatch, `scan/simd.rs` full-vector load bounds/scalar tails
and `store/str.rs` validated UTF-8/immutable source-span invariants. x86 AVX2
and SSSE3 are runtime-detected; aarch64 NEON uses its baseline target feature;
other targets retain portable scanning. `encodify` and `simdutf8` add decoding
and validation paths. This accepts the dependency migration under the existing
untrusted-input/host-isolation contract; it is a focused integration review,
not a complete audit of all unsafe transitive code. Email regression tests
cover malformed encodings, short/vector-boundary inputs, nested messages,
header injection, limits and attachments. No native library or network
dependency is added by this feature.

## Optional Rust OCR review (2026-11-25 Chicago)

`ocrs` 0.14.0 and RTen 0.28 implement CPU inference in Rust. The sole registry
owner is [Robert Knight](https://github.com/robertknight), whose public profile
lists London, United Kingdom, and Shortest Path Software. Public ownership
records show no Russian organizational owner; this is not a statement about
every contributor's nationality or affiliations.

The model pair recommended by the upstream example is pinned by SHA256 and
size in `scripts/ocr_models.json`. Its model card identifies CC-BY-SA 4.0;
weights are separate deployment assets, not embedded in the MIT/Apache library.
The setup script records license/attribution in NOTICE.json. The backend never
uses Python, ONNX Runtime C++, Tesseract or Leptonica; ONNX is a model file format
read by the Rust RTen runtime. Tesseract stays available/default when selected.

## Remaining native components

| Component | Where / reason |
|---|---|
| Tesseract (C++) and Leptonica (C) | Default Tesseract OCR backend; host-installed executable and image-processing libraries, plus language data. System codec dependencies vary by distribution. |
| AWS-LC (`aws-lc-sys`, C/assembly) | Optional `http_client`: reqwest → rustls → aws-lc-rs. CLI, Python and native Node packages enable HTTP. It is not part of text-only/default core or offline all-formats parsing. |
| CPython (C) | Python binding's host runtime through PyO3; not a core parser dependency. |
| Node/V8 (C++) | Native Node binding's host runtime through N-API; not a core parser dependency. |
| `alloca` C shim | Criterion development/benchmark dependency only; not production dependency edges. |
| OS APIs, libc and platform trust stores | Rust standard library/OS bindings and HTTP certificate verification; platform dependent. |

`ring` appears in Cargo.lock/metadata as an optional dependency of TLS packages,
but is not enabled in the reviewed workspace's all-features graph (including
`--target all`). Cargo.lock presence alone does not imply linked native code.
`rayon-core`/`wasm-bindgen-shared` also have Cargo `links` metadata but are Rust;
that field alone does not prove C/C++ content. ZIP compression uses Rust zlib-rs/
miniz_oxide rather than system zlib in the reviewed production graph.

Reproduce with `cargo metadata --locked --all-features --format-version 1`,
`cargo tree --workspace --all-features -i aws-lc-sys`,
`cargo tree --workspace --all-features -i alloca`, and target-specific dependency
trees. Inspect build scripts and active dependency edges, not just names ending
in `-sys`. `ldd` on the installed Tesseract describes that host's libraries only.

## Licensing and attribution

Markkit is MIT. Root `LICENSE` and `NOTICE` are the single source; every
published artifact ships both: crates.io crates via symlinks in
`crates/markkit{,-cli}/` (cargo packages link targets), the native npm package
via byte-identical copies in `crates/markkit-js/` (npm skips symlinks), WASM via
`build.sh` copying into `pkg/`, and Python via PEP 639 `license`/`license-files`
in `pyproject.toml` (wheel `dist-info/licenses/`, `License-Expression: MIT`).
`scripts/tests/test_licensing.py` enforces all of this plus NOTICE coverage of
hayro's embedded assets; CI pack checks assert LICENSE/NOTICE in npm archives.

`NOTICE` is required, not decorative: the CLI, Python and native Node packages
enable `pdf-ocr` and so embed BSD-3-Clause data. Dependency crates are
permissive (`deny.toml` allowlist); MPL-2.0 crates (cssparser, selectors) are
used unmodified, source on crates.io. OCR model weights (CC-BY-SA 4.0) and
Tesseract are never bundled. When adding a dependency that embeds data or
requires notice reproduction, update `NOTICE` and the test.

## See also

- [Format Support](../formats/format-support.md): OCR support and renderer limits.
- [Threat Model](threat-model.md): untrusted-input budgets and host isolation.
