#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../../../.." && pwd)"
RESIDUAL_CASE="${1:?case required}"
RESIDUAL_BINARY="${2:?binary required}"
set -- --self-check "$RESIDUAL_BINARY" "$ROOT/compat/.cache/tmux-src/tmux"
source <(sed -e "s|^COMPAT_DIR=.*|COMPAT_DIR='$ROOT/compat'|" -e '/^if \[ "$SELF_CHECK" -eq 1 \]; then$/,$d' "$ROOT/compat/tui-client-commands.sh")
attach_both_at 80 24
case "$RESIDUAL_CASE" in
  capture-tab-trailing)
    rich_capture_case capture-tab-trailing 'ABC\t\r\nNEXT' same '' -C -S 0 -E 4 ;;
  capture-tab-internal)
    rich_capture_case capture-tab-internal 'ABC\tDEF\r\nNEXT' same '' -C -S 0 -E 4 ;;
  capture-tab-wide)
    rich_capture_case capture-tab-wide '界\t\r\nNEXT' same '' -C -S 0 -E 4 ;;
  capture-low-indexed-colour)
    rich_capture_case capture-low-indexed-colour '\033[38;5;1mRED\033[0m\r\nNEXT' same '' -C -e -S 0 -E 0 ;;
  lock-session|has-session|list-windows)
    run_on_both set-option -gw pane-base-index 1
    self_check_run "$RESIDUAL_CASE-base-index-equivalence" "$RESIDUAL_CASE" -t '=cli:win.1'
    self_check_expect "$RESIDUAL_CASE resolves pane-base-index 1" exit=0 stdout=0 stderr=0
    zz_command set-option -gw pane-base-index 2 >/dev/null
    self_check_run "$RESIDUAL_CASE-base-index-sabotage" "$RESIDUAL_CASE" -t '=cli:win.1'
    expected_stdout=0
    [ "$RESIDUAL_CASE" != list-windows ] || expected_stdout=1
    self_check_expect "$RESIDUAL_CASE loses pane 1" exit=1 "stdout=$expected_stdout" stderr=1 ;;
  *) exit 2 ;;
esac
printf 'residual self-check %s: %s failures\n' "$RESIDUAL_CASE" "$SELF_CHECK_FAILURES"
[ "$SELF_CHECK_FAILURES" -eq 0 ]
