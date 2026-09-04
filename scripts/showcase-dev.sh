#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WEB="$ROOT/examples/ui-showcase/web"

if ! command -v cargo-watch >/dev/null 2>&1; then
    echo "missing cargo-watch; install it with: cargo install cargo-watch --locked" >&2
    exit 2
fi

if ! command -v npm >/dev/null 2>&1; then
    echo "missing npm; Node.js is required for the showcase dev server" >&2
    exit 2
fi

if [[ ! -x "$WEB/node_modules/.bin/vite" ]]; then
    npm --prefix "$WEB" install
fi

if [[ "$(uname -s)" == "Darwin" ]]; then
    python3 "$ROOT/scripts/prepare-showcase-fonts.py"
fi

"$ROOT/scripts/build-showcase-wasm.sh"

cleanup() {
    if [[ -n "${VITE_PID:-}" ]]; then
        kill "$VITE_PID" 2>/dev/null || true
        wait "$VITE_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT INT TERM

npm --prefix "$WEB" run dev &
VITE_PID=$!

cd "$ROOT"
cargo watch \
    --postpone \
    --delay 0.3 \
    --watch examples/ui-showcase/Cargo.toml \
    --watch examples/ui-showcase/Cargo.lock \
    --watch examples/ui-showcase/src \
    --watch examples/ui-showcase/assets \
    --watch crates/zz-ui/Cargo.toml \
    --watch crates/zz-ui/src \
    --watch crates/zz-ui/assets \
    --watch assets \
    --watch scripts/build-showcase-wasm.sh \
    --shell "$ROOT/scripts/build-showcase-wasm.sh"
