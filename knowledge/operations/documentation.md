---
type: Playbook
title: Documentation Architecture
description: Where public docs, rustdoc, and the knowledge bundle live and what each must cover.
tags:
  - markkit
  - docs
---

# Documentation Architecture

## Trees

| Tree | Audience | Holds |
|---|---|---|
| `README.md` | Users | Pitch, surfaces table, formats, quick start per surface, examples, configuration, limits, features, performance, security |
| `docs/` | Users | `getting-started.md` (Rust walkthrough), `pdf.md` (PDF text, layout, OCR, limits) |
| `examples/README.md` | Users | Index of Rust, Python, Node.js and WASM examples |
| rustdoc (`crates/*/src/lib.rs`) | Rust users | API reference, feature table, examples; docs.rs builds with `[package.metadata.docs.rs]` features |
| `crates/markkit-python/README.md` | Python users | Shipped as the PyPI readme: install, API table, result keys, limits, errors, PDF |
| `markkit.pyi` (repo root) | Python users | Type stubs; maturin ships it as `markkit/__init__.pyi` + `py.typed` |
| `crates/markkit-js/README.md` | Node.js users | Shipped as the npm readme; `index.d.ts` is generated from napi rustdoc in `crates/markkit-js/src/lib.rs` |
| `crates/markkit-wasm/README.md`, `crates/markkit-wasm/js/index.d.ts` | Browser/edge users | Shipped in the WASM npm package |
| `knowledge/` | Contributors, agents | OKF bundle: design decisions, threats, processes |
| `AGENTS.md` | Coding agents | Commands, rules, knowledge map |
| `CHANGELOG.md` | Everyone | Keep a Changelog, Unreleased section first |

Durable engineering knowledge goes in `knowledge/`, not `docs/`. User-facing guides
link to GitHub-rendered knowledge only where no user doc exists. Binding READMEs
use absolute GitHub links: they render on PyPI and npm, where relative links break.

## Requirements

- All public API items have rustdoc; `cargo doc --workspace --no-deps` with
  `RUSTDOCFLAGS=-Dwarnings` is clean (CI lint job)
- docs.rs: `crates/markkit/Cargo.toml` `[package.metadata.docs.rs]` enables every
  feature except `pdf-ocr-ocrs` and sets `--cfg docsrs`; `lib.rs` turns on
  `doc_cfg` under `docsrs` so items show their feature badge
- Rust snippets in `README.md`, `docs/getting-started.md` and `docs/pdf.md` are
  doctests (`#[cfg(doctest)]` `include_str!` structs in `crates/markkit/src/lib.rs`).
  Write them self-contained: wrap `?` and undefined inputs in a `fn`, use
  `rust,no_run` for file I/O, never hidden `# ` lines (GitHub renders them).
  Fences without a language tag are compiled as Rust
- Python stubs match the native module: `crates/markkit-python/tests/test_stubs.py`
  compares function names, parameter names and `Limits` fields; `python.yml`
  asserts the wheel contains `__init__.pyi` and `py.typed`
- README feature list, feature-flag table, and binding snippets match the code
- Binding READMEs match exported functions (see [Python Package](../runtimes/python-package.md),
  [Node.js Package](../runtimes/node-package.md))
- User-facing changes get a `CHANGELOG.md` Unreleased entry
- New features include a usage example (see [Examples](examples.md))
- Knowledge changes follow the [Knowledge Maintenance Contract](../knowledge-contract.md)

## See also

- [Knowledge Maintenance Contract](../knowledge-contract.md), rules for `knowledge/`
- [Maintenance](maintenance.md), the documentation section of each pass
