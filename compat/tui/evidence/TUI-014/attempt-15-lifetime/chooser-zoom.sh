#!/usr/bin/env bash
set -eEuo pipefail
eval "$(sed -e 's|^COMPAT_DIR=.*|COMPAT_DIR="$PWD/compat"|' -e '/^if \[ "$SELF_CHECK" -eq 1 \]; then/,$d' compat/tui-choosers.sh)"
attach_both_at 80 24
zoom_case
for side in zz tmux; do
  printf '%s zoom and prefix: ' "$side"
  side_command "$side" display-message -p '#{window_zoomed_flag} #{client_prefix}'
done
printf 'chooser zoom checks=%s failures=%s\n' "$CHECKS" "$FAILURES"
[ "$FAILURES" -eq 0 ]
