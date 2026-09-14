#!/usr/bin/env bash
set -euo pipefail

WEB_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WEB_MANIFEST="$WEB_ROOT/clients/web/Cargo.toml"
WEB_TARGET="$WEB_ROOT/target/web-client"
WEB_DIST="$WEB_ROOT/clients/web/dist-dev"
WEB_DEV_BUILD=1
WEB_TOOLCHAIN="${WEB_TOOLCHAIN:-nightly}"
WEB_PROFILE="debug"
WEB_BUILD_ARGS=(--profile dev)

if [[ "${1:-}" == "--release" && "$#" == 1 ]]; then
    WEB_PROFILE="release"
    WEB_BUILD_ARGS=(--release)
    WEB_DIST="$WEB_ROOT/clients/web/dist"
    WEB_DEV_BUILD=0
elif [[ "$#" != 0 ]]; then
    echo "usage: scripts/build-web-wasm.sh [--release]" >&2
    exit 2
fi

if ! rustup run "$WEB_TOOLCHAIN" rustc --version >/dev/null 2>&1; then
    echo "missing Rust $WEB_TOOLCHAIN; run: just web-setup" >&2
    exit 2
fi

if ! rustup target list --installed --toolchain "$WEB_TOOLCHAIN" | rg -qx 'wasm32-unknown-unknown'; then
    echo "missing WASM target; run: just web-setup" >&2
    exit 2
fi

if ! command -v wasm-bindgen >/dev/null 2>&1 || [[ "$(wasm-bindgen --version)" != "wasm-bindgen 0.2.128" ]]; then
    echo "wasm-bindgen 0.2.128 is required; run: just web-setup" >&2
    exit 2
fi

cd "$WEB_ROOT"
ZZ_DEV_BUILD="$WEB_DEV_BUILD" rustup run "$WEB_TOOLCHAIN" cargo build --locked --lib "${WEB_BUILD_ARGS[@]}" \
    --manifest-path "$WEB_MANIFEST" \
    --target-dir "$WEB_TARGET" \
    --target wasm32-unknown-unknown

mkdir -p "$WEB_DIST/wasm" "$WEB_DIST/licenses"
wasm-bindgen "$WEB_TARGET/wasm32-unknown-unknown/$WEB_PROFILE/zz_web_client.wasm" \
    --out-dir "$WEB_DIST/wasm" \
    --out-name zz_web_client \
    --target web \
    --no-typescript
cp "$WEB_ROOT/clients/web/web/index.html" "$WEB_DIST/index.html"
cp "$WEB_ROOT/clients/web/web/main.js" "$WEB_DIST/main.js"
cp "$WEB_ROOT/clients/web/web/style.css" "$WEB_DIST/style.css"
WEB_ICON=zz
if [[ "$WEB_DEV_BUILD" == 1 ]]; then WEB_ICON=zz-dev; fi
cp "$WEB_ROOT/assets/linux/hicolor/256x256/apps/$WEB_ICON.png" "$WEB_DIST/favicon.png"
cp "$WEB_ROOT/clients/web/assets/fonts/inter/LICENSE.txt" "$WEB_DIST/licenses/inter.txt"
cp "$WEB_ROOT/clients/web/assets/fonts/lilex/LICENSE.txt" "$WEB_DIST/licenses/lilex.txt"

echo "browser client ready: $WEB_DIST"
