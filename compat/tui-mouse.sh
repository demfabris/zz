#!/usr/bin/env bash
# Pointer, paste and focus differential for the raw TUI.
#
# Every other TUI fixture drives KEYS. Nothing in the campaign ever drove a
# POINTER at an attached client and compared what the two binaries did with
# it, and pointer handling is the largest single thing the tmux-compat
# campaign accepted as native (keys.root-native-mouse, 27 items,
# keys.copy-mode-native-mouse, 14, mouse.bound-context, 5, and
# formats.mouse-context, 8). Under the 2026-09-10 triage the raw TUI renders
# the pin's cells and runs the pin's commands, so those accepted decisions no
# longer cover it and each of their items is a measurement waiting to be
# taken. This fixture takes them. It is TUI-008's proof surface.
#
# THE OUTER PINNED TMUX IS THE DECODER AND THE INJECTOR. Both binaries attach
# inside ONE outer pinned tmux, one window each. Reading is
# `capture-pane -p -e` on the outer pane, so attribute order, batching and
# cursor-movement spelling collapse before anything is compared and the colour
# class does not. Writing is `send-keys -H` on the outer pane, which puts raw
# bytes on the inner client's stdin exactly as a terminal would: every gesture
# here is a REAL SGR mouse report (`\e[<b;col;rowM`, `\e[<b;col;rowm`, the
# 1006 encoding both clients ask for), a real bracketed paste
# (`\e[200~ ... \e[201~`) or a real focus report (`\e[I`, `\e[O`). No gesture
# is faked through a command, and no client is asked to pretend.
#
# WHERE A GESTURE IS AIMED. Never at a hard-coded cell. Each case reads
# `#{pane_left} #{pane_top} #{pane_right} #{pane_bottom}` and the status line's
# own row from the SIDE it is aiming at, and turns that into the 1-based
# screen column and row the SGR report carries. compat/tui-pane-geometry.sh
# already asserts that those numbers agree between the two binaries, so a case
# that aims at pane 1's third cell aims at the same cell on both.
#
# THE CHANNEL OF EVERY CASE. A pointer gesture is only interesting for what it
# CHANGES, so every case names the observable it compares and compares nothing
# else:
#
#   case                        channel
#   -------------------------------------------------------------------------
#   click-selects-pane          the inner server's active pane index
#   click-user-binding          a global option a user's own
#                               `bind -n MouseDown1Pane` sets
#   click-user-binding-target   `#{mouse_x}`, `#{mouse_y}` and `#{mouse_pane}`
#                               as that binding's own command expands them
#   border-user-binding         a global option a user's own
#                               `bind -n MouseDown1Border` sets
#   status-user-binding         a global option a user's own
#                               `bind -n WheelDownStatus` sets
#   drag-selects                the paste buffer the drag leaves, plus
#                               `#{pane_in_mode}` and `#{selection_present}`
#                               once the button is up
#   wheel-up-pane               `#{pane_in_mode}` and `#{pane_mode}`
#   wheel-up-alternate          the same, with an alternate-screen program up
#   double-click-word           the paste buffer the gesture leaves
#   triple-click-line           the paste buffer the gesture leaves
#   right-click-pane            the decoded screen (a menu, or nothing)
#   border-click                `#{pane_marked_set}` and the active pane
#                               index
#   border-drag-resize          `#{pane_width}` of the pane left of the border
#   status-click-window         the current window index
#   status-wheel-up/down        the current window index
#   status-right-click          the decoded screen (a window menu, or nothing),
#                               for the plain and the alt-modified gesture
#   copy-mode-wheel             the copy cursor's line
#   copy-mode-drag              `#{selection_present}` and the copy cursor
#   copy-mode-double-click      the paste buffer
#   app-mouse-report            the SGR report a program in the pane received,
#                               read off the pane's own decoded screen
#   app-mouse-report-mouse-off  the same with `mouse off`
#   app-mouse-drag              the three reports a program under button-event
#                               tracking received from a press, a motion with
#                               the button held and the release that ends the
#                               drag, in order
#   app-mouse-double-click      the four reports the same program received from
#                               two clicks inside the click timeout, in order
#   paste-into-pane             the bracketed paste a program in the pane
#                               received, read off its decoded screen
#   paste-into-copy-mode        the decoded screen and `#{pane_in_mode}` while
#                               the mode is up, and the decoded screen again
#                               once it is left
#   paste-into-prompt           the command prompt's own row
#   paste-under-menu            the decoded screen and the menu's option
#   focus-in-out-on             the focus reports a program in the pane
#                               received, with `focus-events on`
#   focus-in-out-off            the same with `focus-events off`
#
# CONTROLLED DYNAMIC VALUES, set on both sides before the first case:
#   status-right ''      the default ends in a clock and in %d-%b-%y; that
#   status-left L        belongs to compat/status-row.sh, not here.
#   automatic-rename off plus a fixed window name: the default follows the
#                        running command and would race a gesture.
#   display-time         held at MESSAGE_HOLD_MS so a message a gesture raises
#                        cannot expire between the two sides' readings.
#   default-shell        /bin/sh, so a job runs under the same shell whatever
#                        the box's $SHELL is.
#   the inner shell      ENV= PS1='$ ' exec /bin/sh: no rc file, and a prompt
#                        with no host, user, path or clock.
#   mouse on             every case but the two that name `mouse off`.
#   the outer decoder    status off, and its own `mouse` left alone: nothing
#                        this fixture writes ever reaches the outer server's
#                        key handling, because `send-keys -H` writes bytes
#                        straight to the pane.
#
# THE ESCAPE THAT CLOSES A MENU IS NOT SYMMETRIC IN ITS EFFECT: on the side
# that has a menu up it closes the menu, and on a side that has none it
# reaches the pane's own program. Both menu cases therefore respawn the pane's
# shell once the comparison is taken, so the next case starts from the same
# pane on both sides whatever the escape did.
#
# A SETTLE MARKER IS READ OFF THE STYLED CAPTURE, so it is always a run of
# cells the case's own gesture cannot put a style boundary inside: the two
# cases that select text on the screen settle on the last word of the sample,
# which no gesture here covers.
#
# SETTLED READINGS. Every reading is taken after the case's marker has reached
# the side's screen AND that screen has stopped changing between two polls, and
# every state a gesture creates has its own bounded `wait_for` on an
# observable before that settle, so no comparison is taken while one side has
# the state and the other does not. No wait here is a sleep. The one place a
# bounded wait cannot be written is a gesture whose CORRECT result on one side
# is "nothing happened": those cases wait for the OTHER observable both sides
# do share (a settled screen after a marker that the gesture cannot swallow)
# and then read.
#
# RECORDED CASES. A recorded case prints both measured values and does not
# fail the run; every one says why in the driver, never at runtime. TUI-008 is
# not verified while any case here is recorded.
#
# --self-check drives the same driver with ONE deliberate one-sided difference
# per channel and requires the comparison to report it in the channel that was
# sabotaged, plus controls that sabotage nothing and must stay quiet. A fixture
# that only passes has proved nothing.
#
# A divergence is a finding: the script exits 1 so a caller can gate on it, and
# prints both sides so the next lane has the measurement. It reaps every server
# and daemon it starts.
set -eEuo pipefail
export ZZ_TRAY=0

usage() {
  printf 'usage: compat/tui-mouse.sh [--self-check] [ZZ_BIN [TMUX_BIN]]\n' >&2
  printf '       ZZ_BIN=path TMUX_BIN=path compat/tui-mouse.sh\n' >&2
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
SCRATCH_DIR="$(mktemp -d /tmp/zzms.XXXXXX)"
TOKEN="${SCRATCH_DIR##*.}"
OUTER_SOCKET_NAME="zzmso-$TOKEN"
INNER_SOCKET_NAME="zzmsi-$TOKEN"
ZZ_SOCKET="/tmp/zzms-$TOKEN.sock"
OUTER_SESSION="driver"
INNER_SESSION="mus"
WINDOW_NAME="win"
ZZ_HOME="$SCRATCH_DIR/zz-home"
TMUX_HOME="$SCRATCH_DIR/tmux-home"
OUTER_HOME="$SCRATCH_DIR/outer-home"
ZZ_LOG_DIR="$SCRATCH_DIR/zz-logs"
CASE_LABEL=""
FAILURES=0
CHECKS=0
RECORDS=0
LAST_DIFFERED=0
MESSAGE_HOLD_MS=20000
INNER_SHELL="ENV= PS1='\$ ' exec /bin/sh"
SABOTAGE=""
mkdir -p "$ZZ_HOME" "$TMUX_HOME" "$OUTER_HOME" "$ZZ_LOG_DIR"

scrubbed() {
  env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL \
    -u XDG_STATE_HOME -u ZZ_LOG_DIR -u XDG_RUNTIME_DIR \
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
  local pid
  trap - EXIT ERR INT TERM
  set +e
  tmux_outer_command kill-server >/dev/null 2>&1
  zz_command kill-server >/dev/null 2>&1
  tmux_inner_command kill-server >/dev/null 2>&1
  for pid in $(pgrep -f -- "$SCRATCH_DIR" 2>/dev/null); do
    kill "$pid" >/dev/null 2>&1
  done
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

dump_state() {
  local label="$1" side
  printf 'wait that ran out: %s (case %s)\n' "$label" "${CASE_LABEL:-none yet}" >&2
  for side in zz tmux; do
    printf -- '--- %s screen ---\n' "$side" >&2
    tmux_outer_command capture-pane -p -t "=$OUTER_SESSION:$side" 2>&1 | cat -v >&2 || true
    printf -- '--- %s panes ---\n' "$side" >&2
    side_command "$side" list-panes -t "=$INNER_SESSION" \
      -F '#{pane_id} active=#{pane_active} #{pane_left},#{pane_top} #{pane_width}x#{pane_height} mode=#{pane_in_mode}' >&2 2>&1 || true
  done
}

wait_for() {
  local label="$1" attempt
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

capture_screen() {
  tmux_outer_command capture-pane -p -e -S 0 -E "$((ROWS_UNDER_TEST - 1))" \
    -t "=$OUTER_SESSION:$1"
}
capture_plain() {
  tmux_outer_command capture-pane -p -S 0 -E "$((ROWS_UNDER_TEST - 1))" \
    -t "=$OUTER_SESSION:$1"
}
outer_pane_is() {
  [ "$(tmux_outer_command display-message -p -t "$1" '#{pane_width}x#{pane_height}' 2>/dev/null)" = "$2" ]
}
client_attached() {
  [ "$(side_command "$1" list-clients -F '#{client_session}' 2>/dev/null)" = "$INNER_SESSION" ]
}
screen_has() {
  capture_plain "$1" | grep -Fq -- "$2"
}
screen_lacks() {
  screen_has "$1" "$2" && return 1
  return 0
}
both_screen_has() {
  wait_for "$2 on the zz screen" screen_has zz "$1"
  wait_for "$2 on the tmux screen" screen_has tmux "$1"
}
both_screen_lacks() {
  wait_for "$2 gone from the zz screen" screen_lacks zz "$1"
  wait_for "$2 gone from the tmux screen" screen_lacks tmux "$1"
}
wait_settled() {
  local side="$1" marker="$2" label="$3" previous="" current attempt
  for ((attempt = 0; attempt < 200; attempt++)); do
    current="$(capture_screen "$side" 2>/dev/null || true)"
    if [ -n "$previous" ] && [ "$current" = "$previous" ] &&
      printf '%s' "$current" | grep -Fq -- "$marker"; then
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

# --- the injector ----------------------------------------------------------
#
# `send-keys -H` on the OUTER pane writes the bytes it is given straight to
# that pane's pty, which is the inner client's stdin. Nothing in the outer
# server's own key handling sees them, so the outer `mouse` option and the
# outer key tables cannot colour a result.
send_bytes() {
  local side="$1" text="$2" hex
  hex="$(printf '%s' "$text" | od -An -tx1 | tr -s ' \n' '  ')"
  # shellcheck disable=SC2086
  tmux_outer_command send-keys -t "=$OUTER_SESSION:$side" -H $hex ||
    die "the outer tmux refused send-keys -H for $side"
}
# An SGR 1006 mouse report: button code, 1-based column, 1-based row, and M
# for a press or m for a release.
send_mouse() {
  local side="$1" button="$2" column="$3" row="$4" kind="$5"
  send_bytes "$side" "$(printf '\033[<%s;%s;%s%s' "$button" "$column" "$row" "$kind")"
}
send_mouse_both() {
  send_mouse zz "$@"
  send_mouse tmux "$@"
}
# A press and its release at the same cell. The self-check can aim one side's
# click at a different column, which is the one-sided difference a pointer
# fixture most needs to be able to make.
CLICK_SABOTAGE_SIDE=""
CLICK_SABOTAGE_COLUMN=""
BORDER_SABOTAGE_COLUMN=""
COPY_PASTE_SABOTAGE_SIDE=""
click_both() {
  local button="$1" column="$2" row="$3" side aimed
  for side in zz tmux; do
    aimed="$column"
    [ "$side" = "$CLICK_SABOTAGE_SIDE" ] && aimed="$CLICK_SABOTAGE_COLUMN"
    send_mouse "$side" "$button" "$aimed" "$row" M
    send_mouse "$side" "$button" "$aimed" "$row" m
  done
}

# --- geometry --------------------------------------------------------------
#
# Where a pane sits, read from the side being aimed at. `pane_left` and
# `pane_top` are 0-based in the client's own screen; an SGR report is 1-based,
# so a gesture at the pane's own (dx, dy) is at (pane_left + 1 + dx,
# pane_top + 1 + dy).
pane_geometry() {
  side_command "$1" display-message -p -t "$2" \
    '#{pane_left} #{pane_top} #{pane_right} #{pane_bottom} #{pane_width} #{pane_height}'
}
pane_field() {
  pane_geometry "$1" "$2" | awk -v n="$3" '{ print $n }'
}
# The status row is the last row of the client's screen while status-position
# is bottom, which every case here leaves it at.
status_row() {
  printf '%s\n' "$ROWS_UNDER_TEST"
}
active_pane_index() {
  side_command "$1" list-panes -t "=$INNER_SESSION" -F '#{pane_active} #{pane_index}' 2>/dev/null |
    awk '$1 == 1 { print $2; exit }'
}
active_pane_index_is() {
  [ "$(active_pane_index "$1")" = "$2" ]
}
pane_in_mode() {
  side_command "$1" display-message -p -t "$2" '#{pane_in_mode}' 2>/dev/null
}
pane_in_mode_is() {
  [ "$(pane_in_mode "$1" "$2")" = "$3" ]
}

# --- the comparison --------------------------------------------------------
#
# Every case compares one named value between the two sides. `same` asserts;
# `record` prints both and says why.
assert_value() {
  compare_value same "$1" "$2" "$3"
}
# `check_value PREFIX name zz pin` reads PREFIX_MODE and PREFIX_REASON from
# the disposition block, so a case never decides at runtime whether it asserts.
check_value() {
  local prefix="$1" mode_name="${1}_MODE" reason_name="${1}_REASON"
  shift
  RECORD_REASON="${!reason_name}"
  compare_value "${!mode_name}" "$@"
  RECORD_REASON=""
}
compare_value() {
  local mode="$1" name="$2" zz_value="$3" pin_value="$4"
  if [ "$mode" = same ]; then
    CHECKS=$((CHECKS + 1))
  else
    RECORDS=$((RECORDS + 1))
  fi
  if [ "$zz_value" = "$pin_value" ]; then
    LAST_DIFFERED=0
    printf 'ok    %s: both %s\n' "$name" "$(printf '%s' "$zz_value" | cat -v)"
    return 0
  fi
  LAST_DIFFERED=1
  if [ "$mode" = same ]; then
    FAILURES=$((FAILURES + 1))
    printf 'DIFF  %s\n' "$name"
  else
    printf 'note  %s recorded, not asserted: %s\n' "$name" "$RECORD_REASON"
  fi
  printf '        tmux: %s\n' "$(printf '%s' "$pin_value" | cat -v)"
  printf '        zz:   %s\n' "$(printf '%s' "$zz_value" | cat -v)"
  return 0
}
RECORD_REASON=""
record_value() {
  RECORD_REASON="$2"
  compare_value record "$1" "$3" "$4"
  RECORD_REASON=""
}
# The whole decoded screen as one value, row index by row index.
assert_screen() {
  local name="$1" zz_rows tmux_rows index count differing
  mapfile -t zz_rows < <(capture_screen zz)
  mapfile -t tmux_rows < <(capture_screen tmux)
  CHECKS=$((CHECKS + 1))
  count=0
  differing=-1
  for ((index = 0; index < ROWS_UNDER_TEST; index++)); do
    if [ "${zz_rows[index]-}" != "${tmux_rows[index]-}" ]; then
      [ "$differing" -ge 0 ] || differing="$index"
      count=$((count + 1))
    fi
  done
  if [ "$count" -eq 0 ]; then
    LAST_DIFFERED=0
    printf 'ok    %s: all %s rows identical\n' "$name" "$ROWS_UNDER_TEST"
    return 0
  fi
  LAST_DIFFERED=1
  FAILURES=$((FAILURES + 1))
  printf 'DIFF  %s: %s of %s rows differ, first %s\n' \
    "$name" "$count" "$ROWS_UNDER_TEST" "$differing"
  for ((index = 0; index < ROWS_UNDER_TEST; index++)); do
    if [ "${zz_rows[index]-}" != "${tmux_rows[index]-}" ]; then
      printf '        row %s tmux: %s\n' "$index" "$(printf '%s' "${tmux_rows[index]-}" | cat -v)"
      printf '        row %s zz:   %s\n' "$index" "$(printf '%s' "${zz_rows[index]-}" | cat -v)"
    fi
  done
  return 0
}
check_screen() {
  local prefix="$1" mode_name="${1}_MODE" reason_name="${1}_REASON"
  if [ "${!mode_name}" = same ]; then
    assert_screen "$2"
  else
    record_screen "$2" "${!reason_name}"
  fi
}
record_screen() {
  local name="$1" reason="$2" zz_rows tmux_rows index count
  mapfile -t zz_rows < <(capture_screen zz)
  mapfile -t tmux_rows < <(capture_screen tmux)
  RECORDS=$((RECORDS + 1))
  count=0
  for ((index = 0; index < ROWS_UNDER_TEST; index++)); do
    [ "${zz_rows[index]-}" != "${tmux_rows[index]-}" ] && count=$((count + 1))
  done
  if [ "$count" -eq 0 ]; then
    LAST_DIFFERED=0
    printf 'ok    %s: all %s rows identical\n' "$name" "$ROWS_UNDER_TEST"
    return 0
  fi
  LAST_DIFFERED=1
  printf 'note  %s recorded, not asserted: %s\n' "$name" "$reason"
  for ((index = 0; index < ROWS_UNDER_TEST; index++)); do
    if [ "${zz_rows[index]-}" != "${tmux_rows[index]-}" ]; then
      printf '        row %s tmux: %s\n' "$index" "$(printf '%s' "${tmux_rows[index]-}" | cat -v)"
      printf '        row %s zz:   %s\n' "$index" "$(printf '%s' "${zz_rows[index]-}" | cat -v)"
    fi
  done
  return 0
}

# --- the session -----------------------------------------------------------

write_attach() {
  local side="$1" destination="$2"
  printf '#!/usr/bin/env bash\n' >"$destination"
  if [ "$side" = zz ]; then
    printf 'exec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL -u XDG_STATE_HOME -u XDG_RUNTIME_DIR HOME=%q XDG_CONFIG_HOME=%q ZZ_LOG_DIR=%q TMUX_TMPDIR=/tmp %q --socket %q attach-session -t %q\n' \
      "$ZZ_HOME" "$ZZ_HOME/config" "$ZZ_LOG_DIR" "$ZZ_BIN" "$ZZ_SOCKET" "=$INNER_SESSION" >>"$destination"
  else
    printf 'exec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL -u XDG_STATE_HOME -u XDG_RUNTIME_DIR HOME=%q XDG_CONFIG_HOME=%q TMUX_TMPDIR=/tmp %q -L %q attach-session -t %q\n' \
      "$TMUX_HOME" "$TMUX_HOME/config" "$TMUX_BIN" "$INNER_SOCKET_NAME" "=$INNER_SESSION" >>"$destination"
  fi
  chmod +x "$destination"
}
set_on_both() {
  side_command zz set-option -g "$1" "$2" >/dev/null || die "zz refused set-option -g $1"
  side_command tmux set-option -g "$1" "$2" >/dev/null || die "tmux refused set-option -g $1"
}
run_on_both() {
  side_command zz "$@" >/dev/null || die "zz refused $1"
  side_command tmux "$@" >/dev/null || die "tmux refused $1"
}
send_both() {
  local side pane
  for side in zz tmux; do
    pane="$(side_command "$side" list-panes -t "=$INNER_SESSION" -F '#{pane_active} #{pane_id}' |
      awk '$1 == 1 { print $2; exit }')"
    [ -n "$pane" ] || die "$side has no active pane"
    side_command "$side" send-keys -t "$pane" "$1" Enter >/dev/null || die "$side refused send-keys"
  done
}
send_to_pane_both() {
  local target="$1"
  shift
  run_on_both send-keys -t "$target" "$@"
}
mark_both() {
  local name="$1"
  send_both "printf '\\033[H\\033[2JMARK-%s\\n' $name"
  settle_both "MARK-$name" "$name"
}
option_on_both() {
  wait_for "$3 on zz" option_is zz "$1" "$2"
  wait_for "$3 on tmux" option_is tmux "$1" "$2"
}
option_is() {
  [ "$(side_command "$1" show-options -gqv "$2" 2>/dev/null)" = "$3" ]
}
option_value() {
  side_command "$1" show-options -gqv "$2" 2>/dev/null
}

start_both() {
  write_attach zz "$SCRATCH_DIR/attach-zz.sh"
  write_attach tmux "$SCRATCH_DIR/attach-tmux.sh"
  zz_command new-session -d -s "$INNER_SESSION" -n "$WINDOW_NAME" \
    -x "$COLUMNS_UNDER_TEST" -y "$ROWS_UNDER_TEST" "$INNER_SHELL" ||
    die "could not create the zz session"
  tmux_inner_command -f /dev/null new-session -d -s "$INNER_SESSION" -n "$WINDOW_NAME" \
    -x "$COLUMNS_UNDER_TEST" -y "$ROWS_UNDER_TEST" "$INNER_SHELL" ||
    die "could not create the tmux session"
  set_on_both status-right ''
  set_on_both status-left L
  set_on_both automatic-rename off
  set_on_both display-time "$MESSAGE_HOLD_MS"
  set_on_both default-shell /bin/sh
  set_on_both mouse on
  tmux_outer_command -f /dev/null new-session -d -s "$OUTER_SESSION" -n zz \
    -x "$COLUMNS_UNDER_TEST" -y "$ROWS_UNDER_TEST" "$SCRATCH_DIR/attach-zz.sh" ||
    die "could not create the outer session"
  tmux_outer_command set-option -g status off >/dev/null
  tmux_outer_command new-window -d -n tmux "$SCRATCH_DIR/attach-tmux.sh" >/dev/null
  wait_for 'outer zz pane' outer_pane_is "=$OUTER_SESSION:zz" "${COLUMNS_UNDER_TEST}x${ROWS_UNDER_TEST}"
  wait_for 'outer tmux pane' outer_pane_is "=$OUTER_SESSION:tmux" "${COLUMNS_UNDER_TEST}x${ROWS_UNDER_TEST}"
  wait_for 'zz client attached' client_attached zz
  wait_for 'tmux client attached' client_attached tmux
  run_on_both rename-window -t "=$INNER_SESSION:0" "$WINDOW_NAME"
}

# Two panes side by side, pane 0 active, for every case that needs a border or
# a second target.
split_both() {
  run_on_both split-window -h -t "=$INNER_SESSION:0.0" "$INNER_SHELL"
  run_on_both select-pane -t "=$INNER_SESSION:0.0"
  wait_for 'zz pane 0 active' active_pane_index_is zz 0
  wait_for 'tmux pane 0 active' active_pane_index_is tmux 0
}
unsplit_both() {
  run_on_both kill-pane -t "=$INNER_SESSION:0.1"
  wait_for 'zz back to one pane' pane_count_is zz 1
  wait_for 'tmux back to one pane' pane_count_is tmux 1
}
pane_count_is() {
  [ "$(side_command "$1" list-panes -t "=$INNER_SESSION" -F x 2>/dev/null | wc -l)" = "$2" ]
}
# A side that never entered copy mode has no mode to cancel, so this is the
# one place the two sides are driven separately and tolerantly. Both are then
# waited back to "not in a mode" before the next case.
leave_copy_mode_both() {
  local side
  for side in zz tmux; do
    side_command "$side" send-keys -X -t "=$INNER_SESSION:0.0" cancel >/dev/null 2>&1 || true
  done
  wait_for 'the pin out of copy mode' pane_in_mode_is tmux "=$INNER_SESSION:0.0" 0
  wait_for 'zz out of copy mode' pane_in_mode_is zz "=$INNER_SESSION:0.0" 0
}

# A pane running a program instead of the interactive shell, so a case can
# arm a mode the shell never arms and read back exactly what reached it. The
# line discipline is put in non-canonical mode with echo off first: a mouse
# report, a bracketed paste and a focus report all arrive without a newline,
# and a canonical read would hold them until one came.
respawn_program_both() {
  run_on_both respawn-pane -k -t "=$INNER_SESSION:0.0" "$@"
}
respawn_shell_both() {
  respawn_program_both sh -c "$INNER_SHELL"
  settle_both '$' 'the shell back in pane 0'
}
# What a program in pane 0 has printed, with the marker line and every blank
# line dropped, as one value.
program_output() {
  local side="$1" marker="$2"
  capture_plain "$side" | sed -n "/$marker/,\$p" | sed "1d" | sed '/^[[:space:]]*$/d' |
    head -n 4 | cat -v | tr '\n' '|'
}

# --- the cases -------------------------------------------------------------

# `bind -n MouseDown1Pane { select-pane -t=; send -M }`: a click in a pane that
# is not the active one makes it active. Channel: the inner server's own active
# pane index, which is not a screen reading at all.
case_click_selects_pane() {
  CASE_LABEL=click-selects-pane
  split_both
  mark_both click
  local left top
  left="$(pane_field tmux "=$INNER_SESSION:0.1" 1)"
  top="$(pane_field tmux "=$INNER_SESSION:0.1" 2)"
  click_both 0 "$((left + 3))" "$((top + 3))"
  wait_for 'the pin selected pane 1' active_pane_index_is tmux 1
  settle_both MARK-click 'the click on pane 1'
  assert_value click-selects-pane/active-pane \
    "$(active_pane_index zz)" "$(active_pane_index tmux)"
  run_on_both select-pane -t "=$INNER_SESSION:0.0"
  unsplit_both
}

# A user's OWN root mouse binding. This is the premise the whole accepted
# group rests on: zz parses and stores every mouse key name (zz-mux
# command.rs parse_mouse_key), so the bind is accepted, and the question is
# whether the gesture ever reaches it.
case_click_user_binding() {
  CASE_LABEL=click-user-binding
  run_on_both set-option -gu @mousekey
  local side
  for side in zz tmux; do
    side_command "$side" bind-key -n MouseDown1Pane set-option -g @mousekey \
      "$(binding_value_for "$side")" >/dev/null || die "$side refused bind-key"
  done
  mark_both userbind
  local left top
  left="$(pane_field tmux "=$INNER_SESSION:0.0" 1)"
  top="$(pane_field tmux "=$INNER_SESSION:0.0" 2)"
  click_both 0 "$((left + 4))" "$((top + 2))"
  wait_for 'the pin ran its own mouse binding' option_is tmux @mousekey \
    "$(binding_value_for tmux)"
  settle_both MARK-userbind 'the user binding click'
  check_value USER_BINDING click-user-binding/option \
    "$(option_value zz @mousekey)" "$(option_value tmux @mousekey)"
  run_on_both unbind-key -n MouseDown1Pane
  run_on_both set-option -gu @mousekey
}

# The event the binding was invoked FROM. `set-option -F` expands its value
# through the command's own format tree, which is where format.c publishes
# mouse_x, mouse_y and mouse_pane from the invoking mouse record.
case_click_user_binding_target() {
  CASE_LABEL=click-user-binding-target
  run_on_both set-option -gu @mousectx
  run_on_both bind-key -n MouseDown1Pane set-option -gF @mousectx \
    '#{mouse_x},#{mouse_y},#{mouse_pane},#{mouse_status_line}'
  mark_both userctx
  local left top
  left="$(pane_field tmux "=$INNER_SESSION:0.0" 1)"
  top="$(pane_field tmux "=$INNER_SESSION:0.0" 2)"
  click_both 0 "$((left + 7))" "$((top + 5))"
  wait_for 'the pin published the mouse context' pin_option_set @mousectx
  settle_both MARK-userctx 'the mouse context click'
  check_value MOUSE_CONTEXT click-user-binding-target/context \
    "$(option_value zz @mousectx)" "$(option_value tmux @mousectx)"
  run_on_both unbind-key -n MouseDown1Pane
  run_on_both set-option -gu @mousectx
}
# The target spelling only a bound mouse event can resolve. `cmd_find_target`
# answers a bare `=` and a bare `{mouse}` from `cmdq_get_event(item)->m`, so a
# command a mouse binding runs against `-t=` reaches the pane the pointer
# landed on rather than the active one. The click here lands in pane 1 while
# pane 0 is active and the binding writes a PANE option through `-t=`, so the
# pane the spelling resolved to is the pane that carries the option afterwards.
case_click_mouse_target() {
  CASE_LABEL=click-mouse-target
  split_both
  local spelling left top
  for spelling in '=' '{mouse}'; do
    run_on_both set-option -pu -t "=$INNER_SESSION:0.0" @mousetgt
    run_on_both set-option -pu -t "=$INNER_SESSION:0.1" @mousetgt
    run_on_both bind-key -n MouseDown1Pane set-option -p -t "$spelling" @mousetgt clicked
    mark_both mousetgt
    left="$(pane_field tmux "=$INNER_SESSION:0.1" 1)"
    top="$(pane_field tmux "=$INNER_SESSION:0.1" 2)"
    click_both 0 "$((left + 3))" "$((top + 3))"
    wait_for "the pin resolved $spelling to the clicked pane" \
      pin_pane_option_set "=$INNER_SESSION:0.1" @mousetgt
    settle_both MARK-mousetgt "the $spelling target click"
    assert_value "click-mouse-target/$spelling-clicked-pane" \
      "$(pane_option_value zz "=$INNER_SESSION:0.1" @mousetgt)" \
      "$(pane_option_value tmux "=$INNER_SESSION:0.1" @mousetgt)"
    assert_value "click-mouse-target/$spelling-other-pane" \
      "$(pane_option_value zz "=$INNER_SESSION:0.0" @mousetgt)" \
      "$(pane_option_value tmux "=$INNER_SESSION:0.0" @mousetgt)"
    run_on_both unbind-key -n MouseDown1Pane
    run_on_both select-pane -t "=$INNER_SESSION:0.0"
    wait_for 'the pin back on pane 0' active_pane_index_is tmux 0
  done
  run_on_both set-option -pu -t "=$INNER_SESSION:0.0" @mousetgt
  run_on_both set-option -pu -t "=$INNER_SESSION:0.1" @mousetgt
  unsplit_both
}
pane_option_value() {
  side_command "$1" show-options -pqv -t "$2" "$3" 2>/dev/null
}
pin_pane_option_set() {
  [ -n "$(pane_option_value tmux "$1" "$2")" ]
}

# What the root mouse binding sets. Both sides set the same word in every
# driven case; the self-check gives one side another.
BINDING_VALUE_ZZ=""
BINDING_VALUE_TMUX=""
binding_value_for() {
  local override="BINDING_VALUE_${1^^}"
  printf '%s' "${!override:-fired}"
}
pin_option_set() {
  [ -n "$(option_value tmux "$1")" ]
}

# The LOCATION half of a mouse key name, on the two locations that are not a
# pane body. These run AFTER case_status_clicks, because rebinding a name the
# pin has a stock binding for takes that stock binding away for the rest of the
# run and status-clicks is what measures it. `server_client_check_mouse` resolves a divider cell to
# KEYC_MOUSE_LOCATION_BORDER and a status cell inside a window range to
# KEYC_MOUSE_LOCATION_STATUS, so `MouseDown1Border` and `WheelDownStatus` are
# the names those gestures carry, and a user's own binding on either has to
# fire on both binaries.
case_border_user_binding() {
  CASE_LABEL=border-user-binding
  local right top
  split_both
  run_on_both set-option -gu @borderkey
  local side
  for side in zz tmux; do
    side_command "$side" bind-key -n MouseDown1Border set-option -g @borderkey \
      "$(binding_value_for "$side")" >/dev/null || die "$side refused bind-key"
  done
  mark_both borderbind
  right="$(pane_field tmux "=$INNER_SESSION:0.0" 3)"
  top="$(pane_field tmux "=$INNER_SESSION:0.0" 2)"
  click_both 0 "$((right + 2))" "$((top + 4))"
  wait_for 'the pin ran its own border binding' option_is tmux @borderkey \
    "$(binding_value_for tmux)"
  settle_both MARK-borderbind 'the border click'
  assert_value border-user-binding/option \
    "$(option_value zz @borderkey)" "$(option_value tmux @borderkey)"
  run_on_both unbind-key -n MouseDown1Border
  run_on_both set-option -gu @borderkey
  unsplit_both
}

case_status_user_binding() {
  CASE_LABEL=status-user-binding
  run_on_both new-window -d -t "=$INNER_SESSION:" -n second "$INNER_SHELL"
  run_on_both select-window -t "=$INNER_SESSION:0"
  run_on_both set-option -gu @statuskey
  run_on_both bind-key -n WheelDownStatus set-option -g @statuskey fired
  mark_both statusbind
  local column row
  row="$(status_row)"
  column="$(status_column_of second)"
  [ -n "$column" ] || die 'the second window is not on the status row'
  send_mouse_both 65 "$column" "$row" M
  wait_for 'the pin ran its own status binding' option_is tmux @statuskey fired
  settle_both 'second' 'the status wheel binding'
  assert_value status-user-binding/option \
    "$(option_value zz @statuskey)" "$(option_value tmux @statuskey)"
  run_on_both unbind-key -n WheelDownStatus
  run_on_both set-option -gu @statuskey
  run_on_both kill-window -t "=$INNER_SESSION:1"
  wait_for 'the pin back to one window' pin_window_count_is 1
}

# `bind -n WheelUpPane { if -F '#{||:#{alternate_on},#{pane_in_mode},#{mouse_any_flag}}' { send -M } { copy-mode -e } }`:
# a wheel over a pane whose program asked for nothing enters copy mode.
case_wheel_up_pane() {
  CASE_LABEL=wheel-up-pane
  mark_both wheel
  local left top
  left="$(pane_field tmux "=$INNER_SESSION:0.0" 1)"
  top="$(pane_field tmux "=$INNER_SESSION:0.0" 2)"
  send_mouse_both 64 "$((left + 4))" "$((top + 4))" M
  wait_for 'the pin entered copy mode on a wheel' pane_in_mode_is tmux "=$INNER_SESSION:0.0" 1
  settle_both MARK-wheel 'the wheel over the pane'
  check_value WHEEL wheel-up-pane/pane-in-mode \
    "$(pane_in_mode zz "=$INNER_SESSION:0.0")" "$(pane_in_mode tmux "=$INNER_SESSION:0.0")"
  check_value WHEEL wheel-up-pane/pane-mode \
    "$(side_command zz display-message -p -t "=$INNER_SESSION:0.0" '#{pane_mode}')" \
    "$(side_command tmux display-message -p -t "=$INNER_SESSION:0.0" '#{pane_mode}')"
  leave_copy_mode_both
}

# `bind -n MouseDrag1Pane { if -F '#{||:#{pane_in_mode},#{mouse_any_flag}}' { send -M } { copy-mode -M } }`
# with the copy table's `bind -Tcopy-mode MouseDragEnd1Pane { send -X
# copy-pipe-and-cancel }` behind it: a press, a motion with the button held and
# a release over a pane whose program asked for nothing enters copy mode,
# selects, copies and leaves again. The mode is gone by the time the release
# has been handled, so the observable is the PASTE BUFFER the gesture left,
# plus the mode midway through the drag, before the release.
case_drag_selects() {
  CASE_LABEL=drag-selects
  respawn_program_both sh -c "printf 'DRAGLINE alpha beta gamma\n'; exec sleep 600"
  settle_both 'gamma' 'the drag sample'
  delete_buffer_both
  local left top
  left="$(pane_field tmux "=$INNER_SESSION:0.0" 1)"
  top="$(pane_field tmux "=$INNER_SESSION:0.0" 2)"
  send_mouse_both 0 "$((left + 1))" "$((top + 1))" M
  send_mouse_both 32 "$((left + 8))" "$((top + 1))" M
  wait_for 'the pin entered copy mode on a drag' pane_in_mode_is tmux "=$INNER_SESSION:0.0" 1
  send_mouse_both 0 "$((left + 8))" "$((top + 1))" m
  wait_for 'the pin ended its drag' pane_in_mode_is tmux "=$INNER_SESSION:0.0" 0
  settle_both 'gamma' 'the drag released'
  check_value DRAG drag-selects/buffer "$(buffer_sample zz)" "$(buffer_sample tmux)"
  check_value DRAG drag-selects/pane-in-mode \
    "$(pane_in_mode zz "=$INNER_SESSION:0.0")" "$(pane_in_mode tmux "=$INNER_SESSION:0.0")"
  check_value DRAG drag-selects/selection \
    "$(side_command zz display-message -p -t "=$INNER_SESSION:0.0" '#{selection_present}')" \
    "$(side_command tmux display-message -p -t "=$INNER_SESSION:0.0" '#{selection_present}')"
  delete_buffer_both
  leave_copy_mode_both
  respawn_shell_both
}

# `bind -n DoubleClick1Pane { ... copy-mode -H; send -X select-word; run -d0.3;
# send -X copy-pipe-and-cancel }` and the TripleClick line sibling. input.rs
# never sets click_count above 1, so zz-terminal's own double and triple click
# paths are unreachable from the raw TUI whatever the key tables say.
case_multi_click() {
  CASE_LABEL=multi-click
  respawn_program_both sh -c "printf 'MULTI alpha beta gamma\n'; exec sleep 600"
  settle_both 'gamma' 'the multi-click sample'
  delete_buffer_both
  local left top column
  left="$(pane_field tmux "=$INNER_SESSION:0.0" 1)"
  top="$(pane_field tmux "=$INNER_SESSION:0.0" 2)"
  column="$((left + 8))"
  click_both 0 "$column" "$((top + 1))"
  click_both 0 "$column" "$((top + 1))"
  wait_for 'the pin copied a word' pin_buffer_has alpha
  settle_both 'gamma' 'the double click'
  check_value MULTI_CLICK multi-click/double-buffer \
    "$(buffer_sample zz)" "$(buffer_sample tmux)"
  delete_buffer_both
  click_both 0 "$column" "$((top + 1))"
  click_both 0 "$column" "$((top + 1))"
  click_both 0 "$column" "$((top + 1))"
  wait_for 'the pin copied a line' pin_buffer_has gamma
  settle_both 'gamma' 'the triple click'
  check_value MULTI_CLICK multi-click/triple-buffer \
    "$(buffer_sample zz)" "$(buffer_sample tmux)"
  delete_buffer_both
  respawn_shell_both
}
# A side with no buffer at all is not an error here, so this never dies.
delete_buffer_both() {
  local side
  for side in zz tmux; do
    side_command "$side" delete-buffer >/dev/null 2>&1 || true
  done
}
buffer_sample() {
  side_command "$1" show-buffer 2>/dev/null | head -n 1 | cat -v
}
pin_buffer_has() {
  case "$(buffer_sample tmux)" in
  *"$1"*) return 0 ;;
  *) return 1 ;;
  esac
}

# `bind -n MouseDown3Pane { ... display-menu ... }`: a right click with no
# application mouse raises the pin's own pane menu. Channel: the decoded
# screen, because a menu IS cells.
case_right_click_pane() {
  CASE_LABEL=right-click-pane
  mark_both rightclick
  local left top
  left="$(pane_field tmux "=$INNER_SESSION:0.0" 1)"
  top="$(pane_field tmux "=$INNER_SESSION:0.0" 2)"
  send_mouse_both 2 "$((left + 6))" "$((top + 4))" M
  both_screen_has 'Kill' 'the pane menu'
  settle_both Kill 'the right click'
  check_screen RIGHT_CLICK right-click-pane/screen
  send_bytes zz $'\033'
  send_bytes tmux $'\033'
  both_screen_lacks 'Kill' 'the pane menu closed'
  send_mouse_both 2 "$((left + 6))" "$((top + 4))" m
  send_mouse_both 2 "$((left + 2))" "$((top + 1))" M
  both_screen_has 'Kill' 'the pane menu over a word'
  settle_both Kill 'the right click over a word'
  check_screen RIGHT_CLICK_WORD right-click-pane/over-a-word
  send_bytes zz $'\033'
  send_bytes tmux $'\033'
  both_screen_lacks 'Kill' 'the pane menu over a word closed'
  send_mouse_both 2 "$((left + 2))" "$((top + 1))" m
  respawn_shell_both
}

# `bind -n MouseDrag1Border { resize-pane -M }`. Channel: the width of the
# pane left of the border. input.rs has no border hit test at all, so a drag
# that starts on a divider column falls through to whatever owns that cell.
case_border_drag() {
  CASE_LABEL=border-drag
  split_both
  mark_both border
  local right top border_column before_width
  before_width="$(pane_field tmux "=$INNER_SESSION:0.0" 5)"
  right="$(pane_field tmux "=$INNER_SESSION:0.0" 3)"
  top="$(pane_field tmux "=$INNER_SESSION:0.0" 2)"
  border_column="$((right + 2))"
  local drag_column=$((border_column - 8)) zz_drag_column
  zz_drag_column="${BORDER_SABOTAGE_COLUMN:-$drag_column}"
  send_mouse_both 0 "$border_column" "$((top + 4))" M
  send_mouse zz 32 "$zz_drag_column" "$((top + 4))" M
  send_mouse tmux 32 "$drag_column" "$((top + 4))" M
  send_mouse zz 0 "$zz_drag_column" "$((top + 4))" m
  send_mouse tmux 0 "$drag_column" "$((top + 4))" m
  wait_for 'the pin resized on a border drag' pin_pane_width_changed "$before_width"
  settle_both MARK-border 'the border drag'
  check_value BORDER border-drag/pane-width \
    "$(pane_field zz "=$INNER_SESSION:0.0" 5)" "$(pane_field tmux "=$INNER_SESSION:0.0" 5)"
  unsplit_both
}
pin_pane_width_changed() {
  [ "$(pane_field tmux "=$INNER_SESSION:0.0" 5)" != "$1" ]
}

# `bind -n MouseDown1Border { select-pane -M }`, and cmd-select-pane.c:137-149
# makes `-M` on the already-marked pane `server_clear_marked` and never a
# change of the active pane. Channel: `#{pane_marked_set}` and the active pane
# index, with pane 1 marked and pane 0 active before the click.
case_border_click() {
  CASE_LABEL=border-click
  split_both
  run_on_both select-pane -m -t "=$INNER_SESSION:0.1"
  wait_for 'the pin marked pane 1' pin_marked_set_is 1
  run_on_both select-pane -t "=$INNER_SESSION:0.0"
  wait_for 'the pin back on pane 0' active_pane_index_is tmux 0
  mark_both borderclick
  local right top
  right="$(pane_field tmux "=$INNER_SESSION:0.0" 3)"
  top="$(pane_field tmux "=$INNER_SESSION:0.0" 2)"
  click_both 0 "$((right + 2))" "$((top + 4))"
  wait_for 'the pin cleared its mark on a border click' pin_marked_set_is 0
  settle_both MARK-borderclick 'the border click'
  assert_value border-click/marked-set "$(marked_set zz)" "$(marked_set tmux)"
  assert_value border-click/active-pane \
    "$(active_pane_index zz)" "$(active_pane_index tmux)"
  unsplit_both
}
marked_set() {
  side_command "$1" display-message -p -t "=$INNER_SESSION:0.0" \
    '#{pane_marked_set}' 2>/dev/null
}
pin_marked_set_is() {
  [ "$(marked_set tmux)" = "$1" ]
}

# The status line's own ranges. `bind -n MouseDown1Status { switch-client -t= }`
# resolves the window under the pointer from the range the status line
# published, so every gesture here is aimed at a WINDOW range: the pin's
# status keys carry their location, and the empty right of the row is
# StatusDefault, whose own names (`WheelDownStatusDefault` and the rest) the
# pin leaves unbound. `WheelUpStatus` and `WheelDownStatus` are
# previous-window and next-window, and `MouseDown3Status` is the window menu.
case_status_clicks() {
  CASE_LABEL=status-clicks
  run_on_both new-window -d -t "=$INNER_SESSION:" -n second "$INNER_SHELL"
  run_on_both select-window -t "=$INNER_SESSION:0"
  mark_both status
  assert_value status-clicks/row \
    "$(capture_plain zz | tail -n 1)" "$(capture_plain tmux | tail -n 1)"
  local column row
  row="$(status_row)"
  column="$(status_column_of second)"
  [ -n "$column" ] || die 'the second window is not on the status row'
  click_both 0 "$column" "$row"
  wait_for 'the pin switched window on a status click' pin_window_is 1
  settle_both 'second' 'the status click'
  check_value STATUS status-clicks/window-after-name-click \
    "$(current_window zz)" "$(current_window tmux)"
  run_on_both select-window -t "=$INNER_SESSION:0"
  wait_for 'the pin back on window 0' pin_window_is 0
  settle_both 'second' 'back on window 0'

  send_mouse_both 65 "$column" "$row" M
  wait_for 'the pin took the status wheel' pin_window_is 1
  settle_both 'second' 'the status wheel down'
  check_value STATUS status-clicks/window-after-wheel-down \
    "$(current_window zz)" "$(current_window tmux)"
  send_mouse_both 64 "$column" "$row" M
  wait_for 'the pin took the status wheel back' pin_window_is 0
  settle_both 'second' 'the status wheel up'
  check_value STATUS status-clicks/window-after-wheel-up \
    "$(current_window zz)" "$(current_window tmux)"

  send_mouse_both 2 "$column" "$row" M
  both_screen_has 'Rename' 'the window menu'
  settle_both Rename 'the status right click'
  check_screen STATUS_MENU status-clicks/right-click-screen
  send_bytes zz $'\033'
  send_bytes tmux $'\033'
  both_screen_lacks 'Rename' 'the window menu closed'
  send_mouse_both 2 "$column" "$row" m

  send_mouse_both 10 "$column" "$row" M
  both_screen_has 'Rename' 'the alt window menu'
  settle_both Rename 'the alt status right click'
  check_screen STATUS_MENU status-clicks/alt-right-click-screen
  send_bytes zz $'\033'
  send_bytes tmux $'\033'
  both_screen_lacks 'Rename' 'the alt window menu closed'
  send_mouse_both 10 "$column" "$row" m
  respawn_shell_both
  run_on_both kill-window -t "=$INNER_SESSION:1"
  wait_for 'the pin back to one window' pin_window_count_is 1
}
status_column_of() {
  local row index
  row="$(capture_plain tmux | tail -n 1)"
  index="$(awk -v haystack="$row" -v needle="$1" 'BEGIN { print index(haystack, needle) }')"
  [ "$index" -gt 0 ] || return 1
  printf '%s\n' "$index"
}
current_window() {
  side_command "$1" display-message -p -t "=$INNER_SESSION:" '#{window_index}' 2>/dev/null
}
pin_window_is() {
  [ "$(current_window tmux)" = "$1" ]
}
pin_window_count_is() {
  [ "$(side_command tmux list-windows -t "=$INNER_SESSION" -F x | wc -l)" = "$1" ]
}

# The forward path, which both binaries already have: a program that asked for
# mouse reporting gets the event, and the two clients have to hand it the same
# bytes. With `mouse on` the pin reaches it through `send -M` inside its own
# stock binding; with `mouse off` it never consults a table at all.
case_app_mouse() {
  local label="$1" mouse="$2"
  CASE_LABEL="app-mouse-$label"
  set_on_both mouse "$mouse"
  local side
  for side in zz tmux; do
    side_command "$side" respawn-pane -k -t "=$INNER_SESSION:0.0" sh -c \
      "stty -echo -icanon min 1 time 0; printf 'APPMOUSE\n\033[?1000h$(app_mouse_extra "$side")'; exec cat -v" >/dev/null ||
      die "$side refused respawn-pane"
  done
  settle_both APPMOUSE "the mouse program for $label"
  local left top
  left="$(pane_field tmux "=$INNER_SESSION:0.0" 1)"
  top="$(pane_field tmux "=$INNER_SESSION:0.0" 2)"
  click_both 0 "$((left + 5))" "$((top + 3))"
  wait_for "the pin's program saw the click for $label" program_saw tmux APPMOUSE 'M'
  settle_both APPMOUSE "the click for $label"
  assert_value "app-mouse-$label/report" \
    "$(program_output zz APPMOUSE)" "$(program_output tmux APPMOUSE)"
  respawn_shell_both
  set_on_both mouse on
}
# The SGR extension one side's program asks for. Both ask for it in every
# driven case; the self-check names a side here, whose program then arms the
# old X10 reporting alone, which changes the spelling of every report that
# side receives and nothing else.
SABOTAGE_ENCODING=""
app_mouse_extra() {
  [ "$SABOTAGE_ENCODING" = "$1" ] && return 0
  printf '%s' '\033[?1006h'
}
program_saw() {
  case "$(program_output "$1" "$2")" in
  *"$3"*) return 0 ;;
  *) return 1 ;;
  esac
}

# --- the pin's fallthrough, with tracking armed ----------------------------
#
# `\033[?1002h` is BUTTON-EVENT tracking: the pane asks for presses, releases
# and motion with a button held, which is what a program that wants drags arms.
# `#{mouse_any_flag}` is then 1 on both binaries, so the pin's own
# `MouseDown1Pane` and `MouseDrag1Pane` rows take their `send -M` branch and
# hand their event to the pane. What the two cases below drive is the gestures
# whose remaining events are bound to NOTHING in the root table, which is where
# `server_client_handle_key` ends at `forward_key` and `window_pane_key` hands
# the event to the pane rather than swallowing it.
arm_button_event_both() {
  local marker="$1" side
  for side in zz tmux; do
    side_command "$side" respawn-pane -k -t "=$INNER_SESSION:0.0" sh -c \
      "stty -echo -icanon min 1 time 0; printf '$marker\n\033[?1002h\033[?1006h'; exec cat -v" >/dev/null ||
      die "$side refused respawn-pane"
  done
  settle_both "$marker" "the button-event program behind $marker"
}
# The one row a program under tracking prints its reports onto: `cat -v` wraps
# nothing, so every report of a gesture lands on the row under the marker. The
# two cases below read that row and nothing else, where the rest of the fixture
# reads four rows: a message another case left on the row below would otherwise
# ride along in a channel that is about the reports.
app_reports() {
  capture_plain "$1" | sed -n "/$2/,\$p" | sed "1d" | sed '/^[[:space:]]*$/d' |
    head -n 1 | cat -v
}
# How many SGR reports that row carries: every report this fixture can produce
# starts `\e[<`.
report_count() {
  app_reports "$1" "$2" | grep -o -- '\[<' | wc -l | tr -d ' '
}
program_saw_reports() {
  [ "$(report_count "$1" "$2")" -ge "$3" ]
}
# `KEYC_CLICK_TIMEOUT` is 300 ms and the timer that runs out at the end of it
# replays the stored press under a `DoubleClick` name, so a reading taken
# before that has happened cannot see what the replay did. This waits for the
# report count to HOLD at what it should be for longer than the timeout:
# twelve consecutive polls at 50 ms, with any poll that finds a different count
# starting the twelve again. It is a bounded wait on an observable, not a
# sleep: a count that moves is what it is watching for.
reports_held() {
  local side="$1" marker="$2" want="$3" attempt held=0
  for ((attempt = 0; attempt < 200; attempt++)); do
    if [ "$(report_count "$side" "$marker")" = "$want" ]; then
      held=$((held + 1))
      [ "$held" -ge 12 ] && return 0
    else
      held=0
    fi
    sleep 0.05
  done
  dump_state "the $side report count holding at $want"
  die "the $side report count never held at $want"
}

# A press, a motion with the button held and the release that ends the drag.
# The press is `MouseDown1Pane` and the motion `MouseDrag1Pane`, both stock
# root rows that run `send -M` while the pane tracks. The release is
# `MouseDragEnd1Pane`, which `key-bindings.c` installs in the two copy tables
# and in NEITHER root table: with the client on root and the pane in no mode,
# root is the first and only table tried, nothing matches, and the pin forwards
# the release to the pane. Channel: the three reports the program received, in
# the order it received them.
case_app_mouse_drag() {
  CASE_LABEL=app-mouse-drag
  arm_button_event_both DRAGREPORT
  local left top row
  left="$(pane_field tmux "=$INNER_SESSION:0.0" 1)"
  top="$(pane_field tmux "=$INNER_SESSION:0.0" 2)"
  row="$((top + 3))"
  send_mouse_both 0 "$((left + 2))" "$row" M
  send_mouse_both 32 "$((left + 8))" "$row" M
  send_mouse_both 0 "$((left + 8))" "$row" m
  wait_for "the pin's program saw the press, the motion and the release" \
    program_saw_reports tmux DRAGREPORT 3
  settle_both DRAGREPORT 'the drag under button-event tracking'
  assert_value app-mouse-drag/reports \
    "$(app_reports zz DRAGREPORT)" "$(app_reports tmux DRAGREPORT)"
  respawn_shell_both
}

# Two clicks at one cell inside the click timeout. The first press is
# `MouseDown1Pane` and the first release `MouseUp1Pane`, which root does not
# bind; the second press is `SecondClick1Pane`, which root does not bind
# either, so both fall through to the pane the moment they arrive. The timer
# behind them replays the stored press as `DoubleClick1Pane`, whose stock row
# does run `send -M` while the pane tracks - and that replayed event is the one
# `server_client_check_mouse` marks `m->ignore`, which `input_key_mouse` drops.
# So the pane sees press, release, press, release and nothing else, and the
# ORDER is the whole point: a second press held back until its timer expires
# would arrive behind its own release.
case_app_mouse_double_click() {
  CASE_LABEL=app-mouse-double-click
  arm_button_event_both CLICKREPORT
  local left top row column
  left="$(pane_field tmux "=$INNER_SESSION:0.0" 1)"
  top="$(pane_field tmux "=$INNER_SESSION:0.0" 2)"
  row="$((top + 3))"
  column="$((left + 5))"
  click_both 0 "$column" "$row"
  click_both 0 "$column" "$row"
  wait_for "the pin's program saw both clicks" program_saw_reports tmux CLICKREPORT 4
  reports_held tmux CLICKREPORT 4
  settle_both CLICKREPORT 'the double click under button-event tracking'
  assert_value app-mouse-double-click/reports \
    "$(app_reports zz CLICKREPORT)" "$(app_reports tmux CLICKREPORT)"
  respawn_shell_both
}

# Bracketed paste, into a pane, into copy mode, into the command prompt and
# under a menu. The bytes are a real `\e[200~ ... \e[201~`, which is what the
# outer terminal sends when a user pastes.
PASTE_BYTES=$'\033[200~pasted-text\033[201~'
PASTE_BYTES_ZZ=""
PASTE_BYTES_TMUX=""
paste_bytes_for() {
  local override="PASTE_BYTES_${1^^}"
  printf '%s' "${!override:-$PASTE_BYTES}"
}
case_paste_into_pane() {
  CASE_LABEL=paste-into-pane
  respawn_program_both sh -c "stty -echo -icanon min 1 time 0; printf 'PASTEPANE\n\033[?2004h'; exec cat -v"
  settle_both PASTEPANE 'the paste program'
  send_bytes zz "$(paste_bytes_for zz)"
  send_bytes tmux "$(paste_bytes_for tmux)"
  wait_for "the pin's program saw the paste" program_saw tmux PASTEPANE 'pasted-text'
  settle_both PASTEPANE 'the paste into the pane'
  assert_value paste-into-pane/received \
    "$(program_output zz PASTEPANE)" "$(program_output tmux PASTEPANE)"
  respawn_shell_both
}
case_paste_into_copy_mode() {
  CASE_LABEL=paste-into-copy-mode
  respawn_shell_both
  run_on_both clear-history -t "=$INNER_SESSION:0.0"
  mark_both pastecopy
  run_on_both copy-mode -t "=$INNER_SESSION:0.0"
  wait_for 'the pin in copy mode' pane_in_mode_is tmux "=$INNER_SESSION:0.0" 1
  wait_for 'zz in copy mode' pane_in_mode_is zz "=$INNER_SESSION:0.0" 1
  settle_both MARK-pastecopy 'copy mode before the paste'
  if [ -n "$COPY_PASTE_SABOTAGE_SIDE" ]; then
    side_command "$COPY_PASTE_SABOTAGE_SIDE" send-keys -X -t "=$INNER_SESSION:0.0" cancel \
      >/dev/null 2>&1 || true
    wait_for 'the sabotaged side out of copy mode' \
      pane_in_mode_is "$COPY_PASTE_SABOTAGE_SIDE" "=$INNER_SESSION:0.0" 0
    settle_both MARK-pastecopy 'copy mode left on one side before the paste'
  fi
  send_bytes zz "$(paste_bytes_for zz)"
  send_bytes tmux "$(paste_bytes_for tmux)"
  settle_both MARK-pastecopy 'the paste under copy mode'
  assert_value paste-into-copy-mode/pane-in-mode \
    "$(pane_in_mode zz "=$INNER_SESSION:0.0")" "$(pane_in_mode tmux "=$INNER_SESSION:0.0")"
  assert_screen paste-into-copy-mode/screen
  leave_copy_mode_both
  settle_both MARK-pastecopy 'copy mode left after the paste'
  check_screen PASTE_COPY paste-into-copy-mode/after-cancel-screen
  respawn_shell_both
}
case_paste_into_prompt() {
  CASE_LABEL=paste-into-prompt
  mark_both pasteprompt
  run_on_both bind-key -T prefix Q command-prompt -p 'PASTEPROMPT ' 'set-option -g @pasted "%%"'
  send_bytes zz $'\002'
  send_bytes tmux $'\002'
  send_bytes zz 'Q'
  send_bytes tmux 'Q'
  both_screen_has PASTEPROMPT 'the paste prompt'
  settle_both MARK-pasteprompt 'the paste prompt'
  send_bytes zz "$(paste_bytes_for zz)"
  send_bytes tmux "$(paste_bytes_for tmux)"
  settle_both MARK-pasteprompt 'the paste at the prompt'
  assert_value paste-into-prompt/row \
    "$(capture_plain zz | tail -n 1)" "$(capture_plain tmux | tail -n 1)"
  send_bytes zz $'\033'
  send_bytes tmux $'\033'
  both_screen_lacks PASTEPROMPT 'the paste prompt cancelled'
  run_on_both unbind-key -T prefix Q
}
case_paste_under_menu() {
  CASE_LABEL=paste-under-menu
  run_on_both set-option -gu @menupick
  run_on_both bind-key -T prefix E display-menu -x 4 -y 8 -T PASTEMENU \
    'Alpha item' a 'set-option -g @menupick alpha' \
    'Beta item' b 'set-option -g @menupick beta'
  mark_both pastemenu
  send_bytes zz $'\002'
  send_bytes tmux $'\002'
  send_bytes zz 'E'
  send_bytes tmux 'E'
  both_screen_has PASTEMENU 'the paste menu'
  settle_both MARK-pastemenu 'the paste menu'
  send_bytes zz "$(paste_bytes_for zz)"
  send_bytes tmux "$(paste_bytes_for tmux)"
  settle_both MARK-pastemenu 'the paste under the menu'
  check_screen PASTE_MENU paste-under-menu/screen
  assert_value paste-under-menu/option \
    "$(option_value zz @menupick)" "$(option_value tmux @menupick)"
  send_bytes zz $'\033'
  send_bytes tmux $'\033'
  both_screen_lacks PASTEMENU 'the paste menu closed'
  run_on_both unbind-key -T prefix E
  run_on_both set-option -gu @menupick
}

FOCUS_BYTES=$'\033[O\033[I'
FOCUS_BYTES_ZZ=""
FOCUS_BYTES_TMUX=""
focus_bytes_for() {
  local override="FOCUS_BYTES_${1^^}"
  printf '%s' "${!override:-$FOCUS_BYTES}"
}
# Focus reporting, with `focus-events` on and off. The bytes are a real
# `\e[O` and `\e[I`, which is what a terminal sends when its window loses and
# regains focus.
case_focus() {
  local label="$1" events="$2"
  CASE_LABEL="focus-$label"
  set_on_both focus-events "$events"
  respawn_program_both sh -c "stty -echo -icanon min 1 time 0; printf 'FOCUSPROG\n\033[?1004h'; exec cat -v"
  settle_both FOCUSPROG "the focus program with focus-events $events"
  send_bytes zz "$(focus_bytes_for zz)"
  send_bytes tmux "$(focus_bytes_for tmux)"
  if [ "$events" = on ]; then
    wait_for "the pin's program saw a focus report" program_saw tmux FOCUSPROG '['
  fi
  settle_both FOCUSPROG "the focus reports with focus-events $events"
  if [ "$events" = on ]; then
    assert_value "focus-$label/received" \
      "$(program_output zz FOCUSPROG)" "$(program_output tmux FOCUSPROG)"
  else
    check_value FOCUS_OFF "focus-$label/received" \
      "$(program_output zz FOCUSPROG)" "$(program_output tmux FOCUSPROG)"
  fi
  respawn_shell_both
  set_on_both focus-events off
}

# --- dispositions ----------------------------------------------------------
#
# Each mode below is `same` where the two binaries are measured to agree and
# `record` where they do not, fixed here and never discovered at runtime, so a
# sabotage can drive a recorded channel in a case where it asserts.
USER_BINDING_MODE=same
USER_BINDING_REASON=""
MOUSE_CONTEXT_MODE=same
MOUSE_CONTEXT_REASON=""
WHEEL_MODE=same
WHEEL_REASON=""
DRAG_MODE=same
DRAG_REASON=""
MULTI_CLICK_MODE=same
MULTI_CLICK_REASON=""
BORDER_MODE=same
BORDER_REASON=""
STATUS_MODE=same
STATUS_REASON=""
STATUS_MENU_MODE=same
STATUS_MENU_REASON=""
PASTE_MENU_MODE=same
PASTE_MENU_REASON=""
FOCUS_OFF_MODE=same
FOCUS_OFF_REASON=""
PASTE_COPY_MODE=same
PASTE_COPY_REASON=""
RIGHT_CLICK_MODE=same
RIGHT_CLICK_REASON=""
RIGHT_CLICK_WORD_MODE=record
RIGHT_CLICK_WORD_REASON="formats.mouse-context: the same MouseDown3Pane gesture over a cell whose row carries text. DEFAULT_PANE_MENU renders three of its items off format:mouse_word, format:mouse_line and format:mouse_hyperlink, which the daemon answers empty on zz because it has no synchronous read of the live grid under a cell, so the pin raises a 14-row menu carrying Copy Line where zz raises a 12-row menu without it. Over the blank cell right-click-pane/screen aims at, the three names are empty on the pin too and the menus agree"

run_cases() {
  start_both
  case_click_selects_pane
  case_click_user_binding
  case_click_user_binding_target
  case_click_mouse_target
  case_wheel_up_pane
  case_drag_selects
  case_multi_click
  case_right_click_pane
  case_border_drag
  case_border_click
  case_status_clicks
  case_border_user_binding
  case_status_user_binding
  case_app_mouse mouse-on on
  case_app_mouse mouse-off off
  case_app_mouse_drag
  case_app_mouse_double_click
  case_paste_into_pane
  case_paste_into_copy_mode
  case_paste_into_prompt
  case_paste_under_menu
  case_focus events-on on
  case_focus events-off off

  printf '%s asserted checks, %s recorded checks\n' "$CHECKS" "$RECORDS"
  if [ "$FAILURES" -ne 0 ]; then
    printf '%s of %s asserted checks differ\n' "$FAILURES" "$CHECKS"
    exit 1
  fi
  printf 'all %s asserted checks identical\n' "$CHECKS"
  exit 0
}

# --- self-check ------------------------------------------------------------
#
# Each sabotage drives the same case with ONE deliberate one-sided difference
# and requires an ASSERTED check in that case to report it. A recorded check
# cannot satisfy a sabotage: it never fails.
SELF_CHECK_FAILURES=0

self_check_case() {
  local name="$1" expectation="$2" before="$FAILURES"
  shift 2
  "$@"
  local differed=$((FAILURES - before))
  FAILURES="$before"
  if [ "$expectation" = catches ] && [ "$differed" -gt 0 ]; then
    printf 'ok    self-check %s: caught\n' "$name"
    return 0
  fi
  if [ "$expectation" = quiet ] && [ "$differed" -eq 0 ]; then
    printf 'ok    self-check %s: no difference reported\n' "$name"
    return 0
  fi
  SELF_CHECK_FAILURES=$((SELF_CHECK_FAILURES + 1))
  printf 'FAIL  self-check %s: %s asserted checks differed, expected %s\n' \
    "$name" "$differed" "$expectation"
}

# The same root mouse binding set to a different value on one side. Both
# binaries run their own binding now, so click-user-binding/option carries the
# difference and nothing else can.
sc_one_sided_binding_value() {
  BINDING_VALUE_ZZ=other
  case_click_user_binding
  BINDING_VALUE_ZZ=""
}
# The border name's own binding set differently on one side.
sc_one_sided_border_binding() {
  BINDING_VALUE_ZZ=other
  case_border_user_binding
  BINDING_VALUE_ZZ=""
}
# zz's click aimed at the pane it is already in while the pin's lands in the
# other one. click-selects-pane/active-pane is the only asserted check in that
# case and has to carry the difference.
sc_one_sided_click_target() {
  CLICK_SABOTAGE_SIDE=zz
  CLICK_SABOTAGE_COLUMN=3
  case_click_selects_pane
  CLICK_SABOTAGE_SIDE=""
  CLICK_SABOTAGE_COLUMN=""
}
# A different paste on one side. paste-into-pane/received is the only asserted
# check in that case and has to carry the difference.
sc_one_sided_paste() {
  PASTE_BYTES_ZZ=$'\033[200~other-text\033[201~'
  case_paste_into_pane
  PASTE_BYTES_ZZ=""
}
# The SGR extension dropped from one side's program. Both clients then hand
# that program a report in the old X10 spelling and the other side an SGR one,
# and app-mouse-mouse-on/report is where that shows.
sc_one_sided_mouse_encoding() {
  SABOTAGE_ENCODING=zz
  case_app_mouse sc-encoding on
  SABOTAGE_ENCODING=""
}
# Only the focus-out report sent to one side.
sc_one_sided_focus() {
  FOCUS_BYTES_ZZ=$'\033[O'
  case_focus sc-focus on
  FOCUS_BYTES_ZZ=""
}
# A status-left only one side carries. status-clicks/row is asserted whole.
sc_one_sided_status_left() {
  side_command zz set-option -g status-left LL >/dev/null
  case_status_clicks
  side_command zz set-option -g status-left L >/dev/null
}

# The context click aimed one cell further along on zz. The binding sets the
# same word on both sides, so click-user-binding-target/context is the only
# asserted check that can carry it, and it carries it in mouse_x.
sc_one_sided_mouse_context() {
  CLICK_SABOTAGE_SIDE=zz
  CLICK_SABOTAGE_COLUMN=$(($(pane_field tmux "=$INNER_SESSION:0.0" 1) + 9))
  case_click_user_binding_target
  CLICK_SABOTAGE_SIDE=""
  CLICK_SABOTAGE_COLUMN=""
}
# zz's target click aimed into pane 0 while the pin's lands in pane 1, so the
# pane each side's `-t=` resolved to is a different pane.
sc_one_sided_mouse_target() {
  CLICK_SABOTAGE_SIDE=zz
  CLICK_SABOTAGE_COLUMN=3
  case_click_mouse_target
  CLICK_SABOTAGE_SIDE=""
  CLICK_SABOTAGE_COLUMN=""
}
# zz's border drag released four cells short of the pin's.
sc_one_sided_border_drag() {
  local right
  right="$(pane_field tmux "=$INNER_SESSION:0.0" 3)"
  BORDER_SABOTAGE_COLUMN=$((right + 2 - 4))
  case_border_drag
  BORDER_SABOTAGE_COLUMN=""
}
# The pin's own `WheelDownStatus` unbound on zz only, so a wheel over the
# status row steps the pin's window and leaves zz's where it was. Both wheel
# checks carry it: the wheel up then steps zz back from window 0.
sc_one_sided_status_wheel() {
  side_command zz unbind-key -T root WheelDownStatus >/dev/null 2>&1
  case_status_clicks
  side_command zz bind-key -T root WheelDownStatus next-window >/dev/null 2>&1
}
# The pin's own `WheelUpPane` unbound on zz only, so the wheel enters the pin's
# copy mode and leaves zz scrolling its own viewport, which is no mode at all.
# Both wheel checks carry it.
sc_one_sided_wheel_up_pane() {
  side_command zz unbind-key -T root WheelUpPane >/dev/null 2>&1
  case_wheel_up_pane
  side_command zz bind-key -T root WheelUpPane \
    'if-shell -F "#{||:#{alternate_on},#{pane_in_mode},#{mouse_any_flag}}" { send-keys -M } { copy-mode -e }' \
    >/dev/null 2>&1
}
# The copy table's own `MouseDragEnd1Pane` unbound on zz only, so zz's drag
# selects and stays in the mode where the pin's copies and leaves. All three
# drag checks carry it.
sc_one_sided_drag_end() {
  side_command zz unbind-key -T copy-mode MouseDragEnd1Pane >/dev/null 2>&1
  case_drag_selects
  side_command zz bind-key -T copy-mode MouseDragEnd1Pane \
    send-keys -X copy-pipe-and-cancel >/dev/null 2>&1
}
# The pin's own `DoubleClick1Pane` unbound on zz only. The second press still
# becomes a `SecondClick` on both sides and the timer still expires, so the
# difference is only in what the expiring timer runs, and multi-click's double
# check is the one that can carry it: the triple click that follows runs the
# row that is still bound on both sides.
sc_one_sided_double_click() {
  side_command zz unbind-key -T root DoubleClick1Pane >/dev/null 2>&1
  case_multi_click
  side_command zz bind-key -T root DoubleClick1Pane \
    'select-pane -t= ; if-shell -F "#{||:#{pane_in_mode},#{mouse_any_flag}}" { send-keys -M } { copy-mode -H ; send-keys -X select-word ; run-shell -d 0.3 ; send-keys -X copy-pipe-and-cancel }' \
    >/dev/null 2>&1
}
# A longer paste on zz only. The menu eats the same leading characters on both
# sides and `a` picks the same item on both, so the option is unmoved and the
# tail the pane is left holding is the whole difference.
# paste-under-menu/screen is the only channel that can carry it.
sc_one_sided_menu_paste_tail() {
  PASTE_BYTES_ZZ=$'\033[200~pasted-text-and-more\033[201~'
  case_paste_under_menu
  PASTE_BYTES_ZZ=""
  respawn_shell_both
}
# zz's own pane marked and the pin's not. Both sides still raise the pin's pane
# menu, so the case's waits are unmoved and the two rows the mark decides -
# `#{?pane_marked_set,,-}Swap Marked` and `#{?pane_marked,Unmark,Mark}` - are
# the whole difference. right-click-pane/screen is the only channel that can
# carry it.
sc_one_sided_marked_pane() {
  side_command zz select-pane -m -t "=$INNER_SESSION:0.0" >/dev/null 2>&1
  case_right_click_pane
  side_command zz select-pane -M >/dev/null 2>&1
}
# `key-bindings.c`'s own `DEFAULT_WINDOW_MENU`, the eleven items the pin's
# `MouseDown3Status` raises. It is spelled out once here so the position
# sabotage below can rebind zz with the SAME menu and nothing but the position
# changed.
WINDOW_MENU_ITEMS=(
  '#{?#{>:#{session_windows},1},,-}Swap Left' l '{ swap-window -t :-1 }'
  '#{?#{>:#{session_windows},1},,-}Swap Right' r '{ swap-window -t :+1 }'
  '#{?pane_marked_set,,-}Swap Marked' s '{ swap-window }'
  ''
  Kill X '{ kill-window }'
  Respawn R '{ respawn-window -k }'
  '#{?pane_marked,Unmark,Mark}' m '{ select-pane -m }'
  Rename n '{ command-prompt -F -I "#W" { rename-window -t "#{window_id}" "%%" } }'
  ''
  'New After' w '{ new-window -a }'
  'New At End' W '{ new-window }'
)
bind_window_menu_on_zz() {
  local key
  for key in MouseDown3Status M-MouseDown3Status; do
    side_command zz bind-key -T root "$key" display-menu -t = \
      -x "$1" -y "$2" -T '#[align=centre]#{window_index}:#{window_name}' \
      "${WINDOW_MENU_ITEMS[@]}" >/dev/null 2>&1
  done
}
# zz's window menu raised at the screen CENTRE instead of over the status
# range the gesture landed in. The eleven items, the title and the borders are
# the pin's on both sides and only where the menu sits differs, so
# status-clicks/right-click-screen is the only channel that can carry it and it
# carries exactly the `-x W` and `-y W` this landing made answer from the
# invoking event.
sc_one_sided_status_menu_position() {
  bind_window_menu_on_zz C C
  case_status_clicks
  bind_window_menu_on_zz W W
}
# The pin's own `MouseDown1Border` unbound on zz only, so zz's border click
# runs nothing and the pane it had marked stays marked. This has to run before
# the border-binding sabotage: that one's case rebinds `MouseDown1Border` and
# unbinds it again, which takes the pin's stock binding away for the rest of
# the run.
sc_one_sided_border_click() {
  side_command zz unbind-key -T root MouseDown1Border >/dev/null 2>&1
  case_border_click
  side_command zz bind-key -T root MouseDown1Border select-pane -M >/dev/null 2>&1
}
# `MouseDragEnd1Pane` bound in zz's ROOT table only. The name is the one a
# release that ends a drag carries, and a binding that claims it is a binding
# that stops it falling through to the pane, so zz's program sees the press and
# the motion and never the release. The binding sets a user option, which is
# silent: nothing but the missing report reaches the screen. app-mouse-drag/reports is the only asserted
# check in that case and has to carry it.
sc_one_sided_drag_release() {
  side_command zz bind-key -T root MouseDragEnd1Pane set-option -g @claimed dragend >/dev/null
  case_app_mouse_drag
  side_command zz unbind-key -T root MouseDragEnd1Pane >/dev/null
  side_command zz set-option -gu @claimed >/dev/null 2>&1
}
# `SecondClick1Pane` bound in zz's ROOT table only, so zz's second press is
# claimed by a binding where the pin's falls through to the pane and zz's
# program sees three reports where the pin's sees four.
# app-mouse-double-click/reports is the only asserted check in that case and
# has to carry it.
sc_one_sided_second_click() {
  side_command zz bind-key -T root SecondClick1Pane set-option -g @claimed second >/dev/null
  case_app_mouse_double_click
  side_command zz unbind-key -T root SecondClick1Pane >/dev/null
  side_command zz set-option -gu @claimed >/dev/null 2>&1
}
# zz out of copy mode before the paste, so its pane takes the text the mode
# would have eaten. paste-into-copy-mode/after-cancel-screen is where the two
# screens part.
sc_one_sided_copy_mode_paste() {
  COPY_PASTE_SABOTAGE_SIDE=zz
  case_paste_into_copy_mode
  COPY_PASTE_SABOTAGE_SIDE=""
}
# Only the focus-out report sent to one side, with focus-events OFF: the pane
# asked for reports and gets them on both sides whatever the option says, so
# the case asserts and the missing report is a difference.
sc_one_sided_focus_off() {
  FOCUS_BYTES_ZZ=$'\033[O'
  case_focus sc-focus-off off
  FOCUS_BYTES_ZZ=""
}

run_self_check() {
  start_both
  printf 'self-check: one deliberate one-sided difference per channel\n'

  self_check_case 'control, a click with nothing sabotaged' quiet \
    case_click_selects_pane
  self_check_case 'control, the same paste on both sides' quiet \
    case_paste_into_pane
  self_check_case 'control, the same focus reports on both sides' quiet \
    case_focus sc-control on

  self_check_case 'a one-sided paste' catches sc_one_sided_paste
  self_check_case 'the SGR mouse extension dropped on zz only' catches \
    sc_one_sided_mouse_encoding
  self_check_case 'only the focus-out report sent to zz' catches sc_one_sided_focus
  self_check_case 'a status-left only zz carries' catches sc_one_sided_status_left
  self_check_case "zz's click aimed at the pane it already sits in" catches \
    sc_one_sided_click_target
  self_check_case 'the same root mouse binding set differently on zz' catches \
    sc_one_sided_binding_value
  self_check_case 'MouseDown1Border unbound on zz only' catches \
    sc_one_sided_border_click
  self_check_case 'the border mouse binding set differently on zz' catches \
    sc_one_sided_border_binding
  self_check_case "zz's context click aimed one cell further along" catches \
    sc_one_sided_mouse_context
  self_check_case "zz's border drag released four cells short" catches \
    sc_one_sided_border_drag
  self_check_case 'a longer paste under the menu on zz only' catches \
    sc_one_sided_menu_paste_tail
  self_check_case "zz's own pane marked and the pin's not" catches \
    sc_one_sided_marked_pane
  self_check_case "zz's window menu centred instead of over its status range" \
    catches sc_one_sided_status_menu_position
  self_check_case 'WheelDownStatus unbound on zz only' catches \
    sc_one_sided_status_wheel
  self_check_case 'WheelUpPane unbound on zz only' catches \
    sc_one_sided_wheel_up_pane
  self_check_case "the copy table's MouseDragEnd1Pane unbound on zz only" catches \
    sc_one_sided_drag_end
  self_check_case 'DoubleClick1Pane unbound on zz only' catches \
    sc_one_sided_double_click
  self_check_case "zz's mouse-target click aimed into the other pane" catches \
    sc_one_sided_mouse_target
  self_check_case 'zz out of copy mode before the paste' catches \
    sc_one_sided_copy_mode_paste
  self_check_case 'MouseDragEnd1Pane bound in zz-s root table only' catches \
    sc_one_sided_drag_release
  self_check_case 'SecondClick1Pane bound in zz-s root table only' catches \
    sc_one_sided_second_click
  self_check_case 'only the focus-out report sent to zz with focus-events off' catches \
    sc_one_sided_focus_off

  if [ "$SELF_CHECK_FAILURES" -ne 0 ]; then
    printf '%s self-check cases did not behave as required\n' "$SELF_CHECK_FAILURES"
    exit 1
  fi
  printf 'self-check: every sabotage caught in its own channel\n'
  exit 0
}

if [ "$SELF_CHECK" -eq 1 ]; then
  run_self_check
else
  run_cases
fi
