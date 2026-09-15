#!/usr/bin/env bash
# Two-sided probe for TUI-014's pane modes: an outer pinned tmux drives one
# attached pin client and one attached zz client at 80x24, the same driver
# shape compat/tui-client-commands.sh uses. It runs the command under test on
# both inner servers, settles both screens, and prints each side's screen,
# cursor and pane-mode state with a diff, so a mode surface can be iterated
# against the pin without running the whole fixture.
#
# usage: mode-probe.sh <command...>   PANE and CLIENT stand for each side's own
# spelling. Ends the mode on both sides afterwards and prints the restored
# screens too. Every server it starts is a throwaway on a short /tmp socket
# with its own HOME and XDG_CONFIG_HOME, loaded from /dev/null, and a trap
# reaps all three.
set -eEuo pipefail
T="${ZZ_COMPAT_TMUX:-/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux}"
Z="${ZZ_BIN:-/home/demfabris/dev/zz-tui-modes-10/target/debug/zz}"
SCRATCH="$(mktemp -d /tmp/zzmp.XXXXXX)"
TOKEN="${SCRATCH##*.}"
OUT="zzmpo-$TOKEN"
INN="zzmpi-$TOKEN"
SOCK="/tmp/zzmp-$TOKEN.sock"
OH="$SCRATCH/oh"
IH="$SCRATCH/ih"
ZH="$SCRATCH/zh"
mkdir -p "$OH/config" "$IH/config" "$ZH/config" "$SCRATCH/logs"
scrubbed() {
  env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL \
    -u XDG_STATE_HOME -u ZZ_LOG_DIR TMUX_TMPDIR=/tmp ZZ_TRAY=0 "$@"
}
o() { scrubbed HOME="$OH" XDG_CONFIG_HOME="$OH/config" "$T" -L "$OUT" "$@"; }
i() { scrubbed HOME="$IH" XDG_CONFIG_HOME="$IH/config" "$T" -L "$INN" "$@"; }
z() { scrubbed HOME="$ZH" XDG_CONFIG_HOME="$ZH/config" ZZ_LOG_DIR="$SCRATCH/logs" "$Z" --socket "$SOCK" "$@"; }
side() { case "$1" in tmux) shift; i "$@" ;; zz) shift; z "$@" ;; esac; }
cleanup() {
  local s=$?
  trap - EXIT INT TERM
  o kill-server >/dev/null 2>&1 || true
  i kill-server >/dev/null 2>&1 || true
  z kill-server >/dev/null 2>&1 || true
  rm -f -- "$SOCK"
  rm -rf -- "$SCRATCH"
  exit $s
}
trap cleanup EXIT INT TERM

SH="ENV= PS1='\$ ' exec /bin/sh"
z new-session -d -s cli -n win -x 80 -y 24 "$SH"
i -f /dev/null new-session -d -s cli -n win -x 80 -y 24 "$SH"
for s in zz tmux; do
  side "$s" set-option -g status-right ''
  side "$s" set-option -g status-left L
  side "$s" set-option -g automatic-rename off
  side "$s" new-window -d -t '=cli:1' -n two "$SH"
  for p in $(side "$s" list-panes -a -F '#{pane_id}'); do
    side "$s" select-pane -t "$p" -T ptitle
    side "$s" send-keys -t "$p" "printf 'SCENE-%s\\n' $p" Enter
  done
done
printf '#!/usr/bin/env bash\nexec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL -u XDG_STATE_HOME HOME=%q XDG_CONFIG_HOME=%q ZZ_LOG_DIR=%q ZZ_TRAY=0 TMUX_TMPDIR=/tmp %q --socket %q attach-session -t =cli\n' \
  "$ZH" "$ZH/config" "$SCRATCH/logs" "$Z" "$SOCK" >"$SCRATCH/attach-zz.sh"
printf '#!/usr/bin/env bash\nexec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL -u XDG_STATE_HOME HOME=%q XDG_CONFIG_HOME=%q TMUX_TMPDIR=/tmp %q -L %q attach-session -t =cli\n' \
  "$IH" "$IH/config" "$T" "$INN" >"$SCRATCH/attach-tmux.sh"
chmod +x "$SCRATCH/attach-zz.sh" "$SCRATCH/attach-tmux.sh"
o -f /dev/null new-session -d -s driver -n zz -x 80 -y 24 "$SCRATCH/attach-zz.sh"
o set-option -g status off
o new-window -d -n tmux "$SCRATCH/attach-tmux.sh"

wait_for() {
  local label="$1" attempt
  shift
  for ((attempt = 0; attempt < 200; attempt++)); do
    "$@" >/dev/null 2>&1 && return 0
    sleep 0.05
  done
  printf 'gave up waiting for %s\n' "$label" >&2
  return 1
}
attached() { [ "$(side "$1" list-clients -F '#{client_session}')" = cli ]; }
wait_for 'zz attached' attached zz
wait_for 'tmux attached' attached tmux

cap() { o capture-pane -p -e -S 0 -E 23 -t "=driver:$1"; }
capp() { o capture-pane -p -S 0 -E 23 -t "=driver:$1"; }
cursor() { o display-message -p -t "=driver:$1" '#{cursor_x},#{cursor_y} flag=#{cursor_flag} pane=#{pane_width}x#{pane_height} shape=#{cursor_shape}'; }
state() {
  side "$1" list-panes -a -F 'P #{session_name}:#{window_index}.#{pane_index} mode=#{pane_in_mode}/#{pane_mode} #{pane_width}x#{pane_height}'
}
settle() {
  local s="$1" previous="" current attempt
  for ((attempt = 0; attempt < 120; attempt++)); do
    current="$(cap "$s")"
    [ -n "$previous" ] && [ "$current" = "$previous" ] && return 0
    previous="$current"
    sleep 0.05
  done
  return 0
}
settle zz
settle tmux

report() {
  local label="$1" s
  printf '=== %s\n' "$label"
  for s in tmux zz; do
    printf -- '--- %s plain\n' "$s"
    capp "$s" | cat -A
    printf -- '--- %s styled\n' "$s"
    cap "$s" | cat -v
    printf -- '--- %s state\n' "$s"
    state "$s"
  done
  printf -- '--- styled diff (tmux vs zz)\n'
  diff <(cap tmux | cat -v) <(cap zz | cat -v) && printf 'STYLED SCREENS IDENTICAL\n'
  printf -- '--- state diff (tmux vs zz)\n'
  diff <(state tmux) <(state zz) && printf 'STATE IDENTICAL\n'
  printf -- '--- cursor tmux: %s\n--- cursor zz:   %s\n' "$(cursor tmux)" "$(cursor zz)"
}

report BEFORE

PANE_T="$(i list-panes -t '=cli' -F '#{pane_active} #{pane_id}' | awk '$1==1{print $2}')"
PANE_Z="$(z list-panes -t '=cli' -F '#{pane_active} #{pane_id}' | awk '$1==1{print $2}')"
CLIENT_T="$(i list-clients -F '#{client_name}' | head -n1)"
CLIENT_Z="$(z list-clients -F '#{client_name}' | head -n1)"
run_side() {
  local s="$1" pane client
  shift
  case "$s" in
  tmux) pane="$PANE_T" client="$CLIENT_T" ;;
  zz) pane="$PANE_Z" client="$CLIENT_Z" ;;
  esac
  local -a args=()
  local a
  for a in "$@"; do
    case "$a" in
    PANE) args+=("$pane") ;;
    CLIENT) args+=("$client") ;;
    *) args+=("$a") ;;
    esac
  done
  set +e
  side "$s" "${args[@]}" >"$SCRATCH/$s.out" 2>"$SCRATCH/$s.err"
  printf '%s\n' "$?" >"$SCRATCH/$s.rc"
  set -e
}
run_side zz "$@"
run_side tmux "$@"
for s in tmux zz; do
  printf '### %s rc=%s\n' "$s" "$(cat "$SCRATCH/$s.rc")"
  printf '### %s stdout\n' "$s"; cat -A "$SCRATCH/$s.out"
  printf '### %s stderr\n' "$s"; cat -A "$SCRATCH/$s.err"
done
wait_for 'the pin in a mode' test "$(i display-message -p -t "$PANE_T" '#{pane_in_mode}')" = 1 || true
settle tmux
settle zz
report AFTER

for s in tmux zz; do
  case "$s" in tmux) pane="$PANE_T" ;; zz) pane="$PANE_Z" ;; esac
  for key in Escape q; do
    [ "$(side "$s" display-message -p -t "$pane" '#{pane_in_mode}')" = 0 ] && break
    o send-keys -t "=driver:$s" "$key"
    sleep 0.4
  done
done
settle tmux
settle zz
report RESTORED
