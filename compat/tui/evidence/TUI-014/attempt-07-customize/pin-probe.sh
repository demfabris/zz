#!/usr/bin/env bash
set -eEuo pipefail
cd "$(dirname "$0")/../../../../.."
eval "$(sed -e 's|^COMPAT_DIR=.*|COMPAT_DIR="$PWD/compat"|' -e '/^if \[ "$SELF_CHECK" -eq 1 \]; then/,$d' compat/tui-client-commands.sh)"
attach_both_at 80 24
run_both customize-mode -t PANE
for key in open Right Down Enter Escape; do
  if [ "$key" != open ]; then tmux_outer_command send-keys -t "=$OUTER_SESSION:tmux" "$key"; fi
  sleep .3
  printf '\n%s\n' "$key"
  capture_screen tmux
  cursor_tuple tmux
done
