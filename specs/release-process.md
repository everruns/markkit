# Release Process

## Versioning

Follows [Semantic Versioning](https://semver.org/):
- MAJOR: Breaking API changes
- MINOR: New features, backward-compatible
- PATCH: Bug fixes

## Release Checklist

Releases are agent-driven; CI creates the tag, GitHub Release and crates.io upload.

1. Ask the agent: "Create release v0.X.Y".
2. Agent bumps `version` in root `Cargo.toml`, moves `CHANGELOG.md` Unreleased entries
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
`publish-python.yml` / `publish-js.yml`.

## Artifacts

| Target | Tool | Registry |
|--------|------|----------|
| Rust crate | `cargo publish` | crates.io |
| Python wheel | `maturin build` | PyPI |
| npm package | `napi build` | npm |
