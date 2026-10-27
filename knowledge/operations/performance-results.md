---
type: Playbook
title: Performance Results
description: Benchmark harnesses, measured metrics, committed result locations, naming, and run contract.
tags:
  - markkit
  - benchmarks
  - performance
---

# Performance Results

## Status
Implemented

Benchmark runs are historical artifacts committed to the repo, following
everruns/bashkit's `bashkit-bench` layout. Reviewers compare a PR's run with the
latest committed run from the same moniker.

## Harnesses

| Harness | Purpose | Results |
|---------|---------|---------|
| `markkit-bench` (`crates/markkit-bench`) | End-to-end extraction per case: latency, ops/s, throughput, heap allocations | `crates/markkit-bench/results/` |
| Criterion (`crates/markkit/benches/extraction.rs`) | Statistically rigorous micro-benchmarks | `target/criterion/` (not committed) |

## Metrics (per case)

| Field | Meaning |
|-------|---------|
| `mean_ns`, `median_ns`, `stddev_ns`, `min_ns`, `max_ns`, `times_ns` | Wall time of timed iterations (after warmup) |
| `ops_per_sec` | `1 / mean` |
| `throughput_mib_s` | Input bytes / mean. Office inputs are compressed, so MiB/s is lower than for HTML |
| `allocations`, `allocated_bytes` | Heap allocations and bytes requested in one operation (realloc counts as one) |
| `peak_heap_bytes` | Peak live heap above the pre-call level in one operation |
| `input_bytes`, `output_bytes` | Input size, Markdown size |
| `errors`, `error_messages` | Failed operations; a run with errors exits non-zero |

Report-level: system info (moniker, OS, arch, CPUs, markkit version, git
commit, build profile), iterations, warmup, `peak_rss_bytes` (Linux `VmHWM`).

Allocation stats come from a counting global allocator in the harness binary and
are measured on one extra untimed pass, so counting overhead never enters timings.

## Corpus

Generated in `crates/markkit-bench/src/cases.rs`: deterministic, offline, no
committed binaries, within default builder limits. Categories: `html`, `docx`,
`spreadsheet`, `detect`. Adding or changing a case changes the numbers; note it
in the PR and in `log.md`, since older runs stop being comparable for that case.

## Result Files

`results/bench-<moniker>-<unix-ts>.json` plus matching `.md`. Moniker defaults
to `<hostname>-<os>-<arch>`, overridable with `--moniker`, sanitized to
`[A-Za-z0-9._-]` with no leading dot (no path traversal from hostnames).

## Run Commands

| Recipe | Saves | Use |
|--------|-------|-----|
| `just bench` | yes | Default run (20 iterations, 3 warmup) |
| `just bench-accurate` | yes | 100 iterations, 10 warmup |
| `just bench-quick [--category C] [--filter S]` | no | Exploration |
| `just bench-list` | no | List cases |
| `just bench-criterion` | no | Criterion micro-benches |

## Contract

- Release builds only (`--release`); debug numbers are meaningless.
- Commit saved runs for performance-relevant PRs and before releases.
- Compare runs from the same moniker only; CPU, OS and container differ.
- `cargo test -p markkit-bench` runs every case once, so a broken case fails CI.

## See also

- [Testing Strategy](testing.md), where benchmarks sit among test categories
- [Architecture](../foundations/architecture.md), crate map
- [Threat Model](../security/threat-model.md), limits the corpus stays within
