## Coding-agent guidance

### Style

Telegraph. Drop filler/grammar. Min tokens.

### Critical Thinking

Fix root cause. Unsure: read more code; if stuck, ask w/ short options. Unrecognized changes: assume other agent; keep going. If causes issues, stop + ask.

### Principles

- Always work on top of latest remote main: fetch `origin/main`, rebase (or recreate worktree) before editing
- Important decisions as comments on top of file
- Code testable, smoke testable, runnable locally
- Small, incremental PR-sized changes
- Write failing test before fixing bug
- All input is untrusted: parsing changes need security tests

### Knowledge

`knowledge/` is the canonical OKF bundle and persistent project memory (replaces the old `specs/`). Read relevant knowledge before changing behavior; update it in the same change when decisions, behavior, constraints, threats, tests, or operations change. Rules: `knowledge/knowledge-contract.md` (`just check-okf` enforces OKF v0.2).

| Knowledge | Description |
|------|-------------|
| knowledge-contract | Knowledge maintenance rules + OKF v0.2 conformance rules |
| foundations/architecture | Extraction pipeline, design principles, crate structure, builder config |
| foundations/ir | Canonical IR between parsers and the markdown renderer |
| formats/html-extraction | HTML element mapping, stripping, whitespace, depth limits |
| formats/format-support | Formats, bytes-in API, detection order, DOCX + spreadsheet mapping |
| integrations/tool-contract | `MarkkitTool` LLM tool (`tool` feature), Everruns toolkit contract |
| security/security-model | One-page threat/invariant summary |
| security/threat-model | Threats T1-T6, mitigations, test map, resource limits, invariants S1-S12 |
| runtimes/python-package | PyO3 `markkit` module, maturin, PyPI |
| runtimes/wasm-package | wasm-bindgen `@everruns/markkit-wasm`, browser/edge, build + tests |
| runtimes/node-package | NAPI-RS `@everruns/markkit`, npm, binding parity table |
| operations/testing | Test layout, categories, CI jobs |
| operations/documentation | Doc trees and what each must cover |
| operations/examples | `examples/` contents and requirements |
| operations/maintenance | Maintenance invocation contract + checklist + workflow credential audit |
| operations/release-process | Agent-driven release, crates.io + PyPI + npm |
| operations/performance-results | Benchmark harness, metrics, committed results |
| operations/eval | mira eval study: dataset, checks, metrics, baseline |

### Documentation

- User docs: `README.md`, `docs/`
- API reference: rustdoc in `crates/markkit/src/lib.rs` and modules
- Binding READMEs (`crates/markkit-python/README.md`, `crates/markkit-js/README.md`, `crates/markkit-wasm/README.md`) ship to PyPI/npm, keep them in sync with exports
- Durable engineering knowledge: `knowledge/`, never `docs/`

### Shipping

"Ship" / "ship it" / "fix and ship" = landed on `main` via squash-merge, every CI check green on the PR and on the merge commit's `main` run, tested (positive, negative, security where relevant), smoke tested, knowledge/docs/CHANGELOG updated, ready to release. Use the `ship` skill. Not done at push, PR, or auto-merge; done when `main` is green. Red `main` after merge: fix forward immediately. Ship ≠ release: tag/publish only on "Create release" (`knowledge/operations/release-process.md`).

### Maintenance invocation

"Run maintenance" (incl. "maintainace" / "maintaiance") = analyze, fix, validate, ship through green CI and squash-merge. Use the `maintain` and `ship` skills; checklist in `knowledge/operations/maintenance.md`. Don't stop at a report, local commit, or deferral issues. Analysis-only needs an explicit request. Preserve security contracts; never merge red CI.

### Cloud Agent Setup

```bash
./scripts/init-cloud-env.sh   # Install just + gh
just build
```

### Local Dev

```bash
just --list        # All commands
just build         # Build
just test          # cargo test --workspace
just lint          # clippy -D warnings
just check         # fmt + clippy + test + knowledge + scripts + examples
just check-okf     # knowledge/ OKF conformance
just examples      # Run every example
just bench         # Benchmark harness, saves results to crates/markkit-bench/results
just bench-criterion # Criterion micro-benches (crates/markkit/benches)
just pre-pr        # Pre-PR checks
```

### Crate Map

| Crate | Purpose |
|-------|---------|
| `markkit` | Core extraction library (HTML, DOCX, spreadsheets) |
| `markkit-cli` | `markkit` CLI |
| `markkit-bench` | Benchmark harness: ops/s, throughput, allocations; results committed |
| `markkit-eval` | Extraction-quality eval, mira study (`just eval`) |
| `markkit-python` | Python bindings (PyO3), module `markkit` |
| `markkit-js` | Node.js bindings (NAPI-RS), package `@everruns/markkit` |
| `markkit-wasm` | Browser/edge WebAssembly (wasm-bindgen), package `@everruns/markkit-wasm` |

### Rust

- Stable Rust, pinned in `rust-toolchain.toml`
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`
- Licenses/advisories: `cargo deny check` (`deny.toml`), `cargo audit`
- Format deps stay feature-gated; `docx,spreadsheet` build must not link reqwest/hyper (CI checks)

### Pre-PR Checklist

1. `just pre-pr` (fmt, clippy, test, check-okf, script tests, examples, deny)
2. Tests cover positive and negative cases
3. Security tests if change touches parsing, limits, or URL fetching (see `knowledge/security/threat-model.md`)
4. Rebase on main: `git fetch origin main && git rebase origin/main`
5. Update knowledge if behavior or durable facts change; add a `knowledge/log.md` entry
6. `CHANGELOG.md` Unreleased entry for user-facing changes
7. CI green before merge; resolve all PR comments
8. After merge: `main` CI green on the merge commit

### CI

- GitHub Actions; `ci.yml` aggregate `Check` job gates merge
- **NEVER merge when CI is red.** No exceptions.

### Commits

[Conventional Commits](https://www.conventionalcommits.org): `type(scope): description`

Types: feat, fix, docs, refactor, test, chore, ci

- Updates to `knowledge/` and `AGENTS.md`: `chore` type
- NEVER add links to Claude sessions in PR body or commits

### Commit Attribution

All commits attributed to the real human user, never a bot/agent identity, no AI `Co-authored-by` trailers, no "generated by" text. Verify `git config user.name`/`user.email` are human before committing; if missing or bot-like, set from `$GIT_USER_NAME`/`$GIT_USER_EMAIL` (`.claude/hooks/fix-git-identity.sh` does this at session start). If those are missing too, stop and ask.

### PRs

Squash and Merge. Use `.github/pull_request_template.md`. Center the description on functional change and impact; add Before / After evidence (CLI output, test results) whenever behavior changes.

- Prefer small, shippable PRs; split independent changes
- When asked for separate PRs, don't bundle

See `CONTRIBUTING.md` for details.
