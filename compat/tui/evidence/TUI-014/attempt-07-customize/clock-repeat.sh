#!/usr/bin/env bash
set -eEuo pipefail
cd "$(dirname "$0")/../../../../.."
eval "$(sed -e 's|^COMPAT_DIR=.*|COMPAT_DIR="$PWD/compat"|' -e '/^if \[ "$SELF_CHECK" -eq 1 \]; then/,$d' compat/tui-client-commands.sh)"
attach_both_at 80 24
for clock_style in 24-with-seconds 12-with-seconds; do
  set_window_on_both clock-mode-style "$clock_style"
  CASE_CLOCK_FACE=2
  case_run "clock-repeat-$clock_style" same '' -- clock-mode -t PANE
  restore_case "clock-repeat-$clock_style-closed"
done
printf 'clock repeat checks=%s failures=%s\n' "$CHECKS" "$FAILURES"
[ "$FAILURES" -eq 0 ]
