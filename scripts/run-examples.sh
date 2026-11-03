#!/usr/bin/env bash
# Run every Rust example (CI Test job and `just examples`). A broken example
# fails the build. Keep in sync with [[example]] targets in crates/markkit/Cargo.toml
# and knowledge/operations/examples.md. Binding examples run in python.yml,
# js.yml and the ci.yml wasm job.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

run() {
    echo "── example: $*"
    cargo run -q -p markkit "$@"
}

for ex in basic custom_selectors configuration ir_access transforms documents main_content limits; do
    run --example "$ex"
done
run --features tool --example agent_tool
run --features docx,spreadsheet,pdf --example convert
