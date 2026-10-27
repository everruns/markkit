# Markkit – Extract markdown from any format
# Run `just` to see available recipes

set dotenv-load := false

# Keep in lockstep with the okf-lint step in .github/workflows/ci.yml
okf_lint_version := "0.1.1"
okf_lint_max_line_length := "10000"

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

# Run all checks (format, lint, test, knowledge, scripts, examples)
check: fmt-check lint test check-okf test-scripts examples

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

# Run repository script tests (scripts/tests)
test-scripts:
    python3 -m unittest discover -s scripts/tests -p 'test_*.py'

# Run every example (CI runs this too)
examples:
    #!/usr/bin/env bash
    set -euo pipefail
    for ex in basic custom_selectors configuration ir_access; do
        echo "── example: $ex"
        cargo run -q -p markkit --example "$ex"
    done
    echo "── example: agent_tool"
    cargo run -q -p markkit --features tool --example agent_tool

# Build the WASM package and run its Node tests (needs wasm32 target + wasm-bindgen-cli)
wasm:
    bash scripts/install-wasm-bindgen.sh
    bash crates/markkit-wasm/scripts/build.sh release
    node --test crates/markkit-wasm/__test__/*.test.mjs

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

# Run the benchmark harness and save JSON/Markdown results to crates/markkit-bench/results/
bench:
    cargo run -p markkit-bench --release -- --save

# Benchmark with more iterations for accuracy and save results
bench-accurate:
    cargo run -p markkit-bench --release -- --iterations 100 --warmup 10 --save

# Exploratory run, nothing saved (e.g. just bench-quick --category html)
bench-quick *args:
    cargo run -p markkit-bench --release -- {{args}}

# List benchmark cases
bench-list:
    cargo run -p markkit-bench --release -- --list

# Run criterion micro-benchmarks for the core crate
bench-criterion *args:
    cargo bench -p markkit --bench extraction {{args}}

# ─── Eval ────────────────────────────────────────────────────

# Run the LLM evaluation harness (stub, not yet implemented)
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
pre-pr: fmt lint test check-okf test-scripts examples deny
    @echo "All pre-PR checks passed"

# Check current version
version:
    @grep '^version' Cargo.toml | head -1 | cut -d'"' -f2

# Dry-run publish
publish-dry-run:
    cargo publish -p markkit --dry-run

# ─── Docs & Knowledge ───────────────────────────────────────

# okf-lint covers the OKF spec rules; check_okf.py covers bundle-local
# conventions it does not enforce (see knowledge/knowledge-contract.md).
# okf-lint is pinned in CI and skipped here when absent: just install-okf-lint
# Validate the knowledge/ OKF v0.2 bundle
check-okf:
    #!/usr/bin/env bash
    set -euo pipefail
    python3 scripts/check_okf.py knowledge
    if command -v okf-lint >/dev/null; then
        okf-lint knowledge --max-line-length {{ okf_lint_max_line_length }}
        echo "okf-lint {{ okf_lint_version }}: knowledge conforms to OKF v0.2"
    else
        echo "okf-lint not installed — skipping (just install-okf-lint)"
    fi

# Install the pinned upstream OKF spec linter
install-okf-lint:
    cargo install okf-lint --version {{ okf_lint_version }} --locked

# Build documentation
docs:
    cargo doc --workspace --no-deps --open

# Build documentation (no open)
docs-build:
    cargo doc --workspace --no-deps
