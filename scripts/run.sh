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

if [[ "$PLATFORM" == "linux" ]]; then
    mkdir -p logs
    export ZZ_LOG_DIR="${ZZ_LOG_DIR:-$PWD/logs}"
    # ZZ_CARGO_FEATURES opts a dev run into compiled-out features (CLI
    # --features merges into it); the macOS path reads it inside xtask.
    if [[ "$VERBOSE" == "--verbose" ]]; then
        exec cargo run -p zz ${ZZ_CARGO_FEATURES:+--features "$ZZ_CARGO_FEATURES"} -- --verbose
    fi
    exec cargo run -p zz ${ZZ_CARGO_FEATURES:+--features "$ZZ_CARGO_FEATURES"}
fi

zig_version="${ZZ_ZIG_VERSION:?the just recipe supplies this; export it to run the script directly}"
version="$(zig version)"
if [[ "$version" != "$zig_version" ]]; then
    echo "Zig $zig_version is required, found $version" >&2
    exit 2
fi

cargo xtask bundle-cef --output dist/zz-dev
mkdir -p logs
export ZZ_LOG_DIR="${ZZ_LOG_DIR:-$PWD/logs}"

if [[ "$VERBOSE" == "--verbose" ]]; then
    ./dist/zz-dev/zz.app/Contents/MacOS/zz --verbose >/dev/null 2>&1 &
else
    ./dist/zz-dev/zz.app/Contents/MacOS/zz >/dev/null 2>&1 &
fi
disown
