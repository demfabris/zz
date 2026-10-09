#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT="${STORYBOOK_PORT:-8097}"
SERVER_PID=""

cleanup() {
    if [[ -n "$SERVER_PID" ]]; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT INT TERM

if ! command -v cargo-watch >/dev/null 2>&1; then
    echo "missing cargo-watch; run: cargo install cargo-watch --locked" >&2
    exit 2
fi

cd "$ROOT"
"$ROOT/scripts/build-storybook.sh"
python3 -m http.server --bind 127.0.0.1 --directory "$ROOT/clients/storybook/dist" "$PORT" >/dev/null 2>&1 &
SERVER_PID=$!
echo "storybook at http://127.0.0.1:$PORT/ (rebuilds on save; reload the page)"

cargo watch \
    --postpone \
    --delay 0.3 \
    --watch clients/storybook/Cargo.toml \
    --watch clients/storybook/src \
    --watch clients/storybook/web \
    --watch crates/zz-ui/src \
    --watch crates/zpui-kit/src \
    --watch crates/zpui-kit/assets \
    --watch crates/zpui/src \
    --watch crates/zpui-web/src \
    --watch crates/zpui-wgpu/src \
    --watch scripts/build-storybook.sh \
    --shell "$ROOT/scripts/build-storybook.sh"
