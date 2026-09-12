#!/usr/bin/env bash
# Whole-screen differential for the chooser and command-output surfaces.
#
# tui-stock-keys.sh proves that prefix s, prefix w, prefix = and prefix f reach
# the right command and end in the right state. It never looked at what the
# chooser DRAWS: the pin's mode tree - the row layout with its key column and
# tree glyphs, the selection bar, the preview box under the rows, the help box,
# the chooser's own search and filter prompts - against zz's own chooser. This
# fixture compares every decoded cell of both screens plus the cursor at named
# settled checkpoints while those surfaces are up, and again once they are gone.
#
# THE DECODED SCREEN IS THE CONTRACT, NOT THE BYTE STREAM. Both binaries attach
# inside ONE outer pinned tmux, one window each, and the outer tmux is the
# decoder: `capture-pane -p -e` re-emits SGR from its own grid, so attribute
# order, batching, redundant resets and cursor-movement spelling collapse on
# both sides, while the colour CLASS does not. The driver is tui-indicators.sh's,
# which is status-row.sh's widened to the whole screen.
#
# THE SCENE, built identically on both sides before the first checkpoint:
#   session cho     window 0 `win`, the one attached, and window 1 `two`
#                   split in two. The attached window is deliberately a single
#                   pane: its borders would carry the border colour that
#                   tui-screen-diff.sh records under someone else's decision,
#                   and every restoration here is asserted whole. The split
#                   window is reached through the tree and its preview.
#   session many    MANY_WINDOWS windows m00..m11, so the window tree is longer
#                   than the rows the pin gives it and has to scroll
#   session other   one window `far`
#   every pane      titled ptitle and running INNER_SHELL, with one line of
#                   fixed output, so the preview boxes carry known cells
#   two buffers     alpha and beta, for the buffer tree
#
# CONTROLLED DYNAMIC VALUES, set on both sides and never left to chance:
#   status-right ''      the default ends in a clock and in %d-%b-%y, which the
#                        pin expands through libc strftime and zz expands
#                        locale-independently. That belongs to status-row.sh.
#   status-left L        a fixed literal, so the left of the row is asserted.
#   automatic-rename off plus a -n name on every window: the default name follows
#                        the running command and would race a checkpoint.
#   select-pane -T ptitle on EVERY pane: the pin seeds a pane title from
#                        gethostname and the default tree row and preview label
#                        both print it. zz reports the shell name through its
#                        shell integration, the recorded pane.runtime-facts
#                        decision, not a divergence of this screen.
#   buffer creation time the pin's buffer row format is `#{t/p:buffer_created}:
#                        #{buffer_sample}`, and t/p prints HH:MM for a buffer
#                        younger than a day. Both sides create their buffers in
#                        the same wall-clock minute: the fixture waits, bounded,
#                        until the clock is at least BUFFER_MINUTE_MARGIN
#                        seconds away from the next minute before it creates
#                        them, and the comparison of those cells is then exact.
#   copy-mode-position-format
#                        the pin's default begins with #{t/p:top_line_time},
#                        and run-shell's view mode draws it. Pinned on both
#                        sides to the rest of that default, as tui-indicators.sh
#                        pins it, so the indicator draws and carries no clock.
#   the inner shell      ENV= PS1='$ ' exec /bin/sh: no rc file, and a prompt
#                        that carries no host, user, path or clock.
# DECLARED, NOT PINNED: choose-client lists each client by its tty, and the two
#   inner clients run on two different outer ptys. The client case is recorded
#   (see CLIENT_REASON), so that value is never asserted.
# Nothing else is masked. Anything not in those lists is compared.
#
# SETTLED CHECKPOINTS. A chooser swallows every key typed into it, so, as in
# tui-indicators.sh, each phase marks FIRST - `printf 'MARK-%s\n' NAME` into the
# pane, settled on both screens - and only then opens its surface. After that,
# every step types its keys into both clients and waits, bounded, on each screen
# for three things: the screen differs from the one captured just before the
# keys went in, it carries the step's needle (text the PIN's surface is known to
# draw at that step), and it is unchanged between two polls. The change and the
# stillness are judged on the STYLED capture, because a selection or a tag can
# move nothing but colours; the needle is matched on the plain one, because a
# row's text crosses colour changes. The pin's side is a
# hard wait: a pin that never reaches its own needle is a broken fixture and the
# run dies with diagnostics. The zz side is the same bounded wait made soft: a zz
# screen that never draws the pin's needle is a divergence, not a hang, so the
# wait runs out, says so, and the comparison that follows reports the cells. A
# zz screen that changed and then held still for two seconds without the needle
# has settled on something else, and the soft wait stops there. A needle never
# ends in a blank: capture-pane trims trailing blanks. No wait in this file is a
# sleep.
#
# THE CURSOR is read with `display-message -p` against each OUTER pane, so it is
# the cursor the inner client left in the outer terminal. mode_tree_draw hides
# it on the selected row (mode-tree.c:1035) and a chooser prompt shows it where
# the prompt's caret is (mode-tree.c:1063), and both are compared.
#
# `z`: the pin's mode tree has no z key (mode_tree_key, mode-tree.c). What makes
# the tree take the whole window is the -Z every stock binding passes -
# choose-tree -Zs, choose-tree -Zw, choose-buffer -Z, choose-client -Z,
# find-window -Z - which zooms the pane for the mode's lifetime and puts Z in
# the window's flags. The status row is part of every screen compared here, so
# the zoom is asserted at every checkpoint, and `v` (preview off, big, normal)
# is driven as the key that resizes the tree itself. Every case but zoom_case
# opens its chooser in the single-pane attached window, where window_zoom
# refuses (window.c, one pane) and the -Z shows nothing; zoom_case splits that
# window so the zoom, the Z flag and the release on exit are all on screen.
#
# MODES. `same` asserts the whole decoded screen and the cursor. `text` asserts
# every glyph, every column and the cursor and records only the styles. `record`
# asserts nothing and has to say why. A reason that starts `SIBLING:<lane> `
# names the sibling lane of this cycle whose landing the case waits on; any
# other recorded case keeps its clause open.
#
# --self-check runs the driver against a deliberate one-sided difference in each
# channel - a tree row, a preview cell, a tag mark, the cursor, the -Z zoom and
# the chooser's own prompt row - and requires the comparison to catch each in
# that channel, plus two equivalences it must NOT report. A fixture that only
# passes has proved nothing.
set -eEuo pipefail

usage() {
  printf 'usage: compat/tui-choosers.sh [--self-check] [ZZ_BIN [TMUX_BIN]]\n' >&2
  printf '       ZZ_BIN=path TMUX_BIN=path compat/tui-choosers.sh\n' >&2
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
SCRATCH_DIR="$(mktemp -d /tmp/zzch.XXXXXX)"
TOKEN="${SCRATCH_DIR##*.}"
OUTER_SOCKET_NAME="zzcho-$TOKEN"
INNER_SOCKET_NAME="zzchi-$TOKEN"
ZZ_SOCKET="/tmp/zzch-$TOKEN.sock"
OUTER_SESSION="driver"
INNER_SESSION="cho"
WINDOW_NAME="win"
PANE_TITLE="ptitle"
MANY_WINDOWS=12
BUFFER_MINUTE_MARGIN=15
ZZ_HOME="$SCRATCH_DIR/zz-home"
TMUX_HOME="$SCRATCH_DIR/tmux-home"
OUTER_HOME="$SCRATCH_DIR/outer-home"
ZZ_LOG_DIR="$SCRATCH_DIR/zz-logs"
CASE_LABEL=""
ZZ_PID=""
FAILURES=0
CHECKS=0
RECORDS=0
SIBLINGS=0
LAST_ROWS_DIFFERED=0
LAST_CURSOR_DIFFERED=0
INNER_SHELL="ENV= PS1='\$ ' exec /bin/sh"
COPY_POSITION_FORMAT='#[align=right][#{copy_position}/#{copy_position_limit}]'
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
screen_of() {
  capture_plain "$1" 2>/dev/null || true
}
styled_screen_of() {
  capture_screen "$1" 2>/dev/null || true
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
client_prefix_is() {
  [ "$(side_command "$1" display-message -c "$(client_name "$1")" -p '#{client_prefix}' 2>/dev/null)" = "$2" ]
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
type_on_side() {
  local side="$1"
  shift
  tmux_outer_command send-keys -t "=$OUTER_SESSION:$side" "$@" ||
    die "the outer tmux refused send-keys for $side"
}
type_on_both() {
  type_on_side zz "$@"
  type_on_side tmux "$@"
}

every_pane_output_is() {
  local side="$1"
  local pane
  for pane in $(side_command "$side" list-panes -a -F '#{pane_id}'); do
    side_command "$side" capture-pane -p -t "$pane" | grep -Fq "SCENE-$2" || return 1
  done
}

build_scene() {
  local side="$1"
  local index pane name
  side_command "$side" set-option -g automatic-rename off
  side_command "$side" new-window -d -t "=$INNER_SESSION:1" -n two "$INNER_SHELL" ||
    die "$side refused new-window"
  side_command "$side" split-window -d -h -t "=$INNER_SESSION:1" "$INNER_SHELL" ||
    die "$side refused split-window"
  side_command "$side" new-session -d -s many -n m00 -x "$COLUMNS_UNDER_TEST" -y "$ROWS_UNDER_TEST" \
    "$INNER_SHELL" || die "$side refused new-session many"
  for ((index = 1; index < MANY_WINDOWS; index++)); do
    printf -v name 'm%02d' "$index"
    side_command "$side" new-window -d -t "=many:$index" -n "$name" "$INNER_SHELL" ||
      die "$side refused new-window $name"
  done
  side_command "$side" new-session -d -s other -n far -x "$COLUMNS_UNDER_TEST" -y "$ROWS_UNDER_TEST" \
    "$INNER_SHELL" || die "$side refused new-session other"
  for pane in $(side_command "$side" list-panes -a -F '#{pane_id}'); do
    side_command "$side" select-pane -t "$pane" -T "$PANE_TITLE" || die "$side refused select-pane -T"
    side_command "$side" send-keys -t "$pane" "printf 'SCENE-%s\\n' $pane" Enter ||
      die "$side refused send-keys"
  done
}

seconds_into_minute_below() {
  [ "$((10#$(date +%S)))" -lt "$1" ]
}

make_buffers() {
  local attempt
  for ((attempt = 0; attempt < 400; attempt++)); do
    seconds_into_minute_below "$((60 - BUFFER_MINUTE_MARGIN))" && break
    sleep 0.05
  done
  seconds_into_minute_below "$((60 - BUFFER_MINUTE_MARGIN))" ||
    die 'a wall-clock minute with room for both buffers did not come within 20 seconds'
  run_on_both set-buffer -b alpha 'ALPHA-ONE'
  run_on_both set-buffer -b beta "$(printf 'BETA-ONE\nBETA-TWO\nBETA-THREE')"
}

attach_both_at() {
  local columns="$1"
  local rows="$2"
  local side pane
  COLUMNS_UNDER_TEST="$columns"
  ROWS_UNDER_TEST="$rows"
  tmux_outer_command kill-server >/dev/null 2>&1 || true
  for side in zz tmux; do
    side_command "$side" kill-session -t "=$INNER_SESSION" >/dev/null 2>&1 || true
    side_command "$side" kill-session -t =many >/dev/null 2>&1 || true
    side_command "$side" kill-session -t =other >/dev/null 2>&1 || true
    side_command "$side" delete-buffer -b alpha >/dev/null 2>&1 || true
    side_command "$side" delete-buffer -b beta >/dev/null 2>&1 || true
  done
  zz_command new-session -d -s "$INNER_SESSION" -n "$WINDOW_NAME" -x "$columns" -y "$rows" \
    "$INNER_SHELL" || die "could not create the zz session"
  tmux_inner_command -f /dev/null new-session -d -s "$INNER_SESSION" -n "$WINDOW_NAME" \
    -x "$columns" -y "$rows" "$INNER_SHELL" || die "could not create the tmux session"
  set_on_both status-right ''
  set_on_both status-left L
  set_on_both automatic-rename off
  set_on_both copy-mode-position-format "$COPY_POSITION_FORMAT"
  build_scene zz
  build_scene tmux
  wait_for 'every zz pane printed its scene line' every_pane_output_is zz '%'
  wait_for 'every tmux pane printed its scene line' every_pane_output_is tmux '%'
  make_buffers
  tmux_outer_command -f /dev/null new-session -d -s "$OUTER_SESSION" -n zz \
    -x "$columns" -y "$rows" "$SCRATCH_DIR/attach-zz.sh" ||
    die "could not create the outer session"
  tmux_outer_command set-option -g status off
  tmux_outer_command new-window -d -n tmux "$SCRATCH_DIR/attach-tmux.sh"
  wait_for "outer zz pane at ${columns}x${rows}" outer_pane_is "=$OUTER_SESSION:zz" "${columns}x${rows}"
  wait_for "outer tmux pane at ${columns}x${rows}" outer_pane_is "=$OUTER_SESSION:tmux" "${columns}x${rows}"
  wait_for "zz client attached" client_attached zz
  wait_for "tmux client attached" client_attached tmux
  for side in zz tmux; do
    pane="$(active_pane "$side")"
    side_command "$side" select-pane -t "$pane" -T "$PANE_TITLE" || die "$side refused select-pane -T"
  done
}

# THE SETTLE, see the header. $2 is hard for the pin and soft for zz.
wait_screen() {
  local side="$1"
  local hardness="$2"
  local label="$3"
  local before="$4"
  local needle="$5"
  local previous="" current plain attempt still=0
  for ((attempt = 0; attempt < 200; attempt++)); do
    current="$(styled_screen_of "$side")"
    if [ -n "$previous" ] && [ "$current" = "$previous" ] &&
      { [ -z "$before" ] || [ "$current" != "$before" ]; }; then
      plain="$(screen_of "$side")"
      if [ -z "$needle" ] || printf '%s' "$plain" | grep -Fq -- "$needle"; then
        return 0
      fi
      still=$((still + 1))
      if [ "$hardness" = soft ] && [ "$still" -ge 40 ]; then
        printf 'note  %s: the %s screen changed and then held still for 2 seconds without it\n' "$label" "$side"
        return 0
      fi
    else
      still=0
    fi
    previous="$current"
    sleep 0.05
  done
  if [ "$hardness" = hard ]; then
    dump_state "$label"
    die "$label did not settle within 10 seconds"
  fi
  printf 'note  %s: the %s screen never settled on it within 10 seconds\n' "$label" "$side"
}

# Type the same keys into both clients and settle each screen on NEEDLE.
step() {
  local needle="$1"
  shift
  local before_zz before_tmux
  before_zz="$(styled_screen_of zz)"
  before_tmux="$(styled_screen_of tmux)"
  type_on_both "$@"
  wait_screen tmux hard "$CASE_LABEL: $needle on the tmux screen" "$before_tmux" "$needle"
  wait_screen zz soft "$CASE_LABEL: $needle on the zz screen" "$before_zz" "$needle"
}

# prefix then KEY, with the armed prefix observed on both clients first.
prefix_step() {
  local needle="$1"
  local key="$2"
  type_on_both C-b
  wait_for 'the armed prefix on the zz client' client_prefix_is zz 1
  wait_for 'the armed prefix on the tmux client' client_prefix_is tmux 1
  step "$needle" "$key"
}

mark_both() {
  local name="$1"
  send_both "printf 'MARK-%s\\n' $name"
  wait_screen zz hard "MARK-$name on the zz screen" '' "MARK-$name"
  wait_screen tmux hard "MARK-$name on the tmux screen" '' "MARK-$name"
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
    if [ "${zz_rows[index]-}" != "${tmux_rows[index]-}" ]; then
      [ "$differing" -ge 0 ] || differing="$index"
      count=$((count + 1))
    fi
  done
  LAST_ROWS_DIFFERED=0
  LAST_CURSOR_DIFFERED=0
  [ "$differing" -lt 0 ] || LAST_ROWS_DIFFERED=1
  [ "$zz_cursor" = "$tmux_cursor" ] || LAST_CURSOR_DIFFERED=1
  if [ -n "${ZZ_CHOOSER_CAPTURE_DIR:-}" ]; then
    mkdir -p "$ZZ_CHOOSER_CAPTURE_DIR"
    printf '%s\n' "${tmux_rows[@]}" "cursor $tmux_cursor" >"$ZZ_CHOOSER_CAPTURE_DIR/$name.$styled.tmux.txt"
    printf '%s\n' "${zz_rows[@]}" "cursor $zz_cursor" >"$ZZ_CHOOSER_CAPTURE_DIR/$name.$styled.zz.txt"
  fi
  if [ "$LAST_ROWS_DIFFERED" -eq 0 ] && [ "$LAST_CURSOR_DIFFERED" -eq 0 ]; then
    return 0
  fi
  printf '      case %s\n' "$name"
  if [ "$LAST_ROWS_DIFFERED" -eq 1 ]; then
    printf '      %s of %s rows differ, first differing row %s\n' "$count" "$total" "$differing"
    for ((index = 0; index < total; index++)); do
      if [ "${zz_rows[index]-}" != "${tmux_rows[index]-}" ]; then
        printf '        %2d tmux: %s\n' "$index" "$(printf '%s' "${tmux_rows[index]-}" | cat -v)"
        printf '        %2d zz:   %s\n' "$index" "$(printf '%s' "${zz_rows[index]-}" | cat -v)"
      fi
    done
  else
    printf '      all %s rows identical\n' "$total"
  fi
  printf '      cursor tmux: %s\n' "$tmux_cursor"
  printf '      cursor zz:   %s\n' "$zz_cursor"
  return 1
}

verdict() {
  local name="$1"
  local mode="$2"
  local reason="${3:-}"
  case "$reason" in
  SIBLING:*) SIBLINGS=$((SIBLINGS + 1)) ;;
  esac
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
    [ "$mode" = same ] || printf 'note  %s is identical, the record can close\n' "$name"
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
# THE SESSION TREE, prefix s: choose-tree -Zs. Sessions only, collapsed, the
# attached session selected, and its windows side by side in the preview box
# (window_tree_draw_session). j moves to the next session and the preview
# follows it; Right expands it into its windows. q ends the mode and the
# pane underneath has to come back exactly.
session_tree_case() {
  CASE_LABEL=session-tree
  mark_both sessions
  prefix_step "┌ $INNER_SESSION (sort: index)" s
  verdict session-tree-open same
  step '┌ many (sort: index)' j
  verdict session-tree-next same
  step '└─> ' Right
  verdict session-tree-expanded same
  step 'MARK-sessions' q
  verdict session-tree-closed same
}

# THE WINDOW TREE, prefix w: choose-tree -Zw. Every session expanded into its
# windows, the current window selected, its panes side by side in the preview
# (window_tree_draw_window). Then the mode-tree vocabulary, one key a
# checkpoint: t tags the current row and moves down (mode-tree.c, case 't'),
# T untags everything, O cycles the sort order, / opens the chooser's own search
# prompt at the foot of its screen, v cycles the preview off, big and back to
# normal.
#
# HELP THAT DOES NOT FIT. F1 sets mtd->help, and mode_tree_draw_help returns
# without drawing anything when the box is taller than the screen
# (mode-tree.c:1402, `sy < box_h`): the window tree's help is the twenty shared
# lines, the tree's own and Exit mode, far more than 23 rows. The pin then
# swallows the next key whole. So at 80x24 F1 shows nothing, the j after it
# moves nothing, and the / after that opens the search prompt over a selection
# that never moved: one checkpoint proves all three. The help box itself is
# compared at a size it fits, in tall_case.
window_tree_case() {
  CASE_LABEL=window-tree
  mark_both windows
  prefix_step '┌ 0 (sort: index)' w
  verdict window-tree-open same
  step '0*: ' t
  verdict window-tree-tagged same
  step '0: win' T
  verdict window-tree-untagged same
  step '(sort: name)' O
  verdict window-tree-sorted same
  step '(sort: index)' O O O
  verdict window-tree-sort-wrapped same
  step '(search) ' F1 j /
  verdict window-tree-unfit-help-then-search same
  step '┌ 1 (sort: index)' t w o Enter
  verdict window-tree-searched same
  step '(M-h)' v
  verdict window-tree-preview-off same
  step '┌ 1 (sort' v
  verdict window-tree-preview-big same
  step '┌ 1 (sort' v
  verdict window-tree-preview-normal same
  step 'MARK-windows' q
  verdict window-tree-closed same
}

# THE HELP BOX, at 100x40 where it fits: mode_tree_draw_help centres a
# tree-mode-border-style box over the tree, the shared lines, the mode's own
# lines from its help callback, and Exit mode. Any key closes it and is
# swallowed. Once for the window tree, once for the buffer tree.
tall_case() {
  CASE_LABEL=tall
  attach_both_at 100 40
  mark_both tall
  prefix_step '┌ 0 (sort: index)' w
  verdict tall-window-tree-open same
  step 'Exit mode' F1
  verdict tall-window-tree-help same
  step '┌ 0 (sort: index)' j
  verdict tall-window-tree-help-closed same
  step 'MARK-tall' q
  prefix_step '(sort: creation)' =
  verdict tall-buffer-tree-open same
  step 'Exit mode' C-h
  verdict tall-buffer-tree-help same
  step '(sort: creation)' j
  verdict tall-buffer-tree-help-closed same
  step 'MARK-tall' q
  verdict tall-closed same
}

# FILTER, f: the chooser's own `(filter) ` prompt (mode-tree.c, case 'f'), whose
# answer is a format the window tree keeps a row for when it expands true. The
# box title then carries `(filter: active)`. `c` is the pin's undo for it
# (mode-tree.c, case 'c'): mode_tree_clear_prompt then mode_tree_clear_filter,
# which rebuilds the tree from every row again and drops `(filter: active)`
# from the title while mode_tree_set_current keeps the selected row.
FILTER_FORMAT='#{==:#{window_name},two}'
filter_case() {
  CASE_LABEL=filter
  mark_both filter
  prefix_step '┌ 0 (sort: index)' w
  step '(filter) ' f
  verdict filter-prompt same
  step "(filter) $FILTER_FORMAT" -l "$FILTER_FORMAT"
  verdict filter-typed same
  step '(filter: active)' Enter
  verdict filter-applied same
  step '0: win' c
  verdict filter-cleared same
  step 'MARK-filter' q
  verdict filter-closed same
}

# SCROLLING. The window tree holds 3 sessions and 3 + MANY_WINDOWS windows, more
# rows than mode_tree_set_height gives the list above the preview, so G has to
# scroll the list and g has to bring it back (mode_tree_check_selected).
scroll_case() {
  CASE_LABEL=scroll
  mark_both scroll
  prefix_step '┌ 0 (sort: index)' w
  step '(M-h)' G
  verdict tree-scrolled-bottom same
  step '(0)' g
  verdict tree-scrolled-top same
  step 'MARK-scroll' q
  verdict tree-scroll-closed same
}

# THE BUFFER TREE, prefix =: choose-buffer -Z. Rows are
# `#{t/p:buffer_created}: #{buffer_sample}` behind the buffer name, newest
# in creation order, and the preview is the selected buffer's content
# (window_buffer_draw).
buffer_case() {
  CASE_LABEL=buffer-tree
  mark_both buffers
  prefix_step '(sort: creation)' =
  verdict buffer-tree-open same
  step '(sort: creation)' j
  verdict buffer-tree-next same
  step '*: ' t
  verdict buffer-tree-tagged same
  step 'MARK-buffers' q
  verdict buffer-tree-closed same
}

# THE BUFFER FILTER, f and c inside choose-buffer. mode_tree_key is one
# function for every mode-tree mode, so the buffer tree takes the same `f` and
# `c` as the window tree, and the shared help box it draws says so
# (mode_tree_help_start, `f  Filter %1s`). The filter is expanded per buffer
# with format_defaults_paste_buffer (window-buffer.c), so a #{buffer_name} test
# names one of the two buffers; `c` brings the other back.
BUFFER_FILTER_FORMAT='#{==:#{buffer_name},alpha}'
buffer_filter_case() {
  CASE_LABEL=buffer-filter
  mark_both bufferfilter
  prefix_step '(sort: creation)' =
  step '(filter) ' f
  verdict buffer-filter-prompt same
  step "(filter) $BUFFER_FILTER_FORMAT" -l "$BUFFER_FILTER_FORMAT"
  verdict buffer-filter-typed same
  step '(filter: active)' Enter
  verdict buffer-filter-applied same
  step 'beta' c
  verdict buffer-filter-cleared same
  step 'MARK-bufferfilter' q
  verdict buffer-filter-closed same
}

# FIND-WINDOW, prefix f: command-prompt { find-window -Z -- '%%' }, the one
# stock chooser prompt with no -P, so it stays on the status row on both sides
# and is asserted whole. The answer opens the window tree filtered to what
# matched.
find_window_case() {
  CASE_LABEL=find-window
  mark_both find
  prefix_step '(find-window)' f
  verdict find-window-prompt same
  step '(find-window) two' -l two
  verdict find-window-typed same
  step '(filter: active)' Enter
  verdict find-window-tree same
  step 'MARK-find' q
  verdict find-window-closed same
}

# COMMAND OUTPUT, the second acceptance clause. run-shell's output reaches the
# pin as a view-mode pane grid (window_view_mode, cmd-run-shell.c) and the raw
# TUI as its command-output surface. Driven fully - the long output, C-s to the
# stock `(search down)` prompt, a submitted search, a selection, and M-w, which
# in the stock emacs table is copy-selection-and-cancel and so is also the way
# back to the pane - with the grid, the submitted search and the restoration
# asserted and the copied text compared as a note.
# The view surface itself now matches: output-shown and output-searched are
# asserted whole. Two divergences the modes landing did not carry are recorded
# with the bytes that name them.
#
# THE PANE PROMPT. Both stock search bindings are `command-prompt -P`
# (key-bindings.c:569-570 and 654), and -P is window_pane_set_prompt, whose
# prompt redraw_draw_pane_prompt (screen-redraw.c:1524) draws over the pane's
# LAST row - its first under status-position top - leaving the status row
# alone. zz raises the same prompt on the client and its raw TUI draws it on
# the status row instead, so the pin's row 22 carries `(search down) ` while
# zz's row 23 does and zz's row 22 still carries the output line. Closing it
# needs the -P flag on the wire with the pane it targets, which is more than
# this obligation's chooser surfaces.
#
# THE SEARCH MARK UNDER A SELECTION. window_copy_command clears
# `data->searchmark` for every command that is not `search-*` unless the
# command's clear column says never (window-copy.c:3767), so begin-selection
# and cursor-right drop the marks: the pin's `77` is plain and only the
# selection is painted. zz keeps the current-match cell painted under the
# selection. zz-terminal already carries that rule
# (CopyModeAction::clears_search_marks, session.rs:8734) and applies it to a
# pane's copy mode; the retained command output does not reach it.
OUTPUT_PROMPT_REASON='the pin draws the stock search prompt over the pane with command-prompt -P (window_pane_set_prompt, screen-redraw.c:1524) and zz draws the same prompt on the status row; measured 2026-09-12, pin row 22 `(search down) ` with the status row kept, zz row 23'
OUTPUT_MARK_REASON='the pin clears the search marks on any non-search copy-mode command (window-copy.c:3767) so the selection alone is painted; zz keeps the current-match cell under the selection in the retained command output; measured 2026-09-12, row 18 `77` plain on the pin and in the current-match style on zz'
OUTPUT_REASON='the copied text is compared as a note because the view surfaces above it are compared whole'
command_output_case() {
  CASE_LABEL=command-output
  mark_both output
  local before_zz before_tmux
  before_zz="$(styled_screen_of zz)"
  before_tmux="$(styled_screen_of tmux)"
  on_both_active run-shell -t PANE 'seq 1 200'
  wait_screen tmux hard 'the run-shell output on the tmux screen' "$before_tmux" '/177]'
  wait_screen zz soft 'the run-shell output on the zz screen' "$before_zz" '/177]'
  verdict output-shown same
  step '(search down)' C-s
  verdict output-search-prompt record "$OUTPUT_PROMPT_REASON"
  step '(search down)' -l 77
  verdict output-search-typed record "$OUTPUT_PROMPT_REASON"
  step '' Enter
  verdict output-searched same
  type_on_both C-Space
  step '' Right Right
  verdict output-selected record "$OUTPUT_MARK_REASON"
  step 'MARK-output' M-w
  verdict output-closed same
  wait_for 'the copied selection on the tmux side' top_buffer_is tmux buffer0
  if wait_soft top_buffer_is zz buffer0 &&
    [ "$(side_command zz show-buffer 2>/dev/null)" = "$(side_command tmux show-buffer 2>/dev/null)" ]; then
    printf 'note  output-copied: the copied selection is the same text on both sides\n'
  else
    printf 'note  output-copied recorded, not asserted: %s: tmux copied %q, zz copied %q\n' \
      "$OUTPUT_REASON" "$(side_command tmux show-buffer 2>/dev/null)" "$(side_command zz show-buffer 2>/dev/null)"
  fi
}
top_buffer_is() {
  [ "$(side_command "$1" list-buffers -F '#{buffer_name}' 2>/dev/null | head -n 1)" = "$2" ]
}
wait_soft() {
  local attempt
  for ((attempt = 0; attempt < 200; attempt++)); do
    if "$@" >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.05
  done
  return 1
}

# THE ZOOM, -Z. mode_tree_zoom (mode-tree.c:613) reads WINDOW_ZOOMED first and
# calls window_zoom only when the window was not zoomed already; mode_tree_free
# unzooms only in that case. Both halves are driven here, in a window split so
# window_zoom has something to do: prefix w over the split zooms the pane for
# the mode's lifetime and puts Z in the window's flags on the status row, q
# gives the split back, and over a window the user zoomed first with prefix z
# (resize-pane -Z) the same open and close leave the zoom alone. Each of those
# checkpoints settles on the window's flags in the status row rather than on
# the tree, because that is the last cell the pin's own redraw of the step
# touches; the stillness between two polls covers whichever half a client
# draws first.
ZOOMED_STATUS="L0:$WINDOW_NAME*Z"
UNZOOMED_STATUS="L0:$WINDOW_NAME* 1:two"
zoom_case() {
  CASE_LABEL=zoom
  local side pane
  for side in zz tmux; do
    side_command "$side" split-window -d -h -t "=$INNER_SESSION:$WINDOW_NAME" "$INNER_SHELL" ||
      die "$side refused split-window"
    pane="$(side_command "$side" list-panes -t "=$INNER_SESSION:$WINDOW_NAME" \
      -F '#{pane_active} #{pane_id}' | awk '$1 == 0 { print $2; exit }')"
    [ -n "$pane" ] || die "$side has no second pane in $WINDOW_NAME"
    side_command "$side" select-pane -t "$pane" -T "$PANE_TITLE" || die "$side refused select-pane -T"
    side_command "$side" send-keys -t "$pane" "printf 'ZOOM-SCENE\\n'" Enter ||
      die "$side refused send-keys"
  done
  wait_screen tmux hard 'the split pane on the tmux screen' '' 'ZOOM-SCENE'
  wait_screen zz hard 'the split pane on the zz screen' '' 'ZOOM-SCENE'
  mark_both zoom
  prefix_step "$ZOOMED_STATUS" w
  verdict zoom-tree-open same
  step "$UNZOOMED_STATUS" q
  verdict zoom-tree-closed same
  prefix_step "$ZOOMED_STATUS" z
  verdict zoom-manual same
  prefix_step '┌ 0 (sort: index)' w
  verdict zoom-prezoomed-open same
  step 'MARK-zoom' q
  verdict zoom-prezoomed-closed same
  prefix_step "$UNZOOMED_STATUS" z
  verdict zoom-manual-released same
}

# CHOOSE-CLIENT, prefix D. Recorded: zz does not implement choose-client at all;
# the command sits in commands.native-client-tools, an accepted gap outside this
# lane. The pin's client tree lists each client by its tty, a value this
# fixture cannot pin (see the header). Last case of its attach: q ends the pin's
# mode, and on zz, where no mode opened, it would be typed into the shell.
CLIENT_REASON='commands.native-client-tools, accepted and outside this lane: zz does not implement choose-client, and the pin lists each client by its tty'
client_case() {
  CASE_LABEL=client-tree
  mark_both clients
  type_on_both C-b
  wait_for 'the armed prefix on the zz client' client_prefix_is zz 1
  wait_for 'the armed prefix on the tmux client' client_prefix_is tmux 1
  local before_tmux
  before_tmux="$(styled_screen_of tmux)"
  type_on_both D
  wait_screen tmux hard 'the client tree on the tmux screen' "$before_tmux" 'session cho'
  wait_screen zz soft 'the client tree on the zz screen' '' 'session cho'
  verdict client-tree-open record "$CLIENT_REASON"
}

write_attach zz "$SCRATCH_DIR/attach-zz.sh"
write_attach tmux "$SCRATCH_DIR/attach-tmux.sh"

zz_command -f /dev/null daemon >"$SCRATCH_DIR/zz-daemon.out" 2>"$SCRATCH_DIR/zz-daemon.err" &
ZZ_PID=$!
wait_for "zz daemon socket" test -S "$ZZ_SOCKET"

run_cases() {
  printf 'chooser and command-output differential at %sx%s (pin %s)\n' \
    "$COLUMNS_UNDER_TEST" "$ROWS_UNDER_TEST" "$(basename -- "$TMUX_BIN")"
  attach_both_at 80 24
  CASE_LABEL=baseline
  mark_both baseline
  verdict baseline same
  session_tree_case
  window_tree_case
  filter_case
  scroll_case
  buffer_case
  buffer_filter_case
  find_window_case
  command_output_case
  zoom_case
  client_case
  tall_case

  if [ "$FAILURES" -ne 0 ]; then
    printf '%s of %s asserted comparisons differ, %s recorded (%s for a sibling lane)\n' \
      "$FAILURES" "$CHECKS" "$RECORDS" "$SIBLINGS"
    exit 1
  fi
  printf 'all %s asserted comparisons identical, %s recorded not asserted (%s for a sibling lane)\n' \
    "$CHECKS" "$RECORDS" "$SIBLINGS"
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
    if [ "$LAST_CURSOR_DIFFERED" -ne 1 ]; then
      outcome='no cursor difference reported'
    elif [ "$LAST_ROWS_DIFFERED" -ne 0 ]; then
      outcome='a cursor-only sabotage was reported in the rows'
    fi
    ;;
  none)
    if [ "$LAST_ROWS_DIFFERED" -ne 0 ] || [ "$LAST_CURSOR_DIFFERED" -ne 0 ]; then
      outcome='reported a difference where both sides are the same'
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

planted_on_zz() {
  side_command zz capture-pane -p -t "=$INNER_SESSION:1.1" | grep -q '^PLANTED'
}
outer_cursor_is() {
  [ "$(cursor_tuple zz)" = "$1" ]
}
outer_cursor_is_not() {
  [ "$(cursor_tuple zz)" != "$1" ]
}

run_self_check() {
  printf 'self-check: one deliberate difference per channel, plus two equivalences\n'
  attach_both_at 80 24

  # The equivalence first: the same window tree opened on both sides with
  # nothing planted must report nothing, or every sabotage below would be
  # satisfied by a comparison that always reports.
  CASE_LABEL='self-check equivalence'
  mark_both equal
  prefix_step '┌ 0 (sort: index)' w
  compare_rows self-check-equivalence styled || true
  self_check_case 'equivalence: the same window tree on both sides' none

  # A tag mark on one side only: t typed into the pin's client alone.
  CASE_LABEL='self-check tag'
  local before
  before="$(styled_screen_of tmux)"
  type_on_side tmux t
  wait_screen tmux hard 'the one-sided tag' "$before" '0*: '
  compare_rows self-check-tag styled || true
  self_check_case 'tag, a mark on one side only' rows
  type_on_side tmux T k
  wait_screen tmux hard 'the one-sided tag withdrawn' '' '┌ 0 (sort: index)'
  step 'MARK-equal' q

  # A preview cell: one line of output in the zz side's second pane of window
  # 1 only. No tree row names pane content, so only the preview box can carry
  # the difference.
  CASE_LABEL='self-check preview'
  side_command zz send-keys -t "=$INNER_SESSION:1.1" "printf 'PLANTED\\n'" Enter ||
    die 'zz refused send-keys'
  wait_for 'the planted preview line' planted_on_zz
  mark_both preview
  prefix_step '┌ 0 (sort: index)' w
  step '┌ 1 (sort: index)' j
  compare_rows self-check-preview styled || true
  self_check_case 'preview, one cell of one previewed pane differs' rows
  step 'MARK-preview' q

  # A tree row: one side's second window renamed. The row and nothing else.
  CASE_LABEL='self-check row'
  side_command zz rename-window -t "=$INNER_SESSION:1" twx || die 'zz refused rename-window'
  mark_both row
  prefix_step '┌ 0 (sort: index)' w
  compare_rows self-check-row styled || true
  self_check_case 'row, one tree row differs on one side' rows
  step 'MARK-row' q
  side_command zz rename-window -t "=$INNER_SESSION:1" two || die 'zz refused rename-window'

  # The cursor: a space typed at one side's shell prompt after the mode closed.
  # capture-pane trims trailing blanks, so only the cursor may report it.
  CASE_LABEL='self-check cursor'
  attach_both_at 80 24
  mark_both cursor
  prefix_step '┌ 0 (sort: index)' w
  step 'MARK-cursor' q
  local zz_before
  zz_before="$(cursor_tuple zz)"
  type_on_side zz Space
  wait_for 'the one-sided space' outer_cursor_is_not "$zz_before"
  compare_rows self-check-cursor styled || true
  self_check_case 'cursor, one column at one prompt' cursor
  type_on_side zz BSpace

  # The second equivalence: the restoration after a chooser, with nothing
  # planted, is the same screen on both sides.
  CASE_LABEL='self-check restoration'
  wait_for 'the space withdrawn' outer_cursor_is "$zz_before"
  compare_rows self-check-restoration styled || true
  self_check_case 'equivalence: the pane restored after a chooser' none

  # A zoom on one side only: the attached window split on both sides, then
  # prefix z into the pin's client alone. The layout and the window's Z flag on
  # the status row are the channel zoom_case asserts, and nothing else here
  # reaches them.
  CASE_LABEL='self-check zoom'
  local side pane
  for side in zz tmux; do
    side_command "$side" split-window -d -h -t "=$INNER_SESSION:$WINDOW_NAME" "$INNER_SHELL" ||
      die "$side refused split-window"
    pane="$(side_command "$side" list-panes -t "=$INNER_SESSION:$WINDOW_NAME" \
      -F '#{pane_active} #{pane_id}' | awk '$1 == 0 { print $2; exit }')"
    [ -n "$pane" ] || die "$side has no second pane in $WINDOW_NAME"
    side_command "$side" select-pane -t "$pane" -T "$PANE_TITLE" || die "$side refused select-pane -T"
    side_command "$side" send-keys -t "$pane" "printf 'ZOOM-SCENE\\n'" Enter ||
      die "$side refused send-keys"
  done
  wait_screen tmux hard 'the split pane on the tmux screen' '' 'ZOOM-SCENE'
  wait_screen zz hard 'the split pane on the zz screen' '' 'ZOOM-SCENE'
  mark_both zoomed
  before="$(styled_screen_of tmux)"
  type_on_side tmux C-b
  wait_for 'the armed prefix on the tmux client' client_prefix_is tmux 1
  type_on_side tmux z
  wait_screen tmux hard 'the one-sided zoom' "$before" "$ZOOMED_STATUS"
  compare_rows self-check-zoom styled || true
  self_check_case 'zoom, the attached window zoomed on one side only' rows
  before="$(styled_screen_of tmux)"
  type_on_side tmux C-b
  wait_for 'the armed prefix on the tmux client' client_prefix_is tmux 1
  type_on_side tmux z
  wait_screen tmux hard 'the one-sided zoom withdrawn' "$before" "$UNZOOMED_STATUS"

  # The chooser's own prompt row: f typed into the pin's buffer tree alone.
  # Only the prompt row and the cursor on it can carry the difference.
  CASE_LABEL='self-check buffer filter prompt'
  mark_both prompt
  prefix_step '(sort: creation)' =
  before="$(styled_screen_of tmux)"
  type_on_side tmux f
  wait_screen tmux hard 'the one-sided filter prompt' "$before" '(filter) '
  compare_rows self-check-buffer-filter-prompt styled || true
  self_check_case 'prompt, the buffer tree filter prompt on one side only' rows

  if [ "$SELF_CHECK_FAILURES" -ne 0 ]; then
    printf '%s self-check expectations unmet\n' "$SELF_CHECK_FAILURES"
    exit 1
  fi
  printf 'self-check complete: every sabotage was caught and both equivalences passed\n'
}

if [ "$SELF_CHECK" -eq 1 ]; then
  run_self_check
else
  run_cases
fi
