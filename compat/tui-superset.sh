#!/usr/bin/env bash
# The zz-only commands beside ordinary terminal behaviour.
#
# tui-screen-diff.sh compares two screens driven the same way. This fixture is
# for the twenty-five verbs the pin has no counterpart for: it drives them on
# the raw TUI, records what each one DRAWS or REFUSES, and then checks that the
# terminal around them is still the pin's.
#
# WHAT A REFUSAL IS. `zz agent-send` answers `agent commands require the zz app`
# in a raw TUI. The pin has no agent-send at all, so that answer is a DECLARED
# case with no counterpart, not a divergence. What would be a divergence is a
# zz verb that changes what an ordinary tmux command means, leaves a canvas the
# pin would not draw, or takes a key away from the pane that owns it. The three
# clauses below are ordered that way: what the verbs do, what the terminal does
# around them, and what a second client sees.
#
# REACHABLE BY THE DECLARED NAME. crates/zz-protocol/src/catalog.rs
# NATIVE_COMMAND_NAMES is the list, and `names` below walks it from the same
# source of truth rather than from a copy: every name has to resolve to a
# command, which is the opposite of `unknown command: NAME`. No compatibility
# profile, no activation mode and no width invokes any of them; the only paths
# are the CLI and a user binding, and every verb here is driven through both.
#
# THE TWO PATHS. Every verb is driven from the CLI, where its answer is stdout,
# stderr and an exit status, and through an ordinary `bind-key -n` binding,
# where its answer is the client's message row. A verb that answers nothing
# visible is bound as a sequence, `verb ; rename-window TOKEN`, and the case
# waits for the token on that same row: a bound sequence runs its second command
# on both binaries even when the first one failed, so the rename alone proves
# nothing, but a zz client message stays on the row indefinitely, so a verb that
# refused keeps its message where the status row would be and the token never
# arrives. A message longer than the client is cut at the client's width, and
# the two that are asserted whole rather than cut.
#
# WHAT CLAUSE 2 DRIVES AROUND EACH SURFACE: create (new-window), select
# (select-window, select-pane), split, resize, detach and reattach, the same
# commands against the same explicit targets on both sides. A pane surface
# survives a reattach because a pane is session state; the sidebar and the
# command-output overlay do not, because they are the client's.
#
# THE DECODED SCREEN IS THE CONTRACT. Every screen comparison reads the outer
# pinned tmux's own grid through `capture-pane -p -e`, so attribute order,
# batching and cursor spelling collapse on both sides before anything is
# compared, and the colour class does not. The cursor is read whole from the
# outer pane. A wait is always a bounded wait_for or wait_settled on an
# observable; no wait in this file is a sleep.
#
# CONTROLLED DYNAMIC VALUES, set on both sides before the first comparison:
#   status-right ''      the default ends in a clock and in a strftime date.
#   status-left L        fixed literal, so the left of the row is asserted.
#   automatic-rename off plus rename-window win.
#   select-pane -T title on every pane before a canvas comparison.
#   the inner shell      ENV= PS1='$ ' exec /bin/sh: no rc file, and a prompt
#                        that carries no host, user, path or clock.
#   HOME and XDG_CONFIG_HOME per side, inside the scratch directory, so
#   import-tmux-config really has no tmux configuration to find and the sidebar
#   tree has no user hosts in it.
# A canvas comparison is always preceded by a clear and a marker on both sides,
# because a zz verb that splits and un-splits reflows the pane and the pin's
# untouched pane never did.
#
# THE SIDEBAR TAKES COLUMNS FROM THE SESSION. focus-sidebar makes the client
# report 120 - 28 - 1 = 91 usable columns, so the shared window becomes 91 wide
# for every client on that session, exactly as an ordinary 91-column tmux client
# would. Clause 3 therefore gives the second client 91 columns on BOTH sides:
# with a client WIDER than the window, the pin fills the leftover area with a
# border and middle dots (screen-redraw.c) and the raw TUI leaves it blank.
# MEASURED 2026-09-13 with two PLAIN clients, 120 and 91 columns, and no zz verb
# anywhere: the pin's 120-column client draws `│` at column 91 and `·` across
# columns 92..119 on every window row, the raw TUI draws spaces. That gap is the
# ordinary multi-client canvas, not a superset command, so it is recorded in
# TUI-012's evidence for the client-inventory obligation and is deliberately
# not a case of this file: sizing the second client to the window keeps every
# cell of clause 3 asserted instead of waiving a band of columns.
#
# --self-check drives one deliberate fault per channel: a name the catalog does
# not carry, a refusal message that does not match, a surface that never drew,
# a canvas that stayed different after the surface withdrew, and a key the
# sidebar was supposed to swallow. A fixture that only passes has proved nothing.
#
# ZZ_SUPERSET_CAPTURE_DIR, when set, keeps every capture as text. A bounded wait
# that runs out dumps diagnostics into ZZ_SUPERSET_DIAGNOSTICS_DIR or a fresh
# /tmp directory it names.
set -eEuo pipefail

usage() {
  printf 'usage: compat/tui-superset.sh [--self-check] [ZZ_BIN [TMUX_BIN]]\n' >&2
  printf '       ZZ_BIN=path TMUX_BIN=path compat/tui-superset.sh\n' >&2
}

COMPAT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd -- "$COMPAT_DIR/.." && pwd)"
SELF_CHECK=0
POSITIONAL=()
for argument in "$@"; do
  case "$argument" in
  --self-check) SELF_CHECK=1 ;;
  -*)
    usage
    exit 2
    ;;
  *) POSITIONAL+=("$argument") ;;
  esac
done
[ "${#POSITIONAL[@]}" -le 2 ] || { usage; exit 2; }
ZZ_INPUT="${POSITIONAL[0]:-${ZZ_BIN:-$REPO_DIR/target/debug/zz}}"
TMUX_INPUT="${POSITIONAL[1]:-${TMUX_BIN:-${ZZ_COMPAT_TMUX:-$COMPAT_DIR/.cache/tmux-src/tmux}}}"

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
CATALOG="$REPO_DIR/crates/zz-protocol/src/catalog.rs"
[ -r "$CATALOG" ] || { printf 'error: catalog not readable: %s\n' "$CATALOG" >&2; exit 2; }

COLUMNS_UNDER_TEST=120
ROWS_UNDER_TEST=30
NARROW_COLUMNS=91
PANE_TITLE="supertitle"
WINDOW_NAME="win"
SCRATCH_DIR="$(mktemp -d /tmp/zzsup.XXXXXX)"
TOKEN="${SCRATCH_DIR##*.}"
OUTER_SOCKET_NAME="zzsuo-$TOKEN"
INNER_SOCKET_NAME="zzsui-$TOKEN"
ZZ_SOCKET="/tmp/zzsu-$TOKEN.sock"
OUTER_SESSION="driver"
INNER_SESSION="super"
ZZ_HOME="$SCRATCH_DIR/zz-home"
TMUX_HOME="$SCRATCH_DIR/tmux-home"
OUTER_HOME="$SCRATCH_DIR/outer-home"
ZZ_LOG_DIR="$SCRATCH_DIR/zz-logs"
ZZ_CLIENT_STDERR="$SCRATCH_DIR/zz-client.err"
TMUX_CLIENT_STDERR="$SCRATCH_DIR/tmux-client.err"
CAPTURE_DIR="${ZZ_SUPERSET_CAPTURE_DIR:-}"
DIAGNOSTICS_DIR=""
CASE_GROUP="setup"
ZZ_PID=""
FAILURES=0
CHECKS=0
RECORDS=0
SELF_CHECK_FAILURES=0
LAST_STATUS=0
LAST_OUTPUT=""
LAST_ROWS_DIFFERED=0
LAST_CURSOR_DIFFERED=0
PIN_ATTACHED=0
mkdir -p "$ZZ_HOME" "$TMUX_HOME" "$OUTER_HOME" "$ZZ_LOG_DIR"
[ -z "$CAPTURE_DIR" ] || mkdir -p "$CAPTURE_DIR"

scrubbed() {
  env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL \
    -u XDG_STATE_HOME -u ZZ_LOG_DIR \
    TMUX_TMPDIR=/tmp "$@"
}
# Every one-shot command is bounded. A client command that never answers is a
# fixture error in twenty seconds instead of a run that stalls until the
# harness kills it; the daemon itself is started with the bound disabled,
# which is what a duration of 0 means to timeout(1).
ZZ_CALL_TIMEOUT=20
tmux_outer_command() {
  scrubbed HOME="$OUTER_HOME" XDG_CONFIG_HOME="$OUTER_HOME/config" \
    timeout "$ZZ_CALL_TIMEOUT" "$TMUX_BIN" -L "$OUTER_SOCKET_NAME" "$@"
}
zz_command() {
  scrubbed HOME="$ZZ_HOME" XDG_CONFIG_HOME="$ZZ_HOME/config" ZZ_LOG_DIR="$ZZ_LOG_DIR" \
    timeout "$ZZ_CALL_TIMEOUT" "$ZZ_BIN" --socket "$ZZ_SOCKET" "$@"
}
tmux_inner_command() {
  scrubbed HOME="$TMUX_HOME" XDG_CONFIG_HOME="$TMUX_HOME/config" \
    timeout "$ZZ_CALL_TIMEOUT" "$TMUX_BIN" -L "$INNER_SOCKET_NAME" "$@"
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
  # /tmp is a tmpfs on the campaign box and the daemon can still be writing its
  # ring log while the tree comes down, so the removal is retried rather than
  # leaving a scratch directory behind.
  local attempt
  for ((attempt = 0; attempt < 20; attempt++)); do
    rm -rf -- "$SCRATCH_DIR" 2>/dev/null && break
    sleep 0.1
  done
  rm -rf -- "$SCRATCH_DIR" 2>/dev/null || true
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

die() {
  printf 'error: %s\n' "$*" >&2
  exit 2
}

diagnostics_dir() {
  if [ -z "$DIAGNOSTICS_DIR" ]; then
    DIAGNOSTICS_DIR="${ZZ_SUPERSET_DIAGNOSTICS_DIR:-$(mktemp -d /tmp/zzsup-diag.XXXXXX)}"
    mkdir -p "$DIAGNOSTICS_DIR"
  fi
  printf '%s\n' "$DIAGNOSTICS_DIR"
}

# A timeout is a failed check, and a failed check has to say what it was waiting
# for and what the screen showed at that moment.
dump_diagnostics() {
  local label="$1"
  local dir window log
  dir="$(diagnostics_dir)"
  {
    printf 'wait that ran out: %s\n' "$label"
    printf 'at: %s\n' "$(date -Is 2>/dev/null || date)"
    printf 'case group: %s\n' "$CASE_GROUP"
    printf 'zz: %s\n' "$ZZ_BIN"
    printf 'tmux: %s\n' "$TMUX_BIN"
    printf 'zz socket: %s\n' "$ZZ_SOCKET"
    printf 'outer socket: %s\n' "$OUTER_SOCKET_NAME"
    printf 'last command status: %s\n' "$LAST_STATUS"
    printf 'last command output: %s\n' "$LAST_OUTPUT"
  } >"$dir/what-fired.txt" 2>&1 || true
  for window in zz zzb tmux tmuxb; do
    tmux_outer_command capture-pane -p -e -S - -t "=$OUTER_SESSION:$window" \
      >"$dir/outer-$window.screen.txt" 2>&1 || true
  done
  zz_command list-panes -a -F '#{session_name}:#{window_index}.#{pane_index} #{pane_id} #{pane_width}x#{pane_height} dead=#{pane_dead} active=#{pane_active}' \
    >"$dir/zz.list-panes.txt" 2>&1 || true
  zz_command capture-pane -p -S -30 -t "$(active_pane zz)" \
    >"$dir/zz.active-pane.txt" 2>&1 || true
  zz_command list-clients -F '#{client_name} #{client_width}x#{client_height}' \
    >"$dir/zz.list-clients.txt" 2>&1 || true
  tmux_inner_command list-panes -a -F '#{session_name}:#{window_index}.#{pane_index} #{pane_id} #{pane_width}x#{pane_height}' \
    >"$dir/tmux.list-panes.txt" 2>&1 || true
  cp -f -- "$SCRATCH_DIR/zz-daemon.out" "$dir/zz-daemon.stdout.txt" 2>/dev/null || true
  cp -f -- "$SCRATCH_DIR/zz-daemon.err" "$dir/zz-daemon.stderr.txt" 2>/dev/null || true
  cp -f -- "$ZZ_CLIENT_STDERR" "$dir/zz-client.stderr.txt" 2>/dev/null || true
  for log in "$ZZ_LOG_DIR"/*; do
    [ -f "$log" ] || continue
    cp -f -- "$log" "$dir/ring-$(basename -- "$log").txt" 2>/dev/null || true
  done
  printf 'diagnostics retained in %s:\n' "$dir" >&2
  ls -1 -- "$dir" >&2 || true
}

# The reporting form of wait_for: a case that owns the failure message uses this
# and prints its own, where wait_for's own timeout is a fixture error.
wait_for_quietly() {
  local attempt
  for ((attempt = 0; attempt < WAIT_ATTEMPTS; attempt++)); do
    if "$@" >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.05
  done
  return 1
}
wait_for() {
  local label="$1"
  local attempt
  shift
  for ((attempt = 0; attempt < 240; attempt++)); do
    if "$@" >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.05
  done
  dump_diagnostics "$label"
  die "$label did not happen within 12 seconds"
}

CURSOR_FORMAT='#{cursor_x},#{cursor_y} flag=#{cursor_flag} pane=#{pane_width}x#{pane_height} shape=#{cursor_shape} blinking=#{cursor_blinking} very_visible=#{cursor_very_visible} colour=#{cursor_colour}'

outer_pane_is() {
  [ "$(tmux_outer_command display-message -p -t "$1" '#{pane_width}x#{pane_height}' 2>/dev/null)" = "$2" ]
}
outer_window_rows() {
  case "$1" in
  zzb | tmuxb) printf '%s\n' "$ROWS_UNDER_TEST" ;;
  *) printf '%s\n' "$ROWS_UNDER_TEST" ;;
  esac
}
capture_screen() {
  tmux_outer_command capture-pane -p -e -S 0 -E "$((ROWS_UNDER_TEST - 1))" \
    -t "=$OUTER_SESSION:$1"
}
capture_plain() {
  tmux_outer_command capture-pane -p -S 0 -E "$((ROWS_UNDER_TEST - 1))" \
    -t "=$OUTER_SESSION:$1"
}
cursor_tuple() {
  tmux_outer_command display-message -p -t "=$OUTER_SESSION:$1" "$CURSOR_FORMAT"
}
# The client's message row is the pin's message row: the last row of the
# client, in place of the status line, is where a command's answer lands when
# the command came from a key rather than from the CLI.
client_message_row() {
  capture_plain "$1" | tail -1
}

# Every zz verb in this file runs through here: the combined output and the
# status are kept so a case can assert the exact message and nothing else has
# to re-run the command.
# Standard input is closed for every one of them: agent-send and send-text read
# it when no text is on the command line, and a one-shot that blocks on an
# inherited terminal is a hang, not a measurement.
run_zz() {
  set +e
  LAST_OUTPUT="$(zz_command "$@" 2>&1 </dev/null)"
  LAST_STATUS=$?
  set -e
}

pass() {
  CHECKS=$((CHECKS + 1))
  printf 'ok    %s %s\n' "$CASE_GROUP" "$1"
}
fail() {
  CHECKS=$((CHECKS + 1))
  FAILURES=$((FAILURES + 1))
  printf 'DIFF  %s %s\n' "$CASE_GROUP" "$1"
  [ "$#" -lt 2 ] || printf '      %s\n' "$2"
}
record() {
  RECORDS=$((RECORDS + 1))
  printf 'note  %s %s recorded, not asserted: %s\n' "$CASE_GROUP" "$1" "$2"
}

# A refusal is asserted whole: the status has to be non-zero AND the message has
# to be the exact declared one, so a verb that starts answering something else
# is caught even while it keeps refusing.
refuses() {
  local name="$1"
  local expected="$2"
  shift 2
  run_zz "$@"
  if [ "$LAST_STATUS" -eq 0 ]; then
    fail "$name" "exit 0; the verb accepted what it declares it refuses"
    return 0
  fi
  if [ "$LAST_OUTPUT" != "$expected" ]; then
    fail "$name" "want: $expected"
    printf '      got:  %s\n' "$LAST_OUTPUT"
    return 0
  fi
  pass "$name"
}
accepts() {
  local name="$1"
  shift
  run_zz "$@"
  if [ "$LAST_STATUS" -ne 0 ]; then
    fail "$name" "exit $LAST_STATUS: $LAST_OUTPUT"
    return 0
  fi
  pass "$name"
}

# Bounded, never a sleep: the text has to reach the decoded screen.
WAIT_ATTEMPTS=240
wait_text() {
  local side="$1"
  local text="$2"
  local attempt
  for ((attempt = 0; attempt < WAIT_ATTEMPTS; attempt++)); do
    if capture_plain "$side" 2>/dev/null | grep -Fq -- "$text"; then
      return 0
    fi
    sleep 0.05
  done
  return 1
}
wait_no_text() {
  local side="$1"
  local text="$2"
  local attempt
  for ((attempt = 0; attempt < WAIT_ATTEMPTS; attempt++)); do
    if ! capture_plain "$side" 2>/dev/null | grep -Fq -- "$text"; then
      return 0
    fi
    sleep 0.05
  done
  return 1
}
wait_message_row() {
  local side="$1"
  local text="$2"
  local attempt
  for ((attempt = 0; attempt < WAIT_ATTEMPTS; attempt++)); do
    if client_message_row "$side" 2>/dev/null | grep -Fq -- "$text"; then
      return 0
    fi
    sleep 0.05
  done
  return 1
}
screen_has() {
  local name="$1"
  local side="$2"
  local text="$3"
  if wait_text "$side" "$text"; then
    pass "$name"
  else
    fail "$name" "the $side screen never carried: $text"
  fi
}
screen_lacks() {
  local name="$1"
  local side="$2"
  local text="$3"
  if wait_no_text "$side" "$text"; then
    pass "$name"
  else
    fail "$name" "the $side screen still carries: $text"
  fi
}
# A message longer than the client is cut at the client's width before it
# reaches the row, so a case whose message does not fit asserts the cut text
# rather than a prefix it chose: that asserts the truncation too.
message_row_is_truncated() {
  local name="$1"
  local side="$2"
  local message="$3"
  local want="${message:0:COLUMNS_UNDER_TEST}"
  if wait_message_row "$side" "$want"; then
    pass "$name"
  else
    fail "$name" "the $side message row never carried the message cut to $COLUMNS_UNDER_TEST columns"
    printf '      want: %s\n' "$want"
    printf '      row:  %s\n' "$(client_message_row "$side")"
  fi
}
message_row_has() {
  local name="$1"
  local side="$2"
  local text="$3"
  if wait_message_row "$side" "$text"; then
    pass "$name"
  else
    fail "$name" "the $side message row never carried: $text"
    printf '      row:  %s\n' "$(client_message_row "$side")"
  fi
}
equals() {
  local name="$1"
  local want="$2"
  local got="$3"
  if [ "$want" = "$got" ]; then
    pass "$name"
  else
    fail "$name" "want: $want"
    printf '      got:  %s\n' "$got"
  fi
}

INNER_SHELL="ENV= PS1='\$ ' exec /bin/sh"

active_pane() {
  side_command "$1" list-panes -t "=$INNER_SESSION" -F '#{pane_active} #{pane_id}' |
    awk '$1 == 1 { print $2; exit }'
}
first_pane() {
  side_command "$1" list-panes -t "=$INNER_SESSION" -F '#{pane_id}' | head -1
}
pane_geometry() {
  side_command "$1" list-panes -t "=$INNER_SESSION" \
    -F '#{pane_index} #{pane_height} active=#{pane_active}' | sort
}
window_size() {
  side_command "$1" display-message -p -t "=$INNER_SESSION:0" '#{window_width}x#{window_height}'
}
window_size_is() {
  [ "$(window_size "$1")" = "$2" ]
}
window_has_size() {
  local name="$1" side="$2" want="$3"
  if wait_for_quietly window_size_is "$side" "$want"; then
    pass "$name"
  else
    fail "$name" "want: $want"
    printf '      got:  %s\n' "$(window_size "$side")"
  fi
}
send_to_pane() {
  local side="$1"
  local pane
  pane="$(active_pane "$side")"
  [ -n "$pane" ] || die "$side has no active pane"
  side_command "$side" send-keys -t "$pane" "$2" Enter || die "$side refused send-keys"
}
send_both() {
  send_to_pane zz "$1"
  [ "$PIN_ATTACHED" -eq 0 ] || send_to_pane tmux "$1"
}
clear_both() {
  send_both "printf '\\033[2J\\033[3J\\033[H'"
}
# The clear and the marker are ONE command line. Sent as two, the second line's
# echo can interleave with the first's before the shell has read it: measured
# 2026-09-13 right after the command-output overlay closed, where the pane's
# own grid read `pprintf 'MARK-%s\n' closedrintf '\033[2J...'` and neither
# command ran. One line cannot interleave with itself.
clear_and_mark() {
  send_both "printf '\\033[2J\\033[3J\\033[HMARK-%s\\n' $1"
  settle_both "MARK-$1" "$1"
}
pin_pane_titles() {
  local side pane
  for side in zz tmux; do
    [ "$side" = zz ] || [ "$PIN_ATTACHED" -eq 1 ] || continue
    while read -r pane; do
      [ -n "$pane" ] || continue
      side_command "$side" select-pane -t "$pane" -T "$PANE_TITLE" >/dev/null 2>&1 || true
    done < <(side_command "$side" list-panes -t "=$INNER_SESSION" -F '#{pane_id}')
  done
}
pane_holds_marker() {
  local pane
  pane="$(active_pane "$1")"
  [ -n "$pane" ] || return 1
  side_command "$1" capture-pane -p -S -20 -t "$pane" 2>/dev/null | grep -Fq "$2"
}
# A checkpoint is settled when the marker is on the screen AND the screen has
# not changed between two polls. Both halves are observable and bounded.
wait_settled() {
  local window="$1"
  local marker="$2"
  local label="$3"
  local side="${4:-}"
  local previous=""
  local current attempt
  for ((attempt = 0; attempt < 240; attempt++)); do
    current="$(capture_plain "$window" 2>/dev/null || true)"
    if [ -n "$previous" ] && [ "$current" = "$previous" ] &&
      { printf '%s' "$current" | grep -Fq "$marker" ||
        { [ -n "$side" ] && pane_holds_marker "$side" "$marker"; }; }; then
      return 0
    fi
    previous="$current"
    sleep 0.05
  done
  dump_diagnostics "$label"
  die "$label did not settle within 12 seconds"
}
settle_zz() {
  wait_settled zz "$1" "$2 settled on the zz screen" zz
}
settle_both() {
  settle_zz "$1" "$2"
  [ "$PIN_ATTACHED" -eq 0 ] ||
    wait_settled tmux "$1" "$2 settled on the tmux screen" tmux
}
# Rows first, then the cursor, over the whole decoded screen of both windows.
diff_screens() {
  local name="$1"
  local left="$2"
  local right="$3"
  local left_rows right_rows left_cursor right_cursor index differing total
  mapfile -t left_rows < <(capture_screen "$left")
  mapfile -t right_rows < <(capture_screen "$right")
  left_cursor="$(cursor_tuple "$left")"
  right_cursor="$(cursor_tuple "$right")"
  if [ -n "$CAPTURE_DIR" ]; then
    printf '%s\n' "${left_rows[@]-}" >"$CAPTURE_DIR/$CASE_GROUP.$name.$left.screen.txt"
    printf '%s\n' "${right_rows[@]-}" >"$CAPTURE_DIR/$CASE_GROUP.$name.$right.screen.txt"
    {
      printf '%s: %s\n' "$left" "$left_cursor"
      printf '%s: %s\n' "$right" "$right_cursor"
    } >"$CAPTURE_DIR/$CASE_GROUP.$name.cursor.txt"
  fi
  total="$ROWS_UNDER_TEST"
  differing=-1
  for ((index = 0; index < total; index++)); do
    if [ "${left_rows[index]-}" != "${right_rows[index]-}" ]; then
      differing="$index"
      break
    fi
  done
  LAST_ROWS_DIFFERED=0
  LAST_CURSOR_DIFFERED=0
  [ "$differing" -lt 0 ] || LAST_ROWS_DIFFERED=1
  [ "$left_cursor" = "$right_cursor" ] || LAST_CURSOR_DIFFERED=1
  if [ "$LAST_ROWS_DIFFERED" -eq 0 ] && [ "$LAST_CURSOR_DIFFERED" -eq 0 ]; then
    return 0
  fi
  printf '      %s against %s at %s\n' "$left" "$right" "$name"
  if [ "$LAST_ROWS_DIFFERED" -eq 1 ]; then
    printf '      first differing row %s of %s\n' "$differing" "$total"
    printf '        %s: %s\n' "$right" "$(printf '%s' "${right_rows[differing]-}" | cat -v)"
    printf '        %s: %s\n' "$left" "$(printf '%s' "${left_rows[differing]-}" | cat -v)"
  else
    printf '      all %s rows identical\n' "$total"
  fi
  printf '      cursor %s: %s\n' "$right" "$right_cursor"
  printf '      cursor %s: %s\n' "$left" "$left_cursor"
  return 1
}
# The canvas the pin would draw, cell for cell, after the surface withdrew.
# Both sides are cleared and marked first: a verb that splits and un-splits
# reflows its pane and the pin's untouched pane never did.
canvas_is_the_pin() {
  local name="$1"
  pin_pane_titles
  clear_and_mark "$name"
  if diff_screens "$name" zz tmux; then
    pass "$name"
  else
    fail "$name" "the canvas is not the one the pin draws"
  fi
}

# --- attaching -------------------------------------------------------------

write_attach() {
  local side="$1"
  local destination="$2"
  printf '#!/usr/bin/env bash\n' >"$destination"
  if [ "$side" = zz ]; then
    printf 'exec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL -u XDG_STATE_HOME HOME=%q XDG_CONFIG_HOME=%q ZZ_LOG_DIR=%q TMUX_TMPDIR=/tmp %q --socket %q attach-session -t %q 2> >(tee -a %q >&2)\n' \
      "$ZZ_HOME" "$ZZ_HOME/config" "$ZZ_LOG_DIR" "$ZZ_BIN" "$ZZ_SOCKET" "=$INNER_SESSION" "$ZZ_CLIENT_STDERR" >>"$destination"
  else
    printf 'exec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL -u XDG_STATE_HOME HOME=%q XDG_CONFIG_HOME=%q TMUX_TMPDIR=/tmp %q -L %q attach-session -t %q 2> >(tee -a %q >&2)\n' \
      "$TMUX_HOME" "$TMUX_HOME/config" "$TMUX_BIN" "$INNER_SOCKET_NAME" "=$INNER_SESSION" "$TMUX_CLIENT_STDERR" >>"$destination"
  fi
  chmod +x "$destination"
}

pin_dynamic_values() {
  local side="$1"
  side_command "$side" set-option -g status-right '' || die "$side refused status-right"
  side_command "$side" set-option -g status-left L || die "$side refused status-left"
  side_command "$side" set-option -g automatic-rename off || die "$side refused automatic-rename"
}

client_count_is() {
  [ "$(side_command "$1" list-clients -F '#{client_session}' 2>/dev/null | grep -c "^$INNER_SESSION$")" = "$2" ]
}

# One outer window per client. The zz session always exists; the pin session and
# its clients only where a case compares canvases.
open_client() {
  local window="$1"
  local side="$2"
  local columns="$3"
  local script="$SCRATCH_DIR/attach-$side.sh"
  tmux_outer_command new-window -d -n "$window" "$script" || die "could not open $window"
  if [ "$columns" != "$COLUMNS_UNDER_TEST" ]; then
    tmux_outer_command resize-window -t "=$OUTER_SESSION:$window" -x "$columns" -y "$ROWS_UNDER_TEST" ||
      die "could not resize $window"
  fi
  wait_for "outer $window at ${columns}x$ROWS_UNDER_TEST" \
    outer_pane_is "=$OUTER_SESSION:$window" "${columns}x$ROWS_UNDER_TEST"
}

# A fresh pair of sessions and a fresh outer server for every group, so an
# option or a pane a group left behind cannot reach the next one.
fresh_group() {
  local label="$1"
  local with_pin="$2"
  CASE_GROUP="$label"
  PIN_ATTACHED="$with_pin"
  tmux_outer_command kill-server >/dev/null 2>&1 || true
  zz_command kill-session -t "=$INNER_SESSION" >/dev/null 2>&1 || true
  tmux_inner_command kill-session -t "=$INNER_SESSION" >/dev/null 2>&1 || true
  zz_command new-session -d -s "$INNER_SESSION" -n "$WINDOW_NAME" \
    -x "$COLUMNS_UNDER_TEST" -y "$ROWS_UNDER_TEST" "$INNER_SHELL" ||
    die "could not create the zz session"
  pin_dynamic_values zz
  tmux_outer_command -f /dev/null new-session -d -s "$OUTER_SESSION" -n holder \
    -x "$COLUMNS_UNDER_TEST" -y "$ROWS_UNDER_TEST" "sleep 3600" ||
    die "could not create the outer session"
  tmux_outer_command set-option -g status off
  tmux_outer_command set-option -g remain-on-exit off
  if [ "$with_pin" -eq 1 ]; then
    tmux_inner_command -f /dev/null new-session -d -s "$INNER_SESSION" -n "$WINDOW_NAME" \
      -x "$COLUMNS_UNDER_TEST" -y "$ROWS_UNDER_TEST" "$INNER_SHELL" ||
      die "could not create the tmux session"
    pin_dynamic_values tmux
  fi
}

attach_default_clients() {
  open_client zz zz "$COLUMNS_UNDER_TEST"
  wait_for "zz client attached" client_count_is zz 1
  zz_command rename-window -t "=$INNER_SESSION:0" "$WINDOW_NAME" >/dev/null
  zz_command select-pane -t "=$INNER_SESSION:0.0" -T "$PANE_TITLE" >/dev/null
  if [ "$PIN_ATTACHED" -eq 1 ]; then
    open_client tmux tmux "$COLUMNS_UNDER_TEST"
    wait_for "tmux client attached" client_count_is tmux 1
    tmux_inner_command rename-window -t "=$INNER_SESSION:0" "$WINDOW_NAME" >/dev/null
    tmux_inner_command select-pane -t "=$INNER_SESSION:0.0" -T "$PANE_TITLE" >/dev/null
  fi
}

# A verb reached through a user binding, which is the second path the contract
# names. -n keeps the key out of every prefix table, and the key is sent to the
# CLIENT through its outer pane, never to the inner pane.
bind_and_press() {
  local key="$1"
  shift
  zz_command bind-key -n "$key" "$@" || die "zz refused bind-key -n $key $*"
  tmux_outer_command send-keys -t "=$OUTER_SESSION:${CLIENT_WINDOW:-zz}" "$key"
}
press() {
  tmux_outer_command send-keys -t "=$OUTER_SESSION:${CLIENT_WINDOW:-zz}" "$@"
}
# Bounded, and it passes only when the pane's own grid holds the text, which no
# amount of re-sending can produce while a surface owns the keyboard.
# An Escape and the next character merge into one Alt- key at the terminal
# parser, so the Escape that unfocuses a surface and the key that has to reach
# the pane are sent in separate rounds, each round ending in a bounded poll on
# the only observable there is: the pane's own grid. Nothing here can pass while
# a surface owns the keyboard, because a focused surface never hands the key on.
press_until_pane() {
  local pane="$1"
  local text="$2"
  local unfocus="${3:-}"
  local attempt poll
  for ((attempt = 0; attempt < 24; attempt++)); do
    if [ -n "$unfocus" ] && [ $((attempt % 2)) -eq 0 ]; then
      press "$unfocus"
    else
      press -l "$text"
      press Enter
    fi
    for ((poll = 0; poll < 10; poll++)); do
      if pane_grid_has zz "$pane" "$text"; then
        return 0
      fi
      sleep 0.05
    done
  done
  return 1
}
CLIENT_WINDOW=zz

SIDEBAR_MARKER='zz at '
PICKER_MARKER='Select pane kind'
CARD_FOOTER='open in the zz app'

# --- clause 1: the verbs ----------------------------------------------------

native_names() {
  awk '/^pub static NATIVE_COMMAND_NAMES/,/^\];/' "$CATALOG" |
    grep -o '"[a-z][a-z-]*"' | tr -d '"'
}

run_names() {
  fresh_group names 0
  local name total=0
  local names=()
  mapfile -t names < <(native_names)
  for name in "${names[@]}"; do
    [ -n "$name" ] || continue
    total=$((total + 1))
    run_zz "$name" </dev/null
    if [ "$LAST_OUTPUT" = "unknown command: $name" ]; then
      fail "name-$name" 'the catalog declares it and the server does not know it'
    else
      pass "name-$name"
    fi
  done
  [ "$total" -ge 25 ] || die "the catalog yielded only $total native names"
  printf '      %s declared native names, every one reachable by that name\n' "$total"
}

pane_count() {
  side_command "$1" list-panes -t "=$INNER_SESSION" -F '#{pane_id}' | grep -c .
}
window_count() {
  side_command "$1" list-windows -t "=$INNER_SESSION" -F '#{window_index}' | grep -c .
}
pane_count_is() {
  [ "$(pane_count "$1")" = "$2" ]
}
window_count_is() {
  [ "$(window_count "$1")" = "$2" ]
}
pane_grid() {
  side_command "$1" capture-pane -p -S -30 -t "$2" 2>/dev/null
}
pane_grid_has() {
  pane_grid "$1" "$2" | grep -Fq -- "$3"
}
pane_grid_lacks() {
  local name="$1"
  local pane="$2"
  local text="$3"
  if pane_grid zz "$pane" | grep -Fq -- "$text"; then
    fail "$name" "the pane grid took a key the surface owned: $text"
  else
    pass "$name"
  fi
}
sidebar_up() {
  capture_plain "${CLIENT_WINDOW:-zz}" | grep -Fq "$SIDEBAR_MARKER"
}
sidebar_down() {
  ! capture_plain "${CLIENT_WINDOW:-zz}" | grep -Fq "$SIDEBAR_MARKER"
}
sidebar_cursor_is() {
  sidebar_up &&
    [ "$(tmux_outer_command display-message -p -t "=$OUTER_SESSION:${CLIENT_WINDOW:-zz}" '#{cursor_flag}')" = "$1" ]
}
sidebar_focused() {
  sidebar_cursor_is 0
}
sidebar_has_focus() {
  if wait_for_quietly sidebar_focused; then
    pass "$1"
  else
    fail "$1" 'the drawn sidebar never took keyboard focus'
    return 1
  fi
}

run_sidebar_verbs() {
  fresh_group sidebar 0
  attach_default_clients
  local pane
  pane="$(first_pane zz)"

  # The CLI is not an interactive client, so the daemon refuses the way the pin
  # refuses a client command with no client.
  refuses direct 'focus-sidebar requires an interactive client' focus-sidebar

  bind_and_press F8 focus-sidebar
  screen_has binding zz "$SIDEBAR_MARKER"
  equals window-columns "$((COLUMNS_UNDER_TEST - 29))x$((ROWS_UNDER_TEST - 1))" "$(window_size zz)"

  # INPUT OWNERSHIP, ordered without a sleep: both keys travel the same client
  # input stream, so once the withdrawal the second key asked for is on the
  # screen the first key has already been handled. j belongs to the sidebar
  # table (crates/zz-client/src/chrome.rs TUI_SIDEBAR_DEFAULTS) and the pane
  # must never see it.
  press j
  press q
  wait_for 'the sidebar withdrawn by q' sidebar_down
  pane_grid_lacks focused-key-is-the-sidebars "$pane" j
  equals window-columns-restored "${COLUMNS_UNDER_TEST}x$((ROWS_UNDER_TEST - 1))" "$(window_size zz)"

  # SHOWN BUT NOT FOCUSED. Escape is SidebarCancel: the tree stays drawn and the
  # pane owns the keyboard again, which is the contract's rule that local chrome
  # consumes ordinary keys only inside the input context that owns them.
  press F8
  wait_for 'the sidebar shown again' sidebar_up
  if press_until_pane "$pane" ZKEY Escape; then
    pass unfocused-hands-the-key-to-the-pane
  else
    fail unfocused-hands-the-key-to-the-pane 'the unfocused sidebar kept the keyboard'
  fi
  screen_has still-drawn-while-unfocused zz "$SIDEBAR_MARKER"
  press F8
  sidebar_has_focus refocused || {
    dump_diagnostics 'the sidebar focused again'
    die 'the sidebar did not regain focus within 12 seconds'
  }
  press q
  wait_for 'the sidebar withdrawn' sidebar_down
  pass withdrawn
}

run_picker_verbs() {
  fresh_group picker 0
  attach_default_clients

  accepts direct split-window --kind picker
  wait_for 'the picker pane' pane_count_is zz 2
  screen_has card zz "$PICKER_MARKER"
  screen_has card-terminal zz 'Terminal (t)'
  screen_has card-browser zz 'Browser (b)'
  screen_has card-agent zz 'Agent (a) — runs in the zz app'
  screen_has card-editor zz 'Editor (e)'
  screen_has card-keys zz '↑/↓ or j/k · Enter · Esc'

  # The Editor choice is refused by the daemon and the card stays up. The
  # message is the product's own experimental gate, not a parity switch: the
  # other three kinds need no flag at all.
  press e
  message_row_has editor-refused zz \
    'editor panes are experimental; enable experimental-editor-pane in Settings → Advanced first'
  screen_has survives-a-refusal zz "$PICKER_MARKER"

  # Esc is the picker's cancel and it runs kill-pane.
  press Escape
  wait_for 'the picker cancelled' pane_count_is zz 1
  screen_lacks cancelled zz "$PICKER_MARKER"

  bind_and_press F7 split-window --kind picker
  wait_for 'the picker pane through a binding' pane_count_is zz 2
  screen_has binding zz "$PICKER_MARKER"
  press Enter
  screen_lacks enter-materializes zz "$PICKER_MARKER"
  wait_for 'the materialized pane' pane_count_is zz 2
  pass enter-keeps-the-pane
  run_zz kill-pane -t "$(active_pane zz)"
  wait_for 'the materialized pane killed' pane_count_is zz 1

  # select-pane-kind is the verb the picker's own keys run, and a picker pane
  # is the only pane awaiting a kind: on any other pane the daemon says so.
  local picker_pane terminal_pane
  terminal_pane="$(first_pane zz)"
  refuses kind-needs-a-choice \
    'select-pane-kind requires exactly one of: terminal, browser, agent, editor' \
    select-pane-kind -t "$terminal_pane"
  refuses kind-editor-is-gated \
    'editor panes are experimental; enable experimental-editor-pane in Settings → Advanced first' \
    select-pane-kind -t "$terminal_pane" editor
  refuses kind-on-a-pane-not-awaiting-one "pane $terminal_pane is not awaiting a type selection" \
    select-pane-kind -t "$terminal_pane" browser
  accepts split-again split-window --kind picker
  wait_for 'the picker pane again' pane_count_is zz 2
  picker_pane="$(active_pane zz)"
  accepts kind-browser select-pane-kind -t "$picker_pane" browser
  screen_has kind-browser-card zz 'Browser'
  screen_lacks kind-browser-replaces-the-card zz "$PICKER_MARKER"
  run_zz kill-pane -t "$(active_pane zz)"
  wait_for 'the converted pane killed' pane_count_is zz 1
}

run_browser_verbs() {
  fresh_group browser 0
  attach_default_clients
  local browser_pane

  refuses url-needs-a-value 'set-browser-url needs a URL' set-browser-url
  refuses tabs-need-a-value 'set-browser-tabs needs at least one URL' set-browser-tabs
  refuses profile-needs-a-value 'set-browser-profile needs exactly one profile name' set-browser-profile
  refuses capture-needs-a-path 'capture-browser needs an output path (-o)' capture-browser

  accepts split split-window --kind browser
  wait_for 'the browser pane' pane_count_is zz 2
  browser_pane="$(active_pane zz)"
  screen_has card zz 'Browser'
  screen_has card-url zz 'about:blank'
  screen_has card-footer zz "$CARD_FOOTER"

  # No Kitty graphics reach a pane inside the pinned tmux, so the card is what
  # the raw TUI draws for a browser and there is no frame to screenshot.
  refuses screenshot-in-a-raw-tui 'browser screenshots require the zz app' \
    capture-browser -t "$browser_pane" -o /tmp/zz-superset-never-written.png
  accepts url set-browser-url -t "$browser_pane" https://example.invalid/
  screen_has card-follows-url zz 'https://example.invalid/'
  accepts tabs set-browser-tabs -t "$browser_pane" https://first.invalid/ https://second.invalid/
  screen_has card-follows-tabs zz 'https://first.invalid/'
  accepts profile set-browser-profile -t "$browser_pane" work

  bind_and_press F5 set-browser-url -t "$browser_pane" https://bound.invalid/
  screen_has binding zz 'https://bound.invalid/'

  accepts new-window new-window --kind browser
  wait_for 'the browser window' window_count_is zz 2
  pass browser-adds-a-window
  run_zz kill-window -t "=$INNER_SESSION:1"
  wait_for 'the browser window killed' window_count_is zz 1
  run_zz kill-pane -t "$browser_pane"
  wait_for 'the browser pane killed' pane_count_is zz 1
}

run_agent_verbs() {
  fresh_group agent 0
  attach_default_clients
  local agent_pane

  refuses send-needs-text 'agent-send needs text on the command line or on standard input' agent-send
  refuses respond-needs-a-choice 'agent-respond needs exactly one of --allow, --deny, or --option ID' agent-respond
  refuses provider-needs-a-value 'set-agent-provider needs exactly one provider' set-agent-provider
  refuses restart-on-another-kind "pane $(first_pane zz) is not an agent" \
    restart-agent-pane -t "$(first_pane zz)"

  accepts split split-window --kind agent
  wait_for 'the agent pane' pane_count_is zz 2
  agent_pane="$(active_pane zz)"
  screen_has card zz 'Agent'
  screen_has card-footer zz "$CARD_FOOTER"

  refuses send-in-a-raw-tui 'agent commands require the zz app' agent-send -t "$agent_pane" hello
  accepts restart restart-agent-pane -t "$agent_pane"
  screen_has card-after-restart zz 'Agent'
  accepts provider set-agent-provider -t "$agent_pane" claude
  screen_has card-follows-provider zz 'Claude Code'
  bind_and_press F4 set-agent-provider -t "$agent_pane" codex
  screen_has binding zz 'Codex'

  refuses editor-path-on-another-kind "pane $agent_pane is not an editor" \
    set-editor-path -t "$agent_pane" /tmp/zz-superset-editor.txt
  run_zz kill-pane -t "$agent_pane"
  wait_for 'the agent pane killed' pane_count_is zz 1
}

marks_message() {
  printf '%s has no shell-integration marks; %s needs a shell that emits OSC 133 prompt marks (ghostty, kitty, wezterm, or starship shell integration all do)' \
    "$1" "$2"
}

run_output_verbs() {
  fresh_group output 0
  attach_default_clients
  local pane
  pane="$(first_pane zz)"

  # A pane id carries a %, so the message is built with the id as an argument
  # rather than as part of a format.
  refuses show-needs-marks "$(marks_message "$pane" show-last-output)" show-last-output -t "$pane"
  refuses send-needs-marks "$(marks_message "$pane" send-last-output)" send-last-output -t "$pane"

  run_zz tools
  if [ "$LAST_STATUS" -eq 0 ] && printf '%s' "$LAST_OUTPUT" | head -1 | grep -Fq '# Workspace tools'; then
    pass tools-direct
  else
    fail tools-direct "exit $LAST_STATUS, first line: $(printf '%s' "$LAST_OUTPUT" | head -1)"
  fi

  bind_and_press F9 tools
  screen_has tools-overlay zz '# Workspace tools'
  # The overlay owns the keyboard while it is up, and both keys travel the same
  # client stream, so the pane must not have seen the first one.
  press z
  press Escape
  wait_for 'the overlay closed' overlay_closed
  pane_grid_lacks overlay-owns-its-keys "$pane" z
  screen_lacks tools-overlay-closed zz '# Workspace tools'

  accepts search-direct copy-mode-search-prompt -t "$pane"
  message_row_has search-without-an-overlay-direct zz 'terminal search is unsupported here'
  bind_and_press F6 copy-mode-search-prompt -t "$pane"
  message_row_has search-without-an-overlay-bound zz 'terminal search is unsupported here'
}

overlay_closed() {
  ! capture_plain "${CLIENT_WINDOW:-zz}" | grep -Fq '# Workspace tools'
}

run_misc_verbs() {
  fresh_group misc 0
  attach_default_clients
  local pane
  pane="$(first_pane zz)"

  refuses text-needs-a-value 'send-text needs text on the command line or on standard input' send-text
  accepts text send-text -t "$pane" 'SENTTEXT'
  wait_for 'the sent text in the pane' pane_grid_has zz "$pane" SENTTEXT
  pass text-reaches-the-pane
  bind_and_press F3 send-text -t "$pane" 'BOUNDTEXT'
  wait_for 'the bound text in the pane' pane_grid_has zz "$pane" BOUNDTEXT
  pass text-through-a-binding

  accepts marker debug-marker superset-marker
  bind_and_press F2 debug-marker superset-bound-marker
  accepts reload reload-config
  bind_and_press F1 reload-config

  # HOME and XDG_CONFIG_HOME are inside the scratch directory, so there really
  # is no tmux configuration to import and the refusal is the declared one.
  refuses import-with-nothing-to-import 'no tmux configuration found' import-tmux-config
}

# EVERY declared verb through a user binding, which is the second of the two
# paths the contract names. One key is rebound before each case, so the case
# that follows cannot inherit the one before it.
#
# A verb that answers nothing visible is bound as a SEQUENCE, `verb ; rename-window
# TOKEN`, and the case waits for the token on the client's message row. Both
# halves of that were measured on 2026-09-13 rather than assumed: a bound
# sequence runs its second command on BOTH binaries even when the first one
# failed, so the rename alone proves nothing, and a zz client message stays on
# the row indefinitely (still there after eleven seconds), so a verb that
# refused keeps its message where the status row would be and the token never
# arrives. The token is therefore a positive observable for `the verb ran and
# said nothing`.
BOUND_KEY=F8
bind_the_key() {
  zz_command bind-key -n "$BOUND_KEY" "$@" || die "zz refused bind-key -n $BOUND_KEY $*"
  press "$BOUND_KEY"
}
bound_message() {
  local name="$1"
  local message="$2"
  shift 2
  bind_the_key "$@"
  message_row_has "$name" zz "$message"
}
bound_screen() {
  local name="$1"
  local text="$2"
  shift 2
  bind_the_key "$@"
  screen_has "$name" zz "$text"
}
# The verb that answers nothing at all. The binding is a sequence whose second
# command reports on the client's own message row, so the case asserts that the
# key ran the verb through the binding; WHAT the verb answered is asserted on
# the direct path, where the answer is stdout, stderr and an exit status. That
# split is deliberate and measured: a bound sequence runs its later commands on
# both binaries even when an earlier one failed, and a status repaint overwrites
# a message that a failure had just put on the row, so a token here cannot tell
# success from failure and is not asked to.
bound_reaches_the_verb() {
  local name="$1"
  shift
  local token="BOUND-$name"
  # The reporter is the FIRST command of the sequence, not the last: the verb
  # this form is for takes free-form arguments and swallows the `;` separator
  # along with everything after it. The sequence still runs in order and zz does
  # not stop at a failure, so the token says the binding fired and the verb ran
  # after it.
  bind_the_key display-message "$token" ';' "$@"
  message_row_has "$name" zz "$token"
}

run_binding_pass() {
  fresh_group bindings 0
  attach_default_clients
  local pane picker_pane browser_pane
  pane="$(first_pane zz)"

  # The two that answer nothing go first, while the message row is still the
  # status row and no earlier refusal is sitting on it.
  bound_reaches_the_verb debug-marker debug-marker bound-pass-marker
  bound_message reload-config 'Reloaded zz configuration' reload-config

  bound_message agent-respond \
    'agent-respond needs exactly one of --allow, --deny, or --option ID' agent-respond
  bound_message agent-send \
    'agent-send needs text on the command line or on standard input' agent-send
  bound_message capture-browser 'capture-browser needs an output path (-o)' capture-browser
  bound_message copy-mode-search-prompt 'terminal search is unsupported here' \
    copy-mode-search-prompt -t "$pane"
  bound_message import-tmux-config 'no tmux configuration found' import-tmux-config
  bound_message restart-agent-pane "pane $pane is not an agent" restart-agent-pane -t "$pane"
  bound_message select-pane-kind \
    'select-pane-kind requires exactly one of: terminal, browser, agent, editor' select-pane-kind
  bind_the_key send-last-output -t "$pane"
  message_row_is_truncated send-last-output zz "$(marks_message "$pane" send-last-output)"
  bind_the_key show-last-output -t "$pane"
  message_row_is_truncated show-last-output zz "$(marks_message "$pane" show-last-output)"
  bound_message set-agent-provider 'set-agent-provider needs exactly one provider' set-agent-provider
  bound_message set-browser-profile 'set-browser-profile needs exactly one profile name' set-browser-profile
  bound_message set-browser-tabs 'set-browser-tabs needs at least one URL' set-browser-tabs
  bound_message set-browser-url 'set-browser-url needs a URL' set-browser-url
  bound_message set-editor-path "pane $pane is not an editor" \
    set-editor-path -t "$pane" /tmp/zz-superset-editor.txt

  bind_the_key send-text -t "$pane" BOUNDPASSTEXT
  if wait_for_quietly pane_grid_has zz "$pane" BOUNDPASSTEXT; then
    pass send-text
  else
    fail send-text 'the bound send-text never reached the pane'
  fi

  bound_screen focus-sidebar "$SIDEBAR_MARKER" focus-sidebar
  press q
  wait_for 'the sidebar withdrawn' sidebar_down

  bound_screen tools '# Workspace tools' tools
  press Escape
  wait_for 'the overlay closed' overlay_closed

  bound_screen picker-split "$PICKER_MARKER" split-window --kind picker
  wait_for 'the bound picker pane' pane_count_is zz 2
  picker_pane="$(active_pane zz)"
  press Escape
  wait_for 'the bound picker cancelled' pane_count_is zz 1

  bound_screen browser-split 'about:blank' split-window --kind browser
  wait_for 'the bound browser pane' pane_count_is zz 2
  browser_pane="$(active_pane zz)"
  run_zz kill-pane -t "$browser_pane"
  wait_for 'the bound browser pane killed' pane_count_is zz 1

  bound_screen agent-split 'Agent' split-window --kind agent
  wait_for 'the bound agent pane' pane_count_is zz 2
  run_zz kill-pane -t "$(active_pane zz)"
  wait_for 'the bound agent pane killed' pane_count_is zz 1

  bind_the_key new-window --kind browser
  if wait_for_quietly window_count_is zz 2; then
    pass browser-window
  else
    fail browser-window 'the bound new-window --kind browser added no window'
  fi
  run_zz kill-window -t "=$INNER_SESSION:1"
  wait_for 'the bound browser window killed' window_count_is zz 1
}

# --- clause 2: the terminal around the surfaces -----------------------------

both() {
  side_command zz "$@" || die "zz refused $1"
  side_command tmux "$@" || die "tmux refused $1"
}
pane_widths() {
  side_command "$1" list-panes -t "=$INNER_SESSION" -F '#{pane_width}' | sort -u | tr '\n' ' '
}
# The same ordinary commands on both sides, every one against an explicit
# target so neither binary has to agree about what a loose target means while
# one of them has a zz surface up.
ordinary_sequence() {
  both new-window -d -n extra "$INNER_SHELL"
  both select-window -t "=$INNER_SESSION:0"
  both split-window -v -t "=$INNER_SESSION:0.0" "$INNER_SHELL"
  both select-pane -t "=$INNER_SESSION:0.0"
  both resize-pane -t "=$INNER_SESSION:0.0" -D 3
  both select-pane -t "=$INNER_SESSION:0.1"
}
window_inventory() {
  side_command "$1" list-windows -t "=$INNER_SESSION" \
    -F '#{window_index} #{window_name} #{window_width}x#{window_height} active=#{window_active}' | sort
}
ordinary_commands_agree() {
  local name="$1"
  ordinary_sequence
  equals "$name-layout" "$(pane_geometry tmux)" "$(pane_geometry zz)"
  equals "$name-zz-panes-fill-the-window" \
    "$(side_command zz display-message -p -t "=$INNER_SESSION:0" '#{window_width} ')" \
    "$(pane_widths zz)"
  equals "$name-pin-panes-fill-the-window" \
    "$(side_command tmux display-message -p -t "=$INNER_SESSION:0" '#{window_width} ')" \
    "$(pane_widths tmux)"
  # The window a surface is not in has to be the pin's too, name, size, active
  # flag and all, except for the columns the sidebar takes from the session.
  if [ "$(window_size zz)" = "$(window_size tmux)" ]; then
    equals "$name-windows" "$(window_inventory tmux)" "$(window_inventory zz)"
  else
    equals "$name-window-inventory-apart-from-the-sidebar-columns" \
      "$(window_inventory tmux | sed "s/ [0-9]*x/ x/")" \
      "$(window_inventory zz | sed "s/ [0-9]*x/ x/")"
    equals "$name-window-columns-under-the-sidebar" \
      "$((COLUMNS_UNDER_TEST - 29))x$((ROWS_UNDER_TEST - 1))" "$(window_size zz)"
  fi
}
drop_extra_panes() {
  both kill-pane -t "=$INNER_SESSION:0.1"
  both kill-window -t "=$INNER_SESSION:1"
}

detach_and_reattach() {
  zz_command detach-client -s "=$INNER_SESSION" >/dev/null 2>&1 || true
  tmux_inner_command detach-client -s "=$INNER_SESSION" >/dev/null 2>&1 || true
  wait_for 'the zz client gone' client_count_is zz 0
  wait_for 'the tmux client gone' client_count_is tmux 0
  open_client zz zz "$COLUMNS_UNDER_TEST"
  open_client tmux tmux "$COLUMNS_UNDER_TEST"
  wait_for 'the zz client back' client_count_is zz 1
  wait_for 'the tmux client back' client_count_is tmux 1
}

run_terminal_around_the_sidebar() {
  fresh_group around-sidebar 1
  attach_default_clients
  canvas_is_the_pin baseline

  bind_and_press F8 focus-sidebar
  wait_for 'the sidebar up' sidebar_up
  ordinary_commands_agree with-the-sidebar-up
  press q
  wait_for 'the sidebar withdrawn' sidebar_down
  canvas_is_the_pin withdrawn

  # Client-local state does not survive the client: a reattached client draws
  # the canvas the pin draws, with no sidebar and the full window width.
  press F8
  wait_for 'the sidebar up again' sidebar_up
  detach_and_reattach
  screen_lacks reattached-without-the-sidebar zz "$SIDEBAR_MARKER"
  window_has_size reattached-window-columns zz \
    "${COLUMNS_UNDER_TEST}x$((ROWS_UNDER_TEST - 1))"
  canvas_is_the_pin reattached
  drop_extra_panes
  canvas_is_the_pin one-pane-again
}

# A pane surface is removed rather than withdrawn, so the pin gets the same
# pane arithmetic: an ordinary split where zz gets the zz kind, and a kill on
# both sides afterwards.
run_terminal_around_a_pane_surface() {
  local label="$1"
  local kind="$2"
  local marker="$3"
  fresh_group "around-$label" 1
  attach_default_clients
  canvas_is_the_pin baseline

  side_command zz split-window --kind "$kind" -t "=$INNER_SESSION:0.0" || die "zz refused a $kind split"
  side_command tmux split-window -v -t "=$INNER_SESSION:0.0" "$INNER_SHELL" ||
    die "tmux refused split-window"
  screen_has surface zz "$marker"
  equals layout "$(pane_geometry tmux)" "$(pane_geometry zz)"

  # A pane is session state, so it is still there for the client that comes
  # back, and the layout it comes back to is the pin's.
  detach_and_reattach
  screen_has surface-survives-a-reattach zz "$marker"
  equals layout-after-a-reattach "$(pane_geometry tmux)" "$(pane_geometry zz)"

  both select-pane -t "=$INNER_SESSION:0.0"
  both resize-pane -t "=$INNER_SESSION:0.0" -D 3
  equals layout-after-ordinary-commands "$(pane_geometry tmux)" "$(pane_geometry zz)"
  # The picker's card is drawn for the ACTIVE pane only (render.rs draws the
  # picker arm under `if active`), so the surface is selected again before the
  # marker is asked for; a browser or Agent card draws either way.
  both select-pane -t "=$INNER_SESSION:0.1"
  screen_has surface-after-ordinary-commands zz "$marker"

  both kill-pane -t "=$INNER_SESSION:0.1"
  screen_lacks surface-gone zz "$marker"
  canvas_is_the_pin removed
}

run_terminal_around_the_overlay() {
  fresh_group around-overlay 1
  attach_default_clients
  canvas_is_the_pin baseline

  bind_and_press F9 tools
  screen_has overlay zz '# Workspace tools'

  # The overlay is client state, not session state: it does not survive the
  # client, and the canvas the next client draws is the pin's.
  detach_and_reattach
  screen_lacks overlay-does-not-survive-a-reattach zz '# Workspace tools'
  canvas_is_the_pin reattached
  bind_and_press F9 tools
  screen_has overlay-again zz '# Workspace tools'

  # A command that does not move the layout leaves the overlay up and means
  # exactly what it means on the pin.
  both rename-window -t "=$INNER_SESSION:0" renamed
  both set-option -g status-left Q
  both select-pane -t "=$INNER_SESSION:0.0"
  screen_has overlay-survives-commands-that-keep-the-layout zz '# Workspace tools'
  equals layout-under-those-commands "$(pane_geometry tmux)" "$(pane_geometry zz)"
  both set-option -g status-left L
  both rename-window -t "=$INNER_SESSION:0" "$WINDOW_NAME"

  # MEASURED 2026-09-13: the first command that moves the layout withdraws the
  # overlay by itself. Nothing is pressed to close it, and that matters: with
  # the overlay already gone, an Escape sent to close it reaches the pane
  # instead and the pane's terminal eats the next typed line with it.
  ordinary_commands_agree with-the-overlay-up
  wait_for 'the overlay withdrawn by a layout change' overlay_closed
  pass withdrawn-by-a-layout-change
  canvas_is_the_pin withdrawn
  drop_extra_panes
  canvas_is_the_pin one-pane-again
}

# --- clause 3: a second client ---------------------------------------------
#
# Every client here is as wide as the window the sidebar leaves, so no client is
# wider than its window and every cell of both second clients is asserted. See
# the header for the measurement that made that the honest shape.
SECOND_COLUMNS=62
FIRST_COLUMNS=91

run_second_client() {
  fresh_group clients 1
  open_client zz zz "$FIRST_COLUMNS"
  open_client zzb zz "$SECOND_COLUMNS"
  open_client tmux tmux "$SECOND_COLUMNS"
  open_client tmuxb tmux "$SECOND_COLUMNS"
  wait_for 'two zz clients' client_count_is zz 2
  wait_for 'two tmux clients' client_count_is tmux 2
  both rename-window -t "=$INNER_SESSION:0" "$WINDOW_NAME"
  both select-pane -t "=$INNER_SESSION:0.0" -T "$PANE_TITLE"
  clear_and_mark before

  if diff_screens before zzb tmuxb; then
    pass second-client-baseline
  else
    fail second-client-baseline 'the second clients differ before any zz verb'
  fi

  CLIENT_WINDOW=zz
  bind_and_press F8 focus-sidebar
  wait_for 'the sidebar on the first client' sidebar_up
  screen_has sidebar-on-the-first-client zz "$SIDEBAR_MARKER"
  screen_lacks sidebar-not-on-the-second-client zzb "$SIDEBAR_MARKER"
  if diff_screens shown zzb tmuxb; then
    pass second-client-unchanged-while-shown
  else
    fail second-client-unchanged-while-shown \
      'showing the sidebar in one client changed the other'
  fi
  equals window-columns-under-the-sidebar \
    "${SECOND_COLUMNS}x$((ROWS_UNDER_TEST - 1))" "$(window_size zz)"

  press q
  wait_for 'the sidebar withdrawn' sidebar_down
  if diff_screens withdrawn zzb tmuxb; then
    pass second-client-unchanged-after-withdrawal
  else
    fail second-client-unchanged-after-withdrawal \
      'withdrawing the sidebar in one client changed the other'
  fi

  # A pane, unlike the sidebar, is session state: both clients draw the card.
  # Neither draws a frame, because no Kitty graphics reach a pane inside the
  # pinned tmux, and the screenshot verb says so in the message a raw TUI gives.
  local browser_pane
  side_command zz split-window --kind browser -t "=$INNER_SESSION:0.0" || die 'zz refused split-window --kind browser'
  wait_for 'the browser pane' pane_count_is zz 2
  browser_pane="$(active_pane zz)"
  screen_has browser-card-on-the-first-client zz "$CARD_FOOTER"
  screen_has browser-card-on-the-second-client zzb "$CARD_FOOTER"
  refuses browser-screenshot-without-the-app 'browser screenshots require the zz app' \
    capture-browser -t "$browser_pane" -o /tmp/zz-superset-never-written.png
  CLIENT_WINDOW=zz
}

# --- self-check -------------------------------------------------------------
#
# One deliberate fault per channel this file asserts in, each one required to be
# reported by the same code the real cases use. A sabotage is run with the
# fixture's own counters saved and restored, so a self-check run reports only
# what the sabotages proved.

expect_report() {
  local name="$1"
  shift
  local before_failures="$FAILURES"
  local before_checks="$CHECKS"
  "$@" >/dev/null 2>&1 || true
  if [ "$FAILURES" -gt "$before_failures" ]; then
    printf 'ok    self-check %s\n' "$name"
  else
    SELF_CHECK_FAILURES=$((SELF_CHECK_FAILURES + 1))
    printf 'FAIL  self-check %s: the fault was not reported\n' "$name"
  fi
  FAILURES="$before_failures"
  CHECKS="$before_checks"
}

run_self_check() {
  printf 'self-check: one deliberate fault per channel\n'
  WAIT_ATTEMPTS=40
  fresh_group self-check 1
  attach_default_clients
  local pane
  pane="$(first_pane zz)"

  # The reachability channel: a name the server does not know answers exactly
  # the way run_names requires a declared name not to.
  run_zz zz-not-a-declared-verb
  if [ "$LAST_OUTPUT" = 'unknown command: zz-not-a-declared-verb' ]; then
    printf 'ok    self-check an undeclared name answers unknown command\n'
  else
    SELF_CHECK_FAILURES=$((SELF_CHECK_FAILURES + 1))
    printf 'FAIL  self-check undeclared name: got %s\n' "$LAST_OUTPUT"
  fi

  expect_report 'a refusal message that does not match' \
    refuses sabotage 'not the message this verb gives' focus-sidebar
  expect_report 'a verb that accepts where a refusal is declared' \
    refuses sabotage 'focus-sidebar requires an interactive client' list-sessions
  expect_report 'a surface that never drew' \
    screen_has sabotage zz "$SIDEBAR_MARKER"
  expect_report 'a message row that never carried the message' \
    message_row_has sabotage zz 'a message no zz verb ever prints'
  expect_report 'a cut message the row never carried' \
    message_row_is_truncated sabotage zz \
    'a message no zz verb ever prints, written long enough that the row would have to cut it at the client width before it could ever match'

  # The binding channel: the key the case bound is not the key it presses, so
  # the verb never runs and the row never carries its token.
  zz_command bind-key -n F8 display-message BOUND-sabotage >/dev/null
  expect_report 'a bound verb whose key was never pressed' \
    message_row_has sabotage zz BOUND-sabotage

  # The input-ownership channel: the key really does reach the pane when it is
  # typed into the pane, so pane_grid_lacks has to report it.
  zz_command send-keys -t "$pane" 'ZSABOTAGE' Enter >/dev/null
  wait_for 'the sabotage key in the pane' pane_grid_has zz "$pane" ZSABOTAGE
  expect_report 'a key the pane really did take' \
    pane_grid_lacks sabotage "$pane" ZSABOTAGE

  expect_report 'an absent sidebar cannot own keyboard focus' \
    sidebar_has_focus sabotage
  bind_and_press F8 focus-sidebar
  sidebar_has_focus focus-control
  press Escape
  wait_for 'the pane cursor after sidebar Escape' sidebar_cursor_is 1
  expect_report 'a drawn sidebar that never regained focus' \
    sidebar_has_focus sabotage
  press F8
  sidebar_has_focus refocus-control
  press q
  wait_for 'the refocused sidebar withdrawn by q' sidebar_down
  expect_report 'a window that never reaches its required size' \
    window_has_size sabotage zz '1x1'

  # The canvas channel: one side's status row is painted red and nothing else
  # changes, so the whole-screen comparison has to report it.
  zz_command set-option -g status-style bg=red >/dev/null
  expect_report 'a canvas that is not the pin' canvas_is_the_pin sabotage
  zz_command set-option -gu status-style >/dev/null

  # The geometry channel: one side keeps a pane the other does not.
  zz_command split-window -v -t "=$INNER_SESSION:0.0" "$INNER_SHELL" >/dev/null
  expect_report 'a layout only one side has' \
    equals sabotage "$(pane_geometry tmux)" "$(pane_geometry zz)"

  if [ "$SELF_CHECK_FAILURES" -ne 0 ]; then
    printf '%s self-check expectations unmet\n' "$SELF_CHECK_FAILURES"
    return 1
  fi
  printf 'self-check complete: every fault was reported by the channel that owns it\n'
  return 0
}

# --- run --------------------------------------------------------------------

write_attach zz "$SCRATCH_DIR/attach-zz.sh"
write_attach tmux "$SCRATCH_DIR/attach-tmux.sh"
ZZ_CALL_TIMEOUT=0 zz_command -f /dev/null daemon >"$SCRATCH_DIR/zz-daemon.out" 2>"$SCRATCH_DIR/zz-daemon.err" &
ZZ_PID=$!
wait_for "zz daemon socket" test -S "$ZZ_SOCKET"

if [ "$SELF_CHECK" -eq 1 ]; then
  run_self_check
  exit $?
fi

printf 'superset commands beside tmux behaviour (pin %s)\n' "$(basename -- "$TMUX_BIN")"
# ZZ_SUPERSET_ONLY names one group for a re-run of a single failure; the whole
# file runs when it is unset, and it is unset in every recorded proof.
run_group() {
  case "${ZZ_SUPERSET_ONLY:-}" in
  "" | "$1") shift && "$@" ;;
  *) : ;;
  esac
}
run_group names run_names
run_group sidebar run_sidebar_verbs
run_group picker run_picker_verbs
run_group browser run_browser_verbs
run_group agent run_agent_verbs
run_group output run_output_verbs
run_group misc run_misc_verbs
run_group bindings run_binding_pass
run_group around-sidebar run_terminal_around_the_sidebar
run_group around-picker run_terminal_around_a_pane_surface picker picker "$PICKER_MARKER"
run_group around-browser run_terminal_around_a_pane_surface browser browser "$CARD_FOOTER"
run_group around-agent run_terminal_around_a_pane_surface agent agent "$CARD_FOOTER"
run_group around-overlay run_terminal_around_the_overlay
run_group clients run_second_client

if [ "$FAILURES" -ne 0 ]; then
  printf '%s of %s asserted cases failed, %s recorded\n' "$FAILURES" "$CHECKS" "$RECORDS"
  exit 1
fi
printf 'all %s asserted cases hold, %s recorded not asserted\n' "$CHECKS" "$RECORDS"
