# Markkit – Extract markdown from any format
# Run `just` to see available recipes

set dotenv-load := false

# Default recipe: list all available commands
default:
    @just --list

# ─── Build & Check ──────────────────────────────────────────

# Build the workspace
build:
    cargo build --workspace

# Build in release mode
build-release:
    cargo build --workspace --release

# Run all checks (format, lint, test)
check: fmt-check lint test

# Format code
fmt:
    cargo fmt --all

# Check formatting without modifying
fmt-check:
    cargo fmt --all -- --check

# Lint with clippy
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# ─── Test ────────────────────────────────────────────────────

# Run all tests
test:
    cargo test --workspace

# Run tests with all features
test-all:
    cargo test --workspace --all-features

# Run a specific test
test-one name:
    cargo test --workspace -- {{name}}

# ─── Run ─────────────────────────────────────────────────────

# Run the CLI with arguments
run *args:
    cargo run -p markkit-cli -- {{args}}

# Extract markdown from a URL
extract url:
    cargo run -p markkit-cli -- extract {{url}}

# Extract markdown from a file
extract-file path:
    cargo run -p markkit-cli -- extract --file {{path}}

# ─── Benchmarks ──────────────────────────────────────────────

# Run benchmarks
bench *args:
    cargo run -p markkit-bench --release -- {{args}}

# ─── Eval ────────────────────────────────────────────────────

# Run LLM evaluation harness
eval *args:
    cargo run -p markkit-eval --release -- {{args}}

# ─── Security & Audit ───────────────────────────────────────

# Run cargo-deny checks
deny:
    cargo deny check

# Run cargo-audit
audit:
    cargo audit

# Run cargo-vet
vet:
    cargo vet

# ─── Release ────────────────────────────────────────────────

# Pre-PR checklist
pre-pr: fmt lint test deny
    @echo "All pre-PR checks passed"

# Check current version
version:
    @grep '^version' Cargo.toml | head -1 | cut -d'"' -f2

# Dry-run publish
publish-dry-run:
    cargo publish -p markkit --dry-run

# ─── Docs ───────────────────────────────────────────────────

# Build documentation
docs:
    cargo doc --workspace --no-deps --open

# Build documentation (no open)
docs-build:
    cargo doc --workspace --no-deps
