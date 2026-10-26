# Contributing to Markkit

Thank you for your interest in contributing to Markkit!

## Getting Started

```bash
# Clone the repository
git clone https://github.com/everruns/markkit.git
cd markkit

# Install just (task runner)
cargo install just

# Build
just build

# Run tests
just test
```

## Development Workflow

1. Fork the repository
2. Create a feature branch: `git checkout -b feat/your-feature`
3. Write a failing test, then make your changes
4. Update `knowledge/` and `CHANGELOG.md` if behavior changes
5. Run `just pre-pr` to validate
6. Submit a pull request using the template; PRs are squash-merged

## Code Quality

- Format with `cargo fmt`
- Lint with `cargo clippy -- -D warnings`
- Run license checks with `cargo deny check`
- All tests must pass

## Commit Convention

We use [Conventional Commits](https://www.conventionalcommits.org/):

- `feat(scope): description` – new feature
- `fix(scope): description` – bug fix
- `docs(scope): description` – documentation
- `test(scope): description` – test changes
- `refactor(scope): description` – code refactoring
- `chore(scope): description` – maintenance

## Testing

- Write failing tests before fixing bugs
- Cover positive and negative scenarios
- Security tests for input parsing changes
- Run `just bench` for performance-sensitive changes and commit the saved results
- Run `just examples` when the public API changes

## Knowledge

`knowledge/` is the project's design knowledge in [OKF](https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md)
format: architecture, IR, format mappings, threat model, and processes. New
features either follow the existing knowledge or update it in the same PR.
`just check-okf` validates the bundle; rules are in
[knowledge/knowledge-contract.md](knowledge/knowledge-contract.md).

## Questions?

Open an issue on the [GitHub repository](https://github.com/everruns/markkit/issues).
