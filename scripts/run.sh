#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PLATFORM="${1:-}"
VERBOSE=""
FEATURES=""

if [[ "$PLATFORM" != "mac" && "$PLATFORM" != "linux" ]]; then
    echo "unsupported run platform: $PLATFORM (expected: mac|linux)" >&2
    exit 2
fi

shift
while (( $# > 0 )); do
    case "$1" in
        --) ;;
        --verbose) VERBOSE="--verbose" ;;
        --features)
            if [[ "${2:-}" == "" || "${2:-}" == -* ]]; then
                echo "--features requires a feature list" >&2
                exit 2
            fi
            FEATURES="${FEATURES:+$FEATURES,}$2"
            shift
            ;;
        --features=*)
            if [[ "${1#--features=}" == "" ]]; then
                echo "--features requires a feature list" >&2
                exit 2
            fi
            FEATURES="${FEATURES:+$FEATURES,}${1#--features=}"
            ;;
        *)
            echo "unsupported run option: $1 (expected: --verbose, --features <list>)" >&2
            exit 2
            ;;
    esac
    shift
done

if [[ -n "$FEATURES" ]]; then
    export ZZ_CARGO_FEATURES="${ZZ_CARGO_FEATURES:+$ZZ_CARGO_FEATURES,}$FEATURES"
fi

if [[ "$PLATFORM" == "mac" && "$(uname -s)" != "Darwin" ]]; then
    echo "just run mac requires macOS" >&2
    exit 2
fi

if [[ "$PLATFORM" == "linux" && "$(uname -s)" != "Linux" ]]; then
    echo "just run linux requires Linux" >&2
    exit 2
fi

cd "$ROOT"

unset ZZ_SOCKET ZZ_PANE ZZ_SESSION TMUX TMUX_PANE ZZ_TMUX_EXECUTABLE ZZ_APP_STARTUP_DIRECTORY ZZ_STARTUP_REENTRY ZZ_DEV_BUILD
mkdir -p logs
export ZZ_LOG_DIR="$ROOT/logs"

if [[ "$PLATFORM" == "linux" ]]; then
    ZZ_DEV_BUILD=1 cargo build -p zz --bin zz ${ZZ_CARGO_FEATURES:+--features "$ZZ_CARGO_FEATURES"}
    ZZ_DEV_BUILD=1 cargo build -p zz-cli --bin zz_cli
    target_dir="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
    cp "$target_dir/debug/zz" "$target_dir/debug/zz-dev.$$"
    mv -f "$target_dir/debug/zz-dev.$$" "$target_dir/debug/zz-dev"
    cp "$target_dir/debug/zz_cli" "$target_dir/debug/cli.$$"
    mv -f "$target_dir/debug/cli.$$" "$target_dir/debug/cli"
    bash "$ROOT/scripts/link-dev-cli.sh" "$target_dir/debug/cli" "$target_dir/debug/zz-dev"
    exec "$target_dir/debug/zz-dev" ${VERBOSE:+"$VERBOSE"} app
fi

zig_version="${ZZ_ZIG_VERSION:?the just recipe supplies this; export it to run the script directly}"
version="$(zig version)"
if [[ "$version" != "$zig_version" ]]; then
    echo "Zig $zig_version is required, found $version" >&2
    exit 2
fi

ZZ_DEV_BUILD=1 cargo xtask bundle-cef --output dist/zz-dev
bash "$ROOT/scripts/link-dev-cli.sh" "$ROOT/dist/zz-dev/zz Dev.app/Contents/MacOS/cli" "$ROOT/dist/zz-dev/zz Dev.app/Contents/MacOS/zz"

if [[ "$VERBOSE" == "--verbose" ]]; then
    "$ROOT/dist/zz-dev/zz Dev.app/Contents/MacOS/zz" --verbose app >/dev/null 2>&1 &
else
    "$ROOT/dist/zz-dev/zz Dev.app/Contents/MacOS/zz" app >/dev/null 2>&1 &
fi
disown
