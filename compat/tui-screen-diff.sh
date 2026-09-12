#!/usr/bin/env bash
# Whole-screen differential for an attached client.
#
# tui-pane-geometry.sh compares two numbers and status-row.sh compares one row.
# Neither one ever looked at the rest of the screen, so a raw TUI could paint
# any cell it liked above the status line and no campaign proof would notice.
# This fixture attaches both binaries inside one outer pinned tmux, drives both
# sides with the same input, and compares EVERY decoded cell of both screens
# plus the cursor, at named checkpoints.
#
# THE DECODED SCREEN IS THE CONTRACT, NOT THE BYTE STREAM. The outer pinned
# tmux is the decoder: `capture-pane -p -e` re-emits SGR from the outer pane's
# own grid, so attribute order, batching, redundant resets and cursor-movement
# spelling collapse on both sides before anything is compared. What does NOT
# collapse is the colour CLASS: a named colour, an indexed one and an RGB one
# stay three different cells in the pin's grid, and the recorded colour-classes
# case below measures what each binary does with them.
#
# What the pin collapses was measured rather than assumed. colour.c
# colour_fromstring gives `red` the value 1 and `colour1` the value
# 1|COLOUR_FLAG_256, two DIFFERENT values, and yet both reach the outer grid as
# the same cell and capture-pane re-emits both as \e[41m, so that pair really is
# an equivalence here; --self-check asserts it, along with attribute order in a
# style and two spellings of the same bold cell.
#
# THE CURSOR is read with `display-message -p` against the OUTER pane of each
# side, so it is the cursor the inner client left in the outer terminal:
# position, visibility, shape, blink, very-visible and colour. The pin exposes
# all six to a format (format.c format_cb_cursor_shape and its neighbours), so
# every one of them is asserted here rather than declared a hole.
#
# CONTROLLED DYNAMIC VALUES, set on both sides before the first checkpoint and
# never left to chance:
#   status-right ''      the default ends in a clock, and %H:%M can tick
#                        between the two captures. It also carries %d-%b-%y,
#                        which the pin expands through libc strftime(3) and zz
#                        expands locale-independently, a real divergence that
#                        belongs to status-row.sh and not to this comparison.
#   status-left L        fixed literal, so the left of the row is asserted
#                        rather than emptied.
#   automatic-rename off plus rename-window win: the default name follows the
#                        running command and would race the marker.
#   select-pane -T title fixed: the pin seeds a pane title from gethostname and
#                        zz reports the shell name through its shell
#                        integration. That is the recorded pane.runtime-facts
#                        decision, not a divergence of this screen.
#   the inner shell      ENV= PS1='$ ' exec /bin/sh, handed to new-session and
#                        to split-window as the pane's command: no rc file, and
#                        a prompt that carries no host, user, path or clock.
# Nothing else is masked. Anything not in that list is compared.
#
# SETTLED CHECKPOINTS. Every checkpoint ends by sending `printf 'MARK-%s\n'
# NAME` to both sides and waiting, bounded, for that marker to reach each screen
# AND for each screen to stop changing. The typed command carries MARK-%s, not
# MARK-NAME, so only the shell's own output can satisfy it, and the option
# changes and pane commands of a case reach the server before the send-keys does
# and travel to the outer grid through the same ordered stream, so a repaint the
# case asked for is on the screen before its marker is. The stability half is
# not decoration: see wait_settled for the run that proved the marker alone is
# not enough. No wait in this file is a sleep.
#
# SIZES AND MODES. `same` asserts the whole decoded screen; `record` prints the
# same report and keeps going; `text` splits the two, asserting every glyph,
# every column and the cursor while recording only the styles, for a case whose
# colours belong to a decision somebody else owns. Every size asserts. zz's sidebar used to appear on its own from 109
# columns (crates/zz-tui/src/sidebar.rs AUTO_HIDE_COLUMNS = 80 + 28 + 1), so
# 109 and 120 were recorded rather than waived by omission; width no longer
# invokes it, the sidebar is client-local chrome that only focus-sidebar or a
# user binding shows, and those two widths are asserted like the rest. The
# resize cases deliberately cross the retired threshold in both directions:
# 80 goes out to 120 and back, 109 to 120, 120 down to 100.
#
# THE SIDEBAR CASE runs once, at 120x24, after the size loop. It is the one
# place the two sides are driven differently on purpose: focus-sidebar is a zz
# command with no counterpart in the pin, so the case requires the zz screen to
# DIFFER while the sidebar is up and to be identical to the pin again once it
# is withdrawn. The difference half is what makes the identity half worth
# anything - a sidebar that never drew would pass a test that only asserted
# equality.
#
# --self-check runs the same driver against a deliberate one-sided difference in
# each channel and requires the comparison to catch it in that channel, plus three
# equivalences it must NOT report. A fixture that only passes has proved nothing.
#
# ZZ_SCREEN_CAPTURE_DIR, when set, keeps every capture and cursor tuple as text.
# A bounded wait that runs out dumps the same diagnostics the geometry fixture
# dumps, into ZZ_SCREEN_DIAGNOSTICS_DIR or a fresh /tmp directory it names.
set -eEuo pipefail

usage() {
  printf 'usage: compat/tui-screen-diff.sh [--self-check] [ZZ_BIN [TMUX_BIN]]\n' >&2
  printf '       ZZ_BIN=path TMUX_BIN=path compat/tui-screen-diff.sh\n' >&2
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

# SIZE|MODE|ALTERNATE. ALTERNATE is the width the resize case moves to and back
# from. 80 and 109 cross the retired 109 column threshold upwards and 120
# crosses it downwards, so a width that once changed the canvas is asserted on
# both sides of the move and on the way back.
SIZES=(80x24\|same\|120 100x24\|same\|80 80x10\|same\|100 80x6\|same\|100 109x24\|same\|120 120x24\|same\|100)
PANE_TITLE="screentitle"
WINDOW_NAME="win"
SCRATCH_DIR="$(mktemp -d /tmp/zzsd.XXXXXX)"
TOKEN="${SCRATCH_DIR##*.}"
OUTER_SOCKET_NAME="zzsdo-$TOKEN"
INNER_SOCKET_NAME="zzsdi-$TOKEN"
ZZ_SOCKET="/tmp/zzsd-$TOKEN.sock"
OUTER_SESSION="driver"
INNER_SESSION="screen"
ZZ_HOME="$SCRATCH_DIR/zz-home"
TMUX_HOME="$SCRATCH_DIR/tmux-home"
OUTER_HOME="$SCRATCH_DIR/outer-home"
ZZ_LOG_DIR="$SCRATCH_DIR/zz-logs"
ZZ_CLIENT_STDERR="$SCRATCH_DIR/zz-client.err"
TMUX_CLIENT_STDERR="$SCRATCH_DIR/tmux-client.err"
CAPTURE_DIR="${ZZ_SCREEN_CAPTURE_DIR:-}"
DIAGNOSTICS_DIR=""
COLUMNS_UNDER_TEST=0
ROWS_UNDER_TEST=0
SIZE_LABEL=""
ZZ_PID=""
FAILURES=0
CHECKS=0
RECORDS=0
LAST_ROWS_DIFFERED=0
LAST_CURSOR_DIFFERED=0
mkdir -p "$ZZ_HOME" "$TMUX_HOME" "$OUTER_HOME" "$ZZ_LOG_DIR"
[ -z "$CAPTURE_DIR" ] || mkdir -p "$CAPTURE_DIR"

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
side_home() {
  case "$1" in
  zz) printf '%s\n' "$ZZ_HOME" ;;
  tmux) printf '%s\n' "$TMUX_HOME" ;;
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

diagnostics_dir() {
  if [ -z "$DIAGNOSTICS_DIR" ]; then
    DIAGNOSTICS_DIR="${ZZ_SCREEN_DIAGNOSTICS_DIR:-$(mktemp -d /tmp/zzsd-diag.XXXXXX)}"
    mkdir -p "$DIAGNOSTICS_DIR"
  fi
  printf '%s\n' "$DIAGNOSTICS_DIR"
}

# A timeout is a failed check, and a failed check has to say what it was waiting
# for and what the screen showed at that moment. Every command here may fail: a
# wait can run out before the outer session exists.
dump_diagnostics() {
  local label="$1"
  local dir side log
  dir="$(diagnostics_dir)"
  {
    printf 'wait that ran out: %s\n' "$label"
    printf 'at: %s\n' "$(date -Is 2>/dev/null || date)"
    printf 'size under test: %s\n' "${SIZE_LABEL:-none yet}"
    printf 'zz: %s\n' "$ZZ_BIN"
    printf 'tmux: %s\n' "$TMUX_BIN"
    printf 'zz socket: %s\n' "$ZZ_SOCKET"
    printf 'outer socket: %s\n' "$OUTER_SOCKET_NAME"
    printf 'inner tmux socket: %s\n' "$INNER_SOCKET_NAME"
    printf 'ZZ_LOG_DIR (%s) holds:\n' "$ZZ_LOG_DIR"
    ls -1 -- "$ZZ_LOG_DIR" 2>&1 || true
  } >"$dir/what-fired.txt" 2>&1 || true
  for side in zz tmux; do
    tmux_outer_command capture-pane -p -e -S - -t "=$OUTER_SESSION:$side" \
      >"$dir/outer-$side.screen.txt" 2>&1 || true
    tmux_outer_command display-message -p -t "=$OUTER_SESSION:$side" \
      "$CURSOR_FORMAT" >"$dir/outer-$side.cursor.txt" 2>&1 || true
    side_command "$side" list-clients \
      -F '#{client_name} session=#{client_session} #{client_width}x#{client_height} flags=#{client_flags}' \
      >"$dir/$side.list-clients.txt" 2>&1 || true
    side_command "$side" list-panes -a \
      -F '#{session_name}:#{window_index}.#{pane_index} #{pane_width}x#{pane_height} dead=#{pane_dead}' \
      >"$dir/$side.list-panes.txt" 2>&1 || true
  done
  cp -f -- "$SCRATCH_DIR/zz-daemon.out" "$dir/zz-daemon.stdout.txt" 2>/dev/null || true
  cp -f -- "$SCRATCH_DIR/zz-daemon.err" "$dir/zz-daemon.stderr.txt" 2>/dev/null || true
  cp -f -- "$ZZ_CLIENT_STDERR" "$dir/zz-client.stderr.txt" 2>/dev/null || true
  cp -f -- "$TMUX_CLIENT_STDERR" "$dir/tmux-client.stderr.txt" 2>/dev/null || true
  for log in "$ZZ_LOG_DIR"/*; do
    [ -f "$log" ] || continue
    cp -f -- "$log" "$dir/ring-$(basename -- "$log").txt" 2>/dev/null || true
  done
  printf 'diagnostics retained in %s:\n' "$dir" >&2
  ls -1 -- "$dir" >&2 || true
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
  dump_diagnostics "$label"
  die "$label did not happen within 10 seconds"
}

# The cursor is read WHOLE and asserted whole: position, visibility, shape,
# blink, very-visible and colour. The last four used to be recorded instead,
# because the raw TUI wrote DECSCUSR and an OSC 12 cursor colour on every
# cursor placement where pinned tmux writes neither, so the outer tmux reported
# shape=block blinking=1 colour=#e5c07b for zz against shape=default blinking=0
# colour=none for the pin at every size and every checkpoint.
#
# tty.c tty_update_cursor is the pin's rule and it was read rather than
# guessed: with the pane's screen at SCREEN_CURSOR_DEFAULT the pin emits Se
# only if it had previously changed the style, and tty_force_cursor_colour with
# ccolour -1 emits nothing at all, so a pane whose application never asked
# leaves the outer terminal's own cursor exactly as it found it. The raw TUI
# now does the same, and the whole tuple is one assertion.
CURSOR_ASSERTED_FORMAT='#{cursor_x},#{cursor_y} flag=#{cursor_flag} pane=#{pane_width}x#{pane_height} shape=#{cursor_shape} blinking=#{cursor_blinking} very_visible=#{cursor_very_visible} colour=#{cursor_colour}'
CURSOR_FORMAT="$CURSOR_ASSERTED_FORMAT"

outer_pane_is() {
  [ "$(tmux_outer_command display-message -p -t "$1" '#{pane_width}x#{pane_height}' 2>/dev/null)" = "$2" ]
}
client_attached() {
  [ "$(side_command "$1" list-clients -F '#{client_session}' 2>/dev/null)" = "$INNER_SESSION" ]
}

# Exactly ROWS_UNDER_TEST lines of the visible screen, escapes included, so the
# two sides are compared row index by row index and never off by a trimmed
# trailing blank.
capture_screen() {
  tmux_outer_command capture-pane -p -e -S 0 -E "$((ROWS_UNDER_TEST - 1))" \
    -t "=$OUTER_SESSION:$1"
}
# The marker is waited for against the plain capture: a settle condition must
# not depend on whether the decoder happened to put an SGR run at the start of
# that row. The comparison itself always uses the -e capture above.
capture_plain() {
  tmux_outer_command capture-pane -p -S 0 -E "$((ROWS_UNDER_TEST - 1))" \
    -t "=$OUTER_SESSION:$1"
}
cursor_tuple() {
  tmux_outer_command display-message -p -t "=$OUTER_SESSION:$1" "$CURSOR_ASSERTED_FORMAT"
}

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

set_on_both() {
  side_command zz set-option -g "$1" "$2" || die "zz refused set-option -g $1"
  side_command tmux set-option -g "$1" "$2" || die "tmux refused set-option -g $1"
}
# Every pane command names the pane id it resolved, never a bare session, so a
# case that moved the active pane (a split, a zoom) is still driving the same
# pane on both sides and neither binary has to agree about what a loose target
# means.
active_pane() {
  side_command "$1" list-panes -t "=$INNER_SESSION" -F '#{pane_active} #{pane_id}' |
    awk '$1 == 1 { print $2; exit }'
}
# The pane title reaches the screen twice: through status-right's default and,
# once pane-border-status is on, through pane-border-format. attach_both_at
# pins the first pane's title; a case that splits has a second pane whose title
# neither side pinned, and the pin seeds it from gethostname while zz reports
# the shell name through its shell integration. That is pane.runtime-facts, not
# a divergence of this screen, so every pane's title is pinned before a border
# case can put it on a border.
pin_pane_titles() {
  local side pane
  for side in zz tmux; do
    while read -r pane; do
      [ -n "$pane" ] || continue
      side_command "$side" select-pane -t "$pane" -T "$PANE_TITLE" ||
        die "$side refused select-pane -T"
    done < <(side_command "$side" list-panes -t "=$INNER_SESSION" -F '#{pane_id}')
  done
}

# No terminfo dependency: the pane's shell writes the erase itself.
clear_both() {
  send_both "printf '\\033[2J\\033[3J\\033[H'"
}
send_both() {
  local pane
  pane="$(active_pane zz)"
  [ -n "$pane" ] || die "zz has no active pane"
  zz_command send-keys -t "$pane" "$1" Enter || die "zz refused send-keys"
  pane="$(active_pane tmux)"
  [ -n "$pane" ] || die "tmux has no active pane"
  tmux_inner_command send-keys -t "$pane" "$1" Enter || die "tmux refused send-keys"
}
# The same command against each side's own active pane.
# The target goes before the positional arguments. tmux stops option parsing
# at the first argument, so a trailing -t became part of split-window's shell
# command, the new pane's shell exited at once, and until 2026-09-11 (TUI-004
# attempt-04) every split in this file measured one pane.
run_on_both_active() {
  local side pane
  for side in zz tmux; do
    pane="$(active_pane "$side")"
    [ -n "$pane" ] || die "$side has no active pane"
    side_command "$side" "$1" -t "$pane" "${@:2}" || die "$side refused $1"
  done
}
run_on_both() {
  side_command zz "$@" || die "zz refused $1"
  side_command tmux "$@" || die "tmux refused $1"
}

# Global options outlive the session that was set up with them, so a case that
# sets one would carry it into every later case and into the next size. Both
# servers go back to their own defaults before each attach, and only then are
# the declared values pinned again.
OWNED_OPTIONS=(
  status status-position status-style status-left status-right status-justify
  status-left-length status-right-length
  window-status-format window-status-current-format default-command
  automatic-rename pane-border-status pane-border-format pane-border-lines
  pane-border-style pane-active-border-style
)
reset_owned_options() {
  local side="$1"
  local option
  for option in "${OWNED_OPTIONS[@]}"; do
    side_command "$side" set-option -gu "$option" >/dev/null 2>&1 || true
  done
}

# An option cannot be set on a server that is not running and a server with no
# session does not stay running, so the inner shell is handed to new-session as
# the pane's command rather than through default-command. It runs through the
# same spawn path either way.
INNER_SHELL="ENV= PS1='\$ ' exec /bin/sh"

pin_dynamic_values() {
  local side="$1"
  reset_owned_options "$side"
  side_command "$side" set-option -g status-right '' || die "$side refused status-right"
  side_command "$side" set-option -g status-left L || die "$side refused status-left"
  side_command "$side" set-option -g automatic-rename off || die "$side refused automatic-rename"
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
  pin_dynamic_values zz
  pin_dynamic_values tmux
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

# A substring, not a whole line: the sidebar case below draws zz's sidebar to
# the LEFT of the pane, so there the marker shares its row with sidebar cells
# and a border. The typed command carries MARK-%s, never MARK-<name>, so only
# the shell's own output can satisfy this.
screen_has_marker() {
  capture_plain "$1" | grep -Fq "$2"
}
# THE SETTLE. The marker alone is not enough, and this is measured rather than
# argued: with the marker as the only condition, one run in two of the 80x24
# zoom checkpoint captured the zz side between the marker's newline and the
# prompt the shell wrote next, so row 11 held `$` in one run and nothing in the
# next. The shell writes the marker and the prompt as two separate writes and a
# capture can land between them.
#
# A checkpoint is settled when the marker is on the screen AND the screen has
# not changed between two polls. Both halves are observable and the whole thing
# is bounded; the 50 ms poll is the same interval every bounded wait in this
# harness uses, and it is never the settle by itself.
# A pane too short to keep the marker on screen - one content row under
# pane-border-status at 80x6 - still has it in its own history. The marker is
# accepted from there too; the screen still has to hold still between polls.
pane_holds_marker() {
  local pane
  pane="$(active_pane "$1")"
  [ -n "$pane" ] || return 1
  side_command "$1" capture-pane -p -S -20 -t "$pane" 2>/dev/null | grep -Fq "$2"
}
wait_settled() {
  local side="$1"
  local marker="$2"
  local label="$3"
  local previous=""
  local current attempt
  for ((attempt = 0; attempt < 200; attempt++)); do
    current="$(capture_plain "$side" 2>/dev/null || true)"
    if [ -n "$previous" ] && [ "$current" = "$previous" ] &&
      { printf '%s' "$current" | grep -Fq "$marker" || pane_holds_marker "$side" "$marker"; }; then
      return 0
    fi
    previous="$current"
    sleep 0.05
  done
  dump_diagnostics "$label"
  die "$label did not settle within 10 seconds"
}
settle_both() {
  wait_settled zz "$1" "$2 settled on the zz screen"
  wait_settled tmux "$1" "$2 settled on the tmux screen"
}


# Rows first, then the cursor. The report names the checkpoint, the first row
# index that differs with both sides' bytes through cat -v, and both cursor
# tuples, which is enough to tell a glyph difference from a colour one and both
# from a cursor one without re-running anything.
compare_screens() {
  local name="$1"
  local zz_rows tmux_rows zz_cursor tmux_cursor index differing total
  mapfile -t zz_rows < <(capture_screen zz)
  mapfile -t tmux_rows < <(capture_screen tmux)
  zz_cursor="$(cursor_tuple zz)"
  tmux_cursor="$(cursor_tuple tmux)"
  if [ -n "$CAPTURE_DIR" ]; then
    printf '%s\n' "${zz_rows[@]-}" >"$CAPTURE_DIR/$SIZE_LABEL.$name.zz.screen.txt"
    printf '%s\n' "${tmux_rows[@]-}" >"$CAPTURE_DIR/$SIZE_LABEL.$name.tmux.screen.txt"
    {
      printf 'zz:   %s\n' "$zz_cursor"
      printf 'tmux: %s\n' "$tmux_cursor"
    } >"$CAPTURE_DIR/$SIZE_LABEL.$name.cursor.txt"
  fi
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
  printf '      checkpoint %s at %s\n' "$name" "$SIZE_LABEL"
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

# The same comparison over the PLAIN capture: every cell's glyph and the cursor,
# with the styles left out. It exists for the `text` mode below and for nothing
# else, so a case whose colours are a recorded divergence still asserts every
# glyph, every column those glyphs claim and the cursor they leave behind.
compare_plain_screens() {
  local name="$1"
  local zz_rows tmux_rows zz_cursor tmux_cursor index differing total
  mapfile -t zz_rows < <(capture_plain zz)
  mapfile -t tmux_rows < <(capture_plain tmux)
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
  if [ "$differing" -lt 0 ] && [ "$zz_cursor" = "$tmux_cursor" ]; then
    return 0
  fi
  printf '      checkpoint %s at %s, text and cursor\n' "$name" "$SIZE_LABEL"
  if [ "$differing" -ge 0 ]; then
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

# The named settled checkpoint: mark, wait for the mark on both sides, compare.
# `same` asserts the whole decoded screen. `record` asserts nothing and says WHY
# in its own words; no size records any more, so there is no default reason left
# to inherit. `text` is the split: every glyph, every column and the cursor are
# ASSERTED, and only the styles are recorded, for a case whose colours are a
# divergence somebody else owns and whose geometry is this obligation's claim.
checkpoint() {
  local name="$1"
  local mode="$2"
  local reason="${3:-}"
  send_both "printf 'MARK-%s\\n' $name"
  settle_both "MARK-$name" "$name"
  if [ "$mode" = text ]; then
    CHECKS=$((CHECKS + 1))
    RECORDS=$((RECORDS + 1))
    [ -n "$reason" ] || die "recorded style at $name says nothing about why"
    if compare_plain_screens "$name"; then
      printf 'ok    %s %s every glyph, column and the cursor identical\n' "$SIZE_LABEL" "$name"
    else
      FAILURES=$((FAILURES + 1))
      printf 'DIFF  %s %s\n' "$SIZE_LABEL" "$name"
    fi
    if compare_screens "$name"; then
      printf 'note  %s %s styles identical too, the record can close\n' "$SIZE_LABEL" "$name"
    else
      printf 'note  %s %s styles recorded, not asserted: %s\n' "$SIZE_LABEL" "$name" "$reason"
    fi
    return 0
  fi
  if [ "$mode" = same ]; then
    CHECKS=$((CHECKS + 1))
  else
    RECORDS=$((RECORDS + 1))
  fi
  if compare_screens "$name"; then
    printf 'ok    %s %s\n' "$SIZE_LABEL" "$name"
    return 0
  fi
  if [ "$mode" = same ]; then
    FAILURES=$((FAILURES + 1))
    printf 'DIFF  %s %s\n' "$SIZE_LABEL" "$name"
  else
    [ -n "$reason" ] || die "recorded checkpoint $name says nothing about why"
    printf 'note  %s %s recorded, not asserted: %s\n' "$SIZE_LABEL" "$name" "$reason"
  fi
  return 0
}

run_size() {
  local size="$1"
  local mode="$2"
  local alternate="$3"
  local columns="${size%x*}"
  local rows="${size#*x}"
  SIZE_LABEL="$size"
  attach_both_at "$columns" "$rows"

  checkpoint fresh "$mode"

  set_on_both status off
  checkpoint status-off "$mode"
  set_on_both status on
  checkpoint status-on "$mode"

  run_on_both_active split-window -v "$INNER_SHELL"
  checkpoint split "$mode"

  run_on_both_active resize-pane -Z
  checkpoint zoom "$mode"
  run_on_both_active resize-pane -Z
  checkpoint unzoom "$mode"

  tmux_outer_command resize-window -t "=$OUTER_SESSION:zz" -x "$alternate" -y "$rows"
  tmux_outer_command resize-window -t "=$OUTER_SESSION:tmux" -x "$alternate" -y "$rows"
  wait_for "outer zz pane at ${alternate}x${rows}" outer_pane_is "=$OUTER_SESSION:zz" "${alternate}x${rows}"
  wait_for "outer tmux pane at ${alternate}x${rows}" outer_pane_is "=$OUTER_SESSION:tmux" "${alternate}x${rows}"
  COLUMNS_UNDER_TEST="$alternate"
  SIZE_LABEL="${alternate}x${rows}-from-$size"
  checkpoint resized "$mode"

  tmux_outer_command resize-window -t "=$OUTER_SESSION:zz" -x "$columns" -y "$rows"
  tmux_outer_command resize-window -t "=$OUTER_SESSION:tmux" -x "$columns" -y "$rows"
  wait_for "outer zz pane back at ${columns}x${rows}" outer_pane_is "=$OUTER_SESSION:zz" "${columns}x${rows}"
  wait_for "outer tmux pane back at ${columns}x${rows}" outer_pane_is "=$OUTER_SESSION:tmux" "${columns}x${rows}"
  COLUMNS_UNDER_TEST="$columns"
  SIZE_LABEL="$size"
  checkpoint restored "$mode"

  # A SECOND STATUS ROW WITH A SPLIT. MEASURED 2026-09-11 (TUI-004 attempt-04)
  # at 80x10: after `status 2` the pin's window is 8 rows, %0 h=3 and %1 h=4;
  # zz's server kept 9, %0 h=4 and %1 h=4, because zz-mux set_pane_geometry
  # back-solved the window extent from the ACTIVE pane's reported size alone and
  # the active pane keeps its height when the status block grows, so the other
  # pane kept the row the pin takes away and the raw TUI painted a 4-row
  # viewport into a 3-row box.
  #
  # LANDED 2026-09-12 (TUI-004 attempt-05): options.c options_push_changes runs
  # recalculate_sizes after every write and clients_calculate_size sizes a
  # window at the client's rows minus status_line_size, so zz-mux now emits
  # StatusRowsChanged whenever a status write moves the row count and the daemon
  # resizes every window of the session from its attached clients. Re-measured
  # on the pin over this exact sequence at 80x10, 80x6 and 80x24: `status 2`
  # gives window 8/4/22 with %0 3/1/10 beside %1 4/2/11, status-position leaves
  # every height alone, and the round trip back to `status on` restores 4/4,
  # 2/2 and 11/11. Both checkpoints assert at every size the file drives.
  set_on_both status 2
  checkpoint status-two-rows "$mode"
  set_on_both status-position top
  checkpoint status-top "$mode"
  set_on_both status-position bottom
  set_on_both status on

  # PANE BORDERS. screen-redraw.c draws a pane status line only while
  # pane-border-status is on, and layout_fix_panes hands the row back to the
  # pane while it is off, so this walks all three values with two panes on the
  # screen and compares the borders, their text and the rows they cost.
  #
  # MEASURED 2026-09-11 (TUI-004 attempt-04): the border text, the rows it
  # costs and the border colours are the pin's, cell for cell.
  # redraw_draw_border_span starts from grid_default_cell and applies
  # window_pane_get_border_style: pane-active-border-style (fg=themegreen) next
  # to the client's active pane, pane-border-style (fg=themelightgrey) next to
  # every other, each over the terminal's default ground. The daemon publishes
  # both, expanded per pane, on StatusLine.pane_borders and the raw TUI paints
  # them, so both checkpoints assert every cell and its style.
  pin_pane_titles
  set_on_both pane-border-status top
  checkpoint pane-border-top "$mode"
  set_on_both pane-border-status bottom
  checkpoint pane-border-bottom "$mode"

  # BORDER STYLE ATTRIBUTES. MEASURED 2026-09-11 (cycle 6 modes gate): format_draw
  # starts the pane status line from the border style, so its attributes stay on
  # the border glyphs around the text and #[default] returns to them. The raw TUI
  # kept only the style's colours on that row and dropped bold. The pin's divider
  # row 11 at 80x24 with the lower pane active reads
  # \e[1m\e[31m\e[44m══\e[7m1\e[0;1m\e[31m\e[44m "ptitle"══. The second checkpoint
  # moves the status line to the bottom with the upper pane active, so the
  # divider row carries the upper pane's status line in the active style.
  set_on_both pane-active-border-style 'fg=colour1,bg=colour4,bold'
  set_on_both pane-border-style 'fg=#ff8800'
  set_on_both pane-border-lines double
  set_on_both pane-border-status top
  checkpoint pane-border-attributes-top "$mode"
  set_on_both pane-border-lines heavy
  set_on_both pane-border-status bottom
  run_on_both select-pane -t "=$INNER_SESSION:0.0"
  checkpoint pane-border-attributes-bottom "$mode"
  run_on_both select-pane -t "=$INNER_SESSION:0.1"
  run_on_both set-option -gu pane-active-border-style
  run_on_both set-option -gu pane-border-style
  run_on_both set-option -gu pane-border-lines
  set_on_both pane-border-status off
  checkpoint pane-border-off "$mode"

  # COLOUR CLASSES ON THE STATUS ROW, which is not the same channel as the
  # recorded colour-classes case below: that one is an application writing SGR
  # into the pane body, this one is tty_colours deciding what a status option's
  # named, indexed and RGB colour leaves as. Both grounds are named in every
  # case, because a style that sets only the background is the recorded
  # default-fg divergence and an assertion must not be built on top of one.
  clear_both
  set_on_both status-style 'bg=red,fg=white'
  checkpoint status-style-named "$mode"
  set_on_both status-style 'bg=colour124,fg=colour231'
  checkpoint status-style-indexed "$mode"
  set_on_both status-style 'bg=#1e2030,fg=#c0caf5'
  checkpoint status-style-rgb "$mode"
  side_command zz set-option -gu status-style >/dev/null 2>&1 || true
  side_command tmux set-option -gu status-style >/dev/null 2>&1 || true

  set_on_both window-status-current-format '#[fg=red]N#[fg=colour196]I#[fg=#010203]R'
  checkpoint format-colour-classes "$mode"
  side_command zz set-option -gu window-status-current-format >/dev/null 2>&1 || true
  side_command tmux set-option -gu window-status-current-format >/dev/null 2>&1 || true

  # Two channels the corpus above never touches, both driven identically on the
  # two sides and both RECORDED rather than asserted, because 2026-09-09
  # measured a real divergence in each and neither has a registry owner yet.
  #
  # colour-classes writes the same three cells as a NAMED colour, an INDEXED
  # colour and an RGB one. The pin keeps the class it was given; zz resolves the
  # named and the indexed one through its palette and hands the outer terminal
  # RGB, so \e[31m arrives as \e[38;2;205;0;0m and \e[38;5;196m arrives as
  # \e[38;2;255;0;0m, while the RGB cell passes through unchanged.
  # Cell widths, which the rest of the corpus never exercises: a CJK pair that
  # occupies two columns each, a base letter with a combining accent that
  # occupies none of its own, and an emoji. Both sides are sent the same bytes,
  # so the columns the cells claim, and everything the row after them lines up
  # with, are compared like any other row.
  clear_both
  send_both "printf 'W[%s][%s][%s]|\\n' '你好' 'éä' '🙂'"
  checkpoint wide-glyphs "$mode"

  # Each of the two below clears the screen first: a recorded difference stays
  # on the screen, and the report names the FIRST differing row, so without the
  # clear the second case would report the first case's leftovers instead of
  # its own.
  clear_both
  set_on_both status-style bg=colour4
  checkpoint default-fg record \
    'zz writes an explicit RGB foreground where the pin leaves the foreground default'
  side_command zz set-option -gu status-style >/dev/null 2>&1 || true
  side_command tmux set-option -gu status-style >/dev/null 2>&1 || true

  clear_both
  send_both "printf '\\033[31mNAMED\\033[0m \\033[38;5;196mINDEXED\\033[0m \\033[38;2;1;2;3mRGB\\033[0m\\n'"
  checkpoint colour-classes record \
    'zz resolves a named and an indexed colour to RGB before writing to the terminal'

  # THE SERVER THEME OPTION. options-table.c makes `theme` a server option and
  # server_client_update_theme_colours expands the ten dark-theme-*/light-theme-*
  # colours per client from it, so forcing it light changes what themegreen and
  # themeblack resolve to on the pin's status row. zz's daemon cannot read the
  # option at all: crates/zz-mux/src/command.rs parse_format_option gates by-name
  # reads on TMUX_OPTION_CONSUMERS and neither `theme` nor the ten colours are in
  # it. #{client_theme} answers empty on both binaries from a one-shot client, so
  # the terminal's own light/dark REPORT is not a channel this fixture can drive;
  # the forced option is. MEASURED 2026-09-11 (TUI-004 attempt-04): since the
  # theme arm landed (TUI-004 attempt-02) the daemon resolves the ten slots per
  # client and the whole screen is the pin's at every size, so this asserts.
  clear_both
  run_on_both set-option -s theme light
  checkpoint theme-light "$mode"
  side_command zz set-option -su theme >/dev/null 2>&1 || true
  side_command tmux set-option -su theme >/dev/null 2>&1 || true

  # A STYLED status-left against status-left-length. The pin's status-left is L
  # everywhere else in this file, one character, which no length limit ever
  # reaches; this case is the one that meets it. format-draw.c format_trim_left
  # copies a #[...] section through without counting it against the limit and
  # counts a character by the columns it draws in, so the pin's row draws LEFT.
  # Probed on both binaries 2026-09-09: display-message -p
  # '#{T;=/#{status-left-length}:status-left}' answered '#[fg=red,bold]LEFT' on
  # the pin and '#[fg=red,b' on zz, whose truncate_value counted every printable
  # byte, and the unterminated marker took the whole band off the row.
  #
  # LANDED 2026-09-12 (TUI-004 attempt-05): the trim walks format_width's units,
  # so a style section is copied through at no cost, a run of #s costs the
  # columns it draws escaped and a wide character costs two. Re-measured on the
  # pin over 27 answers, left and right, with markers, escaped hashes, a style
  # after them, wide characters and an unterminated section
  # (crates/zz-mux/src/formats.rs, a_style_section_costs_a_trim_no_column...).
  clear_both
  set_on_both status-left '#[fg=red,bold]LEFT'
  checkpoint styled-left-trim "$mode"
  set_on_both status-left L

  # THE RESIDUE OF THE CURSOR FIX, driven identically on both sides and left
  # LAST because it changes the outer terminal's cursor for the rest of the
  # size. The raw TUI no longer writes DECSCUSR or OSC 12 at all, which is the
  # pin's behaviour for a pane whose application never asked; it is not the
  # pin's behaviour for a pane that DOES ask. The pin tracks s->cstyle per
  # screen and forwards the request; zz's wire has nowhere to carry it, because
  # libghostty's render state reports a concrete cursor style and never `the
  # application has not asked` (RenderStateCursorVisualStyle is BAR, BLOCK,
  # UNDERLINE or BLOCK_HOLLOW), and zz_terminal::Cursor packs those same four
  # into two bits. Recorded here so the cost of the fix is visible on every run.
  clear_both
  send_both "printf '\\033[5 q'"
  checkpoint cursor-style-request record \
    'the pin forwards a DECSCUSR the pane asked for and zz has nowhere to carry the request'
}

# --- the sidebar, driven on one side on purpose -----------------------------
#
# focus-sidebar is a zz command the pin has no counterpart for, and no width
# invokes it. The case therefore asserts BOTH directions: while the sidebar is
# up the zz screen has to DIFFER from the pin's, and once it is withdrawn the
# two screens have to be identical again. Asserting only the second half would
# pass on a sidebar that never drew at all.
#
# Nothing is typed into either shell between the two comparisons, so the only
# thing that moves is the sidebar: the pin's screen is left settled from the
# first checkpoint and is never touched again.
SIDEBAR_MARKER='zz at '
sidebar_on_screen() {
  capture_plain zz | grep -Fq "$SIDEBAR_MARKER"
}
sidebar_off_screen() {
  ! capture_plain zz | grep -Fq "$SIDEBAR_MARKER"
}
# focus-sidebar reaches the sidebar through a USER BINDING, which is the path
# the contract names and the only one the daemon accepts: a one-shot `zz
# focus-sidebar` is not an interactive client and crates/zz-daemon/src/daemon.rs
# answers it 'focus-sidebar requires an interactive client', the way the pin
# refuses a client command with no client. F8 keeps the case clear of every
# default key table, so a lane that changes what prefix-s means cannot change
# what this case measures. The key goes to the zz CLIENT through the outer pane;
# the pin's side is neither bound nor sent anything.
show_sidebar_on_zz() {
  zz_command bind-key -n F8 focus-sidebar || die 'zz refused bind-key -n F8'
  tmux_outer_command send-keys -t "=$OUTER_SESSION:zz" F8
  wait_for 'the sidebar on the zz screen' sidebar_on_screen
}

run_sidebar_case() {
  SIZE_LABEL='120x24-sidebar'
  attach_both_at 120 24
  checkpoint sidebar-before same

  show_sidebar_on_zz
  wait_settled zz "$SIDEBAR_MARKER" 'the sidebar settled on the zz screen'
  CHECKS=$((CHECKS + 1))
  compare_screens sidebar-shown || true
  if [ "$LAST_ROWS_DIFFERED" -eq 1 ]; then
    printf 'ok    %s sidebar-shown differs from the pin, which is what a drawn sidebar means\n' \
      "$SIZE_LABEL"
  else
    FAILURES=$((FAILURES + 1))
    printf 'DIFF  %s sidebar-shown: focus-sidebar left the screen identical to the pin\n' \
      "$SIZE_LABEL"
  fi

  # q on the SIDEBAR key table is ChromeAction::ToggleSidebar, which withdraws
  # it (crates/zz-client/src/chrome.rs TUI_SIDEBAR_DEFAULTS). It goes to the zz
  # CLIENT through the outer pane, not to the inner pane, so no shell sees it.
  tmux_outer_command send-keys -t "=$OUTER_SESSION:zz" q
  wait_for 'the sidebar off the zz screen' sidebar_off_screen
  wait_settled zz "MARK-sidebar-before" 'the withdrawn canvas settled on the zz screen'
  CHECKS=$((CHECKS + 1))
  if compare_screens sidebar-withdrawn; then
    printf 'ok    %s sidebar-withdrawn\n' "$SIZE_LABEL"
  else
    FAILURES=$((FAILURES + 1))
    printf 'DIFF  %s sidebar-withdrawn\n' "$SIZE_LABEL"
  fi
}

# --- self-check ------------------------------------------------------------
#
# Each case attaches a fresh pair, applies a difference to ONE side, marks both
# and compares. A sabotage has to be reported in the channel it was made in; an
# equivalence has to be reported nowhere.
SELF_CHECK_FAILURES=0

# The typed input has to stay identical on both sides or a sabotage would only
# prove that the fixture notices its own different keystrokes. Each side reads
# its own $HOME/<name>, so the command echoed on screen is the same bytes on
# both sides and only the OUTPUT differs.
plant() {
  local side="$1"
  local name="$2"
  local content="$3"
  printf '%s' "$content" >"$(side_home "$side")/$name"
}

self_check_case() {
  local name="$1"
  local expectation="$2"
  local rows_differed="$LAST_ROWS_DIFFERED"
  local cursor_differed="$LAST_CURSOR_DIFFERED"
  local verdict=ok
  case "$expectation" in
  rows)
    [ "$rows_differed" -eq 1 ] || verdict='no row difference reported'
    ;;
  cursor)
    [ "$cursor_differed" -eq 1 ] || verdict='no cursor difference reported'
    ;;
  none)
    if [ "$rows_differed" -ne 0 ] || [ "$cursor_differed" -ne 0 ]; then
      verdict='reported a difference the pin collapses'
    fi
    ;;
  esac
  if [ "$verdict" = ok ]; then
    printf 'ok    self-check %s\n' "$name"
    return 0
  fi
  SELF_CHECK_FAILURES=$((SELF_CHECK_FAILURES + 1))
  printf 'FAIL  self-check %s: %s\n' "$name" "$verdict"
}

self_check_checkpoint() {
  local name="$1"
  send_both "printf 'MARK-%s\\n' $name"
  settle_both "MARK-$name" "$name"
  compare_screens "$name" || true
}

# A sabotage that moves the cursor has to be compared where it left it. Sending
# a marker afterwards would run a whole command and put the cursor back at a
# fresh prompt on both sides, which is how the first attempt at the cursor case
# passed its rows and reported no cursor difference at all. This settles on a
# substring both sides' own sabotage output share instead.
self_check_settle_on() {
  local marker="$1"
  local name="$2"
  settle_both "$marker" "$name"
  compare_screens "$name" || true
}

run_self_check() {
  printf 'self-check: one deliberate difference per channel, plus three the pin collapses\n'

  SIZE_LABEL='80x24-glyph'
  attach_both_at 80 24
  plant zz glyph 'GLYPH-AA'
  plant tmux glyph 'GLYPH-AB'
  send_both 'cat $HOME/glyph'
  self_check_checkpoint glyph
  self_check_case 'glyph, one character of output differs' rows

  SIZE_LABEL='80x24-colour'
  attach_both_at 80 24
  side_command zz set-option -g status-style bg=red || die 'zz refused status-style'
  self_check_checkpoint colour
  self_check_case 'colour, status-style bg=red on one side' rows

  SIZE_LABEL='80x24-border-style'
  attach_both_at 80 24
  run_on_both_active split-window -v "$INNER_SHELL"
  side_command zz set-option -g pane-border-style fg=red || die 'zz refused pane-border-style'
  self_check_checkpoint border-style
  self_check_case 'border style, pane-border-style fg=red on one side' rows

  SIZE_LABEL='80x24-border-attributes'
  attach_both_at 80 24
  run_on_both_active split-window -v "$INNER_SHELL"
  pin_pane_titles
  set_on_both pane-border-status top
  set_on_both pane-active-border-style 'fg=colour1,bg=colour4'
  side_command zz set-option -g pane-active-border-style 'fg=colour1,bg=colour4,bold' ||
    die 'zz refused pane-active-border-style'
  self_check_checkpoint border-attributes
  self_check_case 'border attributes, bold only in pane-active-border-style on one side' rows

  SIZE_LABEL='80x24-cursor'
  attach_both_at 80 24
  plant zz cursorline 'CURSORMARK-WITH-A-LONGER-TAIL'
  plant tmux cursorline 'CURSORMARK'
  send_both 'printf %s "$(cat $HOME/cursorline)"'
  self_check_settle_on CURSORMARK cursor
  self_check_case 'cursor, a different-length unterminated line' cursor

  # The sidebar case's two halves are each other's sabotage: it requires a
  # difference while the sidebar is up and none once it is withdrawn. This is
  # the first half, driven on its own, so the inventory shows the channel being
  # caught rather than only the whole case passing. 120 columns, the width that
  # used to invoke the sidebar by itself.
  SIZE_LABEL='120x24-sidebar-shown'
  attach_both_at 120 24
  send_both "printf 'MARK-%s\\n' sidebar-fresh"
  settle_both 'MARK-sidebar-fresh' sidebar-fresh
  show_sidebar_on_zz
  wait_settled zz "$SIDEBAR_MARKER" 'the sidebar settled on the zz screen'
  compare_screens sidebar-shown || true
  self_check_case 'sidebar, focus-sidebar shown on one side' rows

  # The `text` mode reads the PLAIN capture, which is a second comparison and
  # needs its own two cases: it has to catch a glyph difference, and it has to
  # stay silent about a pure style difference, which is the whole reason it
  # exists. Without both, a case that recorded its styles would assert nothing.
  SIZE_LABEL='80x24-text-mode-glyph'
  attach_both_at 80 24
  plant zz textmode 'TEXTMARK-A'
  plant tmux textmode 'TEXTMARK-B'
  send_both 'cat $HOME/textmode'
  settle_both 'TEXTMARK-' text-mode-glyph
  if compare_plain_screens text-mode-glyph; then
    SELF_CHECK_FAILURES=$((SELF_CHECK_FAILURES + 1))
    printf 'FAIL  self-check text mode, glyph: the plain comparison reported no difference\n'
  else
    printf 'ok    self-check text mode, glyph: caught one character of output\n'
  fi

  SIZE_LABEL='80x24-text-mode-style'
  attach_both_at 80 24
  side_command zz set-option -g status-style bg=red || die 'zz refused status-style'
  send_both "printf 'MARK-%s\\n' textstyle"
  settle_both 'MARK-textstyle' text-mode-style
  if compare_plain_screens text-mode-style; then
    printf 'ok    self-check text mode, style: a pure colour difference is not a text difference\n'
  else
    SELF_CHECK_FAILURES=$((SELF_CHECK_FAILURES + 1))
    printf 'FAIL  self-check text mode, style: the plain comparison reported a colour difference\n'
  fi
  compare_screens text-mode-style-styled || true
  self_check_case 'text mode, the styled comparison still catches that colour' rows

  # The cursor tuple's shape half, now that it is asserted rather than recorded.
  # Both sides type the same command and each reads its own file; only one file
  # carries the DECSCUSR, so the difference is in the cursor and nowhere else.
  SIZE_LABEL='80x24-cursor-shape'
  attach_both_at 80 24
  plant zz cstyle 'CSMARK'
  plant tmux cstyle '\033[5 qCSMARK'
  send_both 'printf "%b" "$(cat $HOME/cstyle)"'
  self_check_settle_on CSMARK cursor-shape
  self_check_case 'cursor shape, DECSCUSR on one side' cursor

  SIZE_LABEL='80x24-geometry'
  attach_both_at 80 24
  side_command zz split-window -v -t "$(active_pane zz)" "$INNER_SHELL" ||
    die 'zz refused split-window'
  self_check_checkpoint geometry
  self_check_case 'geometry, split-window on one side' rows

  # The status-rows sabotage. Both sides take the second status row, and only
  # zz is then pinned back to the window height it had before, which is what the
  # server did before the sizing landed: the row the status block takes came out
  # of the client's screen but not out of the layout. The upper pane is filled
  # first, because a pane whose viewport is one row taller than the box it is
  # painted into only shows it where there is content to push out of the box,
  # and driven at 80x10 for the same reason.
  SIZE_LABEL='80x10-status-rows'
  attach_both_at 80 10
  send_both "printf 'FILL-%s\\n' 1 2 3 4 5 6 7 8"
  settle_both 'FILL-8' status-rows-fill
  run_on_both_active split-window -v "$INNER_SHELL"
  set_on_both status 2
  side_command zz resize-window -t "=$INNER_SESSION" -y 9 ||
    die 'zz refused resize-window'
  self_check_checkpoint status-rows
  self_check_case 'status rows, one side keeps the window a row taller' rows

  # The styled-trim sabotage. Both sides are given the same styled status-left,
  # and zz is then given the value a trim that counted the style section by
  # bytes would have left behind, which is what the row drew before the trim
  # landed: an unterminated #[ and none of the text.
  SIZE_LABEL='80x24-styled-trim'
  attach_both_at 80 24
  set_on_both status-left '#[fg=red,bold]LEFT'
  side_command zz set-option -g status-left '#[fg=red,b' ||
    die 'zz refused status-left'
  self_check_checkpoint styled-trim
  self_check_case 'styled trim, one side keeps what a byte count leaves' rows

  # The pin parses a style into a cell, so the order the attributes were written
  # in is gone by the time capture-pane re-emits it. The foreground is named on
  # both sides on purpose: with the foreground left at default the two binaries
  # already differ, which is the recorded default-fg case above, and an
  # equivalence must not be built on top of a divergence.
  SIZE_LABEL='80x24-style-order'
  attach_both_at 80 24
  side_command zz set-option -g status-style 'bg=colour1,fg=colour7,bold' ||
    die 'zz refused status-style'
  side_command tmux set-option -g status-style 'bold,fg=colour7,bg=colour1' ||
    die 'tmux refused status-style'
  self_check_checkpoint style-order
  self_check_case 'equivalence: attribute order in a style' none

  # colour.c colour_fromstring gives `red` the value 1 and `colour1` the value
  # 1|COLOUR_FLAG_256, two different values, and yet both reach the outer grid
  # as the same cell and capture-pane re-emits both as \e[41m. This is the
  # equivalence measured, not the one assumed.
  SIZE_LABEL='80x24-red-vs-colour1'
  attach_both_at 80 24
  side_command zz set-option -g status-style 'bg=red,fg=colour7' ||
    die 'zz refused status-style'
  side_command tmux set-option -g status-style 'bg=colour1,fg=colour7' ||
    die 'tmux refused status-style'
  self_check_checkpoint red-vs-colour1
  self_check_case 'equivalence: bg=red and bg=colour1 reach the same cell' none

  # The same bold cell written with two spellings: a redundant leading zero in
  # the parameter, and the short form of the reset. Attributes carry no colour,
  # so this equivalence is clear of the colour-class divergence above. Both
  # sides are told to run the same command; each reads its own file.
  SIZE_LABEL='80x24-escape-spelling'
  attach_both_at 80 24
  plant zz spell '\033[1mBOLD\033[0m'
  plant tmux spell '\033[01mBOLD\033[m'
  send_both 'printf "%b\\n" "$(cat $HOME/spell)"'
  self_check_checkpoint escape-spelling
  self_check_case 'equivalence: two spellings of the same bold cell' none

  if [ "$SELF_CHECK_FAILURES" -ne 0 ]; then
    printf '%s self-check expectations unmet\n' "$SELF_CHECK_FAILURES"
    return 1
  fi
  printf 'self-check complete: every sabotage was caught in its own channel and every equivalence passed\n'
  return 0
}

# --- run -------------------------------------------------------------------

write_attach zz "$SCRATCH_DIR/attach-zz.sh"
write_attach tmux "$SCRATCH_DIR/attach-tmux.sh"
zz_command -f /dev/null daemon >"$SCRATCH_DIR/zz-daemon.out" 2>"$SCRATCH_DIR/zz-daemon.err" &
ZZ_PID=$!
wait_for "zz daemon socket" test -S "$ZZ_SOCKET"

if [ "$SELF_CHECK" -eq 1 ]; then
  run_self_check
  exit $?
fi

printf 'whole-screen differential (pin %s)\n' "$(basename -- "$TMUX_BIN")"
for entry in "${SIZES[@]}"; do
  IFS='|' read -r size mode alternate <<<"$entry"
  run_size "$size" "$mode" "$alternate"
done
run_sidebar_case

if [ "$FAILURES" -ne 0 ]; then
  printf '%s of %s asserted checkpoints differ, %s recorded\n' \
    "$FAILURES" "$CHECKS" "$RECORDS"
  exit 1
fi
printf 'all %s asserted checkpoints identical, %s recorded not asserted\n' \
  "$CHECKS" "$RECORDS"
