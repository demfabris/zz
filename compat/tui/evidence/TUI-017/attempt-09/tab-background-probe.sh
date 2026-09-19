#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../../../.." && pwd)"
BINARY="${1:?binary required}"
MODE="${2:-normal}"
set -- "$BINARY" "$ROOT/compat/.cache/tmux-src/tmux"
if [ "$MODE" = self-check ]; then set -- --self-check "$@"; fi
source <(sed -e "s|^COMPAT_DIR=.*|COMPAT_DIR='$ROOT/compat'|" -e '/^if \[ "$SELF_CHECK" -eq 1 \]; then$/,$d' "$ROOT/compat/tui-client-commands.sh")
attach_both_at 80 24
rich_capture_case capture-edited-tab-overwrite-background '\033[44mABC\tDEF\033[0m\r\033[6G\033[41mX\033[0m\033[5;1HNEXT' same '' -C -e -S 0 -E 4
printf 'edited tabs: assertions=%s failures=%s self_check_failures=%s\n' "$CHECKS" "$FAILURES" "$SELF_CHECK_FAILURES"
[ "$FAILURES" -eq 0 ] && [ "$SELF_CHECK_FAILURES" -eq 0 ]
