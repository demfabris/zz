#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PLATFORM="${1:-}"
if [[ "$PLATFORM" != "linux" || "$(uname -s)" != "Linux" ]]; then
    echo "just hot currently requires linux on Linux" >&2
    exit 2
fi
shift
cd "$ROOT"

DX_BIN="${DX_BIN:-dx}"
if ! command -v "$DX_BIN" >/dev/null 2>&1; then
    echo "install dioxus-cli 0.7.10, or set DX_BIN to its dx executable" >&2
    exit 2
fi
if [[ "$("$DX_BIN" --version)" != "dioxus 0.7.10 "* ]]; then
    echo "just hot requires dioxus-cli 0.7.10" >&2
    exit 2
fi

target_dir="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
if [[ -z "${CEF_PATH:-}" ]]; then
    for candidate in "$target_dir/debug" "$ROOT/dist/zz"; do
        if [[ -f "$candidate/libcef.so" ]]; then
            export CEF_PATH="$candidate"
            break
        fi
    done
fi
if [[ ! -f "${CEF_PATH:-}/libcef.so" ]]; then
    echo "set CEF_PATH to a complete Linux CEF runtime, or run just run linux once to stage it" >&2
    exit 2
fi

host="$(rustc -vV | sed -n 's/^host: //p')"
if ! command -v ld.lld >/dev/null 2>&1; then
    linker="$(rustc --print sysroot)/lib/rustlib/$host/bin/gcc-ld/ld.lld"
    if [[ ! -x "$linker" ]]; then
        echo "Subsecond requires ld.lld; install lld" >&2
        exit 2
    fi
    mkdir -p "$target_dir/hotreload-tools"
    ln -sf "$linker" "$target_dir/hotreload-tools/ld.lld"
    export PATH="$target_dir/hotreload-tools:$PATH"
fi

deps_dir="$target_dir/$host/hotreload/deps"
mkdir -p "$deps_dir"
ln -sfn libzz.rlib "$deps_dir/libzz-hotreload.rlib"

unset ZZ_SOCKET ZZ_PANE ZZ_SESSION TMUX TMUX_PANE ZZ_TMUX_EXECUTABLE ZZ_APP_STARTUP_DIRECTORY ZZ_STARTUP_REENTRY
mkdir -p logs
export ZZ_LOG_DIR="$ROOT/logs"
export ZZ_DEV_BUILD=1
export CARGO_BUILD_JOBS=4
exec "$DX_BIN" serve --platform linux --hot-patch \
    --package zz-hotreload --bin zz-hotreload \
    --profile hotreload --features hotreload --args app "$@"
