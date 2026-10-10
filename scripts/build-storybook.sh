#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="$ROOT/clients/storybook/Cargo.toml"
TARGET="$ROOT/target/storybook"
DIST="$ROOT/clients/storybook/dist"
TOOLCHAIN="${WEB_TOOLCHAIN:-nightly}"
PROFILE=debug
BUILD_ARGS=(--profile dev)

if [[ "${1:-}" == "--release" && "$#" == 1 ]]; then
    PROFILE=release
    BUILD_ARGS=(--release)
elif [[ "$#" != 0 ]]; then
    echo "usage: scripts/build-storybook.sh [--release]" >&2
    exit 2
fi

if ! command -v wasm-bindgen >/dev/null 2>&1 || [[ "$(wasm-bindgen --version)" != "wasm-bindgen 0.2.128" ]]; then
    echo "wasm-bindgen 0.2.128 is required; run: just web setup" >&2
    exit 2
fi

cd "$ROOT"
SYNTAX_ARGS=()
if [[ "${WEB_SYNTAX_HIGHLIGHTING:-1}" == 1 ]]; then
    AR="$(command -v llvm-ar || true)"
    if [[ -z "$AR" ]] && command -v brew >/dev/null 2>&1; then
        AR="$(brew --prefix llvm 2>/dev/null)/bin/llvm-ar"
    fi
    if [[ ! -x "$AR" ]]; then
        echo "syntax highlighting needs llvm-ar (brew install llvm); set WEB_SYNTAX_HIGHLIGHTING=0 to skip it" >&2
        exit 2
    fi
    TS_HEADERS="$(cargo metadata --locked --format-version 1 --manifest-path "$MANIFEST" --features syntax-highlighting \
        | python3 -c 'import json, os, sys; print(next(os.path.join(os.path.dirname(p["manifest_path"]), "wasm", "include") for p in json.load(sys.stdin)["packages"] if p["name"] == "tree-sitter-language"))')"
    TS_STUBS="$TARGET/tree-sitter-wasm-stubs"
    mkdir -p "$TS_STUBS"
    : > "$TS_STUBS/stdio.c"
    : > "$TS_STUBS/stdlib.c"
    : > "$TS_STUBS/string.c"
    export AR_wasm32_unknown_unknown="$AR"
    export CFLAGS_wasm32_unknown_unknown="-isystem $TS_HEADERS -Disdigit(c)=((unsigned)(c)-48u<10u)"
    SYNTAX_ARGS=(
        --features syntax-highlighting
        --config "target.wasm32-unknown-unknown.tree-sitter-language.wasm-headers=\"$TS_HEADERS\""
        --config "target.wasm32-unknown-unknown.tree-sitter-language.wasm-src=\"$TS_STUBS\""
    )
fi

rustup run "$TOOLCHAIN" cargo build --locked --lib "${BUILD_ARGS[@]}" ${SYNTAX_ARGS[@]+"${SYNTAX_ARGS[@]}"} \
    --manifest-path "$MANIFEST" \
    --target-dir "$TARGET" \
    --target wasm32-unknown-unknown

mkdir -p "$DIST/wasm"
wasm-bindgen "$TARGET/wasm32-unknown-unknown/$PROFILE/zz_storybook.wasm" \
    --out-dir "$DIST/wasm" \
    --out-name zz_storybook \
    --target web \
    --no-typescript
cp "$ROOT/clients/storybook/web/index.html" "$ROOT/clients/storybook/web/main.js" \
    "$ROOT/clients/storybook/web/style.css" "$DIST/"
echo "built $DIST"
