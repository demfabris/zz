#!/usr/bin/env bash
# Whole-screen differential for the overlay surfaces: the command prompt,
# confirm-before, display-menu, display-popup and display-panes.
#
# compat/attached-client.sh already drives every one of these surfaces as
# BEHAVIOUR - which key closes what, which option a menu item sets, which byte
# a popup's job receives - and those cases stay there as regressions. None of
# them ever compared what the surface LOOKS like: where the prompt sits, which
# cells a menu covers, what a popup border is drawn with, where the cursor is
# left. That is TUI-007, and this fixture is its measurement.
#
# THE DECODED SCREEN IS THE CONTRACT, NOT THE BYTE STREAM. Both binaries attach
# inside ONE outer pinned tmux, one window each, and the outer tmux is the
# decoder: `capture-pane -p -e` re-emits SGR from its own grid, so attribute
# order, batching, redundant resets and cursor-movement spelling collapse on
# both sides before anything is compared, while the colour CLASS does not. The
# whole visible screen is compared row index by row index, plus the cursor tuple
# the inner client left in the outer terminal. The driver is tui-indicators.sh's.
#
# CONTROLLED DYNAMIC VALUES, set on both sides before the first checkpoint:
#   status-right ''      the default ends in a clock and in %d-%b-%y, which the
#                        pin expands through libc strftime and zz expands
#                        locale-independently. That belongs to status-row.sh.
#   status-left L        a fixed literal, so the left of the row is asserted.
#   automatic-rename off plus rename-window win: the default name follows the
#                        running command and would race a marker.
#   select-pane -T title fixed: the pane.runtime-facts decision, not this
#                        screen's.
#   display-time         held at MESSAGE_HOLD_MS so a message cannot expire
#                        between the two sides' captures.
#   display-panes -d 0   display-panes-time would retire the labels on a clock;
#                        -d 0 holds them until a key.
#   default-shell        /bin/sh on both sides, so a popup's job runs under the
#                        same shell whatever the box's $SHELL is.
#   the inner shell      ENV= PS1='$ ' exec /bin/sh: no rc file, and a prompt
#                        that carries no host, user, path or clock.
#   the pane divider     the display-panes cases need two panes, so they carry
#                        a vertical divider. Its COLOUR is not this surface's:
#                        tui-screen-diff.sh records it under BORDER_STYLE_REASON
#                        (the pin draws themegreen over a default ground, the
#                        raw TUI its own blue over an explicit ground), inside
#                        the recorded presentation:tui-status-row-theme-defaults
#                        decision. So in those cases, and only there, the SGR
#                        immediately around each divider glyph is stripped from
#                        BOTH captures before they are compared. The divider
#                        glyph itself, every column, the big digits, the
#                        labels and every other cell's style are compared.
# Nothing else is masked. Anything not in that list is compared.
#
# SETTLED CHECKPOINTS, MARKED BEFORE THE STATE. Every surface here swallows the
# keystroke a marker is typed with, so each case types its marker into the pane
# FIRST, waits for it to settle, and only then opens the surface. The marker
# stays on the screen underneath (the surfaces are placed clear of row 1), so
# every later settle is the same bounded wait: marker present AND the screen
# unchanged between two polls. Every state also has its own observable, waited
# for before the settle, so no comparison is taken while one side has the state
# and the other does not:
#   prompt, confirm      the prompt text on the status row
#   menu                 its title on the screen
#   popup                its title, then the job's own POPUP-BODY line
#   display-panes        the big digits are cells, not text, so the observable
#                        is the screen CHANGING from the marked one on both
#                        sides and then settling
#   message              the message text on the status row
#   a resize             #{client_width}x#{client_height} on the inner client
# Surfaces are opened through the client's own key table (prefix, then a bound
# key), because display-menu and command-prompt from a one-shot client do not
# return until the surface is answered.
#
# MODES. `same` asserts the whole decoded screen and the cursor. `text` asserts
# every glyph, every column, the cursor and the style of every row except the
# prompt/message row (and, under status-position top, the row after it, which
# only carries the SGR capture-pane continues from row 0), and records only that
# row's style: the prompt and message STYLE is the modes lane's this cycle
# (TUI-004, message-style and message-command-style), so a case red only on
# that row's style says SIBLING:modes. `record` asserts nothing and has to say
# why.
#
# ODD SIZE AND USER STYLES. The pin centres a popup and a -x C -y C menu on the
# client's full height (cmd_display_menu_get_pos: tty->sy), which at an even
# height rounds to the same row as the window's height and at an odd height
# does not, and a user style with a foreground and no background leaves the
# terminal's default ground under the cell. So the last section resizes to
# 79x23 and opens a centred menu, a centred -M menu chosen by a click and a
# centred popup of the default size, first with menu-style,
# menu-selected-style, menu-border-style, popup-style and popup-border-style
# set fg-only on both sides and then set fg plus bg, and shows display-panes
# with display-panes-colour and display-panes-active-colour set to
# non-default values on both sides.
#
# NO INPUT REACHES A COVERED PANE. Three cases type a key the surface does not
# answer - `z` into an open menu, `z` into an open popup's job, `Z` into
# display-panes - and the restoration comparison afterwards covers the pane the
# surface sat on: a key that leaked would be on that pane's prompt line.
#
# --self-check runs the driver against a deliberate one-sided difference in each
# channel - a menu item only one side has, a popup border only one side draws
# rounded, a prompt cursor only one side moves, a keystroke only one side's
# covered pane receives, an fg-only menu-border-style only one side sets
# (compared the way `text` compares, with the message row left out, so the
# exclusion cannot hide a surface's style), and a display-panes-active-colour
# only one side sets - and requires the comparison to report each one, plus one
# equivalence it must NOT report. A fixture that only passes has proved
# nothing.
#
# A divergence is a finding: the script exits 1 so a caller can gate on it, and
# prints both sides so the next lane has the measurement. It reaps every server
# and daemon it starts.
set -eEuo pipefail

usage() {
  printf 'usage: compat/tui-overlays.sh [--self-check] [ZZ_BIN [TMUX_BIN]]\n' >&2
  printf '       ZZ_BIN=path TMUX_BIN=path compat/tui-overlays.sh\n' >&2
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
SCRATCH_DIR="$(mktemp -d /tmp/zzov.XXXXXX)"
TOKEN="${SCRATCH_DIR##*.}"
OUTER_SOCKET_NAME="zzovo-$TOKEN"
INNER_SOCKET_NAME="zzovi-$TOKEN"
ZZ_SOCKET="/tmp/zzov-$TOKEN.sock"
OUTER_SESSION="driver"
INNER_SESSION="ovl"
WINDOW_NAME="win"
PANE_TITLE="ovltitle"
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
EXCLUDE_ROWS=""
STATUS_TOP=0
MESSAGE_HOLD_MS=20000
INNER_SHELL="ENV= PS1='\$ ' exec /bin/sh"
POPUP_JOB="printf 'POPUP-BODY\\n'; read line; printf 'POPUP-GOT-%s\\n' \"\$line\"; read line"
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

# Every server this script made, plus any daemon a scrubbed zz invocation
# autostarted under the scratch HOME, is reaped on the way out.
cleanup() {
  local status=$?
  local pid
  trap - EXIT ERR INT TERM
  set +e
  tmux_outer_command kill-server >/dev/null 2>&1
  zz_command kill-server >/dev/null 2>&1
  tmux_inner_command kill-server >/dev/null 2>&1
  if [ -n "$ZZ_PID" ]; then
    kill "$ZZ_PID" >/dev/null 2>&1
    wait "$ZZ_PID" >/dev/null 2>&1
  fi
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
  local label="$1"
  local side
  printf 'wait that ran out: %s (case %s)\n' "$label" "${CASE_LABEL:-none yet}" >&2
  for side in zz tmux; do
    printf -- '--- %s screen ---\n' "$side" >&2
    tmux_outer_command capture-pane -p -t "=$OUTER_SESSION:$side" 2>&1 | cat -v >&2 || true
    printf -- '--- %s clients ---\n' "$side" >&2
    side_command "$side" list-clients -F '#{client_name} #{client_prefix} #{client_width}x#{client_height}' >&2 2>&1 || true
    printf -- '--- %s panes ---\n' "$side" >&2
    side_command "$side" list-panes -t "=$INNER_SESSION" -F '#{pane_id} active=#{pane_active} in_mode=#{pane_in_mode}' >&2 2>&1 || true
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

DIVIDER_RULE=0
DIVIDER_GLYPH=$'\xe2\x94\x82'
capture_screen() {
  if [ "$DIVIDER_RULE" -eq 1 ]; then
    tmux_outer_command capture-pane -p -e -S 0 -E "$((ROWS_UNDER_TEST - 1))" \
      -t "=$OUTER_SESSION:$1" |
      LC_ALL=C sed -E "s/(\x1b\[[0-9;:]*m)*$DIVIDER_GLYPH(\x1b\[[0-9;:]*m)*/$DIVIDER_GLYPH/g"
    return
  fi
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
first_row() {
  capture_plain "$1" | head -n 1
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
client_size_is() {
  [ "$(side_command "$1" list-clients -F '#{client_width}x#{client_height}' 2>/dev/null | head -n 1)" = "$2" ]
}
active_pane() {
  side_command "$1" list-panes -t "=$INNER_SESSION" -F '#{pane_active} #{pane_id}' |
    awk '$1 == 1 { print $2; exit }'
}
active_pane_index_is() {
  [ "$(side_command "$1" list-panes -t "=$INNER_SESSION" -F '#{pane_active} #{pane_index}' 2>/dev/null |
    awk '$1 == 1 { print $2; exit }')" = "$2" ]
}

write_attach() {
  local side="$1"
  local destination="$2"
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
  side_command zz set-option -g "$1" "$2" || die "zz refused set-option -g $1"
  side_command tmux set-option -g "$1" "$2" || die "tmux refused set-option -g $1"
}
run_on_both() {
  side_command zz "$@" || die "zz refused $1"
  side_command tmux "$@" || die "tmux refused $1"
}
client_on_both() {
  local side client argument
  local -a arguments
  for side in zz tmux; do
    client="$(client_name "$side")"
    [ -n "$client" ] || die "$side has no client"
    arguments=()
    for argument in "$@"; do
      if [ "$argument" = CLIENT ]; then
        arguments+=("$client")
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
type_on() {
  local side="$1"
  shift
  tmux_outer_command send-keys -t "=$OUTER_SESSION:$side" "$@" ||
    die "the outer tmux refused send-keys for $side"
}
type_on_both() {
  type_on zz "$@"
  type_on tmux "$@"
}
press_on_both() {
  type_on_both C-b
  type_on_both "$1"
}

bind_on_both() {
  run_on_both bind-key -T prefix M display-menu -x 4 -y 12 -T OVERLAY-MENU \
    'First item' f 'set-option -g @overlay_menu first' \
    '' \
    'Second item' s 'set-option -g @overlay_menu second' \
    '-Disabled item' d 'set-option -g @overlay_menu disabled' \
    'Third item' t 'set-option -g @overlay_menu third'
  run_on_both bind-key -T prefix N display-menu -M -x 4 -y 12 -T OVERLAY-MENU \
    'First item' f 'set-option -g @overlay_menu first' \
    '' \
    'Second item' s 'set-option -g @overlay_menu second' \
    '-Disabled item' d 'set-option -g @overlay_menu disabled' \
    'Third item' t 'set-option -g @overlay_menu third'
  run_on_both bind-key -T prefix P display-popup -w 34 -h 9 -T OVERLAY-POPUP -E "$POPUP_JOB"
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
  set_on_both default-shell /bin/sh
  set_on_both mouse on
  bind_on_both
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

resize_both_to() {
  local columns="$1"
  local rows="$2"
  tmux_outer_command resize-window -t "=$OUTER_SESSION:zz" -x "$columns" -y "$rows" ||
    die 'the outer tmux refused resize-window'
  tmux_outer_command resize-window -t "=$OUTER_SESSION:tmux" -x "$columns" -y "$rows" ||
    die 'the outer tmux refused resize-window'
  COLUMNS_UNDER_TEST="$columns"
  ROWS_UNDER_TEST="$rows"
  wait_for "outer zz pane at ${columns}x${rows}" outer_pane_is "=$OUTER_SESSION:zz" "${columns}x${rows}"
  wait_for "outer tmux pane at ${columns}x${rows}" outer_pane_is "=$OUTER_SESSION:tmux" "${columns}x${rows}"
  wait_for "zz client at ${columns}x${rows}" client_size_is zz "${columns}x${rows}"
  wait_for "tmux client at ${columns}x${rows}" client_size_is tmux "${columns}x${rows}"
}

screen_has() {
  capture_plain "$1" | grep -Fq "$2"
}
screen_lacks() {
  screen_has "$1" "$2" && return 1
  return 0
}
last_row_has() {
  case "$(last_row "$1")" in
  *"$2"*) return 0 ;;
  *) return 1 ;;
  esac
}
last_row_lacks() {
  last_row_has "$1" "$2" && return 1
  return 0
}
last_row_starts_with() {
  case "$(last_row "$1")" in
  "$2"*) return 0 ;;
  *) return 1 ;;
  esac
}
both_last_row_starts_with() {
  wait_for "$2 on the zz status row" last_row_starts_with zz "$1"
  wait_for "$2 on the tmux status row" last_row_starts_with tmux "$1"
}
first_row_has() {
  case "$(first_row "$1")" in
  *"$2"*) return 0 ;;
  *) return 1 ;;
  esac
}
screen_differs_from() {
  [ "$(capture_screen "$1")" != "$2" ]
}
option_is() {
  [ "$(side_command "$1" show-options -gqv "$2" 2>/dev/null)" = "$3" ]
}
both_screen_has() {
  wait_for "$2 on the zz screen" screen_has zz "$1"
  wait_for "$2 on the tmux screen" screen_has tmux "$1"
}
both_screen_lacks() {
  wait_for "$2 gone from the zz screen" screen_lacks zz "$1"
  wait_for "$2 gone from the tmux screen" screen_lacks tmux "$1"
}
both_last_row_has() {
  wait_for "$2 on the zz status row" last_row_has zz "$1"
  wait_for "$2 on the tmux status row" last_row_has tmux "$1"
}
both_last_row_lacks() {
  wait_for "$2 gone from the zz status row" last_row_lacks zz "$1"
  wait_for "$2 gone from the tmux status row" last_row_lacks tmux "$1"
}
both_option_is() {
  wait_for "$3 on zz" option_is zz "$1" "$2"
  wait_for "$3 on tmux" option_is tmux "$1" "$2"
}

wait_settled() {
  local side="$1"
  local marker="$2"
  local label="$3"
  local previous=""
  local current attempt
  for ((attempt = 0; attempt < 200; attempt++)); do
    current="$(capture_screen "$side" 2>/dev/null || true)"
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
mark_both() {
  local name="$1"
  send_both "printf '\\033[H\\033[2JMARK-%s\\n' $name"
  settle_both "MARK-$name" "$name"
}

compare_rows() {
  local name="$1"
  local styled="$2"
  local zz_rows tmux_rows zz_cursor tmux_cursor index differing total count
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
  count=0
  for ((index = 0; index < total; index++)); do
    case " $EXCLUDE_ROWS " in
    *" $index "*)
      zz_rows[index]=""
      tmux_rows[index]=""
      ;;
    esac
    if [ "${zz_rows[index]-}" != "${tmux_rows[index]-}" ]; then
      [ "$differing" -ge 0 ] || differing="$index"
      count=$((count + 1))
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
    printf '      %s of %s rows differ, first %s\n' "$count" "$total" "$differing"
    for ((index = 0; index < total; index++)); do
      if [ "${zz_rows[index]-}" != "${tmux_rows[index]-}" ]; then
        printf '        row %s tmux: %s\n' "$index" "$(printf '%s' "${tmux_rows[index]-}" | cat -v)"
        printf '        row %s zz:   %s\n' "$index" "$(printf '%s' "${zz_rows[index]-}" | cat -v)"
      fi
    done
  else
    printf '      all %s rows identical\n' "$total"
  fi
  printf '      cursor tmux: %s\n' "$tmux_cursor"
  printf '      cursor zz:   %s\n' "$zz_cursor"
  return 1
}

message_rows() {
  if [ "$STATUS_TOP" -eq 1 ]; then
    printf '0 1'
  else
    printf '%s' "$((ROWS_UNDER_TEST - 1))"
  fi
}

verdict() {
  local name="$1"
  local mode="$2"
  local reason="${3:-}"
  if [ "$mode" = text ]; then
    CHECKS=$((CHECKS + 1))
    RECORDS=$((RECORDS + 1))
    [ -n "$reason" ] || die "recorded style at $name says nothing about why"
    local asserted=1
    compare_rows "$name" plain || asserted=0
    EXCLUDE_ROWS="$(message_rows)"
    compare_rows "$name" styled || asserted=0
    EXCLUDE_ROWS=""
    if [ "$asserted" -eq 1 ]; then
      printf 'ok    %s: every glyph, column, the cursor and every row style but the message row identical\n' "$name"
    else
      FAILURES=$((FAILURES + 1))
      printf 'DIFF  %s\n' "$name"
    fi
    if compare_rows "$name" styled >/dev/null; then
      printf 'note  %s: styles identical too, the record can close\n' "$name"
    else
      compare_rows "$name" styled || true
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

# --- the cases -------------------------------------------------------------
#
# THE COMMAND PROMPT. status.c status_prompt_redraw draws the prompt on the
# message line (status_prompt_line_at) inside status_message_area, and
# prompt_draw scrolls the input horizontally once it outgrows that area. The
# cursor is the prompt's own (status_prompt_cursor). Cases: opened, typed, a
# long input past the width with the cursor moved back into it, a size change
# while open, the prompt at the top when status-position is top, and cancelled.
PROMPT_STYLE='SIBLING:modes the prompt row style is message-style and message-command-style, which TUI-004 publishes and consumes this cycle'
prompt_case() {
  CASE_LABEL=command-prompt
  mark_both prompt
  press_on_both ':'
  wait_for 'the zz prompt' last_row_has zz ':'
  wait_for 'the tmux prompt' last_row_has tmux ':'
  settle_both MARK-prompt 'the command prompt'
  verdict prompt-opened text "$PROMPT_STYLE"
  type_on_both -l 'display-message PROMPTTEXT'
  both_last_row_has 'PROMPTTEXT' 'the typed command'
  settle_both MARK-prompt 'the typed command'
  verdict prompt-typed text "$PROMPT_STYLE"
  type_on_both -l ' abcdefghijklmnopqrstuvwxyz0123456789 abcdefghijklmnopqrstuvwxyz0123456789 END'
  wait_for 'the long command on the tmux status row' last_row_has tmux 'END'
  settle_both MARK-prompt 'the long command'
  verdict prompt-wrapped text "$PROMPT_STYLE"
  type_on_both Home
  type_on_both Right Right Right
  settle_both MARK-prompt 'the cursor moved into the long command'
  verdict prompt-cursor-home text "$PROMPT_STYLE"
  resize_both_to 60 20
  settle_both MARK-prompt 'the prompt at 60x20'
  verdict prompt-resized text "$PROMPT_STYLE"
  resize_both_to 80 24
  settle_both MARK-prompt 'the prompt back at 80x24'
  verdict prompt-resized-back text "$PROMPT_STYLE"
  type_on_both Escape
  both_last_row_starts_with 'L' 'the cancelled prompt'
  settle_both MARK-prompt 'the cancelled prompt'
  verdict prompt-cancelled same

  set_on_both status-position top
  STATUS_TOP=1
  mark_both top
  press_on_both ':'
  type_on_both -l 'TOPPROMPT'
  wait_for 'the zz prompt at the top' first_row_has zz 'TOPPROMPT'
  wait_for 'the tmux prompt at the top' first_row_has tmux 'TOPPROMPT'
  settle_both MARK-top 'the prompt at the top'
  verdict prompt-status-top text "$PROMPT_STYLE"
  type_on_both Escape
  wait_for 'the zz top prompt cancelled' screen_lacks zz 'TOPPROMPT'
  wait_for 'the tmux top prompt cancelled' screen_lacks tmux 'TOPPROMPT'
  settle_both MARK-top 'the top prompt cancelled'
  verdict prompt-status-top-cancelled same
  set_on_both status-position bottom
  STATUS_TOP=0
}

# CONFIRM-BEFORE is a prompt too (status_prompt_set with PROMPT_SINGLE), so its
# row style is the same sibling's. -b returns at once, so it can be raised from
# a one-shot client. `n` refuses, and the restoration is asserted whole.
confirm_case() {
  CASE_LABEL=confirm-before
  mark_both confirm
  client_on_both confirm-before -b -t CLIENT -p 'OVERLAY-CONFIRM? ' \
    'set-option -g @overlay_confirm yes'
  both_last_row_has 'OVERLAY-CONFIRM?' 'the confirm prompt'
  settle_both MARK-confirm 'the confirm prompt'
  verdict confirm-opened text "$PROMPT_STYLE"
  resize_both_to 60 20
  settle_both MARK-confirm 'the confirm prompt at 60x20'
  verdict confirm-resized text "$PROMPT_STYLE"
  resize_both_to 80 24
  settle_both MARK-confirm 'the confirm prompt back at 80x24'
  type_on_both n
  both_last_row_lacks 'OVERLAY-CONFIRM?' 'the refused confirm prompt'
  settle_both MARK-confirm 'the refused confirm prompt'
  verdict confirm-refused same
}

# DISPLAY-MENU, bound to prefix M: -x 4 -y 12, a -T title, shortcut keys, a
# separator and a disabled row. menu.c menu_draw_cb draws screen_write_box and
# screen_write_menu into the menu's own screen and copies it to the tty at
# px,py; the cursor is whatever the client's reset state leaves.
menu_case() {
  CASE_LABEL=display-menu
  mark_both menu
  press_on_both M
  both_screen_has OVERLAY-MENU 'the menu'
  settle_both MARK-menu 'the menu'
  verdict menu-opened same
  type_on_both Down
  settle_both MARK-menu 'the menu after Down'
  verdict menu-down same
  type_on_both Down
  settle_both MARK-menu 'the menu after Down past the separator'
  verdict menu-down-separator same
  type_on_both z
  settle_both MARK-menu 'the menu after an unanswered key'
  verdict menu-unanswered-key same
  side_command zz display-message -c "$(client_name zz)" 'OVERLAY-MESSAGE' ||
    die 'zz refused display-message'
  side_command tmux display-message -c "$(client_name tmux)" 'OVERLAY-MESSAGE' ||
    die 'tmux refused display-message'
  both_last_row_has OVERLAY-MESSAGE 'the message over the menu'
  settle_both MARK-menu 'the message over the menu'
  verdict menu-under-message text "$PROMPT_STYLE"
  resize_both_to 100 30
  settle_both MARK-menu 'the menu at 100x30'
  verdict menu-resized text "$PROMPT_STYLE"
  resize_both_to 80 24
  settle_both MARK-menu 'the menu back at 80x24'
  type_on_both Escape
  both_screen_lacks OVERLAY-MENU 'the cancelled menu'
  settle_both MARK-menu 'the cancelled menu'
  verdict menu-cancelled same

  mark_both menukey
  press_on_both M
  both_screen_has OVERLAY-MENU 'the menu'
  settle_both MARK-menukey 'the menu'
  type_on_both s
  both_option_is @overlay_menu second 'the s shortcut'
  both_screen_lacks OVERLAY-MENU 'the menu closed by its shortcut'
  settle_both MARK-menukey 'the menu closed by its shortcut'
  verdict menu-shortcut-closed same

  mark_both menuclick
  press_on_both N
  both_screen_has OVERLAY-MENU 'the -M menu'
  settle_both MARK-menuclick 'the -M menu'
  verdict menu-mouse-opened same
  click_item_on_both 'Third item'
  both_option_is @overlay_menu third 'the clicked third item'
  both_screen_lacks OVERLAY-MENU 'the menu closed by the click'
  settle_both MARK-menuclick 'the menu closed by the click'
  verdict menu-click-closed same
}

# A button-1 press and release at the first cell of TEXT as the pin's screen
# draws it, written as SGR mouse reports into both outer panes. The comparison
# before this has already asserted both menus sit on the same cells.
click_item_on_both() {
  local text="$1"
  local row column line index
  row=""
  index=0
  while IFS= read -r line; do
    if [[ "$line" == *"$text"* ]]; then
      row="$index"
      line="${line%%"$text"*}"
      column="${#line}"
      break
    fi
    index=$((index + 1))
  done < <(capture_plain tmux)
  [ -n "$row" ] || die "no $text on the tmux screen to click"
  type_on_both -l "$(printf '\033[<0;%s;%sM' "$((column + 1))" "$((row + 1))")"
  type_on_both -l "$(printf '\033[<0;%s;%sm' "$((column + 1))" "$((row + 1))")"
}

# DISPLAY-POPUP, bound to prefix P: -w 34 -h 9, -T title and -E, over a job that
# prints POPUP-BODY and reads two lines. popup.c popup_draw_cb draws the box
# with the title, copies the job's screen inside it and places the job's cursor.
# `z` typed into the popup goes to the job, never to the pane under it.
popup_case() {
  CASE_LABEL=display-popup
  mark_both popup
  press_on_both P
  both_screen_has OVERLAY-POPUP 'the popup'
  both_screen_has POPUP-BODY 'the popup job'
  settle_both MARK-popup 'the popup'
  verdict popup-opened same
  type_on_both z Enter
  both_screen_has POPUP-GOT-z 'the popup job read z'
  settle_both MARK-popup 'the popup after a line'
  verdict popup-typed same
  side_command zz display-message -c "$(client_name zz)" 'OVERLAY-MESSAGE' ||
    die 'zz refused display-message'
  side_command tmux display-message -c "$(client_name tmux)" 'OVERLAY-MESSAGE' ||
    die 'tmux refused display-message'
  both_last_row_has OVERLAY-MESSAGE 'the message over the popup'
  settle_both MARK-popup 'the message over the popup'
  verdict popup-under-message text "$PROMPT_STYLE"
  resize_both_to 100 30
  settle_both MARK-popup 'the popup at 100x30'
  verdict popup-resized text "$PROMPT_STYLE"
  resize_both_to 80 24
  settle_both MARK-popup 'the popup back at 80x24'
  type_on_both Enter
  both_screen_lacks OVERLAY-POPUP 'the closed popup'
  settle_both MARK-popup 'the closed popup'
  verdict popup-closed same
}

# DISPLAY-PANES over a horizontal split. cmd-display-panes.c
# cmd_display_panes_draw_pane draws each pane's index centred in the pane, in
# the window_clock_table digits when the pane is large enough, in
# display-panes-active-colour for the active pane and display-panes-colour for
# the rest, with display-panes-format beneath. A digit selects that pane; `Z` is
# not a pane key, closes the labels and goes on to the pane (the -1 return of
# cmd_display_panes_key falls through server_client_clear_overlay to the key
# table). A size change clears the labels without delivering any key:
# display-panes registers no resize callback, and server-client.c:2465 clears
# an overlay whose overlay_resize is NULL.
#
# THE RESIZE HERE CHANGES THE HEIGHT ONLY, for two measured reasons that belong
# to the pane-geometry record and not to this surface. Growing an 80-column -h
# split to 100 columns gives the pin 50|49 and zz 49|50 (the divider one column
# apart), and shrinking back leaves zz's left pane showing scrollback lines the
# pin's does not. Both were measured 2026-09-10 on this pair and are reported
# with the lane's notes; a height-only change keeps the divider where it is, and
# the case re-marks after the round trip so no later comparison carries the
# residue.
display_panes_case() {
  CASE_LABEL=display-panes
  run_on_both split-window -h -t "=$INNER_SESSION:0.0" "$INNER_SHELL"
  run_on_both select-pane -t "=$INNER_SESSION:0.0"
  DIVIDER_RULE=1
  mark_both panes
  verdict panes-split same
  local zz_before tmux_before
  zz_before="$(capture_screen zz)"
  tmux_before="$(capture_screen tmux)"
  client_on_both display-panes -b -d 0 -t CLIENT
  wait_for 'the zz labels' screen_differs_from zz "$zz_before"
  wait_for 'the tmux labels' screen_differs_from tmux "$tmux_before"
  settle_both MARK-panes 'the pane labels'
  verdict panes-shown same
  resize_both_to 80 30
  settle_both MARK-panes 'the pane labels at 80x30'
  verdict panes-resized same
  resize_both_to 80 24
  mark_both panesback
  zz_before="$(capture_screen zz)"
  tmux_before="$(capture_screen tmux)"
  client_on_both display-panes -b -d 0 -t CLIENT
  wait_for 'the zz labels again' screen_differs_from zz "$zz_before"
  wait_for 'the tmux labels again' screen_differs_from tmux "$tmux_before"
  settle_both MARK-panes 'the pane labels again'
  type_on_both 1
  wait_for 'zz selected pane 1' active_pane_index_is zz 1
  wait_for 'tmux selected pane 1' active_pane_index_is tmux 1
  settle_both MARK-panes 'the digit selection'
  verdict panes-selected same

  mark_both panesz
  zz_before="$(capture_screen zz)"
  tmux_before="$(capture_screen tmux)"
  client_on_both display-panes -b -d 0 -t CLIENT
  wait_for 'the zz labels' screen_differs_from zz "$zz_before"
  wait_for 'the tmux labels' screen_differs_from tmux "$tmux_before"
  settle_both MARK-panesz 'the pane labels'
  type_on_both Z
  settle_both MARK-panesz 'the labels closed by a non-pane key'
  verdict panes-closed-by-key same
  DIVIDER_RULE=0
}

# ODD SIZE AND USER STYLES. cmd_display_menu_get_pos centres on tty->sy, the
# client's full height, and clamps against it, so at 79x23 a centred menu and a
# centred popup sit where (sy - 1) / 2 + h / 2 - h puts them. menu.c and
# popup.c leave a user style's missing background as the terminal's default
# ground. The split display-panes left behind is closed first and the pane
# re-marked after the resize, so no residue of resizing a -h split reaches a
# comparison. The popup takes display-popup's default size, half the client
# each way. The display-panes part runs last because its divider rule strips
# the SGR around every vertical line, a menu border's included.
CENTRE_MENU_ITEMS=(
  'First item' f 'set-option -g @overlay_menu first'
  ''
  'Second item' s 'set-option -g @overlay_menu second'
  'Third item' t 'set-option -g @overlay_menu third'
)
style_on_both() {
  set_on_both menu-style "$1"
  set_on_both menu-selected-style "$2"
  set_on_both menu-border-style "$3"
  set_on_both popup-style "$4"
  set_on_both popup-border-style "$5"
}
centred_surfaces() {
  local label="$1"
  mark_both "$label"
  press_on_both C
  both_screen_has CENTRE-MENU 'the centred menu'
  settle_both "MARK-$label" 'the centred menu'
  verdict "centre-menu-$label" same
  type_on_both Down
  settle_both "MARK-$label" 'the centred menu after Down'
  verdict "centre-menu-$label-down" same
  type_on_both Escape
  both_screen_lacks CENTRE-MENU 'the cancelled centred menu'
  settle_both "MARK-$label" 'the cancelled centred menu'
  press_on_both O
  both_screen_has CENTRE-POPUP 'the centred popup'
  both_screen_has POPUP-BODY 'the centred popup job'
  settle_both "MARK-$label" 'the centred popup'
  verdict "centre-popup-$label" same
  type_on_both z Enter
  both_screen_has POPUP-GOT-z 'the centred popup job read z'
  settle_both "MARK-$label" 'the centred popup after a line'
  verdict "centre-popup-$label-typed" same
  type_on_both Enter
  both_screen_lacks CENTRE-POPUP 'the closed centred popup'
  settle_both "MARK-$label" 'the closed centred popup'
  verdict "centre-popup-$label-closed" same
}
odd_size_case() {
  CASE_LABEL=odd-size
  run_on_both kill-pane -t "=$INNER_SESSION:0.1"
  resize_both_to 79 23
  run_on_both bind-key -T prefix C display-menu -x C -y C -T CENTRE-MENU "${CENTRE_MENU_ITEMS[@]}"
  run_on_both bind-key -T prefix D display-menu -M -x C -y C -T CENTRE-MENU "${CENTRE_MENU_ITEMS[@]}"
  run_on_both bind-key -T prefix O display-popup -T CENTRE-POPUP -E "$POPUP_JOB"
  style_on_both 'fg=colour33' 'fg=colour226' 'fg=colour208' 'fg=colour159' 'fg=colour46'
  centred_surfaces fg
  style_on_both 'fg=colour33,bg=colour17' 'fg=colour16,bg=colour226' 'fg=colour208,bg=colour52' \
    'fg=colour159,bg=colour17' 'fg=colour46,bg=colour22'
  centred_surfaces fgbg

  set_on_both @overlay_menu none
  mark_both click
  press_on_both D
  both_screen_has CENTRE-MENU 'the centred -M menu'
  settle_both MARK-click 'the centred -M menu'
  verdict centre-menu-mouse-opened same
  click_item_on_both 'Third item'
  both_option_is @overlay_menu third 'the clicked third item of the centred menu'
  both_screen_lacks CENTRE-MENU 'the centred menu closed by the click'
  settle_both MARK-click 'the centred menu closed by the click'
  verdict centre-menu-click-closed same

  set_on_both display-panes-colour colour33
  set_on_both display-panes-active-colour colour124
  run_on_both split-window -h -t "=$INNER_SESSION:0.0" "$INNER_SHELL"
  run_on_both select-pane -t "=$INNER_SESSION:0.0"
  DIVIDER_RULE=1
  mark_both colours
  local zz_before tmux_before
  zz_before="$(capture_screen zz)"
  tmux_before="$(capture_screen tmux)"
  client_on_both display-panes -b -d 0 -t CLIENT
  wait_for 'the zz coloured labels' screen_differs_from zz "$zz_before"
  wait_for 'the tmux coloured labels' screen_differs_from tmux "$tmux_before"
  settle_both MARK-colours 'the coloured pane labels'
  verdict panes-coloured-shown same
  type_on_both 1
  wait_for 'zz selected pane 1 from the coloured labels' active_pane_index_is zz 1
  wait_for 'tmux selected pane 1 from the coloured labels' active_pane_index_is tmux 1
  settle_both MARK-colours 'the coloured labels closed by a digit'
  verdict panes-coloured-selected same
  DIVIDER_RULE=0
}

write_attach zz "$SCRATCH_DIR/attach-zz.sh"
write_attach tmux "$SCRATCH_DIR/attach-tmux.sh"

zz_command -f /dev/null daemon >"$SCRATCH_DIR/zz-daemon.out" 2>"$SCRATCH_DIR/zz-daemon.err" &
ZZ_PID=$!
wait_for "zz daemon socket" test -S "$ZZ_SOCKET"

run_cases() {
  printf 'overlay differential at %sx%s (pin %s)\n' \
    "$COLUMNS_UNDER_TEST" "$ROWS_UNDER_TEST" "$(basename -- "$TMUX_BIN")"
  attach_both_at 80 24
  CASE_LABEL=baseline
  mark_both baseline
  verdict baseline same
  prompt_case
  confirm_case
  menu_case
  popup_case
  display_panes_case
  odd_size_case

  if [ "$FAILURES" -ne 0 ]; then
    printf '%s of %s asserted comparisons differ, %s recorded\n' "$FAILURES" "$CHECKS" "$RECORDS"
    exit 1
  fi
  printf 'all %s asserted comparisons identical, %s recorded not asserted\n' "$CHECKS" "$RECORDS"
}

# --- self-check ------------------------------------------------------------
SELF_CHECK_FAILURES=0

self_check_case() {
  local name="$1"
  local expectation="$2"
  local outcome=ok
  case "$expectation" in
  rows)
    [ "$LAST_ROWS_DIFFERED" -eq 1 ] || outcome='no row difference reported'
    ;;
  cursor)
    [ "$LAST_ROWS_DIFFERED" -eq 0 ] || outcome='a row difference where only the cursor was planted'
    [ "$LAST_CURSOR_DIFFERED" -eq 1 ] || outcome='no cursor difference reported'
    ;;
  none)
    if [ "$LAST_ROWS_DIFFERED" -ne 0 ] || [ "$LAST_CURSOR_DIFFERED" -ne 0 ]; then
      outcome='reported a difference where none was planted'
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
  printf 'self-check: one deliberate difference per channel, plus one equivalence\n'
  attach_both_at 80 24

  CASE_LABEL='self-check menu item'
  mark_both item
  side_command zz bind-key -T prefix M display-menu -x 4 -y 12 -T OVERLAY-MENU \
    'First item' f 'set-option -g @overlay_menu first' \
    '' \
    'Second item' s 'set-option -g @overlay_menu second' \
    '-Disabled item' d 'set-option -g @overlay_menu disabled' \
    'Third item' t 'set-option -g @overlay_menu third' \
    'Extra item' e 'set-option -g @overlay_menu extra' || die 'zz refused bind-key'
  press_on_both M
  both_screen_has OVERLAY-MENU 'the sabotaged menu'
  settle_both MARK-item 'the sabotaged menu'
  compare_rows self-check-menu-item styled || true
  self_check_case 'menu, an item only one side has' rows
  type_on_both Escape
  both_screen_lacks OVERLAY-MENU 'the sabotaged menu cancelled'
  bind_on_both

  CASE_LABEL='self-check covered pane'
  mark_both leak
  press_on_both M
  both_screen_has OVERLAY-MENU 'the menu'
  settle_both MARK-leak 'the menu'
  side_command zz send-keys -t "$(active_pane zz)" z || die 'zz refused send-keys'
  type_on_both z
  type_on_both Escape
  both_screen_lacks OVERLAY-MENU 'the menu cancelled'
  settle_both MARK-leak 'the menu cancelled'
  compare_rows self-check-covered-pane styled || true
  self_check_case 'covered pane, a keystroke only one side delivers under the menu' rows
  send_both ''

  CASE_LABEL='self-check popup border'
  mark_both border
  side_command zz bind-key -T prefix P display-popup -b rounded -w 34 -h 9 -T OVERLAY-POPUP -E "$POPUP_JOB" ||
    die 'zz refused bind-key'
  press_on_both P
  both_screen_has POPUP-BODY 'the sabotaged popup'
  settle_both MARK-border 'the sabotaged popup'
  compare_rows self-check-popup-border styled || true
  self_check_case 'popup, a border only one side draws rounded' rows
  type_on_both Enter Enter
  both_screen_lacks OVERLAY-POPUP 'the sabotaged popup closed'
  bind_on_both

  CASE_LABEL='self-check prompt cursor'
  mark_both cursor
  press_on_both ':'
  type_on_both -l 'CURSORTEXT'
  both_last_row_has CURSORTEXT 'the typed prompt'
  type_on zz Left
  type_on zz Left
  settle_both MARK-cursor 'the one-sided cursor move'
  compare_rows self-check-prompt-cursor plain || true
  self_check_case 'prompt, a cursor only one side moves' cursor
  type_on_both Escape
  both_last_row_lacks CURSORTEXT 'the prompt cancelled'

  CASE_LABEL='self-check style'
  mark_both style
  side_command zz set-option -g menu-border-style 'fg=colour208' || die 'zz refused set-option'
  press_on_both M
  both_screen_has OVERLAY-MENU 'the one-sided styled menu'
  side_command zz display-message -c "$(client_name zz)" 'OVERLAY-MESSAGE' ||
    die 'zz refused display-message'
  side_command tmux display-message -c "$(client_name tmux)" 'OVERLAY-MESSAGE' ||
    die 'tmux refused display-message'
  both_last_row_has OVERLAY-MESSAGE 'the message over the styled menu'
  settle_both MARK-style 'the one-sided styled menu'
  EXCLUDE_ROWS="$(message_rows)"
  compare_rows self-check-style styled || true
  EXCLUDE_ROWS=""
  self_check_case 'style, an fg-only menu-border-style only one side sets, message row left out' rows
  type_on_both Escape
  both_screen_lacks OVERLAY-MENU 'the styled menu cancelled'
  side_command zz set-option -gu menu-border-style || die 'zz refused set-option -gu'

  CASE_LABEL='self-check equivalence'
  mark_both equal
  press_on_both M
  both_screen_has OVERLAY-MENU 'the menu on both sides'
  settle_both MARK-equal 'the menu on both sides'
  compare_rows self-check-equivalence styled || true
  self_check_case 'equivalence: the same menu on both sides' none
  type_on_both Escape
  both_screen_lacks OVERLAY-MENU 'the equivalent menu cancelled'

  CASE_LABEL='self-check display-panes colour'
  side_command zz set-option -g display-panes-active-colour colour124 || die 'zz refused set-option'
  run_on_both split-window -h -t "=$INNER_SESSION:0.0" "$INNER_SHELL"
  run_on_both select-pane -t "=$INNER_SESSION:0.0"
  DIVIDER_RULE=1
  mark_both colour
  local zz_before tmux_before
  zz_before="$(capture_screen zz)"
  tmux_before="$(capture_screen tmux)"
  client_on_both display-panes -b -d 0 -t CLIENT
  wait_for 'the zz one-sided labels' screen_differs_from zz "$zz_before"
  wait_for 'the tmux one-sided labels' screen_differs_from tmux "$tmux_before"
  settle_both MARK-colour 'the one-sided pane colour'
  compare_rows self-check-panes-colour styled || true
  self_check_case 'display-panes, a display-panes-active-colour only one side sets' rows
  type_on_both Escape
  DIVIDER_RULE=0

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
