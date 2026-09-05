#!/usr/bin/env bash
# Row-level differential for the tmux status line.
#
# The scenario harness compares daemon facts and never a rendered row, and the
# attached-client fixture drives the raw TUI at a width where its sidebar is
# visible, so no campaign proof ever compared what a status format DRAWS.
# This fixture runs both binaries inside an outer pinned tmux at a width below
# the sidebar's auto-hide threshold, where the zz TUI paints the daemon's
# expanded status rows across the full width, applies the same status options
# to both, and diffs the bytes of the last row of each pane after every step,
# escapes included. A divergence is a finding, not a failure of this script:
# it exits 1 so a caller can gate on it, and prints both rows so the next
# lane has the measurement.
set -eEuo pipefail

usage() {
  printf 'usage: compat/status-row.sh [ZZ_BIN [TMUX_BIN]]\n' >&2
  printf '       ZZ_BIN=path TMUX_BIN=path compat/status-row.sh\n' >&2
}

COMPAT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd -- "$COMPAT_DIR/.." && pwd)"
ZZ_INPUT="${1:-${ZZ_BIN:-$REPO_DIR/target/debug/zz}}"
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

COLUMNS_UNDER_TEST=79
ROWS_UNDER_TEST=24
SCRATCH_DIR="$(mktemp -d /tmp/zzsr.XXXXXX)"
TOKEN="${SCRATCH_DIR##*.}"
OUTER_SOCKET_NAME="zzsro-$TOKEN"
INNER_SOCKET_NAME="zzsri-$TOKEN"
ZZ_SOCKET="/tmp/zzsr-$TOKEN.sock"
OUTER_SESSION="driver"
INNER_SESSION="rows"
ZZ_HOME="$SCRATCH_DIR/zz-home"
TMUX_HOME="$SCRATCH_DIR/tmux-home"
OUTER_HOME="$SCRATCH_DIR/outer-home"
ZZ_PID=""
FAILURES=0
mkdir -p "$ZZ_HOME" "$TMUX_HOME" "$OUTER_HOME"

scrubbed() {
  env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL \
    TMUX_TMPDIR=/tmp "$@"
}
tmux_outer_command() {
  scrubbed HOME="$OUTER_HOME" XDG_CONFIG_HOME="$OUTER_HOME/config" \
    "$TMUX_BIN" -L "$OUTER_SOCKET_NAME" "$@"
}
zz_command() {
  scrubbed HOME="$ZZ_HOME" XDG_CONFIG_HOME="$ZZ_HOME/config" \
    "$ZZ_BIN" --socket "$ZZ_SOCKET" "$@"
}
tmux_inner_command() {
  scrubbed HOME="$TMUX_HOME" XDG_CONFIG_HOME="$TMUX_HOME/config" \
    "$TMUX_BIN" -L "$INNER_SOCKET_NAME" "$@"
}
side_command() {
  local side="$1"
  shift
  case "$side" in
  zz) zz_command "$@" ;;
  tmux) tmux_inner_command "$@" ;;
  esac
}

cleanup() {
  local status=$?
  trap - EXIT ERR INT TERM
  set +e
  tmux_outer_command kill-server >/dev/null 2>&1
  zz_command kill-server >/dev/null 2>&1
  tmux_inner_command kill-server >/dev/null 2>&1
  if [ -n "$ZZ_PID" ]; then
    kill "$ZZ_PID" >/dev/null 2>&1
    wait "$ZZ_PID" >/dev/null 2>&1
  fi
  rm -f -- "$ZZ_SOCKET" "/tmp/tmux-$(id -u)/$OUTER_SOCKET_NAME" "/tmp/tmux-$(id -u)/$INNER_SOCKET_NAME"
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

outer_pane_is() {
  local target="$1"
  local expected="$2"
  [ "$(tmux_outer_command display-message -p -t "$target" '#{pane_width}x#{pane_height}' 2>/dev/null)" = "$expected" ]
}

last_row_bytes() {
  local side="$1"
  tmux_outer_command capture-pane -p -e -t "=$OUTER_SESSION:$side" | tail -n 1
}

row_settled() {
  local side="$1"
  local expected="$2"
  [ "$(last_row_bytes "$side")" = "$expected" ]
}

write_attach() {
  local side="$1"
  local destination="$2"
  printf '#!/usr/bin/env bash\n' >"$destination"
  if [ "$side" = zz ]; then
    printf 'exec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL HOME=%q XDG_CONFIG_HOME=%q TMUX_TMPDIR=/tmp %q --socket %q attach-session -t %q\n' \
      "$ZZ_HOME" "$ZZ_HOME/config" "$ZZ_BIN" "$ZZ_SOCKET" "=$INNER_SESSION" >>"$destination"
  else
    printf 'exec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL HOME=%q XDG_CONFIG_HOME=%q TMUX_TMPDIR=/tmp %q -L %q attach-session -t %q\n' \
      "$TMUX_HOME" "$TMUX_HOME/config" "$TMUX_BIN" "$INNER_SOCKET_NAME" "=$INNER_SESSION" >>"$destination"
  fi
  chmod +x "$destination"
}

zz_command -f /dev/null daemon >"$SCRATCH_DIR/zz-daemon.out" 2>"$SCRATCH_DIR/zz-daemon.err" &
ZZ_PID=$!
wait_for "zz daemon socket" test -S "$ZZ_SOCKET"
zz_command new-session -d -s "$INNER_SESSION" -x "$COLUMNS_UNDER_TEST" -y "$ROWS_UNDER_TEST" ||
  die "could not create the zz session"
tmux_inner_command -f /dev/null new-session -d -s "$INNER_SESSION" -x "$COLUMNS_UNDER_TEST" -y "$ROWS_UNDER_TEST" ||
  die "could not create the tmux session"

write_attach zz "$SCRATCH_DIR/attach-zz.sh"
write_attach tmux "$SCRATCH_DIR/attach-tmux.sh"
tmux_outer_command -f /dev/null new-session -d -s "$OUTER_SESSION" -n zz \
  -x "$COLUMNS_UNDER_TEST" -y "$ROWS_UNDER_TEST" "$SCRATCH_DIR/attach-zz.sh" ||
  die "could not create the outer session"
tmux_outer_command set-option -g status off
tmux_outer_command new-window -d -n tmux "$SCRATCH_DIR/attach-tmux.sh"
wait_for "outer zz pane at ${COLUMNS_UNDER_TEST}x${ROWS_UNDER_TEST}" outer_pane_is "=$OUTER_SESSION:zz" "${COLUMNS_UNDER_TEST}x${ROWS_UNDER_TEST}"
wait_for "outer tmux pane at ${COLUMNS_UNDER_TEST}x${ROWS_UNDER_TEST}" outer_pane_is "=$OUTER_SESSION:tmux" "${COLUMNS_UNDER_TEST}x${ROWS_UNDER_TEST}"
client_attached() {
  local side="$1"
  [ "$(side_command "$side" list-clients -F '#{client_session}' 2>/dev/null)" = "$INNER_SESSION" ]
}
wait_for "zz client attached" client_attached zz
wait_for "tmux client attached" client_attached tmux

# The corpus: one status option per step, applied identically to both servers.
# Each step names the option and the value; the row is captured after both
# sides have repainted at least once. Keep the values free of clocks and of
# anything host-specific so the bytes can be equal at all.
CORPUS=(
  "status-left|[#{session_name}]"
  "status-right|#{window_width}x#{window_height} #{client_width}"
  "window-status-format|#I:#W#F"
  "window-status-current-format|#I:#W#F*"
  "status-style|bg=blue,fg=white"
  "status-left-style|bold"
  "status-justify|centre"
  "status-position|bottom"
)

compare_step() {
  local step="$1"
  local zz_row tmux_row
  sleep 0.3
  zz_row="$(last_row_bytes zz)"
  tmux_row="$(last_row_bytes tmux)"
  if [ "$zz_row" = "$tmux_row" ]; then
    printf 'ok    %s\n' "$step"
    return 0
  fi
  FAILURES=$((FAILURES + 1))
  printf 'DIFF  %s\n' "$step"
  printf '      tmux: %s\n' "$(printf '%s' "$tmux_row" | od -An -c | tr -s ' \n' ' ')"
  printf '      zz:   %s\n' "$(printf '%s' "$zz_row" | od -An -c | tr -s ' \n' ' ')"
  printf '      tmux: %q\n' "$tmux_row"
  printf '      zz:   %q\n' "$zz_row"
}

printf 'status row differential at %sx%s (pin %s)\n' "$COLUMNS_UNDER_TEST" "$ROWS_UNDER_TEST" "$(basename -- "$TMUX_BIN")"
compare_step "defaults"
for entry in "${CORPUS[@]}"; do
  option="${entry%%|*}"
  value="${entry#*|}"
  side_command zz set-option -g "$option" "$value" || die "zz refused set-option -g $option"
  side_command tmux set-option -g "$option" "$value" || die "tmux refused set-option -g $option"
  compare_step "$option = $value"
done

if [ "$FAILURES" -ne 0 ]; then
  printf '%s of %s rows differ\n' "$FAILURES" "$((${#CORPUS[@]} + 1))"
  exit 1
fi
printf 'all %s rows identical\n' "$((${#CORPUS[@]} + 1))"
