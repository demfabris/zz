#!/usr/bin/env bash
# Whole-screen differential for the indicator, message and prompt surfaces.
#
# tui-screen-diff.sh drives the canvas: splits, zooms, borders, sizes, status
# options. Every state it reaches is one an ordinary tmux command produces and
# leaves. None of them is TRANSIENT chrome - copy mode's position indicator, the
# view surface, the armed prefix, a message that expires, a prompt that is typed
# into and cancelled. Those five are what TUI-004's third acceptance clause is
# about, and no campaign proof ever drew one.
#
# THE DECODED SCREEN IS THE CONTRACT, NOT THE BYTE STREAM. Both binaries attach
# inside ONE outer pinned tmux, one window each, and the outer tmux is the
# decoder: `capture-pane -p -e` re-emits SGR from its own grid, so attribute
# order, batching, redundant resets and cursor-movement spelling collapse on
# both sides before anything is compared, while the colour CLASS does not. The
# whole visible screen is compared row index by row index, plus the cursor tuple
# the inner client left in the outer terminal. This is the same contract
# tui-screen-diff.sh states at greater length; the driver below is
# status-row.sh's, widened from one row to the screen.
#
# CONTROLLED DYNAMIC VALUES, set on both sides before the first checkpoint:
#   status-right ''      the default ends in a clock and in %d-%b-%y, which the
#                        pin expands through libc strftime and zz expands
#                        locale-independently. That belongs to status-row.sh.
#   status-left L        a fixed literal, so the left of the row is asserted
#                        rather than emptied.
#   automatic-rename off plus rename-window win: the default name follows the
#                        running command and would race a marker.
#   select-pane -T title fixed: the pin seeds a pane title from gethostname and
#                        zz reports the shell name through its shell
#                        integration - the recorded pane.runtime-facts
#                        decision, not a divergence of this screen.
#   display-time         the message case's own clock. status.c
#                        status_message_set arms a timer of display-time
#                        milliseconds (default 750, options-table.c:844), so a
#                        message left at the default would expire between the
#                        two sides' captures. MESSAGE_HOLD_MS holds it open for
#                        the during-comparison and MESSAGE_EXPIRE_MS retires it
#                        for the restoration one; both are set on both sides.
#   copy-mode-position-format
#                        the pin's default begins with #{t/p:top_line_time},
#                        the timestamp of the line at the top of the copy-mode
#                        screen. Pinned on both sides to the rest of that
#                        default - #[align=right][#{copy_position}/#{copy_
#                        position_limit}] - so the indicator still draws and
#                        carries no clock.
#   the inner shell      ENV= PS1='$ ' exec /bin/sh: no rc file, and a prompt
#                        that carries no host, user, path or clock.
# Nothing else is masked. Anything not in that list is compared.
#
# SETTLED CHECKPOINTS, AND WHY THIS FIXTURE MARKS BEFORE IT ACTS. tui-screen-
# diff.sh ends every checkpoint by typing `printf 'MARK-%s\n' NAME` into the
# pane. Three of the five surfaces here SWALLOW that keystroke: a pane in copy
# mode feeds it to the mode's key table, an armed prefix eats it, and an open
# command prompt puts it in the prompt. So each case here marks FIRST, waits for
# the marker to settle on both screens, and only then applies the state. The
# marker stays on the screen underneath every one of these surfaces, so the
# settle after the state is the same bounded wait: the marker present AND the
# screen unchanged between two polls.
#
# AND EVERY STATE HAS ITS OWN OBSERVABLE, waited for before the settle, so no
# comparison can be taken while one side has the state and the other does not:
#   copy and view       #{pane_in_mode} against the pane
#   prefix              #{client_prefix} against the client, which is why the
#                       prefix is pressed as a real key into the outer pane and
#                       not injected with send-keys: the prefix is a CLIENT key
#                       table state and send-keys writes to the pane, under it.
#   message and prompt  the text itself, on the last row of both screens
#
# MODES. `same` asserts the whole decoded screen and the cursor. `text` asserts
# every glyph, every column and the cursor and records only the styles, for a
# surface whose colours belong to a decision this lane does not own. `record`
# asserts nothing and has to say why. A recorded case still prints both sides,
# so the lane that closes it has the bytes without re-deriving them.
#
# --self-check runs the driver against a deliberate one-sided difference in each
# channel - an extra hint cell on the status row, a message whose text differs,
# a prompt that one side leaves open as residue - and requires the comparison to
# report each one, plus one equivalence it must NOT report. A fixture that only
# passes has proved nothing.
#
# A divergence is a finding, not a failure of this script: it exits 1 so a
# caller can gate on it, and prints both sides so the next lane has the
# measurement.
set -eEuo pipefail

usage() {
  printf 'usage: compat/tui-indicators.sh [--self-check] [ZZ_BIN [TMUX_BIN]]\n' >&2
  printf '       ZZ_BIN=path TMUX_BIN=path compat/tui-indicators.sh\n' >&2
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

COLUMNS_UNDER_TEST=80
ROWS_UNDER_TEST=24
SCRATCH_DIR="$(mktemp -d /tmp/zzin.XXXXXX)"
TOKEN="${SCRATCH_DIR##*.}"
OUTER_SOCKET_NAME="zzino-$TOKEN"
INNER_SOCKET_NAME="zzini-$TOKEN"
ZZ_SOCKET="/tmp/zzin-$TOKEN.sock"
OUTER_SESSION="driver"
INNER_SESSION="ind"
WINDOW_NAME="win"
PANE_TITLE="indtitle"
ZZ_HOME="$SCRATCH_DIR/zz-home"
TMUX_HOME="$SCRATCH_DIR/tmux-home"
OUTER_HOME="$SCRATCH_DIR/outer-home"
ZZ_LOG_DIR="$SCRATCH_DIR/zz-logs"
CASE_LABEL=""
ZZ_PID=""
FAILURES=0
CHECKS=0
RECORDS=0
LAST_ROWS_DIFFERED=0
LAST_CURSOR_DIFFERED=0
MESSAGE_HOLD_MS=20000
MESSAGE_EXPIRE_MS=100
COPY_POSITION_FORMAT='#[align=right][#{copy_position}/#{copy_position_limit}]'
INNER_SHELL="ENV= PS1='\$ ' exec /bin/sh"
mkdir -p "$ZZ_HOME" "$TMUX_HOME" "$OUTER_HOME" "$ZZ_LOG_DIR"

scrubbed() {
  env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL \
    -u XDG_STATE_HOME -u ZZ_LOG_DIR \
    TMUX_TMPDIR=/tmp "$@"
}
tmux_outer_command() {
  scrubbed HOME="$OUTER_HOME" XDG_CONFIG_HOME="$OUTER_HOME/config" \
    "$TMUX_BIN" -L "$OUTER_SOCKET_NAME" "$@"
}
zz_command() {
  scrubbed HOME="$ZZ_HOME" XDG_CONFIG_HOME="$ZZ_HOME/config" ZZ_LOG_DIR="$ZZ_LOG_DIR" \
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

# A wait that runs out has to say what it was waiting for and what both screens
# held at that moment, or the next run starts from nothing.
dump_state() {
  local label="$1"
  local side
  printf 'wait that ran out: %s (case %s)\n' "$label" "${CASE_LABEL:-none yet}" >&2
  for side in zz tmux; do
    printf -- '--- %s screen ---\n' "$side" >&2
    tmux_outer_command capture-pane -p -t "=$OUTER_SESSION:$side" 2>&1 | cat -v >&2 || true
    printf -- '--- %s clients ---\n' "$side" >&2
    side_command "$side" list-clients -F '#{client_name} #{client_prefix} #{client_width}x#{client_height}' >&2 2>&1 || true
    printf -- '--- %s panes ---\n' "$side" >&2
    side_command "$side" list-panes -t "=$INNER_SESSION" -F '#{pane_id} in_mode=#{pane_in_mode} mode=#{pane_mode}' >&2 2>&1 || true
  done
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
  dump_state "$label"
  die "$label did not happen within 10 seconds"
}

CURSOR_FORMAT='#{cursor_x},#{cursor_y} flag=#{cursor_flag} pane=#{pane_width}x#{pane_height} shape=#{cursor_shape} blinking=#{cursor_blinking} very_visible=#{cursor_very_visible} colour=#{cursor_colour}'

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
last_row() {
  capture_plain "$1" | tail -n 1
}

outer_pane_is() {
  [ "$(tmux_outer_command display-message -p -t "$1" '#{pane_width}x#{pane_height}' 2>/dev/null)" = "$2" ]
}
client_attached() {
  [ "$(side_command "$1" list-clients -F '#{client_session}' 2>/dev/null)" = "$INNER_SESSION" ]
}
client_name() {
  side_command "$1" list-clients -F '#{client_name}' 2>/dev/null | head -n 1
}
active_pane() {
  side_command "$1" list-panes -t "=$INNER_SESSION" -F '#{pane_active} #{pane_id}' |
    awk '$1 == 1 { print $2; exit }'
}

write_attach() {
  local side="$1"
  local destination="$2"
  printf '#!/usr/bin/env bash\n' >"$destination"
  if [ "$side" = zz ]; then
    printf 'exec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL -u XDG_STATE_HOME HOME=%q XDG_CONFIG_HOME=%q ZZ_LOG_DIR=%q TMUX_TMPDIR=/tmp %q --socket %q attach-session -t %q\n' \
      "$ZZ_HOME" "$ZZ_HOME/config" "$ZZ_LOG_DIR" "$ZZ_BIN" "$ZZ_SOCKET" "=$INNER_SESSION" >>"$destination"
  else
    printf 'exec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL -u XDG_STATE_HOME HOME=%q XDG_CONFIG_HOME=%q TMUX_TMPDIR=/tmp %q -L %q attach-session -t %q\n' \
      "$TMUX_HOME" "$TMUX_HOME/config" "$TMUX_BIN" "$INNER_SOCKET_NAME" "=$INNER_SESSION" >>"$destination"
  fi
  chmod +x "$destination"
}

set_on_both() {
  side_command zz set-option -g "$1" "$2" || die "zz refused set-option -g $1"
  side_command tmux set-option -g "$1" "$2" || die "tmux refused set-option -g $1"
}
run_on_both() {
  side_command zz "$@" || die "zz refused $1"
  side_command tmux "$@" || die "tmux refused $1"
}
# The pane id is substituted where the caller wrote PANE, never appended: tmux's
# argument parser stops at the first positional, so `run-shell 'cmd' -t %0` would
# hand the -t to the shell.
on_both_active() {
  local side pane argument
  local -a arguments
  for side in zz tmux; do
    pane="$(active_pane "$side")"
    [ -n "$pane" ] || die "$side has no active pane"
    arguments=()
    for argument in "$@"; do
      if [ "$argument" = PANE ]; then
        arguments+=("$pane")
      else
        arguments+=("$argument")
      fi
    done
    side_command "$side" "${arguments[@]}" || die "$side refused $1"
  done
}
send_both() {
  local side pane
  for side in zz tmux; do
    pane="$(active_pane "$side")"
    [ -n "$pane" ] || die "$side has no active pane"
    side_command "$side" send-keys -t "$pane" "$1" Enter || die "$side refused send-keys"
  done
}
# A real key into the outer pane, which the attached inner client reads as a
# keystroke. This is the only way to reach a CLIENT key table: send-keys writes
# into the pane, underneath it.
type_on_both() {
  local side
  for side in zz tmux; do
    tmux_outer_command send-keys -t "=$OUTER_SESSION:$side" "$@" ||
      die "the outer tmux refused send-keys for $side"
  done
}

attach_both_at() {
  local columns="$1"
  local rows="$2"
  COLUMNS_UNDER_TEST="$columns"
  ROWS_UNDER_TEST="$rows"
  tmux_outer_command kill-server >/dev/null 2>&1 || true
  zz_command kill-session -t "=$INNER_SESSION" >/dev/null 2>&1 || true
  tmux_inner_command kill-session -t "=$INNER_SESSION" >/dev/null 2>&1 || true
  zz_command new-session -d -s "$INNER_SESSION" -n "$WINDOW_NAME" -x "$columns" -y "$rows" \
    "$INNER_SHELL" || die "could not create the zz session"
  tmux_inner_command -f /dev/null new-session -d -s "$INNER_SESSION" -n "$WINDOW_NAME" \
    -x "$columns" -y "$rows" "$INNER_SHELL" || die "could not create the tmux session"
  set_on_both status-right ''
  set_on_both status-left L
  set_on_both automatic-rename off
  set_on_both display-time "$MESSAGE_HOLD_MS"
  set_on_both copy-mode-position-format "$COPY_POSITION_FORMAT"
  tmux_outer_command -f /dev/null new-session -d -s "$OUTER_SESSION" -n zz \
    -x "$columns" -y "$rows" "$SCRATCH_DIR/attach-zz.sh" ||
    die "could not create the outer session"
  tmux_outer_command set-option -g status off
  tmux_outer_command new-window -d -n tmux "$SCRATCH_DIR/attach-tmux.sh"
  wait_for "outer zz pane at ${columns}x${rows}" outer_pane_is "=$OUTER_SESSION:zz" "${columns}x${rows}"
  wait_for "outer tmux pane at ${columns}x${rows}" outer_pane_is "=$OUTER_SESSION:tmux" "${columns}x${rows}"
  wait_for "zz client attached" client_attached zz
  wait_for "tmux client attached" client_attached tmux
  run_on_both rename-window -t "=$INNER_SESSION:0" "$WINDOW_NAME"
  run_on_both select-pane -t "=$INNER_SESSION:0.0" -T "$PANE_TITLE"
}

screen_has_marker() {
  capture_plain "$1" | grep -Fq "$2"
}
# THE SETTLE. The marker alone is not enough: the shell writes a marker and its
# next prompt as two separate writes, and a capture can land between them. A
# checkpoint is settled when the marker is on the screen AND the screen has not
# changed between two polls. Both halves are observable and the whole wait is
# bounded. No wait in this file is a sleep.
wait_settled() {
  local side="$1"
  local marker="$2"
  local label="$3"
  local previous=""
  local current attempt
  for ((attempt = 0; attempt < 200; attempt++)); do
    current="$(capture_plain "$side" 2>/dev/null || true)"
    if [ -n "$previous" ] && [ "$current" = "$previous" ] &&
      printf '%s' "$current" | grep -Fq "$marker"; then
      return 0
    fi
    previous="$current"
    sleep 0.05
  done
  dump_state "$label"
  die "$label did not settle within 10 seconds"
}
settle_both() {
  wait_settled zz "$1" "$2 settled on the zz screen"
  wait_settled tmux "$1" "$2 settled on the tmux screen"
}

# The marker this case's comparisons settle on, typed into the pane BEFORE the
# state under test is applied. Everything after this in the case leaves it on
# the screen.
mark_both() {
  local name="$1"
  send_both "printf 'MARK-%s\\n' $name"
  settle_both "MARK-$name" "$name"
}

compare_rows() {
  local name="$1"
  local styled="$2"
  local zz_rows tmux_rows zz_cursor tmux_cursor index differing total
  if [ "$styled" = styled ]; then
    mapfile -t zz_rows < <(capture_screen zz)
    mapfile -t tmux_rows < <(capture_screen tmux)
  else
    mapfile -t zz_rows < <(capture_plain zz)
    mapfile -t tmux_rows < <(capture_plain tmux)
  fi
  zz_cursor="$(cursor_tuple zz)"
  tmux_cursor="$(cursor_tuple tmux)"
  total="$ROWS_UNDER_TEST"
  differing=-1
  for ((index = 0; index < total; index++)); do
    if [ "${zz_rows[index]-}" != "${tmux_rows[index]-}" ]; then
      differing="$index"
      break
    fi
  done
  LAST_ROWS_DIFFERED=0
  LAST_CURSOR_DIFFERED=0
  [ "$differing" -lt 0 ] || LAST_ROWS_DIFFERED=1
  [ "$zz_cursor" = "$tmux_cursor" ] || LAST_CURSOR_DIFFERED=1
  if [ "$LAST_ROWS_DIFFERED" -eq 0 ] && [ "$LAST_CURSOR_DIFFERED" -eq 0 ]; then
    return 0
  fi
  printf '      case %s\n' "$name"
  if [ "$LAST_ROWS_DIFFERED" -eq 1 ]; then
    printf '      first differing row %s of %s\n' "$differing" "$total"
    printf '        tmux: %s\n' "$(printf '%s' "${tmux_rows[differing]-}" | cat -v)"
    printf '        zz:   %s\n' "$(printf '%s' "${zz_rows[differing]-}" | cat -v)"
  else
    printf '      all %s rows identical\n' "$total"
  fi
  printf '      cursor tmux: %s\n' "$tmux_cursor"
  printf '      cursor zz:   %s\n' "$zz_cursor"
  return 1
}

# The named comparison. `same` asserts the whole decoded screen and the cursor.
# `text` asserts every glyph, every column and the cursor and records only the
# styles. `record` asserts nothing and has to say why.
verdict() {
  local name="$1"
  local mode="$2"
  local reason="${3:-}"
  if [ "$mode" = text ]; then
    CHECKS=$((CHECKS + 1))
    RECORDS=$((RECORDS + 1))
    [ -n "$reason" ] || die "recorded style at $name says nothing about why"
    if compare_rows "$name" plain; then
      printf 'ok    %s: every glyph, column and the cursor identical\n' "$name"
    else
      FAILURES=$((FAILURES + 1))
      printf 'DIFF  %s\n' "$name"
    fi
    if compare_rows "$name" styled; then
      printf 'note  %s: styles identical too, the record can close\n' "$name"
    else
      printf 'note  %s: styles recorded, not asserted: %s\n' "$name" "$reason"
    fi
    return 0
  fi
  if [ "$mode" = same ]; then
    CHECKS=$((CHECKS + 1))
  else
    RECORDS=$((RECORDS + 1))
  fi
  if compare_rows "$name" styled; then
    printf 'ok    %s\n' "$name"
    return 0
  fi
  if [ "$mode" = same ]; then
    FAILURES=$((FAILURES + 1))
    printf 'DIFF  %s\n' "$name"
  else
    [ -n "$reason" ] || die "recorded case $name says nothing about why"
    printf 'note  %s recorded, not asserted: %s\n' "$name" "$reason"
  fi
  return 0
}

pane_in_mode_is() {
  [ "$(side_command "$1" display-message -p -t "$(active_pane "$1")" '#{pane_in_mode}' 2>/dev/null)" = "$2" ]
}
client_prefix_is() {
  [ "$(side_command "$1" display-message -c "$(client_name "$1")" -p '#{client_prefix}' 2>/dev/null)" = "$2" ]
}
last_row_has() {
  case "$(last_row "$1")" in
  *"$2"*) return 0 ;;
  *) return 1 ;;
  esac
}
last_row_starts_with() {
  case "$(last_row "$1")" in
  "$2"*) return 0 ;;
  *) return 1 ;;
  esac
}
screen_has() {
  capture_plain "$1" | grep -Fq "$2"
}
last_row_lacks() {
  last_row_has "$1" "$2" && return 1
  return 0
}
both_pane_in_mode() {
  wait_for "$2 in mode on the zz pane" pane_in_mode_is zz "$1"
  wait_for "$2 in mode on the tmux pane" pane_in_mode_is tmux "$1"
}
both_client_prefix() {
  wait_for "$2 prefix on the zz client" client_prefix_is zz "$1"
  wait_for "$2 prefix on the tmux client" client_prefix_is tmux "$1"
}
both_last_row_has() {
  wait_for "$2 on the zz status row" last_row_has zz "$1"
  wait_for "$2 on the tmux status row" last_row_has tmux "$1"
}
both_last_row_lacks() {
  wait_for "$2 gone from the zz status row" last_row_lacks zz "$1"
  wait_for "$2 gone from the tmux status row" last_row_lacks tmux "$1"
}
both_last_row_starts_with() {
  wait_for "$2 on the zz status row" last_row_starts_with zz "$1"
  wait_for "$2 on the tmux status row" last_row_starts_with tmux "$1"
}
both_screen_has() {
  wait_for "$2 on the zz screen" screen_has zz "$1"
  wait_for "$2 on the tmux screen" screen_has tmux "$1"
}

seed_scrollback() {
  send_both 'seq 1 60'
}

# --- the cases -------------------------------------------------------------
#
# COPY MODE. window-copy.c:5228 draws copy-mode-position-format at the top right
# of the pane's own grid whenever the mode screen's first line is written and
# hide_position is off, so the pin's indicator is IN THE PANE and the status row
# is untouched. Measured 2026-09-10 on this pair: the pin paints
# `[0/39]` at columns 74..79 of row 0 in copy-mode-position-style, themeblack on
# themeyellow, and leaves the status row alone; the raw TUI paints nothing in
# the pane and ` COPY 62/62 ` at the right of the STATUS ROW instead, and
# #{copy_position} and #{copy_position_limit} answer empty on zz where the pin
# answers 0 and 39. Recorded against options.native-mode-styles, the accepted
# gap that holds option:copy-mode-position-format: zz renders native mode
# surfaces instead of tmux cell grids, so neither the indicator's cells nor its
# numbers are this fixture's to assert. What IS asserted is the restoration:
# once the mode is cancelled both screens have to be the same screen again.
COPY_REASON='options.native-mode-styles, accepted: the pin draws copy-mode-position-format into the pane grid at the top right and zz draws a native COPY badge on the status row instead'
copy_mode_case() {
  CASE_LABEL=copy-mode
  mark_both copy
  on_both_active copy-mode -t PANE
  both_pane_in_mode 1 'copy mode'
  settle_both MARK-copy 'copy mode'
  verdict copy-mode-entered record "$COPY_REASON"
  on_both_active send-keys -X -t PANE cancel
  both_pane_in_mode 0 'copy mode left'
  settle_both MARK-copy 'copy mode left'
  verdict copy-mode-restored same
}

# THE VIEW SURFACE, measured before it was written. The pin has no `view-mode`
# command: window_view_mode (window-copy.c:184) is reached from cfg.c:271 for a
# config error, from cmd-run-shell.c:103 for the output of run-shell, and from
# server-client.c:3061; `copy-mode -e` is not a second surface at all, it is
# copy mode with window-copy.c:616 scroll_exit set, and both binaries answer
# #{pane_mode} copy-mode for it. So the view surface the pin actually reaches is
# run-shell's, and that is what this case drives.
#
# MEASURED 2026-09-10: run-shell -t pane 'printf VIEWLINE-1' puts the pin's pane
# in view-mode with the output as the pane's whole screen and `[0/0]` at the top
# right; zz leaves #{pane_in_mode} at 0 and draws a client-side command-output
# overlay - a ` command output ` rule across row 0 - with a ` VIEW 1/21 ` badge
# on the status row. Same accepted gap, same reason. The restoration is
# asserted: Escape into each client has to bring both screens back.
VIEW_REASON='options.native-mode-styles, accepted: run-shell output reaches the pin as a view-mode pane grid and zz as a native command-output overlay'
view_surface_case() {
  CASE_LABEL=view-surface
  mark_both view
  on_both_active run-shell -t PANE 'printf VIEWLINE-1'
  both_screen_has VIEWLINE-1 'the run-shell output'
  wait_for 'the pin entered view mode' pane_in_mode_is tmux 1
  # The view surface REPLACES the screen on both sides, so the marker typed
  # before it is no longer on either screen. The output itself is what both
  # sides show, so it is what this checkpoint settles on.
  settle_both VIEWLINE-1 'the view surface'
  verdict view-surface-shown record "$VIEW_REASON"
  type_on_both Escape
  wait_for 'the pin left view mode' pane_in_mode_is tmux 0
  settle_both MARK-view 'the view surface withdrawn'
  verdict view-surface-restored same
}

# THE PREFIX. The pin paints NOTHING when the prefix is armed: status.c redraws
# the row from the same formats it always uses and no default format reads
# #{client_prefix}. Measured 2026-09-10 at the tip this fixture was written
# against, the raw TUI painted ` PREFIX ` over the right of the status row, so
# the two screens differed in six cells for as long as a user held C-b. That is
# the divergence this case exists to catch and it is not waivable: the cells are
# on the screen the contract compares. crates/zz-tui/src/render.rs now keeps the
# hint to the sidebar, which is client-local chrome the pin has no counterpart
# for and which only focus-sidebar shows, and the status row overlay carries the
# mode badge alone.
prefix_case() {
  CASE_LABEL=prefix
  mark_both prefix
  type_on_both C-b
  both_client_prefix 1 'the armed'
  settle_both MARK-prefix 'the armed prefix'
  verdict prefix-armed same
  type_on_both C-g
  both_client_prefix 0 'the released'
  settle_both MARK-prefix 'the released prefix'
  verdict prefix-released same
}

# DISPLAY-MESSAGE. status.c status_message_set puts the message on the status
# row and arms a display-time timer; when it fires the row is redrawn from its
# formats. Both halves are compared: the row while the message is up, and the
# row once it has expired.
#
# MEASURED 2026-09-10: the TEXT is the pin's on both sides and so is the cursor,
# and the STYLE is not - the pin resolves message-style, bg=themeyellow,
# fg=themeblack (options-table.c:941), and paints \e[38;2;13;13;13m
# \e[48;2;184;134;11m; the raw TUI paints its own overlay appearance,
# \e[38;2;16;19;24m\e[48;2;216;222;233m, and never reads message-style at all.
# That is the same class as the recorded presentation:tui-status-row-theme-
# defaults decision and it is a finding of this fixture, recorded here with the
# field it would need: the daemon publishes no resolved message style on
# StatusLine, so the client has nothing to dispatch on. The glyphs, the columns
# and the cursor are asserted.
MESSAGE_REASON='the pin resolves message-style (bg=themeyellow,fg=themeblack) and the raw TUI paints its own overlay appearance; the daemon publishes no resolved message style on StatusLine'
message_case() {
  CASE_LABEL=display-message
  mark_both message
  set_on_both display-time "$MESSAGE_HOLD_MS"
  side_command zz display-message -c "$(client_name zz)" 'INDICATOR-MESSAGE' ||
    die 'zz refused display-message'
  side_command tmux display-message -c "$(client_name tmux)" 'INDICATOR-MESSAGE' ||
    die 'tmux refused display-message'
  both_last_row_has INDICATOR-MESSAGE 'the message'
  settle_both MARK-message 'the message'
  verdict message-shown text "$MESSAGE_REASON"
  set_on_both display-time "$MESSAGE_EXPIRE_MS"
  side_command zz display-message -c "$(client_name zz)" 'INDICATOR-EXPIRES' ||
    die 'zz refused display-message'
  side_command tmux display-message -c "$(client_name tmux)" 'INDICATOR-EXPIRES' ||
    die 'tmux refused display-message'
  both_last_row_has INDICATOR-EXPIRES 'the second message'
  both_last_row_lacks INDICATOR-EXPIRES 'the expired message'
  settle_both MARK-message 'the expired message'
  verdict message-restored same
  set_on_both display-time "$MESSAGE_HOLD_MS"
}

# THE COMMAND PROMPT. Opened through the client's own keys - prefix then `:`,
# which is bind-key -T prefix : command-prompt on both sides - because
# `command-prompt -t <client>` from a one-shot client does not return until the
# prompt is answered, so a fixture that drove it that way would deadlock. That
# was measured, not assumed.
#
# MEASURED 2026-09-10: the prompt row's GLYPHS and the cursor are the pin's on
# both sides - `:` alone, then `:list`, with the cursor at column 1 and then
# column 5 of the last row - and the STYLE is not, for the same reason the
# message row's is not: the pin resolves message-style and message-command-style
# (options-table.c:910 and :941) and the raw TUI paints its own overlay
# appearance. One finding, two rows; recorded here, asserted in glyphs, columns
# and cursor. The CANCELLED prompt is asserted whole: once the prompt is gone
# the row is the pin's again, style included.
PROMPT_REASON='the pin resolves message-style and message-command-style for the prompt row and the raw TUI paints its own overlay appearance; the daemon publishes no resolved message style on StatusLine'
prompt_case() {
  CASE_LABEL=command-prompt
  mark_both prompt
  type_on_both C-b
  both_client_prefix 1 'the armed'
  type_on_both ':'
  both_last_row_starts_with ':' 'the command prompt'
  settle_both MARK-prompt 'the command prompt'
  verdict prompt-opened text "$PROMPT_REASON"
  type_on_both l i s t
  both_last_row_has ':list' 'the typed command'
  settle_both MARK-prompt 'the typed command'
  verdict prompt-typed text "$PROMPT_REASON"
  type_on_both Escape
  both_last_row_lacks ':list' 'the cancelled prompt'
  settle_both MARK-prompt 'the cancelled prompt'
  verdict prompt-cancelled same
}

write_attach zz "$SCRATCH_DIR/attach-zz.sh"
write_attach tmux "$SCRATCH_DIR/attach-tmux.sh"

zz_command -f /dev/null daemon >"$SCRATCH_DIR/zz-daemon.out" 2>"$SCRATCH_DIR/zz-daemon.err" &
ZZ_PID=$!
wait_for "zz daemon socket" test -S "$ZZ_SOCKET"

run_cases() {
  printf 'indicator, message and prompt differential at %sx%s (pin %s)\n' \
    "$COLUMNS_UNDER_TEST" "$ROWS_UNDER_TEST" "$(basename -- "$TMUX_BIN")"
  attach_both_at 80 24
  CASE_LABEL=baseline
  mark_both baseline
  verdict baseline same
  seed_scrollback
  CASE_LABEL=scrollback
  mark_both scrollback
  verdict scrollback same
  copy_mode_case
  view_surface_case
  prefix_case
  message_case
  prompt_case

  if [ "$FAILURES" -ne 0 ]; then
    printf '%s of %s asserted comparisons differ, %s recorded\n' "$FAILURES" "$CHECKS" "$RECORDS"
    exit 1
  fi
  printf 'all %s asserted comparisons identical, %s recorded not asserted\n' "$CHECKS" "$RECORDS"
}

# --- self-check ------------------------------------------------------------
#
# One deliberate one-sided difference per channel, plus one equivalence that
# must not be reported. Each case runs the same driver against a fresh pair.
SELF_CHECK_FAILURES=0

self_check_case() {
  local name="$1"
  local expectation="$2"
  local outcome=ok
  case "$expectation" in
  rows)
    [ "$LAST_ROWS_DIFFERED" -eq 1 ] || outcome='no row difference reported'
    ;;
  none)
    if [ "$LAST_ROWS_DIFFERED" -ne 0 ] || [ "$LAST_CURSOR_DIFFERED" -ne 0 ]; then
      outcome='reported a difference the pin collapses'
    fi
    ;;
  esac
  if [ "$outcome" = ok ]; then
    printf 'ok    self-check %s\n' "$name"
    return 0
  fi
  SELF_CHECK_FAILURES=$((SELF_CHECK_FAILURES + 1))
  printf 'FAIL  self-check %s: %s\n' "$name" "$outcome"
}

run_self_check() {
  printf 'self-check: one deliberate difference per channel, plus one the pin collapses\n'

  # A hint cell that comes back. The prefix case's whole claim is that no cell
  # of the status row may carry chrome the pin does not draw, so the sabotage
  # is exactly that: one side's status row carries a PREFIX label while the
  # prefix is armed on both.
  CASE_LABEL='self-check prefix hint'
  attach_both_at 80 24
  mark_both hint
  side_command zz set-option -g status-right ' PREFIX ' || die 'zz refused status-right'
  type_on_both C-b
  both_client_prefix 1 'the armed'
  settle_both MARK-hint 'the armed prefix with a hint cell'
  compare_rows self-check-prefix-hint styled || true
  self_check_case 'prefix, an extra hint cell on one status row' rows
  type_on_both C-g

  # A message whose text differs by one character. The message channel has to
  # report it in the rows, and the `text` mode this fixture uses for the shown
  # message reads the plain capture, so the sabotage is planted in the text.
  CASE_LABEL='self-check message text'
  attach_both_at 80 24
  mark_both msg
  set_on_both display-time "$MESSAGE_HOLD_MS"
  side_command zz display-message -c "$(client_name zz)" 'SABOTAGE-MSG-A' ||
    die 'zz refused display-message'
  side_command tmux display-message -c "$(client_name tmux)" 'SABOTAGE-MSG-B' ||
    die 'tmux refused display-message'
  both_last_row_has SABOTAGE-MSG 'the sabotaged message'
  settle_both MARK-msg 'the sabotaged message'
  compare_rows self-check-message plain || true
  self_check_case 'message, one character of the text differs' rows

  # A prompt that one side leaves open. The restoration comparison is what the
  # prompt case asserts, so the sabotage is a prompt cancelled on one side only.
  CASE_LABEL='self-check prompt residue'
  attach_both_at 80 24
  mark_both residue
  type_on_both C-b
  both_client_prefix 1 'the armed'
  type_on_both ':'
  both_last_row_starts_with ':' 'the command prompt'
  settle_both MARK-residue 'the command prompt'
  tmux_outer_command send-keys -t "=$OUTER_SESSION:tmux" Escape ||
    die 'the outer tmux refused send-keys'
  wait_for 'the pin cancelled its prompt' last_row_starts_with tmux L
  settle_both MARK-residue 'the one-sided cancel'
  compare_rows self-check-prompt-residue styled || true
  self_check_case 'prompt, one side leaves the prompt open as residue' rows

  # The equivalence: the same prefix, armed and released on both sides with
  # nothing planted, must report nothing at all. Without it the three sabotages
  # above would be satisfied by a comparison that always reports a difference.
  CASE_LABEL='self-check equivalence'
  attach_both_at 80 24
  mark_both equal
  type_on_both C-b
  both_client_prefix 1 'the armed'
  settle_both MARK-equal 'the armed prefix on both sides'
  compare_rows self-check-equivalence styled || true
  self_check_case 'equivalence: the same armed prefix on both sides' none
  type_on_both C-g

  if [ "$SELF_CHECK_FAILURES" -ne 0 ]; then
    printf '%s self-check expectations unmet\n' "$SELF_CHECK_FAILURES"
    exit 1
  fi
  printf 'self-check complete: every sabotage was caught and the equivalence passed\n'
}

if [ "$SELF_CHECK" -eq 1 ]; then
  run_self_check
else
  run_cases
fi
