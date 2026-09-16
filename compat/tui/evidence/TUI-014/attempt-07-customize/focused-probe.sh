#!/usr/bin/env bash
set -eEuo pipefail
cd "$(dirname "$0")/../../../../.."
eval "$(sed -e 's|^COMPAT_DIR=.*|COMPAT_DIR="$PWD/compat"|' -e '/^if \[ "$SELF_CHECK" -eq 1 \]; then/,$d' compat/tui-client-commands.sh)"
attach_both_at 80 24
case_run customize-mode-open same '' -- customize-mode -t PANE
for key in Right Down Down Enter C-u 7 Enter; do
  case_run "customize-$key" same '' -- send-keys -t PANE "$key"
done
case_run customize-value same '' -- show-options -sv buffer-limit
case_run customize-close same '' -- send-keys -t PANE q
for side in zz tmux; do
  side_command "$side" list-clients -F '#{client_pid}' > "$SCRATCH_DIR/$side.pid"
done
case_run suspend-client same '' -- suspend-client
for side in zz tmux; do
  pid="$(cat "$SCRATCH_DIR/$side.pid")"
  printf '%s process %s\n' "$side" "$pid"
  ps -o stat= -p "$pid" || true
  kill -CONT "$pid" || true
done
sleep .3
case_run suspend-resumed same '' -- display-message -p -t PANE '#{pane_in_mode}/#{pane_mode}'
printf 'probe checks=%s failures=%s\n' "$CHECKS" "$FAILURES"
[ "$FAILURES" -eq 0 ]
