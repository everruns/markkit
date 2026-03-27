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
3. Make your changes
4. Run `just pre-pr` to validate
5. Submit a pull request

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
- Run `just bench` for performance-sensitive changes

## Specs

The `specs/` directory contains architecture and design documentation. New
features should either comply with existing specs or propose changes.

## Questions?

Open an issue on the [GitHub repository](https://github.com/everruns/markkit/issues).
