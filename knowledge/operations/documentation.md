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
| `README.md` | Users | Pitch, features, quick start per surface, install, feature flags |
| `docs/` | Users | Guides (`getting-started.md`) |
| rustdoc (`crates/*/src/lib.rs`) | Rust users | API reference, feature flags, examples |
| `crates/markkit-python/README.md`, `crates/markkit-js/README.md` | Binding users | Shipped as the PyPI / npm package readme |
| `knowledge/` | Contributors, agents | OKF bundle: design decisions, threats, processes |
| `AGENTS.md` | Coding agents | Commands, rules, knowledge map |
| `CHANGELOG.md` | Everyone | Keep a Changelog, Unreleased section first |

Durable engineering knowledge goes in `knowledge/`, not `docs/`. User-facing guides
link to GitHub-rendered knowledge only where no user doc exists.

## Requirements

- All public API items have rustdoc; `cargo doc --workspace --no-deps` with
  `RUSTDOCFLAGS=-Dwarnings` is clean (CI lint job)
- README feature list, feature-flag table, and binding snippets match the code
- Binding READMEs match exported functions (see [Python Package](../runtimes/python-package.md),
  [Node.js Package](../runtimes/node-package.md))
- User-facing changes get a `CHANGELOG.md` Unreleased entry
- New features include a usage example (see [Examples](examples.md))
- Knowledge changes follow the [Knowledge Maintenance Contract](../knowledge-contract.md)

## See also

- [Knowledge Maintenance Contract](../knowledge-contract.md), rules for `knowledge/`
- [Maintenance](maintenance.md), the documentation section of each pass
