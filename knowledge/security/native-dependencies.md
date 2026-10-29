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

## Hayro review (2026-10-29 UTC)

Hayro 0.8.0 is a Rust PDF renderer, licensed MIT OR Apache-2.0. Public evidence:

- [Repository](https://github.com/LaurenzV/hayro): individual account LaurenzV.
- Repository creation: 2025-04-22. The [earliest surviving default-branch commit](https://github.com/LaurenzV/hayro/commit/38efa5f7d08e68ebca2d1a209d4921c1deffeaf3)
  is “Initial commit”, authored by Laurenz Stampfl that day.
- [Maintainer profile](https://github.com/LaurenzV): Laurenz Stampfl; public location
  Vienna, Austria; company Canva at review time.
- [Crates.io ownership API](https://crates.io/api/v1/crates/hayro/owners): sole
  owner LaurenzV. Version 0.8.0 was published by that account on 2026-10-04.
- Crates.io checksum: `4675ff5acd7ca41c0c34ccd2a3a154f5c86d8b5ec92bef0a114bfb194969e2af`.

These records show no Russian organizational ownership. They cannot certify the
nationality, ancestry, residence or affiliations of every contributor, historical
contributor or transitive dependency author. No such conclusions are inferred
from names. This is a public ownership/location review, not a supply-chain audit.

Use the registry release and committed lockfile, with embedded fonts/CMaps and
optional architecture-specific SIMD disabled. The renderer and its codec/font/
color dependencies are implemented in Rust. It replaces the Poppler executable;
Tesseract remains necessary for text recognition.

## Optional Rust OCR review (2026-10-28 Chicago)

`ocrs` 0.13.1 and RTen 0.26 implement CPU inference in Rust. The sole registry
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

## See also

- [Format Support](../formats/format-support.md): OCR support and renderer limits.
- [Threat Model](threat-model.md): untrusted-input budgets and host isolation.
