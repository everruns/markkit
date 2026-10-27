---
type: Playbook
title: Release Process
description: Versioning, agent-driven release flow, and publication to crates.io, PyPI, and npm.
tags:
  - markkit
  - release
---

# Release Process

## Versioning

Follows [Semantic Versioning](https://semver.org/):
- MAJOR: Breaking API changes
- MINOR: New features, backward-compatible
- PATCH: Bug fixes

## Release Checklist

Releases are agent-driven; CI creates the tag, GitHub Release and crates.io upload.

1. Ask the agent: "Create release v0.X.Y".
2. Agent bumps `version` in root `Cargo.toml`, `crates/markkit-js/package.json` and
   `crates/markkit-wasm/package.json`, moves `CHANGELOG.md` Unreleased entries
   under `## [0.X.Y] - YYYY-MM-DD`, runs `just pre-pr`, and opens a PR titled
   `chore(release): prepare v0.X.Y`.
3. Merging that PR to `main` (squash keeps the title as the commit subject) runs
   `release.yml`, which:
   - verifies the version matches `Cargo.toml` and the commit is on `main`;
   - creates tag `v0.X.Y` (or verifies an existing one points at the same commit);
   - creates the GitHub Release from the CHANGELOG section;
   - dispatches `publish.yml` on the tag.
4. `publish.yml` runs from the tag, dry-runs then uploads `markkit` then `markkit-cli`,
   skipping any crate already published at that version.

Manual fallback: run the **Release** workflow from `main` (workflow_dispatch); it reads the
version from `Cargo.toml`. Python and npm packages are published separately with
`publish-python.yml` / `publish-js.yml` / `publish-wasm.yml`.

## Artifacts

| Target | Tool | Registry |
|--------|------|----------|
| Rust crate | `cargo publish` | crates.io |
| Python wheel | `maturin build` | PyPI |
| npm package | `napi build` | npm |
| npm WASM package | `wasm-bindgen` | npm (`@everruns/markkit-wasm`, OIDC) |

## Native package scope and verification

Run the Python and npm publishers from the same verified release tag as Rust.
The initial npm artifact supports Linux x64/glibc only, and the current Python
wheel publisher targets Linux x64/CPython 3.12 only. Other platforms and interpreters
are not claimed as available; a future native release can expand coverage.

Both publishers default to a credential-free dry run. npm publication requires the
`NPM_TOKEN` secret and fails before building when it is missing, without printing
values; Python publishes through PyPI Trusted Publishing (OIDC), with no stored token. Do not create credentials or alter permissions to complete a release.
Before upload, npm installs and loads its packed tarball (loader plus native binary),
and Python checks metadata and imports the built wheel in a clean virtual environment.
Both builds must preserve the patched Cargo.lock.

## See also

- [Maintenance](maintenance.md), the pre-release pass
- [Python Package](../runtimes/python-package.md), wheel scope
- [Node.js Package](../runtimes/node-package.md), npm scope
