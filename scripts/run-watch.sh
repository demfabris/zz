#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PLATFORM="${1:-}"
MODE="watch"
FEATURES=""

usage() {
    echo "usage: scripts/run-watch.sh <mac|linux> [--features <list>]" >&2
    exit 2
}

if [[ "$PLATFORM" != "mac" && "$PLATFORM" != "linux" ]]; then
    usage
fi

shift
while (( $# > 0 )); do
    case "$1" in
        --) ;;
        --reload) MODE="--reload" ;;
        --features)
            if [[ "${2:-}" == "" || "${2:-}" == -* ]]; then
                usage
            fi
            FEATURES="${FEATURES:+$FEATURES,}$2"
            shift
            ;;
        --features=*)
            if [[ "${1#--features=}" == "" ]]; then
                usage
            fi
            FEATURES="${FEATURES:+$FEATURES,}${1#--features=}"
            ;;
        *) usage ;;
    esac
    shift
done

if [[ -n "$FEATURES" ]]; then
    export ZZ_CARGO_FEATURES="${ZZ_CARGO_FEATURES:+$ZZ_CARGO_FEATURES,}$FEATURES"
fi

if [[ "$PLATFORM" == "mac" && "$(uname -s)" != "Darwin" ]]; then
    echo "just watch mac requires macOS" >&2
    exit 2
fi

if [[ "$PLATFORM" == "linux" && "$(uname -s)" != "Linux" ]]; then
    echo "just watch linux requires Linux" >&2
    exit 2
fi

declare -a OLD_CLIENT_PIDS=()

capture_clients() {
    local client_path="$1"
    local pid

    OLD_CLIENT_PIDS=()
    while IFS= read -r pid; do
        if [[ -n "$pid" ]]; then
            OLD_CLIENT_PIDS+=("$pid")
        fi
    done < <(pgrep -fx "$client_path" || true)
}

stop_old_clients() {
    if (( ${#OLD_CLIENT_PIDS[@]} > 0 )); then
        kill "${OLD_CLIENT_PIDS[@]}" 2>/dev/null || true
    fi
}

reload_mac() {
    local client_path="$ROOT/dist/zz-dev/zz.app/Contents/MacOS/zz"

    capture_clients "$client_path"
    just run mac
    stop_old_clients
}

reload_linux() {
    local client_path="$ROOT/target/debug/zz"

    command -v setsid >/dev/null 2>&1 || {
        echo "just watch linux requires setsid (normally provided by util-linux)" >&2
        exit 2
    }

    capture_clients "$client_path"
    cargo build -p zz --bin zz ${ZZ_CARGO_FEATURES:+--features "$ZZ_CARGO_FEATURES"}
    setsid -f "$client_path"
    stop_old_clients
}

cd "$ROOT"

if [[ "$MODE" == "--reload" ]]; then
    if [[ "$PLATFORM" == "mac" ]]; then
        reload_mac
    else
        reload_linux
    fi
    exit 0
fi

command -v cargo-watch >/dev/null 2>&1 || {
    echo "missing cargo-watch; install it with: cargo install cargo-watch --locked" >&2
    exit 2
}

exec cargo watch \
    --delay 0.4 \
    --shell "scripts/run-watch.sh $PLATFORM --reload"
