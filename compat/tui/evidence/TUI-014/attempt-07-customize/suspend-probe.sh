#!/usr/bin/env bash
set -eEuo pipefail
cd "$(dirname "$0")/../../../../.."
eval "$(sed -e 's|^COMPAT_DIR=.*|COMPAT_DIR="$PWD/compat"|' -e '/^if \[ "$SELF_CHECK" -eq 1 \]; then/,$d' compat/tui-client-commands.sh)"
attach_both_at 80 24
for side in zz tmux; do
  side_command "$side" list-clients -F '#{client_pid} tty=#{client_tty} flags=#{client_flags}'
  side_command "$side" list-clients -F '#{client_pid}' > "$SCRATCH_DIR/$side.pid"
done
case_run suspend-client same '' -- suspend-client
for side in zz tmux; do
  pid="$(cat "$SCRATCH_DIR/$side.pid")"
  printf '%s state after command: ' "$side"
  ps -o stat= -p "$pid" || true
  kill -CONT "$pid" || true
done
sleep .5
case_run resume same '' -- display-message -p resumed
pid="$(cat "$SCRATCH_DIR/zz.pid")"
kill -TSTP "$pid"
sleep .5
printf 'zz state after direct SIGTSTP: '
ps -o stat= -p "$pid" || true
capture_plain zz
kill -CONT "$pid"
cat "$SCRATCH_DIR/zz-daemon.err"
printf 'probe checks=%s failures=%s\n' "$CHECKS" "$FAILURES"
