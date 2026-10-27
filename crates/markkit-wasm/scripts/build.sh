#!/usr/bin/env bash
# Build @everruns/markkit-wasm into pkg/ (ES module, `wasm-bindgen --target web`).
# Needs no bundler and no cross-origin isolation headers to load.
#
# Requires: rustup target add wasm32-unknown-unknown; bash scripts/install-wasm-bindgen.sh
# Optional (smaller output): wasm-opt (binaryen) on PATH.
set -euo pipefail

CRATE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORKSPACE_ROOT="$(cd "$CRATE_DIR/../.." && pwd)"
OUT_DIR="$CRATE_DIR/pkg"
TARGET=wasm32-unknown-unknown
PROFILE="${1:-release}"
case "$PROFILE" in
  release) PROFILE=wasm-release; TARGET_SUBDIR=wasm-release ;;
  dev|debug) PROFILE=dev; TARGET_SUBDIR=debug ;;
  *) echo "unsupported profile: $PROFILE" >&2; exit 1 ;;
esac

cd "$WORKSPACE_ROOT"
echo "==> cargo build ($PROFILE) for $TARGET"
cargo build --locked -p markkit-wasm --target "$TARGET" --profile "$PROFILE"
WASM_IN="${CARGO_TARGET_DIR:-$WORKSPACE_ROOT/target}/$TARGET/$TARGET_SUBDIR/markkit_wasm.wasm"

echo "==> wasm-bindgen --target web"
rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"
wasm-bindgen "$WASM_IN" --target web --out-dir "$OUT_DIR" --out-name markkit_wasm

if command -v wasm-opt >/dev/null 2>&1; then
  echo "==> wasm-opt -Oz"
  wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext \
    "$OUT_DIR/markkit_wasm_bg.wasm" -o "$OUT_DIR/markkit_wasm_bg.wasm"
else
  echo "==> wasm-opt not found; skipping size optimization"
fi

cp "$CRATE_DIR/js/index.js" "$CRATE_DIR/js/index.d.ts" "$OUT_DIR/"
cp "$CRATE_DIR/package.json" "$CRATE_DIR/README.md" "$OUT_DIR/"
echo "==> built $OUT_DIR"
ls -lh "$OUT_DIR"
