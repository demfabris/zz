#!/usr/bin/env bash
set -euo pipefail

WEB_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WEB_GATEWAY_PID=""

if ! command -v cargo-watch >/dev/null 2>&1; then
    echo "missing cargo-watch; run: cargo install cargo-watch --locked" >&2
    exit 2
fi

cleanup() {
    if [[ -n "$WEB_GATEWAY_PID" ]]; then
        kill "$WEB_GATEWAY_PID" 2>/dev/null || true
        wait "$WEB_GATEWAY_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT INT TERM

cd "$WEB_ROOT"
"$WEB_ROOT/scripts/build-web-wasm.sh"
cargo build --locked --package zz-web --target-dir "$WEB_ROOT/target"
"$WEB_ROOT/target/debug/zz-web" --assets "$WEB_ROOT/clients/web/dist" "$@" &
WEB_GATEWAY_PID=$!

cargo watch \
    --postpone \
    --delay 0.3 \
    --watch clients/web/Cargo.toml \
    --watch clients/web/Cargo.lock \
    --watch clients/web/src \
    --watch clients/web/web \
    --watch crates/zz-client/src \
    --watch crates/zz-protocol/src \
    --watch crates/zz-terminal/src \
    --watch crates/zz-ui/src \
    --watch crates/zz-ui/assets \
    --watch examples/ui-showcase/assets/fonts \
    --watch scripts/build-web-wasm.sh \
    --shell "$WEB_ROOT/scripts/build-web-wasm.sh"
