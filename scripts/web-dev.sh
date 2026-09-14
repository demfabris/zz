#!/usr/bin/env bash
set -euo pipefail

WEB_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WEB_GATEWAY_PID=""
WEB_SERVE_ONLY=0
if [[ "${1:-}" == "--serve-only" ]]; then
    WEB_SERVE_ONLY=1
    shift
fi
unset ZZ_SOCKET ZZ_PANE ZZ_SESSION TMUX TMUX_PANE ZZ_TMUX_EXECUTABLE ZZ_APP_STARTUP_DIRECTORY ZZ_STARTUP_REENTRY ZZ_DEV_BUILD

if [[ "$WEB_SERVE_ONLY" == 0 ]] && ! command -v cargo-watch >/dev/null 2>&1; then
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
if [[ "$WEB_SERVE_ONLY" == 0 ]]; then
    "$WEB_ROOT/scripts/build-web-wasm.sh"
fi
ZZ_DEV_BUILD=1 cargo build --locked --package zz-web --target-dir "$WEB_ROOT/target"
if [[ "$WEB_SERVE_ONLY" == 1 ]]; then
    exec "$WEB_ROOT/target/debug/zz-web" "$@"
fi
"$WEB_ROOT/target/debug/zz-web" "$@" &
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
    --watch clients/web/assets/fonts \
    --watch scripts/build-web-wasm.sh \
    --shell "$WEB_ROOT/scripts/build-web-wasm.sh"
