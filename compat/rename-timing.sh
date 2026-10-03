#!/usr/bin/env bash
# Timing differential for automatic-rename and the window-renamed hook.
#
# names.c renames a window from its active pane's command at most once per
# NAME_INTERVAL (500 ms): output inside the interval arms a timer for the rest
# of it instead of renaming, and every rename runs window_set_name, which fires
# window-renamed. This fixture runs the same shell loop in both servers, flipping
# the foreground command between perl and sh about every 200 ms, both printing,
# counts the window-renamed hooks each server fires, and checks that the window
# settles on the same name once the loop stops. MEASURED 2026-09-28 on macOS: the
# pin fires 0-1 hooks over 3 s here and zz with its throttle 0-3; a zz without
# the throttle fired about 14, one per flip. One that never catches up ends on
# a different name. Counts are timing, so they are compared with a
# tolerance, not for equality.
set -eEuo pipefail

usage() {
  printf 'usage: compat/rename-timing.sh [ZZ_BIN [TMUX_BIN]]\n' >&2
}

COMPAT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd -- "$COMPAT_DIR/.." && pwd)"
ZZ_INPUT="${1:-${ZZ_BIN:-$REPO_DIR/target/debug/zz_cli}}"
TMUX_INPUT="${2:-${TMUX_BIN:-${ZZ_COMPAT_TMUX:-$COMPAT_DIR/.cache/tmux-src/tmux}}}"
[ "$#" -le 2 ] || { usage; exit 2; }

resolve_binary() {
  local input="$1"
  if [ -x "$input" ]; then
    printf '%s\n' "$(cd -- "$(dirname -- "$input")" && pwd)/$(basename -- "$input")"
    return 0
  fi
  command -v -- "$input"
}
ZZ_BIN="$(resolve_binary "$ZZ_INPUT")" || { printf 'error: zz binary not found: %s\n' "$ZZ_INPUT" >&2; exit 2; }
TMUX_BIN="$(resolve_binary "$TMUX_INPUT")" || { printf 'error: tmux binary not found: %s\n' "$TMUX_INPUT" >&2; exit 2; }

RUN_SECONDS=3
SCRATCH_DIR="$(mktemp -d /tmp/zzrn.XXXXXX)"
TOKEN="${SCRATCH_DIR##*.}"
TMUX_SOCKET_NAME="zzrn-$TOKEN"
ZZ_SOCKET="/tmp/zzrn-$TOKEN.sock"
SESSION="names"
ZZ_HOME="$SCRATCH_DIR/zz-home"
TMUX_HOME="$SCRATCH_DIR/tmux-home"
ZZ_PID=""
mkdir -p "$ZZ_HOME" "$TMUX_HOME"

scrubbed() {
  env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL \
    TMUX_TMPDIR=/tmp "$@"
}
side_command() {
  local side="$1"
  shift
  case "$side" in
  zz) scrubbed HOME="$ZZ_HOME" XDG_CONFIG_HOME="$ZZ_HOME/config" "$ZZ_BIN" --socket "$ZZ_SOCKET" "$@" ;;
  tmux) scrubbed HOME="$TMUX_HOME" XDG_CONFIG_HOME="$TMUX_HOME/config" "$TMUX_BIN" -L "$TMUX_SOCKET_NAME" "$@" ;;
  esac
}

cleanup() {
  local status=$?
  trap - EXIT ERR INT TERM
  set +e
  side_command zz kill-server >/dev/null 2>&1
  side_command tmux kill-server >/dev/null 2>&1
  if [ -n "$ZZ_PID" ]; then
    kill "$ZZ_PID" >/dev/null 2>&1
    wait "$ZZ_PID" >/dev/null 2>&1
  fi
  rm -f -- "$ZZ_SOCKET" "/tmp/tmux-$(id -u)/$TMUX_SOCKET_NAME"
  rm -rf -- "$SCRATCH_DIR"
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

die() {
  printf 'error: %s\n' "$*" >&2
  exit 2
}

wait_for() {
  local label="$1"
  local attempt
  shift
  for ((attempt = 0; attempt < 200; attempt++)); do
    if "$@" >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.05
  done
  die "$label did not happen within 10 seconds"
}

on_both() {
  side_command zz "$@" || die "zz refused $*"
  side_command tmux "$@" || die "tmux refused $*"
}

window_name() {
  side_command "$1" display-message -p -t "=$SESSION:0" '#W'
}

renames() {
  local value
  value="$(side_command "$1" show-options -gv @renames 2>/dev/null || true)"
  printf '%s\n' "${#value}"
}

name_is() {
  [ "$(window_name "$1")" = "$2" ]
}

scrubbed HOME="$ZZ_HOME" XDG_CONFIG_HOME="$ZZ_HOME/config" "$ZZ_BIN" --socket "$ZZ_SOCKET" -f /dev/null daemon \
  >"$SCRATCH_DIR/zz-daemon.out" 2>"$SCRATCH_DIR/zz-daemon.err" &
ZZ_PID=$!
wait_for "zz daemon socket" test -S "$ZZ_SOCKET"

SHELL_COMMAND="exec /bin/bash --noprofile --norc -i"
side_command zz new-session -d -s "$SESSION" -x 80 -y 24 "$SHELL_COMMAND" || die "could not create the zz session"
side_command tmux -f /dev/null new-session -d -s "$SESSION" -x 80 -y 24 "$SHELL_COMMAND" ||
  die "could not create the tmux session"
wait_for "zz shell named" name_is zz bash
wait_for "tmux shell named" name_is tmux bash
sleep 0.6
on_both set-hook -g window-renamed 'set -ga @renames x'
LOOP="while :; do perl -e 'for (1..20) { print qq(p\\n); select(undef, undef, undef, 0.01) }'; sh -c 'i=0; while [ \$i -lt 20 ]; do echo s; /bin/sleep 0.01; i=\$((i + 1)); done'; done"
side_command zz send-keys -t "=$SESSION:0.0" "$LOOP" Enter
side_command tmux send-keys -t "=$SESSION:0.0" "$LOOP" Enter
sleep "$RUN_SECONDS"
side_command zz send-keys -t "=$SESSION:0.0" C-c
side_command tmux send-keys -t "=$SESSION:0.0" C-c
sleep 1.5

FAILURES=0
zz_count="$(renames zz)"
tmux_count="$(renames tmux)"
zz_name="$(window_name zz)"
tmux_name="$(window_name tmux)"
tolerance=$((tmux_count / 3))
[ "$tolerance" -ge 2 ] || tolerance=2
printf 'window-renamed hooks over %s s: tmux %s, zz %s (tolerance %s)\n' \
  "$RUN_SECONDS" "$tmux_count" "$zz_count" "$tolerance"
for side in zz tmux; do
  if ! side_command "$side" capture-pane -p -t "=$SESSION:0.0" -S -50 | grep -qx s; then
    printf 'DIFF  the loop never ran on %s\n' "$side"
    FAILURES=$((FAILURES + 1))
  fi
done
if [ "$zz_count" -gt $((tmux_count + tolerance)) ] || [ "$zz_count" -lt $((tmux_count - tolerance)) ]; then
  printf 'DIFF  rename rate: tmux %s, zz %s\n' "$tmux_count" "$zz_count"
  FAILURES=$((FAILURES + 1))
else
  printf 'ok    rename rate\n'
fi
if [ "$zz_name" != "$tmux_name" ]; then
  printf 'DIFF  settled name: tmux %q, zz %q\n' "$tmux_name" "$zz_name"
  FAILURES=$((FAILURES + 1))
else
  printf 'ok    settled name %s\n' "$zz_name"
fi
if [ "$FAILURES" -ne 0 ]; then
  exit 1
fi
