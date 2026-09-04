#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="$ROOT/examples/ui-showcase/Cargo.toml"
TARGET_DIR="$ROOT/target/ui-showcase"
PROFILE="debug"
RELEASE=false
TOOLCHAIN="${SHOWCASE_TOOLCHAIN:-nightly}"

if [[ "${1:-}" == "--release" ]]; then
    PROFILE="release"
    RELEASE=true
elif [[ -n "${1:-}" ]]; then
    echo "usage: scripts/build-showcase-wasm.sh [--release]" >&2
    exit 2
fi

if ! rustup run "$TOOLCHAIN" rustc --version >/dev/null 2>&1; then
    echo "missing Rust $TOOLCHAIN toolchain; run: just showcase-setup" >&2
    exit 2
fi

if ! rustup target list --installed --toolchain "$TOOLCHAIN" | grep -qx 'wasm32-unknown-unknown'; then
    echo "missing Rust WASM target for $TOOLCHAIN; run: just showcase-setup" >&2
    exit 2
fi

if ! command -v wasm-bindgen >/dev/null 2>&1; then
    echo "missing wasm-bindgen 0.2.126; run: just showcase-setup" >&2
    exit 2
fi

WASM_BINDGEN_VERSION="$(wasm-bindgen --version | awk '{print $2}')"
if [[ "$WASM_BINDGEN_VERSION" != "0.2.126" ]]; then
    echo "wasm-bindgen 0.2.126 is required, found $WASM_BINDGEN_VERSION; run: just showcase-setup" >&2
    exit 2
fi

cd "$ROOT"
if [[ "$RELEASE" == true ]]; then
    rustup run "$TOOLCHAIN" cargo build --locked --lib --release \
        --manifest-path "$MANIFEST" \
        --target-dir "$TARGET_DIR" \
        --target wasm32-unknown-unknown
else
    rustup run "$TOOLCHAIN" cargo build --locked --lib \
        --manifest-path "$MANIFEST" \
        --target-dir "$TARGET_DIR" \
        --target wasm32-unknown-unknown
fi

WASM="$TARGET_DIR/wasm32-unknown-unknown/$PROFILE/zz_ui_showcase.wasm"
OUT="$ROOT/examples/ui-showcase/web/src/wasm"
mkdir -p "$OUT"
wasm-bindgen "$WASM" \
    --out-dir "$OUT" \
    --out-name zz_ui_showcase \
    --target web \
    --no-typescript

echo "showcase WASM ready: $OUT/zz_ui_showcase_bg.wasm"
