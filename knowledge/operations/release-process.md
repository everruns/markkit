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
| npm package | `napi build` | npm (`@everruns/markkit`, OIDC) |
| npm WASM package | `wasm-bindgen` | npm (`@everruns/markkit-wasm`, OIDC) |

## Native package scope and verification

Run the Python and npm publishers from the same verified release tag as Rust.
The initial npm artifact supports Linux x64/glibc only, and the current Python
wheel publisher targets Linux x64/CPython 3.12 only. Other platforms and interpreters
are not claimed as available; a future native release can expand coverage.

Both publishers default to a credential-free dry run. Native npm and Python publish
through their registries' Trusted Publishing (OIDC), with no stored token.
Do not create credentials or alter permissions to complete a release.

For `@everruns/markkit`, configure npm's GitHub Actions trusted publisher as
organization `everruns`, repository `markkit`, workflow `publish-js.yml`, no
environment, with direct `npm publish` allowed. This is a separate package-level
configuration from `@everruns/markkit-wasm` / `publish-wasm.yml`; changing YAML alone
does not configure npm access. Publisher access changes require maintainer approval.
The native workflow grants `id-token: write` only to its publish job on a GitHub-hosted
runner and requires npm >= 11.5.1 (Node >= 22.14.0). It rejects source commits outside
`main` history and `v*` tags that disagree with the package version. Only the explicit
input `dry_run=false` uploads; every other value runs `npm publish --dry-run`.
The dry run uses a temporary run-specific prerelease version because npm still
rejects an already-published version during `--dry-run`; the source package version
is unchanged. Its required `dry-run` tag is also simulated and never written to the
registry. A successful dry run verifies packaging, not the OIDC exchange. Verify
the first real release's publish log and npm provenance separately. A new npm trusted
publisher must complete its first successful publish within two days or it expires; create
it close to the next authorized release. See [npm trusted publishing](https://docs.npmjs.com/trusted-publishers/).
Before upload, npm installs and loads its packed tarball (loader plus native binary),
and Python checks metadata and imports the built wheel in a clean virtual environment.
Both builds must preserve the patched Cargo.lock.

## See also

- [Maintenance](maintenance.md), the pre-release pass
- [Python Package](../runtimes/python-package.md), wheel scope
- [Node.js Package](../runtimes/node-package.md), npm scope
