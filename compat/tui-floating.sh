#!/usr/bin/env bash
# Whole-screen differential for floating panes in the raw TUI against tmux 3.8.
#
# Both binaries attach inside ONE outer pinned tmux, one window each, and the
# outer tmux decodes what each inner client drew: `capture-pane -p` gives every
# cell's glyph, row by row, and the cursor tuple is the one the inner client left
# in the outer terminal. The glyphs are the contract here: where each float sits,
# which float covers which, the border lines and corners, the status text on a
# float's top border, and what is left of the panes underneath.
#
# The pane list (size, position, floating, active and modal flags) is compared
# too, except while display-popup is up: tmux 3.8's popup is a client overlay
# and zz's is a modal pane (tmux master 34cd5da4), so only the screen is the
# contract there.
#
# THE POPUP TITLE COLUMN. tmux 3.8 draws a display-popup title with
# screen_write_box at the box's x + 2; zz's popup is tmux master's modal pane
# (34cd5da4), whose title is pane-border-format drawn at the pane's xoff + 2,
# one column further right. On the popup-titled checkpoint only, the dashes
# around the title on its top border are dropped from both captures before the
# compare, so the box, its corners and the title text are still asserted.
#
# Cases (catch-up item float.clients, knowledge/designs/floating-panes.md):
#   overlap          two overlapping floats over a vertical split
#   raised           select-pane raises the back float over the front one
#   click-overlap    a click on the overlap activates the front float
#   titled           a -T float under pane-border-status top-floating
#   borderless       a float with pane-border-lines none
#   popup-zoomed     display-popup over a zoomed pane
#   no-tiled         a window with no tiled pane and two floats
#   modal-click      a click outside a modal changes nothing
#   modal-close      a click outside a new-pane -O -C modal kills it
#   popup-titled     display-popup -T over a split, with the per-pane styles
#                    display-popup sets (pane-border-style, window-style)
#   clipped          a float pushed past the left and top window edges shows
#                    its own columns and rows from the clipped offset on
#   cursor-covered   a tiled pane's cursor under a float is hidden
#
# CONTROLLED VALUES, set on both sides: status-right '' and status-left L (the
# clock and the host are not this surface's), window-status-current-format and
# window-status-format '#I:#W' (tmux 3.8's display-popup is a client overlay and
# raises no window flag, while zz's is a modal pane and raises O as 3.8 does for
# new-pane -O; the flag belongs to the status row, not to the float), automatic-
# rename off, default-shell /bin/sh, mouse on, and an inner shell with no rc file
# and a bare '$ ' prompt.
set -eEuo pipefail

usage() {
  printf 'usage: compat/tui-floating.sh [ZZ_BIN [TMUX_BIN]]\n' >&2
}

COMPAT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd -- "$COMPAT_DIR/.." && pwd)"
[ "$#" -le 2 ] || { usage; exit 2; }
ZZ_INPUT="${1:-${ZZ_BIN:-$REPO_DIR/target/debug/zz_cli}}"
TMUX_INPUT="${2:-${TMUX_BIN:-${ZZ_COMPAT_TMUX:-$COMPAT_DIR/.cache/tmux-src/tmux}}}"

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
SCRATCH_DIR="$(mktemp -d /tmp/zzfl.XXXXXX)"
TOKEN="${SCRATCH_DIR##*.}"
OUTER_SOCKET_NAME="zzflo-$TOKEN"
INNER_SOCKET_NAME="zzfli-$TOKEN"
ZZ_SOCKET="/tmp/zzfl-$TOKEN.sock"
OUTER_SESSION="driver"
INNER_SESSION="flt"
WINDOW_NAME="win"
ZZ_HOME="$SCRATCH_DIR/zz-home"
TMUX_HOME="$SCRATCH_DIR/tmux-home"
OUTER_HOME="$SCRATCH_DIR/outer-home"
ZZ_LOG_DIR="$SCRATCH_DIR/zz-logs"
CASE_LABEL=""
COMPARE_FACTS=1
FAILURES=0
CHECKS=0
INNER_SHELL="ENV= PS1='\$ ' exec /bin/sh"
POPUP_JOB="printf 'POPUP-BODY\\n'; read line"
mkdir -p "$ZZ_HOME" "$TMUX_HOME" "$OUTER_HOME" "$ZZ_LOG_DIR"

float_job() {
  printf "printf '%s\\\\n'; ENV= PS1='\$ ' exec /bin/sh" "$1"
}

scrubbed() {
  env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL \
    -u XDG_STATE_HOME -u ZZ_LOG_DIR -u XDG_RUNTIME_DIR \
    TMUX_TMPDIR=/tmp LANG=C.UTF-8 LC_CTYPE=C.UTF-8 "$@"
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
  local label="$1"
  local side
  printf 'wait that ran out: %s (case %s)\n' "$label" "${CASE_LABEL:-none yet}" >&2
  for side in zz tmux; do
    printf -- '--- %s screen ---\n' "$side" >&2
    tmux_outer_command capture-pane -p -t "=$OUTER_SESSION:$side" 2>&1 | cat -v >&2 || true
    printf -- '--- %s panes ---\n' "$side" >&2
    side_command "$side" list-panes -t "=$INNER_SESSION" \
      -F '#{pane_id} #{pane_width}x#{pane_height} #{pane_x},#{pane_y} floating=#{pane_floating_flag} active=#{pane_active}' >&2 2>&1 || true
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

CURSOR_FORMAT='#{cursor_x},#{cursor_y} flag=#{cursor_flag}'

TITLE_RULE=""
capture_plain() {
  if [ -n "$TITLE_RULE" ]; then
    tmux_outer_command capture-pane -p -S 0 -E "$((ROWS_UNDER_TEST - 1))" \
      -t "=$OUTER_SESSION:$1" | sed -E "s/┌─*($TITLE_RULE)─*┐/┌\1┐/"
    return
  fi
  tmux_outer_command capture-pane -p -S 0 -E "$((ROWS_UNDER_TEST - 1))" \
    -t "=$OUTER_SESSION:$1"
}
cursor_tuple() {
  tmux_outer_command display-message -p -t "=$OUTER_SESSION:$1" "$CURSOR_FORMAT"
}
outer_pane_is() {
  [ "$(tmux_outer_command display-message -p -t "$1" '#{pane_width}x#{pane_height}' 2>/dev/null)" = "$2" ]
}
client_attached() {
  [ "$(side_command "$1" list-clients -F '#{client_session}' 2>/dev/null)" = "$INNER_SESSION" ]
}
screen_has() {
  capture_plain "$1" | grep -Fq "$2"
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
  local side="$1"
  local label="$2"
  local previous=""
  local current attempt
  for ((attempt = 0; attempt < 200; attempt++)); do
    current="$(capture_plain "$side" 2>/dev/null || true)$(cursor_tuple "$side" 2>/dev/null || true)"
    if [ -n "$previous" ] && [ "$current" = "$previous" ]; then
      return 0
    fi
    previous="$current"
    sleep 0.1
  done
  dump_state "$label"
  die "$label did not settle within 20 seconds"
}
settle_both() {
  wait_settled zz "$1 settled on the zz screen"
  wait_settled tmux "$1 settled on the tmux screen"
}

write_attach() {
  local side="$1"
  local destination="$2"
  printf '#!/usr/bin/env bash\n' >"$destination"
  if [ "$side" = zz ]; then
    printf 'exec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL -u XDG_STATE_HOME -u XDG_RUNTIME_DIR HOME=%q XDG_CONFIG_HOME=%q ZZ_LOG_DIR=%q TMUX_TMPDIR=/tmp LANG=C.UTF-8 LC_CTYPE=C.UTF-8 %q --socket %q attach-session -t %q\n' \
      "$ZZ_HOME" "$ZZ_HOME/config" "$ZZ_LOG_DIR" "$ZZ_BIN" "$ZZ_SOCKET" "=$INNER_SESSION" >>"$destination"
  else
    printf 'exec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL -u XDG_STATE_HOME -u XDG_RUNTIME_DIR HOME=%q XDG_CONFIG_HOME=%q TMUX_TMPDIR=/tmp LANG=C.UTF-8 LC_CTYPE=C.UTF-8 %q -L %q attach-session -t %q\n' \
      "$TMUX_HOME" "$TMUX_HOME/config" "$TMUX_BIN" "$INNER_SOCKET_NAME" "=$INNER_SESSION" >>"$destination"
  fi
  chmod +x "$destination"
}
write_attach zz "$SCRATCH_DIR/attach-zz.sh"
write_attach tmux "$SCRATCH_DIR/attach-tmux.sh"

set_on_both() {
  side_command zz set-option -g "$1" "$2" || die "zz refused set-option -g $1"
  side_command tmux set-option -g "$1" "$2" || die "tmux refused set-option -g $1"
}
run_on_both() {
  side_command zz "$@" || die "zz refused $*"
  side_command tmux "$@" || die "tmux refused $*"
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
send_bytes() {
  local side="$1" text="$2" hex
  hex="$(printf '%s' "$text" | od -An -tx1 | tr -s ' \n' '  ')"
  # shellcheck disable=SC2086
  tmux_outer_command send-keys -t "=$OUTER_SESSION:$side" -H $hex ||
    die "the outer tmux refused send-keys -H for $side"
}
# A left press and release at a 0-based client cell, as SGR 1006 reports.
click_both() {
  local column="$1" row="$2" side
  for side in zz tmux; do
    send_bytes "$side" "$(printf '\033[<0;%s;%sM' "$((column + 1))" "$((row + 1))")"
    send_bytes "$side" "$(printf '\033[<0;%s;%sm' "$((column + 1))" "$((row + 1))")"
  done
  sleep 0.4
}

pane_facts() {
  side_command "$1" list-panes -t "=$INNER_SESSION" \
    -F '#{pane_index} #{pane_width}x#{pane_height} #{pane_x},#{pane_y} #{pane_floating_flag}#{pane_active}#{pane_modal_flag}'
}
float_pane() {
  side_command "$1" list-panes -t "=$INNER_SESSION" -F '#{pane_index} #{pane_id}' |
    awk -v index_wanted="$2" '$1 == index_wanted { print $2; exit }'
}
tiled_pane() {
  side_command "$1" list-panes -t "=$INNER_SESSION" -F '#{pane_floating_flag} #{pane_id}' |
    awk '$1 == 0 { print $2; exit }'
}
active_index_is() {
  [ "$(side_command "$1" list-panes -t "=$INNER_SESSION" -F '#{pane_active} #{pane_index}' 2>/dev/null |
    awk '$1 == 1 { print $2; exit }')" = "$2" ]
}
pane_count_is() {
  [ "$(side_command "$1" list-panes -t "=$INNER_SESSION" -F x 2>/dev/null | wc -l)" -eq "$2" ]
}

compare_rows() {
  local name="$1"
  local zz_rows tmux_rows zz_cursor tmux_cursor index count=0 zz_facts tmux_facts
  mapfile -t zz_rows < <(capture_plain zz)
  mapfile -t tmux_rows < <(capture_plain tmux)
  zz_cursor="$(cursor_tuple zz)"
  tmux_cursor="$(cursor_tuple tmux)"
  zz_facts="$(pane_facts zz)"
  tmux_facts="$(pane_facts tmux)"
  if [ "$COMPARE_FACTS" -eq 0 ]; then
    zz_facts=""
    tmux_facts=""
  fi
  for ((index = 0; index < ROWS_UNDER_TEST; index++)); do
    [ "${zz_rows[index]-}" = "${tmux_rows[index]-}" ] || count=$((count + 1))
  done
  if [ "$count" -eq 0 ] && [ "$zz_cursor" = "$tmux_cursor" ] && [ "$zz_facts" = "$tmux_facts" ]; then
    return 0
  fi
  printf '      case %s: %s of %s rows differ\n' "$name" "$count" "$ROWS_UNDER_TEST"
  for ((index = 0; index < ROWS_UNDER_TEST; index++)); do
    if [ "${zz_rows[index]-}" != "${tmux_rows[index]-}" ]; then
      printf '        row %2s tmux: %s\n' "$index" "${tmux_rows[index]-}"
      printf '        row %2s zz:   %s\n' "$index" "${zz_rows[index]-}"
    fi
  done
  printf '      cursor tmux: %s\n      cursor zz:   %s\n' "$tmux_cursor" "$zz_cursor"
  if [ "$zz_facts" != "$tmux_facts" ]; then
    printf '      panes tmux:\n%s\n      panes zz:\n%s\n' "$tmux_facts" "$zz_facts"
  fi
  return 1
}
verdict() {
  local name="$1"
  CHECKS=$((CHECKS + 1))
  settle_both "$name"
  if compare_rows "$name"; then
    printf 'ok    %s\n' "$name"
  else
    FAILURES=$((FAILURES + 1))
    printf 'DIFF  %s\n' "$name"
  fi
}

attach_both() {
  tmux_outer_command kill-server >/dev/null 2>&1 || true
  zz_command kill-session -t "=$INNER_SESSION" >/dev/null 2>&1 || true
  tmux_inner_command kill-session -t "=$INNER_SESSION" >/dev/null 2>&1 || true
  zz_command new-session -d -s "$INNER_SESSION" -n "$WINDOW_NAME" \
    -x "$COLUMNS_UNDER_TEST" -y "$ROWS_UNDER_TEST" "$INNER_SHELL" ||
    die "could not create the zz session"
  tmux_inner_command -f /dev/null new-session -d -s "$INNER_SESSION" -n "$WINDOW_NAME" \
    -x "$COLUMNS_UNDER_TEST" -y "$ROWS_UNDER_TEST" "$INNER_SHELL" ||
    die "could not create the tmux session"
  set_on_both status-right ''
  set_on_both status-left L
  set_on_both window-status-current-format '#I:#W'
  set_on_both window-status-format '#I:#W'
  set_on_both automatic-rename off
  set_on_both default-shell /bin/sh
  set_on_both mouse on
  run_on_both bind-key -T prefix P display-popup -w 30 -h 8 -E "$POPUP_JOB"
  run_on_both bind-key -T prefix T display-popup -w 30 -h 8 -T POPUP-TITLE -E "$POPUP_JOB"
  tmux_outer_command -f /dev/null new-session -d -s "$OUTER_SESSION" -n zz \
    -x "$COLUMNS_UNDER_TEST" -y "$ROWS_UNDER_TEST" "$SCRATCH_DIR/attach-zz.sh" ||
    die "could not create the outer session"
  tmux_outer_command set-option -g status off
  tmux_outer_command new-window -d -n tmux "$SCRATCH_DIR/attach-tmux.sh"
  wait_for "outer zz pane" outer_pane_is "=$OUTER_SESSION:zz" "${COLUMNS_UNDER_TEST}x${ROWS_UNDER_TEST}"
  wait_for "outer tmux pane" outer_pane_is "=$OUTER_SESSION:tmux" "${COLUMNS_UNDER_TEST}x${ROWS_UNDER_TEST}"
  wait_for "zz client attached" client_attached zz
  wait_for "tmux client attached" client_attached tmux
}

new_float_on_both() {
  local marker="$1"
  shift
  run_on_both new-pane "$@" "$(float_job "$marker")"
  both_screen_has "$marker" "the $marker float"
}

overlap_cases() {
  CASE_LABEL=overlap
  attach_both
  run_on_both split-window -h "$INNER_SHELL"
  new_float_on_both FLOAT-A -x 30 -y 8 -X 10 -Y 3
  new_float_on_both FLOAT-B -x 30 -y 8 -X 24 -Y 7
  verdict overlap
  CASE_LABEL=raised
  run_on_both select-pane -t "$(float_pane zz 2)"
  verdict raised
  CASE_LABEL=click-overlap
  run_on_both select-pane -t "=$INNER_SESSION:0.0"
  wait_for 'the zz tiled pane active' active_index_is zz 0
  wait_for 'the tmux tiled pane active' active_index_is tmux 0
  click_both 30 9
  wait_for 'the zz front float active' active_index_is zz 2
  wait_for 'the tmux front float active' active_index_is tmux 2
  verdict click-overlap
}

titled_case() {
  CASE_LABEL=titled
  attach_both
  run_on_both split-window -h "$INNER_SHELL"
  set_on_both pane-border-status top-floating
  new_float_on_both FLOAT-T -T FLOATTITLE -x 34 -y 8 -X 20 -Y 5
  both_screen_has FLOATTITLE 'the float title'
  verdict titled
}

borderless_case() {
  CASE_LABEL=borderless
  attach_both
  run_on_both split-window -h "$INNER_SHELL"
  new_float_on_both FLOAT-N -B none -x 24 -y 6 -X 28 -Y 6
  verdict borderless
}

popup_zoomed_case() {
  CASE_LABEL=popup-zoomed
  attach_both
  run_on_both split-window -h "$INNER_SHELL"
  run_on_both resize-pane -Z
  press_on_both P
  both_screen_has POPUP-BODY 'the popup job'
  COMPARE_FACTS=0
  verdict popup-zoomed
  COMPARE_FACTS=1
  type_on_both Enter
  both_screen_lacks POPUP-BODY 'the closed popup'
  verdict popup-zoomed-closed
}

no_tiled_case() {
  CASE_LABEL=no-tiled
  attach_both
  new_float_on_both FLOAT-C -x 30 -y 8 -X 6 -Y 2
  new_float_on_both FLOAT-D -x 30 -y 8 -X 30 -Y 10
  local side
  for side in zz tmux; do
    side_command "$side" kill-pane -t "$(tiled_pane "$side")" || die "$side refused kill-pane"
  done
  wait_for 'zz down to two panes' pane_count_is zz 2
  wait_for 'tmux down to two panes' pane_count_is tmux 2
  verdict no-tiled
}

modal_cases() {
  CASE_LABEL=modal-click
  attach_both
  run_on_both split-window -h "$INNER_SHELL"
  new_float_on_both MODAL-A -O -x 30 -y 8 -X 20 -Y 5
  click_both 2 2
  verdict modal-click
  wait_for 'zz modal kept' pane_count_is zz 3
  wait_for 'tmux modal kept' pane_count_is tmux 3
  local side
  for side in zz tmux; do
    side_command "$side" kill-pane -t "$(float_pane "$side" 2)" || die "$side refused kill-pane"
  done
  both_screen_lacks MODAL-A 'the killed modal'
  CASE_LABEL=modal-close
  new_float_on_both MODAL-C -O -C -x 30 -y 8 -X 20 -Y 5
  click_both 2 2
  wait_for 'zz modal killed by the click' pane_count_is zz 2
  wait_for 'tmux modal killed by the click' pane_count_is tmux 2
  verdict modal-close
}

popup_titled_case() {
  CASE_LABEL=popup-titled
  attach_both
  run_on_both split-window -h "$INNER_SHELL"
  press_on_both T
  both_screen_has POPUP-BODY 'the titled popup job'
  both_screen_has POPUP-TITLE 'the popup title'
  COMPARE_FACTS=0
  TITLE_RULE=POPUP-TITLE
  verdict popup-titled
  TITLE_RULE=""
  COMPARE_FACTS=1
  type_on_both Enter
  both_screen_lacks POPUP-BODY 'the closed titled popup'
}

clipped_case() {
  CASE_LABEL=clipped
  attach_both
  run_on_both new-pane -x 24 -y 8 -X 10 -Y 6 \
    "printf 'R0-ABCDEFGHIJKLMNOP\\nR1-ABCDEFGHIJKLMNOP\\nR2-ABCDEFGHIJKLMNOP\\n'; exec cat"
  both_screen_has R2-ABCDEFGH 'the clipped float'
  run_on_both move-pane -X -4 -Y -2
  verdict clipped
}

cursor_covered_case() {
  CASE_LABEL=cursor-covered
  attach_both
  new_float_on_both FLOAT-Q -x 20 -y 5 -X 0 -Y 0
  run_on_both select-pane -t "=$INNER_SESSION:0.0"
  wait_for 'the zz tiled pane active' active_index_is zz 0
  wait_for 'the tmux tiled pane active' active_index_is tmux 0
  verdict cursor-covered
}

printf 'floating pane differential at %sx%s (%s)\n' \
  "$COLUMNS_UNDER_TEST" "$ROWS_UNDER_TEST" "$("$TMUX_BIN" -V)"
overlap_cases
titled_case
borderless_case
popup_zoomed_case
no_tiled_case
modal_cases
popup_titled_case
clipped_case
cursor_covered_case

if [ "$FAILURES" -ne 0 ]; then
  printf '%s of %s comparisons differ\n' "$FAILURES" "$CHECKS"
  exit 1
fi
printf 'all %s comparisons identical\n' "$CHECKS"
