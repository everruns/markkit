#!/usr/bin/env bash
# Install wasm-bindgen-cli matching the wasm-bindgen crate in Cargo.lock.
# The CLI and the crate must agree exactly on the bindgen schema version.
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
VERSION=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/"/, "", $3); print $3 }' "$REPO_ROOT/Cargo.lock" | sort -u)
if [[ -z "$VERSION" || "$VERSION" == *$'\n'* ]]; then
    echo "Expected exactly one wasm-bindgen version in Cargo.lock" >&2
    exit 1
fi

EXPECTED="wasm-bindgen $VERSION"
if [[ "$(wasm-bindgen --version 2>/dev/null || true)" != "$EXPECTED" ]]; then
    cargo install wasm-bindgen-cli --version "$VERSION" --locked --force
fi
if [[ "$(wasm-bindgen --version)" != "$EXPECTED" ]]; then
    echo "wasm-bindgen on PATH does not match Cargo.lock ($VERSION)" >&2
    exit 1
fi
echo "$EXPECTED (matches Cargo.lock)"
