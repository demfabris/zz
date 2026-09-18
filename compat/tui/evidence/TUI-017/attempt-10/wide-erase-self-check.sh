#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../../../.." && pwd)"
WIDE_BINARY="${1:?binary required}"
set -- --self-check "$WIDE_BINARY" "$ROOT/compat/.cache/tmux-src/tmux"
source <(sed -e "s|^COMPAT_DIR=.*|COMPAT_DIR='$ROOT/compat'|" -e '/^if \[ "$SELF_CHECK" -eq 1 \]; then$/,$d' "$ROOT/compat/tui-client-commands.sh")
attach_both_at 80 24
erased_wide_capture_cases
printf 'wide erase self-check: %s failures\n' "$SELF_CHECK_FAILURES"
[ "$SELF_CHECK_FAILURES" -eq 0 ]
