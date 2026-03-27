# Release Process

## Versioning

Follows [Semantic Versioning](https://semver.org/):
- MAJOR: Breaking API changes
- MINOR: New features, backward-compatible
- PATCH: Bug fixes

## Release Checklist

1. Update version in root `Cargo.toml`
2. Update `CHANGELOG.md`
3. Run `just pre-pr` (fmt, lint, test, deny)
4. Verify nightly CI is green
5. Dry-run publish: `just publish-dry-run`
6. Create git tag: `git tag -a v0.X.Y -m "Release v0.X.Y"`
7. Push tag: `git push origin v0.X.Y`
8. GitHub Actions handles release creation
9. Publish to crates.io, PyPI, npm as needed

## Artifacts

| Target | Tool | Registry |
|--------|------|----------|
| Rust crate | `cargo publish` | crates.io |
| Python wheel | `maturin build` | PyPI |
| npm package | `napi build` | npm |
