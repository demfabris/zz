#!/usr/bin/env bash
# Pane-geometry differential for the raw TUI's chrome.
#
# The scenario harness compares daemon facts, and the daemon's pane is the size
# the CLIENT asks for, so no campaign proof ever compared what an attached
# client leaves for the pane. This fixture attaches both binaries inside an
# outer pinned tmux at the size under test, runs `tput cols; tput lines` in the
# inner pane on each side, and diffs the columns. `ssh -t host zz attach` on a
# stock 80x24 terminal has to leave the pane as wide as pinned tmux leaves it.
#
# Rows are asserted at every width. The pin spends one row of 24 on its status
# line and nothing on pane chrome while pane-border-status is off, so the pane
# gets 23; the raw TUI has to hand over the same 23 whether its sidebar shows
# or not.
set -eEuo pipefail

usage() {
  printf 'usage: compat/tui-pane-geometry.sh [ZZ_BIN [TMUX_BIN]]\n' >&2
  printf '       ZZ_BIN=path TMUX_BIN=path compat/tui-pane-geometry.sh\n' >&2
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

# Each entry is SIZE|MODE, and MODE governs the COLUMNS only: `same` asserts
# both binaries hand the pane the same columns; `record` prints them without
# asserting, for a width where zz's sidebar is chrome the pin has no
# counterpart for. The sidebar's auto-hide threshold is 109 columns — 80 for
# the pane plus 28 for the sidebar and 1 for its border — so 80 and 100 must
# match and 120 is where zz's own chrome starts. Rows are asserted at all
# three: the sidebar is a column of chrome, never a row of it.
SIZES=(80x24\|same 100x24\|same 120x24\|record)
SCRATCH_DIR="$(mktemp -d /tmp/zzgeo.XXXXXX)"
TOKEN="${SCRATCH_DIR##*.}"
OUTER_SOCKET_NAME="zzgeoo-$TOKEN"
INNER_SOCKET_NAME="zzgeoi-$TOKEN"
ZZ_SOCKET="/tmp/zzgeo-$TOKEN.sock"
OUTER_SESSION="driver"
INNER_SESSION="geo"
ZZ_HOME="$SCRATCH_DIR/zz-home"
TMUX_HOME="$SCRATCH_DIR/tmux-home"
OUTER_HOME="$SCRATCH_DIR/outer-home"
ZZ_PID=""
FAILURES=0
CHECKS=0
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
  [ "$(tmux_outer_command display-message -p -t "$1" '#{pane_width}x#{pane_height}' 2>/dev/null)" = "$2" ]
}
client_attached() {
  [ "$(side_command "$1" list-clients -F '#{client_session}' 2>/dev/null)" = "$INNER_SESSION" ]
}
file_has_two_fields() {
  local words
  words="$(cat "$1" 2>/dev/null)" || return 1
  [ "$(printf '%s\n' "$words" | wc -w)" = 2 ]
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

# `tput` inside the pane reports the pty size the client negotiated, which is
# the pane the viewer actually gets — not the size the session was created at.
measure() {
  local side="$1"
  local out="$SCRATCH_DIR/$side.geometry"
  local pane
  rm -f -- "$out"
  pane="$(side_command "$side" list-panes -t "=$INNER_SESSION" -F '#{pane_id}' | head -n 1)"
  [ -n "$pane" ] || die "$side has no pane in =$INNER_SESSION"
  side_command "$side" send-keys -t "$pane" \
    "printf '%s %s\\n' \"\$(tput cols)\" \"\$(tput lines)\" >$out" Enter
  wait_for "$side geometry report" file_has_two_fields "$out"
  cat "$out"
}

zz_command -f /dev/null daemon >"$SCRATCH_DIR/zz-daemon.out" 2>"$SCRATCH_DIR/zz-daemon.err" &
ZZ_PID=$!
wait_for "zz daemon socket" test -S "$ZZ_SOCKET"

printf 'pane geometry differential (pin %s)\n' "$(basename -- "$TMUX_BIN")"
for entry in "${SIZES[@]}"; do
  size="${entry%%|*}"
  mode="${entry##*|}"
  columns="${size%x*}"
  rows="${size#*x}"
  zz_command kill-session -t "=$INNER_SESSION" >/dev/null 2>&1 || true
  tmux_inner_command kill-session -t "=$INNER_SESSION" >/dev/null 2>&1 || true
  tmux_outer_command kill-server >/dev/null 2>&1 || true

  zz_command new-session -d -s "$INNER_SESSION" -x "$columns" -y "$rows" ||
    die "could not create the zz session"
  tmux_inner_command -f /dev/null new-session -d -s "$INNER_SESSION" -x "$columns" -y "$rows" ||
    die "could not create the tmux session"

  write_attach zz "$SCRATCH_DIR/attach-zz.sh"
  write_attach tmux "$SCRATCH_DIR/attach-tmux.sh"
  tmux_outer_command -f /dev/null new-session -d -s "$OUTER_SESSION" -n zz \
    -x "$columns" -y "$rows" "$SCRATCH_DIR/attach-zz.sh" ||
    die "could not create the outer session"
  tmux_outer_command set-option -g status off
  tmux_outer_command new-window -d -n tmux "$SCRATCH_DIR/attach-tmux.sh"
  wait_for "outer zz pane at $size" outer_pane_is "=$OUTER_SESSION:zz" "${columns}x${rows}"
  wait_for "outer tmux pane at $size" outer_pane_is "=$OUTER_SESSION:tmux" "${columns}x${rows}"
  wait_for "zz client attached" client_attached zz
  wait_for "tmux client attached" client_attached tmux

  zz_geometry="$(measure zz)"
  tmux_geometry="$(measure tmux)"
  zz_columns="${zz_geometry%% *}"
  zz_rows="${zz_geometry##* }"
  tmux_columns="${tmux_geometry%% *}"
  tmux_rows="${tmux_geometry##* }"

  if [ "$mode" = same ]; then
    CHECKS=$((CHECKS + 1))
    if [ "$zz_columns" = "$tmux_columns" ]; then
      printf 'ok    %s columns: both %s\n' "$size" "$zz_columns"
    else
      FAILURES=$((FAILURES + 1))
      printf 'DIFF  %s columns: tmux %s, zz %s\n' "$size" "$tmux_columns" "$zz_columns"
    fi
  else
    printf 'note  %s columns recorded, not asserted: tmux %s, zz %s (zz shows its sidebar here)\n' \
      "$size" "$tmux_columns" "$zz_columns"
  fi
  CHECKS=$((CHECKS + 1))
  if [ "$zz_rows" = "$tmux_rows" ]; then
    printf 'ok    %s rows: both %s\n' "$size" "$zz_rows"
  else
    FAILURES=$((FAILURES + 1))
    printf 'DIFF  %s rows: tmux %s, zz %s\n' "$size" "$tmux_rows" "$zz_rows"
  fi

  zz_command kill-session -t "=$INNER_SESSION" >/dev/null 2>&1 || true
  tmux_inner_command kill-session -t "=$INNER_SESSION" >/dev/null 2>&1 || true
  tmux_outer_command kill-server >/dev/null 2>&1 || true
done

if [ "$FAILURES" -ne 0 ]; then
  printf '%s of %s asserted measurements differ\n' "$FAILURES" "$CHECKS"
  exit 1
fi
printf 'all %s asserted measurements identical\n' "$CHECKS"
