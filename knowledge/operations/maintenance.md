---
type: Playbook
title: Maintenance
description: Maintenance invocation contract and the dependency, security, test, docs, and CI checklist.
tags:
  - markkit
  - maintenance
---

# Maintenance

## Invocation contract

"Run maintenance", "maintain", and misspellings such as "maintainace" and
"maintaiance" mean **analyze, fix, and ship**: local validation, push, PR, fix
CI and review findings, then squash-merge with every required check green.
A report, a local commit, or a set of deferral issues is not a completed pass.
Only an explicit analysis-only request narrows this.

The `maintain` skill (`.agents/skills/maintain/`; `.claude/skills` symlinks there)
and the `/maintain` command implement this contract; the `ship` skill completes it.
Preserve security contracts; never weaken limits, tests, or audit policy to ship.

## When to run

- Before every minor or major release
- Monthly during patch-only periods
- After large feature merges

## Checklist

### Dependencies

- Direct dependencies at latest versions, including major upgrades: bump
  `[workspace.dependencies]`, build, test, fix API changes
- `cargo update` after bumps; `Cargo.lock` is tracked and audited
- `cargo audit` and `cargo deny check` clean; every `deny.toml` ignore still
  needed and still justified
- `cargo tree --duplicates` reviewed; no unused dependencies
- Format dependencies stay behind their feature (`docx`, `spreadsheet`, `http_client`);
  CI asserts the `docx,spreadsheet` build links no reqwest/hyper
- Dependabot (`.github/dependabot.yml`) covers cargo and github-actions weekly;
  review and merge its open PRs

### Security

- [Threat Model](../security/threat-model.md) covers every input surface and feature
- Every mitigated threat has a test listed in its Test Coverage block
- New parsers or limits add a threat entry and tests
- Status of open threats (T3 SSRF) is stated honestly

### Tests

- `just test` green; no new `#[ignore]` without a reason
- Property and security suites green
- Coverage (`coverage.yml`) reviewed for large uncovered paths

### Documentation

- `README.md` features, feature flags, quick starts match the code
- Binding READMEs and [Python Package](../runtimes/python-package.md) /
  [Node.js Package](../runtimes/node-package.md) match exported functions
- `docs/` examples compile against the current API
- rustdoc builds clean
- `CHANGELOG.md` Unreleased covers every user-facing change since the last release

### Examples

- `just examples` runs every example (also in CI)
- [Examples](examples.md) lists exactly the examples that exist

### Knowledge

- `just check-okf` green ([Knowledge Maintenance Contract](../knowledge-contract.md))
- Each concept's status matches reality (stubs called stubs, planned called planned)
- Durable decisions made since the last pass are recorded; [Update Log](../log.md) updated

### Code quality

- `cargo fmt`, `cargo clippy -D warnings` clean
- No stale `TODO`, dead code, or commented-out code
- Duplication and needless abstraction removed

### Binding parity

- Python and Node.js expose the same functions; gaps listed in
  [Node.js Package](../runtimes/node-package.md) § Binding parity

### Agent configuration

- `AGENTS.md` / `CLAUDE.md` accurate: commands work, knowledge table lists every concept
- `.claude/skills` still symlinks to `.agents/skills` (`just test-scripts` checks)

### CI health

- Latest CI on `main` green; any failure blocks the rest of the pass
- Nightly green for the past week; failures >2 consecutive days are fixed in this pass
- No `continue-on-error` without a cited open issue

#### Workflow credential audit

Audit `.github/workflows/` itself, not only run results:

- **Checkout credentials**: every `actions/checkout` sets `persist-credentials: false`
  unless the job authenticates a `git`/`gh` call (only `release.yml` pushes a tag)
- **Default permissions**: every workflow declares top-level `permissions: contents: read`;
  write scopes are opt-in per job
- **Secrets**: only in the step that needs them, through `env:`; never via `$GITHUB_ENV`
- **Triggers**: no `pull_request_target`, `workflow_run`, or `issue_comment`
- **Untrusted interpolation**: no `${{ github.event.* }}` or `${{ github.head_ref }}`
  inside a `run:` block; pass through `env:`

## Findings and external blockers

Fix findings in the active pass. A genuine external blocker (upstream bug, missing
permission) or an explicit scope split may become a tracked issue with evidence;
size alone is not a blocker. Never mark a pass complete or merge with red CI.

## Pass record

### 2026-10-26

Migrated `specs/` to the OKF `knowledge/` bundle with `just check-okf` in CI;
added the maintain skill; workflow credential audit (top-level permissions and
`persist-credentials: false` everywhere except the release tag push); examples now
run in CI; docs corrected (Python module name, example list, eval/bench stubs,
`http_client` is default and T3 SSRF is open). `cargo update` applied (6 semver
bumps); `cargo audit` and `cargo deny check` clean apart from the two allowed
unmaintained warnings, both pulled only by the stub `markkit-bench`.

Findings shipped as separate PRs in the same pass: unused dependencies (core
`async-trait`, `futures`, `regex`, `unicode-segmentation`, `encoding_rs`, `base64`,
`mime`, `lol_html`; most stub-crate deps) and pending major upgrades; indented HTML
losing paragraph breaks between blocks (smoke test of `examples/basic.rs`).
Dependency PR: 18 unused deps removed, quick-xml/scraper/html5ever/pyo3/criterion
majors applied, `deny.toml` ignores and unused license allowances dropped;
`cargo audit` reports nothing, `cargo deny` one duplicate (`syn` 1/2, proc-macro
ecosystem split).

## See also

- [Release Process](release-process.md), what follows a pre-release pass
- [Threat Model](../security/threat-model.md), the security section's source of truth
- [Testing Strategy](testing.md), the suites the pass runs
