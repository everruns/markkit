---
type: Test Strategy
title: Testing Strategy
description: Test categories, fixtures, property and security tests, and CI expectations.
tags:
  - markkit
  - testing
---

# Testing Strategy

## Test Layout

| Location | Covers |
|---|---|
| `crates/markkit/src/**` `#[cfg(test)]` | Unit: detection, element conversion, builder, error paths |
| `crates/markkit/tests/html_integration.rs` | End-to-end HTML to markdown |
| `crates/markkit/tests/html_fixtures.rs` | Golden files: `tests/fixtures/html/<name>.html` vs `<name>.md` |
| `crates/markkit/tests/html_security.rs` | XSS payloads, nesting, oversized input |
| `crates/markkit/tests/html_proptest.rs` | Property tests: no panic, no script/style, determinism, limits |
| `crates/markkit/tests/docx_integration.rs`, `spreadsheet_integration.rs` | Office mapping |
| `crates/markkit/tests/document_detection.rs` | Format detection order and explicit unsupported errors |
| `crates/markkit/tests/document_security.rs` | Zip bombs, grid amplification, XML entities, malformed containers |
| `crates/markkit/tests/common/` | In-memory DOCX/XLSX/XLS/ODS fixture builders |
| `crates/markkit-cli/tests/cli_integration.rs` | CLI flags and output |
| `crates/markkit/benches/extraction.rs` | Criterion micro-benchmarks |
| `crates/markkit-bench/` | Benchmark harness; `cargo test` runs every case once |
| `crates/markkit-eval/src/**` `#[cfg(test)]` | Eval dataset validity, check semantics, scorers, subject |

Office fixtures are generated in code (`tests/common`), not committed binaries, so
each security test states exactly which bytes it feeds the parser.

## Test Categories

- **Unit**: detection, element conversion, builder, error paths
- **Integration**: HTML and office pipelines end to end, CLI
- **Property** (`proptest`): arbitrary input never panics, output never contains
  `<script>`/`<style>`, extraction is deterministic, limits hold, source
  indentation between blocks never changes output
- **Security**: one or more tests per mitigated threat; the
  [Threat Model](../security/threat-model.md) maps threats to test names
- **Benchmarks**: `just bench` runs `markkit-bench` (ops/s, throughput, allocations)
  and saves results; `just bench-criterion` for micro-benchmarks. See
  [Performance Results](performance-results.md)
- **Eval**: `just eval` scores extraction quality (mira study, not a gate); its
  unit tests run in `cargo test`. See [Evaluation Harness](eval.md)

## Rules

- Write the failing test first, then fix
- Cover positive and negative cases
- Parsing changes add security tests and update the threat model test map
- New golden fixture: add both `.html` and expected `.md`

## CI

`ci.yml` runs fmt, clippy, rustdoc (`-D warnings`), the knowledge bundle check,
`cargo audit` + `cargo deny`, `cargo test --workspace` (default and all features),
the `docx,spreadsheet` no-HTTP build with a reqwest leak check, and every example.
`nightly.yml` repeats build + test on nightly Rust daily; `coverage.yml` uploads
tarpaulin coverage (`--engine llvm`; ptrace segfaults on threaded socket tests). All must pass before merge.

## See also

- [Threat Model](../security/threat-model.md), threat-to-test map
- [Examples](examples.md), runnable examples CI executes
- [Maintenance](maintenance.md), periodic test-health review
- [Performance Results](performance-results.md), benchmark harness and stored results
