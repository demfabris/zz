#!/usr/bin/env bash
# The stock client-command roster: exact stdout, stderr and exit status of every
# remaining client-side command, plus the attached client's screen and session
# state after each one.
#
# TUI-011 clause 1 asks for a finite command-and-flag roster, clause 2 for a
# comparison of each entry's three CLI channels and its two attached channels.
# This file IS that roster: the table below names every entry, what pinned tmux
# d77c9dc6 does with it, what zz does today, and which of the three dispositions
# the entry takes - PROVED here, DECLARED with its reason, or handed to a child
# obligation. Every PROVED entry is a case below whose five channels are
# asserted; every DECLARED entry is a case whose divergence is printed and
# recorded with the accepted gap that owns it; every CHILD entry is recorded and
# named in the child obligation that will close it.
#
# THE ROSTER
# ---------------------------------------------------------------------------
# entry                     pin d77c9dc6                     zz today                        disposition
# choose-client [-hikNrZ]   window_pane_set_mode, client      opens the same client mode on
#   [-F -f -K -O -t] [tmpl]   mode on the target pane           the attached client, and a     PROVED (CLI errors)
#                                                               clientless CLI answers the     + DECLARED (the
#                                                               attached-client error          clientless entry)
# clock-mode [-t]           window_clock_mode on the target   the same pane mode, drawn by   PROVED
#                             pane, redrawn every second        every attached client, with
#                                                               clock-mode-colour and the
#                                                               four clock-mode-style faces
# switch-mode [-kswZ]       window_switch_mode on the target  the same pane mode, its rows   PROVED (the mode it
#   [-F -t] [command]         pane: one row per session or       expanded from the same         opens and its Escape
#                             window over a (search) prompt      default format, over the       teardown) + DECLARED
#                                                                same prompt                    (the mode's own
#                                                                                               movement, Enter target
#                                                                                               and incremental filter)
# server-access [-adglrw]   socket access control list, and    every lookup, ordering and     PROVED (-l and every
#   [-t] [user|group]         the lookups, orderings and         refusal; the socket admits     refusal) + DECLARED
#                             refusals around it                 its owner alone, so adding     (admitting a second
#                                                                a second identity is           identity),
#                                                                refused                        protocol.socket-acl
# lock-server               locks every client, runs          validates, empty execution,    PROVED (CLI) +
#                             lock-command on each tty          after-lock-server fires       DECLARED (screen),
# lock-session [-t]         locks that session's clients      same                           PROVED + DECLARED
# lock-client [-t]          locks that one client             same                           PROVED + DECLARED
# refresh-client            status jobs rerun, redraw         status render published        PROVED
# refresh-client -S         status jobs rerun, status redraw  status render published        PROVED
# refresh-client -f -F      client flags set                  client flags set               PROVED
# refresh-client -A -B -C   control-client only               control-client only            PROVED
# refresh-client -t         target client, missing-client     same                           PROVED
# refresh-client -c -D -L   pans a terminal client's view     loudly unsupported             DECLARED
#   -R -U -l -r [adjust]                                                                      clients.interactive-refresh
# capture-pane -p -S -E     the requested line range, one     same                           PROVED
#                             line per row of it
# capture-pane -J -q -T     join, quiet, trailing positions   same                           PROVED
# capture-pane -b           fills a named buffer              same                           PROVED
# capture-pane -e           the range with SGR, trimmed       same                           PROVED
# capture-pane (no -S -E)   every visible row, trailing       same                           PROVED
#                             blanks included
# capture-pane -N           each line out to the cells its    same                           PROVED
#                             grid row has allocated, which
#                             grid_expand_line rounds up to
#                             a quarter, a half or the whole
#                             width; -T takes it back to the
#                             cells the row used
# capture-pane -M           the mode screen, the pane when    same                           PROVED
#                             there is no mode
# capture-pane -a           `no alternate screen`, and one    same                           PROVED
#                             empty line under -q
# load-buffer -             caller stdin into a buffer        adopted, same                  PROVED
# save-buffer - / -a -      buffer bytes to caller stdout     adopted, same                  PROVED
# show-buffer [-b]          buffer bytes to stdout            same                           PROVED
# source-file -             caller stdin as a config file     adopted, same                  PROVED
# display-message -I        caller stdin into the pane        adopted, same                  PROVED
# split-window -I           caller stdin into the new pane    adopted, same                  PROVED
# show-hooks [-Bgpw] [-t]   the hook table                    same                           PROVED
# show-messages             the server log                    same shape and the same tty    DECLARED, the log
#                                                               for an attached client, but a  carries client
#                                                               clientless CLI is device-<n>   identity
#                                                               and the pin reprints a
#                                                               command through args_print
# show-messages -J          the running format jobs           the same table, empty and      PROVED
#                                                               with one job armed
# show-messages -T          Terminal <n>: <term> for          the same 234 lines             PROVED
#                             <client>, flags=0x<n>, then
#                             tty_term_describe per code
# show-messages -T -t       that client's terminal, and a     same                           PROVED
#                             name that matches nothing
#                             leaves every terminal in
# ---------------------------------------------------------------------------
#
# THE DRIVER is tui-choosers.sh's, which is tui-indicators.sh's widened: both
# binaries attach one client each inside ONE outer pinned tmux, one window per
# side, and the outer tmux decodes both screens with `capture-pane -p -e`, so
# attribute order, batching and cursor-movement spelling collapse while the
# colour class does not. Every command under test is invoked CLIENTLESS from
# outside, the way a script invokes it, and the attached client is then read.
#
# FIVE CHANNELS PER CASE. exit status, stdout bytes and stderr bytes come from
# the clientless invocation; the screen is the whole decoded screen plus the
# cursor of the attached client; the state is a fixed set of list-* formats.
# `same` asserts all five. `cli` asserts the three CLI channels and records the
# two attached ones with a reason. `record` asserts nothing and has to say why.
# A recorded case keeps its clause open; a DECLARED entry is recorded with the
# accepted gap that owns it and is not counted as parity.
#
# CONTROLLED DYNAMIC VALUES, set identically on both sides:
#   status-right ''      the default ends in a clock and a locale-expanded date;
#                        that belongs to status-row.sh.
#   status-left L        a fixed literal.
#   automatic-rename off plus -n on every window: the default name follows the
#                        running command and would race a checkpoint.
#   select-pane -T ptitle on every pane: the pin seeds a pane title from
#                        gethostname.
#   lock-command true    the pin spawns it on the client's tty. `true` returns
#                        at once, so the lock cases cannot wedge the run; what
#                        the pin draws while it runs is the declared part.
#   the inner shell      ENV= PS1='$ ' exec /bin/sh: no rc file, and a prompt
#                        with no host, user, path or clock.
# DECLARED, NOT PINNED: the server log names its clients. The pin names any
#   tty-bearing client by that tty and a clientless CLI by client-<pid>; zz now
#   names a tty-bearing client by its tty too, and keeps device-<n> for a client
#   with no tty of its own. The pin also reprints a command through its own
#   argument printer. show-messages is recorded for exactly that pair and
#   nothing else is masked.
#
# NORMALIZED, ON BOTH SIDES: a pts number, a job's fd and a job's pid are
#   handed out by the kernel to one process, so no two servers can print the
#   same one and two runs of the pin cannot either. A case that prints one sets
#   CASE_NORMALIZE and the same substitution runs over both sides before the
#   comparison. Nothing else in the line moves, and the self-check plants a
#   difference that survives the substitution to prove it.
#
# SETTLING. A command that draws nothing still has to be given the chance to
# draw: after every invocation each screen is polled until it is unchanged
# between two consecutive polls, and only then compared. Where the pin opens a
# mode the case waits, bounded, for the pin's own needle first and gives zz the
# same bounded wait made soft. The live-job case also waits for the pending-job
# placeholder on each status row: a running job first expands to empty text,
# then to the placeholder on a later timer tick. That case temporarily sets
# status-interval to one second so the bounded wait reaches that tick.
# No wait in this file is a sleep.
#
# --self-check runs the driver against a deliberate one-sided difference in each
# channel - stdout, stderr with the exit status, the screen and the session
# state - and requires the comparison to catch each one in that channel, plus
# two equivalences it must NOT report. A fixture that only passes has proved
# nothing.
set -eEuo pipefail

usage() {
  printf 'usage: compat/tui-client-commands.sh [--self-check] [ZZ_BIN [TMUX_BIN]]\n' >&2
  printf '       ZZ_BIN=path TMUX_BIN=path compat/tui-client-commands.sh\n' >&2
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
SCRATCH_DIR="$(mktemp -d /tmp/zzcc.XXXXXX)"
TOKEN="${SCRATCH_DIR##*.}"
OUTER_SOCKET_NAME="zzcco-$TOKEN"
INNER_SOCKET_NAME="zzcci-$TOKEN"
ZZ_SOCKET="/tmp/zzcc-$TOKEN.sock"
OUTER_SESSION="driver"
INNER_SESSION="cli"
WINDOW_NAME="win"
PANE_TITLE="ptitle"
ZZ_HOME="$SCRATCH_DIR/zz-home"
TMUX_HOME="$SCRATCH_DIR/tmux-home"
OUTER_HOME="$SCRATCH_DIR/outer-home"
ZZ_LOG_DIR="$SCRATCH_DIR/zz-logs"
CASE_LABEL=""
ZZ_PID=""
SUSPENDED_ZZ_PID=""
FAILURES=0
CHECKS=0
RECORDS=0
SIBLINGS=0
LAST_EXIT_DIFFERED=0
LAST_STDOUT_DIFFERED=0
LAST_STDERR_DIFFERED=0
LAST_SCREEN_DIFFERED=0
LAST_STATE_DIFFERED=0
INNER_SHELL="ENV= PS1='\$ ' exec /bin/sh"
SERVER_OWNER="$(id -un)"
SERVER_GROUP="$(id -gn)"
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
  if [ -n "$SUSPENDED_ZZ_PID" ]; then kill -CONT "$SUSPENDED_ZZ_PID" >/dev/null 2>&1; fi
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
    side_command "$side" list-clients -F '#{client_session} #{client_width}x#{client_height}' >&2 2>&1 || true
    printf -- '--- %s panes ---\n' "$side" >&2
    side_command "$side" list-panes -a -F '#{pane_id} in_mode=#{pane_in_mode} mode=#{pane_mode}' >&2 2>&1 || true
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

CURSOR_FORMAT='#{cursor_x},#{cursor_y} flag=#{cursor_flag} pane=#{pane_width}x#{pane_height} shape=#{cursor_shape}'

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

grid_cells_of() {
  tmux_outer_command capture-pane -p -R -t "=$OUTER_SESSION:$1" |
    sed -n -e 's/^\(G [0-9]*x[0-9]*\).*/\1/p' -e '/^[[:space:]]*C /{s/ flags=[^ ]*//;p;}'
}

# The session state a command may move, read the same way from both servers.
# The client's own name is its tty and is never in here; see the header.
state_of() {
  local side="$1"
  side_command "$side" list-sessions \
    -F 'S #{session_name} #{session_windows} #{session_attached}' 2>&1
  side_command "$side" list-windows -a \
    -F 'W #{session_name}:#{window_index} #{window_name} #{window_flags} #{window_panes}' 2>&1
  side_command "$side" list-panes -a \
    -F 'P #{session_name}:#{window_index}.#{pane_index} mode=#{pane_in_mode}/#{pane_mode} #{pane_width}x#{pane_height} #{pane_title}' 2>&1
  side_command "$side" list-buffers -F 'B #{buffer_name} #{buffer_size}' 2>&1
  side_command "$side" list-clients -F 'C #{client_session} #{client_width}x#{client_height} #{client_prefix}' 2>&1
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
pane_in_mode() {
  [ "$(side_command "$1" display-message -p -t "$(active_pane "$1")" '#{pane_in_mode}' 2>/dev/null)" = "$2" ]
}

write_attach() {
  local side="$1"
  local destination="$2"
  printf '#!/usr/bin/env bash\n' >"$destination"
  printf 'exec python3 %q ' "$COMPAT_DIR/tui-client-job.py" >>"$destination"
  if [ "$side" = zz ]; then
    printf 'env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL -u XDG_STATE_HOME HOME=%q XDG_CONFIG_HOME=%q ZZ_LOG_DIR=%q TMUX_TMPDIR=/tmp %q --socket %q attach-session -t %q\n' \
      "$ZZ_HOME" "$ZZ_HOME/config" "$ZZ_LOG_DIR" "$ZZ_BIN" "$ZZ_SOCKET" "=$INNER_SESSION" >>"$destination"
  else
    printf 'env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL -u XDG_STATE_HOME HOME=%q XDG_CONFIG_HOME=%q TMUX_TMPDIR=/tmp %q -L %q attach-session -t %q\n' \
      "$TMUX_HOME" "$TMUX_HOME/config" "$TMUX_BIN" "$INNER_SOCKET_NAME" "=$INNER_SESSION" >>"$destination"
  fi
  chmod +x "$destination"
}

set_on_both() {
  side_command zz set-option -g "$1" "$2" || die "zz refused set-option -g $1"
  side_command tmux set-option -g "$1" "$2" || die "tmux refused set-option -g $1"
}
set_window_on_both() {
  side_command zz set-option -gw "$1" "$2" || die "zz refused set-option -gw $1"
  side_command tmux set-option -gw "$1" "$2" || die "tmux refused set-option -gw $1"
}
run_on_both() {
  side_command zz "$@" >/dev/null || die "zz refused $1"
  side_command tmux "$@" >/dev/null || die "tmux refused $1"
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
  local pane
  side_command "$side" set-option -g automatic-rename off
  side_command "$side" new-window -d -t "=$INNER_SESSION:1" -n two "$INNER_SHELL" ||
    die "$side refused new-window"
  for pane in $(side_command "$side" list-panes -a -F '#{pane_id}'); do
    side_command "$side" select-pane -t "$pane" -T "$PANE_TITLE" || die "$side refused select-pane -T"
    side_command "$side" send-keys -t "$pane" "printf 'SCENE-%s\\n' $pane" Enter ||
      die "$side refused send-keys"
  done
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
  done
  zz_command new-session -d -s "$INNER_SESSION" -n "$WINDOW_NAME" -x "$columns" -y "$rows" \
    "$INNER_SHELL" || die "could not create the zz session"
  tmux_inner_command -f /dev/null new-session -d -s "$INNER_SESSION" -n "$WINDOW_NAME" \
    -x "$columns" -y "$rows" "$INNER_SHELL" || die "could not create the tmux session"
  set_on_both status-right ''
  set_on_both status-left L
  set_on_both automatic-rename off
  set_on_both lock-command true
  build_scene zz
  build_scene tmux
  wait_for 'every zz pane printed its scene line' every_pane_output_is zz '%'
  wait_for 'every tmux pane printed its scene line' every_pane_output_is tmux '%'
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
  settle_screen zz ''
  settle_screen tmux ''
}

# --- settling and comparison ------------------------------------------------
#
# A screen is settled when two consecutive polls are equal. The pin's side is a
# hard wait: a pin that never settles is a broken fixture. The zz side is the
# same bounded wait made soft, so a zz screen that never settles is reported
# and compared rather than killing the run.
settle_screen() {
  local side="$1"
  local before="$2"
  local previous="" current attempt
  for ((attempt = 0; attempt < 120; attempt++)); do
    current="$(styled_screen_of "$side")"
    if [ -n "$previous" ] && [ "$current" = "$previous" ] &&
      { [ -z "$before" ] || [ "$current" != "$before" ]; }; then
      return 0
    fi
    previous="$current"
    sleep 0.05
  done
  if [ -z "$before" ]; then
    printf 'note  %s: the %s screen never settled within 6 seconds\n' "${CASE_LABEL:-scene}" "$side"
  else
    printf 'note  %s: the %s screen never moved off the one before the command\n' \
      "${CASE_LABEL:-scene}" "$side"
  fi
  return 0
}

CASE_NEEDLE_MODE=0
CASE_CLOCK_FACE=0
CASE_STDIN=''
CASE_GRID_CELLS=0

# Two spellings a command prints belong to the process that printed them and no
# two servers can share them: the pts number the kernel gave a client, and the
# fd and pid of a job's own child. A case that prints either sets
# CASE_NORMALIZE, and the SAME substitution runs over both sides, so nothing
# one-sided is hidden - the text around the number is still compared byte for
# byte, which the self-check's normalized-stdout sabotage proves.
CASE_NORMALIZE=''
PER_PROCESS_NUMBERS='s|/dev/pts/[0-9][0-9]*|/dev/pts/N|g;s|fd=[0-9][0-9]*|fd=N|g;s|pid=[0-9][0-9]*|pid=N|g'
# window_clock_timer_callback redraws the face on the whole second and both
# servers wake on the same boundary, so a capture pair taken across one would
# compare two faces rather than two renderings of the same face. Wait for the
# second to turn over, then settle, so the comparison that follows runs inside a
# second that has just started. This is a wall clock, not a fixture event: the
# boundary is what is being waited for, and the wait is bounded at four seconds.
align_clock_face() {
  local start now attempt
  start="$(date +%S)"
  for ((attempt = 0; attempt < 400; attempt++)); do
    now="$(date +%S)"
    # A fresh second, and one far enough from the minute that the whole
    # comparison finishes inside it: the two faces change once a minute, and
    # reading five formats and four screens off two servers under load takes
    # longer than the tail of a second.
    if [ "$now" != "$start" ] && [ "$((10#$now))" -le 55 ]; then
      break
    fi
    start="$now"
    sleep 0.02
  done
  settle_screen tmux ''
  settle_screen zz ''
}

# Run the same clientless invocation against both servers and read all five
# channels. CLIENT and PANE stand for each side's own attached client and
# active pane, whose spellings are the one thing a command's arguments cannot
# share.
run_both() {
  local side argument client pane rc
  local -a arguments
  for side in zz tmux; do
    client="$(client_name "$side")"
    pane="$(active_pane "$side")"
    arguments=()
    for argument in "$@"; do
      case "$argument" in
      CLIENT) arguments+=("$client") ;;
      PANE) arguments+=("$pane") ;;
      *) arguments+=("$argument") ;;
      esac
    done
    set +e
    if [ -n "$CASE_STDIN" ]; then
      printf '%s' "$CASE_STDIN" |
        side_command "$side" "${arguments[@]}" \
          >"$SCRATCH_DIR/$side.out" 2>"$SCRATCH_DIR/$side.err"
    else
      side_command "$side" "${arguments[@]}" \
        >"$SCRATCH_DIR/$side.out" 2>"$SCRATCH_DIR/$side.err" </dev/null
    fi
    rc=$?
    set -e
    printf '%s\n' "$rc" >"$SCRATCH_DIR/$side.rc"
  done
}

capture_seconds_pair() {
  local before_zz before_tmux start now attempt poll
  for ((attempt = 0; attempt < 8; attempt++)); do
    before_zz="$(styled_screen_of zz)"
    before_tmux="$(styled_screen_of tmux)"
    start="$(date +%s)"
    for ((poll = 0; poll < 150; poll++)); do
      now="$(date +%s)"
      [ "$now" != "$start" ] && break
      sleep 0.01
    done
    [ "$now" != "$start" ] || continue
    sleep 0.2
    for ((poll = 0; poll < 100; poll++)); do
      zz_screen="$(styled_screen_of zz)"
      tmux_screen="$(styled_screen_of tmux)"
      zz_cursor="$(cursor_tuple zz)"
      tmux_cursor="$(cursor_tuple tmux)"
      [ "$(date +%s)" = "$now" ] || break
      if [ "$zz_screen" != "$before_zz" ] && [ "$tmux_screen" != "$before_tmux" ]; then
        printf 'clock capture %s: both faces redrew; screen and cursor pair stayed inside epoch second %s\n' "$CASE_LABEL" "$now"
        return 0
      fi
      sleep 0.01
    done
  done
  die "could not capture both seconds faces after redraw within one second"
}

compare_channels() {
  local name="$1"
  local zz_rc tmux_rc zz_state tmux_state zz_screen tmux_screen zz_cursor tmux_cursor
  zz_rc="$(cat "$SCRATCH_DIR/zz.rc")"
  tmux_rc="$(cat "$SCRATCH_DIR/tmux.rc")"
  zz_state="$(state_of zz)"
  tmux_state="$(state_of tmux)"
  if [ "$CASE_CLOCK_FACE" -eq 2 ]; then
    capture_seconds_pair
  else
    zz_screen="$(styled_screen_of zz)"
    tmux_screen="$(styled_screen_of tmux)"
    zz_cursor="$(cursor_tuple zz)"
    tmux_cursor="$(cursor_tuple tmux)"
  fi
  if [ "$CASE_GRID_CELLS" -eq 1 ]; then
    zz_screen="$(grid_cells_of zz)"
    tmux_screen="$(grid_cells_of tmux)"
  fi
  LAST_EXIT_DIFFERED=0
  LAST_STDOUT_DIFFERED=0
  LAST_STDERR_DIFFERED=0
  LAST_SCREEN_DIFFERED=0
  LAST_STATE_DIFFERED=0
  [ "$zz_rc" = "$tmux_rc" ] || LAST_EXIT_DIFFERED=1
  if [ -n "$CASE_NORMALIZE" ]; then
    sed -e "$CASE_NORMALIZE" "$SCRATCH_DIR/zz.out" >"$SCRATCH_DIR/zz.cmp"
    sed -e "$CASE_NORMALIZE" "$SCRATCH_DIR/tmux.out" >"$SCRATCH_DIR/tmux.cmp"
  else
    cp -- "$SCRATCH_DIR/zz.out" "$SCRATCH_DIR/zz.cmp"
    cp -- "$SCRATCH_DIR/tmux.out" "$SCRATCH_DIR/tmux.cmp"
  fi
  cmp -s "$SCRATCH_DIR/zz.cmp" "$SCRATCH_DIR/tmux.cmp" || LAST_STDOUT_DIFFERED=1
  cmp -s "$SCRATCH_DIR/zz.err" "$SCRATCH_DIR/tmux.err" || LAST_STDERR_DIFFERED=1
  { [ "$zz_screen" = "$tmux_screen" ] && [ "$zz_cursor" = "$tmux_cursor" ]; } || LAST_SCREEN_DIFFERED=1
  [ "$zz_state" = "$tmux_state" ] || LAST_STATE_DIFFERED=1
  if [ -n "${ZZ_CLIENT_COMMANDS_CAPTURE_DIR:-}" ]; then
    mkdir -p "$ZZ_CLIENT_COMMANDS_CAPTURE_DIR"
    printf '%s\n' "$tmux_screen" "cursor $tmux_cursor" "$tmux_state" \
      >"$ZZ_CLIENT_COMMANDS_CAPTURE_DIR/$name.tmux.txt"
    printf '%s\n' "$zz_screen" "cursor $zz_cursor" "$zz_state" \
      >"$ZZ_CLIENT_COMMANDS_CAPTURE_DIR/$name.zz.txt"
  fi
  if [ "$LAST_EXIT_DIFFERED" -eq 0 ] && [ "$LAST_STDOUT_DIFFERED" -eq 0 ] &&
    [ "$LAST_STDERR_DIFFERED" -eq 0 ] && [ "$LAST_SCREEN_DIFFERED" -eq 0 ] &&
    [ "$LAST_STATE_DIFFERED" -eq 0 ]; then
    return 0
  fi
  printf '      case %s\n' "$name"
  [ "$LAST_EXIT_DIFFERED" -eq 0 ] ||
    printf '      exit tmux: %s  zz: %s\n' "$tmux_rc" "$zz_rc"
  if [ "$LAST_STDOUT_DIFFERED" -eq 1 ]; then
    printf '      stdout differs\n'
    diff <(cat -v "$SCRATCH_DIR/tmux.cmp") <(cat -v "$SCRATCH_DIR/zz.cmp") |
      sed -e 's/^/        /' | head -40
  fi
  if [ "$LAST_STDERR_DIFFERED" -eq 1 ]; then
    printf '      stderr differs\n'
    diff <(cat -v "$SCRATCH_DIR/tmux.err") <(cat -v "$SCRATCH_DIR/zz.err") |
      sed -e 's/^/        /' | head -20
  fi
  if [ "$LAST_SCREEN_DIFFERED" -eq 1 ]; then
    printf '      screen differs\n'
    diff <(printf '%s\n' "$tmux_screen" | cat -v) <(printf '%s\n' "$zz_screen" | cat -v) |
      sed -e 's/^/        /' | head -40
    [ "$zz_cursor" = "$tmux_cursor" ] ||
      printf '        cursor tmux: %s\n        cursor zz:   %s\n' "$tmux_cursor" "$zz_cursor"
  fi
  if [ "$LAST_STATE_DIFFERED" -eq 1 ]; then
    printf '      state differs\n'
    diff <(printf '%s\n' "$tmux_state") <(printf '%s\n' "$zz_state") |
      sed -e 's/^/        /' | head -30
  fi
  return 1
}

cli_channels_differ() {
  [ "$LAST_EXIT_DIFFERED" -eq 1 ] || [ "$LAST_STDOUT_DIFFERED" -eq 1 ] ||
    [ "$LAST_STDERR_DIFFERED" -eq 1 ]
}
attached_channels_differ() {
  [ "$LAST_SCREEN_DIFFERED" -eq 1 ] || [ "$LAST_STATE_DIFFERED" -eq 1 ]
}

declare -A RECORD_OWNERS=([unattributed]=0)

case_owner() {
  case "$1" in
  refresh-pan-* | refresh-clipboard | refresh-adjustment | client-tree-open)
    printf 'gap:clients.interactive-refresh'
    ;;
  capture-* )
    printf 'TUI-017'
    ;;
  stream-source-file | stream-source-file-effect | stream-display-message | stream-split-window)
    printf 'TUI-018'
    ;;
  lock-* )
    printf 'TUI-015'
    ;;
  clock-mode-open | customize-mode-open | switch-mode | switch-mode-kill | switch-mode-kill-exit | switch-mode-zoom | suspend-client | server-access-bare | server-access-user)
    printf 'TUI-014'
    ;;
  switch-mode-windows | switch-mode-duplicate-windows | copy-over-clock* | clock-over-copy*)
    printf 'TUI-014'
    ;;
  server-access-add)
    printf 'gap:protocol.socket-acl'
    ;;
  messages-log)
    printf 'TUI-016'
    ;;
  esac
}

note_record() {
  local owner key
  owner="$(case_owner "$1")"
  if [ -z "$owner" ]; then
    key=unattributed
  else
    case "$2" in
    DECIDED\ *) key="decided:$owner" ;;
    *) key="$owner" ;;
    esac
  fi
  RECORD_OWNERS[$key]=$((${RECORD_OWNERS[$key]:-0} + 1))
  RECORDS=$((RECORDS + 1))
}

owner_tally() {
  local key entries=()
  for key in $(printf '%s\n' "${!RECORD_OWNERS[@]}" | LC_ALL=C sort); do
    entries+=("$key=${RECORD_OWNERS[$key]}")
  done
  printf 'owners %s' "${entries[*]}"
}

# NAME MODE REASON -- command...
#   same    all five channels asserted
#   cli     exit, stdout and stderr asserted; screen and state recorded
#   record  nothing asserted, the reason says why
case_run() {
  local name="$1"
  local mode="$2"
  local reason="$3"
  shift 3
  [ "$1" = "--" ] && shift
  CASE_LABEL="$name"
  case "$reason" in
  SIBLING:*) SIBLINGS=$((SIBLINGS + 1)) ;;
  esac
  local before_zz before_tmux
  before_zz="$(styled_screen_of zz)"
  before_tmux="$(styled_screen_of tmux)"
  run_both "$@"
  if [ "$CASE_NEEDLE_MODE" -eq 1 ]; then
    wait_for "the pin pane in a mode for $name" pane_in_mode tmux 1
    settle_screen tmux "$before_tmux"
    settle_screen zz ''
  else
    settle_screen zz ''
    settle_screen tmux ''
  fi
  if [ "$CASE_CLOCK_FACE" -eq 1 ]; then
    align_clock_face
  fi
  local same=0
  compare_channels "$name" || same=1
  case "$mode" in
  same)
    CHECKS=$((CHECKS + 1))
    if [ "$same" -eq 0 ]; then
      printf 'ok    %s\n' "$name"
    else
      FAILURES=$((FAILURES + 1))
      printf 'DIFF  %s\n' "$name"
    fi
    ;;
  cli)
    CHECKS=$((CHECKS + 1))
    note_record "$name" "$reason"
    [ -n "$reason" ] || die "recorded attached channels at $name say nothing about why"
    if cli_channels_differ; then
      FAILURES=$((FAILURES + 1))
      printf 'DIFF  %s: the CLI channels differ\n' "$name"
    else
      printf 'ok    %s: exit, stdout and stderr identical\n' "$name"
    fi
    if attached_channels_differ; then
      printf 'note  %s: screen and state recorded, not asserted: %s\n' "$name" "$reason"
    else
      printf 'note  %s: screen and state identical too, the record can close\n' "$name"
    fi
    ;;
  record)
    note_record "$name" "$reason"
    [ -n "$reason" ] || die "recorded case $name says nothing about why"
    if [ "$same" -eq 0 ]; then
      printf 'note  %s is identical on all five channels, the record can close\n' "$name"
    else
      printf 'note  %s recorded, not asserted: %s\n' "$name" "$reason"
    fi
    ;;
  *) die "unknown case mode $mode" ;;
  esac
  CASE_NEEDLE_MODE=0
  CASE_NORMALIZE=''
  CASE_CLOCK_FACE=0
}

# A mode command leaves a mode open on the pane, and the lock draws over the
# client. q ends a mode, and the screen both sides show once everything has
# settled is asserted: whichever side opened one has to give the pane back
# exactly, and a side that opened nothing has to be where it already was.
# Escape first and q only if the mode is still up: mode_tree_key answers q and
# a prompt inside a mode answers Escape, and a q typed at a shell prompt that
# already came back would be a change of its own. Either side can be the one
# holding a mode now that clock-mode opens the pin's own pane mode on zz too.
end_mode() {
  local side="$1"
  local attempt poll key
  for ((attempt = 0; attempt < 3; attempt++)); do
    for key in Escape q; do
      pane_in_mode "$side" 0 && return 0
      tmux_outer_command send-keys -t "=$OUTER_SESSION:$side" "$key" ||
        die 'the outer tmux refused send-keys'
      for ((poll = 0; poll < 20; poll++)); do
        pane_in_mode "$side" 0 && return 0
        sleep 0.05
      done
    done
  done
  pane_in_mode "$side" 0
}

restore_case() {
  local name="$1"
  local side before
  CASE_LABEL="$name"
  for side in tmux zz; do
    if pane_in_mode "$side" 1; then
      before="$(styled_screen_of "$side")"
      end_mode "$side" || true
      wait_for "the $side mode ended for $name" pane_in_mode "$side" 0
      settle_screen "$side" "$before"
    fi
  done
  settle_screen zz ''
  printf '0\n' >"$SCRATCH_DIR/zz.rc"
  printf '0\n' >"$SCRATCH_DIR/tmux.rc"
  : >"$SCRATCH_DIR/zz.out"
  : >"$SCRATCH_DIR/tmux.out"
  : >"$SCRATCH_DIR/zz.err"
  : >"$SCRATCH_DIR/tmux.err"
  CASE_NORMALIZE=''
  CHECKS=$((CHECKS + 1))
  if compare_channels "$name"; then
    printf 'ok    %s\n' "$name"
  else
    FAILURES=$((FAILURES + 1))
    printf 'DIFF  %s\n' "$name"
  fi
}

# `server-access -g -a` is the one form zz refuses, so the pin alone came away
# with an access entry. It is dropped on the pin before anything reads the list
# again, and the restore then asserts both sides back in the same place.
drop_pin_access_entry() {
  tmux_inner_command server-access -g -d "$SERVER_GROUP" >/dev/null 2>&1 || true
  restore_case "$1"
}

# `split-window -I` builds a pane on both sides, so both extra panes are killed
# before the next case reads the state.
restore_extra_panes() {
  local name="$1"
  CASE_LABEL="$name"
  local side extra pane
  for side in zz tmux; do
    extra="$(side_command "$side" list-panes -t "=$INNER_SESSION:$WINDOW_NAME" \
      -F '#{pane_index} #{pane_id}' | awk '$1 > 0 { print $2 }')"
    if [ -n "$extra" ]; then
      for pane in $extra; do
        side_command "$side" kill-pane -t "$pane" >/dev/null 2>&1 || true
      done
      wait_for "the extra $side pane gone for $name" window_pane_count "$side" 1
    fi
  done
  restore_case "$name"
}

window_pane_count() {
  [ "$(side_command "$1" list-panes -t "=$INNER_SESSION:$WINDOW_NAME" -F x | wc -l)" = "$2" ]
}

lifetime_window() {
  run_on_both new-window -t "=$INNER_SESSION:2" -n lifetime "$INNER_SHELL"
  run_both select-pane -t PANE -T "$PANE_TITLE"
}

lifetime_split() {
  run_both split-window -d -h -t PANE "$INNER_SHELL"
  local side pane
  for side in zz tmux; do
    for pane in $(side_command "$side" list-panes -t "=$INNER_SESSION:2" -F '#{pane_id}'); do
      side_command "$side" select-pane -t "$pane" -T "$PANE_TITLE"
    done
  done
}

lifetime_window_close() {
  run_on_both kill-window -t "=$INNER_SESSION:2"
  run_on_both select-window -t "=$INNER_SESSION:0"
  restore_case "$1"
}

customize_edit_array() {
  local name="$1" scope="$2" value="$3"
  run_on_both set-option "$scope" "${name}[100]" "$value"
  run_both customize-mode -N -t PANE
  run_both send-keys -t PANE / "$name" Enter
  run_both send-keys -t PANE Enter C-u "$value" Enter
  run_both copy-mode -q -t PANE
}

customize_fix_cases() {
  local name scope value key
  for name in command-alias terminal-overrides terminal-features codepoint-widths user-keys update-environment status-format pane-colours; do
    scope=-s
    value=review-value
    case "$name" in
      command-alias) value='review=display-message review' ;;
      terminal-overrides) value='review*:colors=256' ;;
      terminal-features) value='review*:RGB' ;;
      codepoint-widths) value='U+0041=1' ;;
      update-environment | status-format) scope=-g ;;
      pane-colours) scope=-gw; value=red ;;
    esac
    customize_edit_array "$name" "$scope" "$value"
    case_run "customize-array-$name" same '' -- show-options "$scope" "$name"
    run_both customize-mode -N -t PANE
    run_both send-keys -t PANE / "$name" Enter Right Down Enter C-u "$value" Enter
    run_both copy-mode -q -t PANE
    case_run "customize-array-child-$name" same '' -- show-options "$scope" "$name"
    run_on_both set-option "${scope}u" "$name"
  done
  for key in C-c C-d C-j Space M-\< M-\> x; do
    run_both customize-mode -t PANE
    case_run "customize-unbound-$key" same '' -- send-keys -t PANE "$key"
    case_run "customize-retained-$key" same '' -- display-message -p -t PANE '#{pane_in_mode}/#{pane_mode}'
    restore_case "customize-unbound-$key-closed"
  done
  for key in q Escape C-g; do
    run_both customize-mode -t PANE
    case_run "customize-exit-$key" same '' -- send-keys -t PANE "$key"
  done
}

customize_fix_self_checks() {
  customize_edit_array command-alias -s 'review=display-message review'
  self_check_run customize-array-control show-options -g command-alias
  self_check_expect 'array root insertion preserves every existing alias' exit=0 stdout=0 stderr=0 screen=0 state=0
  zz_command set-option -s command-alias 'review=display-message review' >/dev/null
  self_check_run customize-array-sabotage show-options -g command-alias
  self_check_expect 'unindexed array replacement erases the seeded aliases' exit=0 stdout=1 stderr=0
  run_on_both set-option -su command-alias
  run_both customize-mode -t PANE
  run_both send-keys -t PANE C-c
  self_check_run customize-interrupt-control display-message -p -t PANE '#{pane_in_mode}/#{pane_mode}'
  self_check_expect 'C-c leaves customize mode open' exit=0 stdout=0 stderr=0 screen=0 state=0
  zz_command copy-mode -q -t "$(active_pane zz)" >/dev/null
  self_check_run customize-interrupt-sabotage display-message -p -t PANE '#{pane_in_mode}/#{pane_mode}'
  self_check_expect 'closing customize mode on C-c changes mode and screen' exit=0 stdout=1 stderr=0 screen=1 state=1
  run_both copy-mode -q -t PANE
}

switch_tail_style_cases() {
  attach_both_at 80 24
  local style
  for style in dim fg=red bg=red underscore; do
    case_run "switch-tail-short-$style" same '' -- switch-mode -w -F "#[$style]#{window_name}#[default]" -t PANE
    restore_case "switch-tail-short-$style-closed"
  done
  case_run switch-tail-boundary-colour same '' -- switch-mode -w -F '#[bg=red]12345678901234567#{window_name}#[default]' -t PANE
  restore_case switch-tail-boundary-colour-closed
}

switch_tail_self_checks() {
  local duplicate
  for duplicate in no yes; do
    attach_both_at 80 24
    if [ "$duplicate" = yes ]; then
      run_on_both new-session -d -s alpha -n "$WINDOW_NAME" -x 80 -y 24 "$INNER_SHELL"
      run_on_both new-session -d -s zulu -n "$WINDOW_NAME" -x 80 -y 24 "$INNER_SHELL"
    fi
    self_check_run "switch-tail-$duplicate-control" switch-mode -w -t PANE
    self_check_expect "switch window tail $duplicate control" exit=0 stdout=0 stderr=0 screen=0 state=0
    zz_command copy-mode -q -t "$(active_pane zz)" >/dev/null
    zz_command switch-mode -w -F '#{window_name} #[dim]#{session_name}:#{window_index}#{window_flags}#[default] #[dim]#{pane_current_command}#[default] #[dim]#{?#{!=:#{pane_title},#{host_short}},#{pane_title},}#[default]#{?#{==:#{window_name},two},, }' -t "$(active_pane zz)" >/dev/null
    self_check_run "switch-tail-$duplicate-sabotage" display-message -p -t PANE '#{pane_in_mode}/#{pane_mode}'
    self_check_expect "default cell after final dim run $duplicate changes the capture style tail" exit=0 stdout=0 stderr=0 screen=1 state=0
    CASE_GRID_CELLS=1
    self_check_run "switch-tail-$duplicate-cells-control" display-message -p -t PANE '#{pane_in_mode}/#{pane_mode}'
    self_check_expect 'allocated default cells preserve the same glyphs and styles' exit=0 stdout=0 stderr=0 screen=0 state=0
    zz_command copy-mode -q -t "$(active_pane zz)" >/dev/null
    zz_command switch-mode -w -F '#{window_name} #[dim]#{session_name}:#{window_index}#{window_flags}#[default] #[dim]#{pane_current_command}#[default] #[dim]#{?#{!=:#{pane_title},#{host_short}},#{pane_title},}#[default]#{?#{==:#{window_name},two},,#[bg=red] }' -t "$(active_pane zz)" >/dev/null
    self_check_run "switch-tail-$duplicate-cells-sabotage" display-message -p -t PANE '#{pane_in_mode}/#{pane_mode}'
    self_check_expect 'a changed blank-cell background survives cell decoding' exit=0 stdout=0 stderr=0 screen=1 state=0
    CASE_GRID_CELLS=0
    run_both copy-mode -q -t PANE
    if [ "$duplicate" = yes ]; then
      run_on_both kill-session -t '=alpha'
      run_on_both kill-session -t '=zulu'
    fi
  done
}

switch_lifetime_cases() {
  run_on_both new-session -d -s zzcc-k -n lifetime-k -x 80 -y 24 "$INNER_SHELL"
  case_run switch-mode-kill same '' -- switch-mode -k -t '=zzcc-k:'
  case_run switch-mode-kill-exit same '' -- copy-mode -q -t '=zzcc-k:'
  restore_case switch-mode-kill-restored
  case_run switch-mode-zoom same '' -- switch-mode -Z -t PANE
  case_run switch-mode-zoom-single same '' -- display-message -p -t PANE '#{window_zoomed_flag}'
  restore_case switch-mode-zoom-restored

  lifetime_window
  lifetime_split
  run_on_both set-option -g @lifetime-kill-hook untouched
  run_on_both set-hook -g after-kill-pane 'set-option -g @lifetime-kill-hook fired'
  case_run switch-mode-kill-visible same '' -- switch-mode -k -t PANE
  CASE_CLOCK_FACE=1
  case_run switch-mode-kill-covered same '' -- clock-mode -t PANE
  case_run switch-mode-kill-uncovered same '' -- send-keys -t PANE Escape
  case_run switch-mode-kill-survives-cover same '' -- list-panes -t "=$INNER_SESSION:2" -F '#{pane_index} #{pane_in_mode}/#{pane_mode}'
  case_run switch-mode-kill-visible-exit same '' -- send-keys -t PANE Escape
  case_run switch-mode-kill-visible-panes same '' -- list-panes -t "=$INNER_SESSION:2" -F '#{pane_index}'
  case_run switch-mode-kill-no-command-hook same '' -- show-options -gv @lifetime-kill-hook
  run_on_both set-hook -gu after-kill-pane
  run_on_both set-option -gu @lifetime-kill-hook
  lifetime_split
  run_both switch-mode -k -t PANE
  run_both clock-mode -t PANE
  case_run switch-mode-kill-drain-stack same '' -- copy-mode -q -t PANE
  case_run switch-mode-kill-drain-panes same '' -- list-panes -t "=$INNER_SESSION:2" -F '#{pane_index}'
  lifetime_window_close switch-mode-kill-visible-restored

  lifetime_window
  case_run switch-mode-zoom-before-split same '' -- switch-mode -Z -t PANE
  lifetime_split
  case_run switch-mode-zoom-during-mode same '' -- resize-pane -Z -t PANE
  case_run switch-mode-zoom-during-mode-flag same '' -- display-message -p -t PANE '#{window_zoomed_flag}'
  case_run switch-mode-zoom-exit same '' -- send-keys -t PANE Escape
  case_run switch-mode-zoom-restored-flag same '' -- display-message -p -t PANE '#{window_zoomed_flag}'
  run_both resize-pane -Z -t PANE
  case_run switch-mode-already-zoomed same '' -- switch-mode -Z -t PANE
  case_run switch-mode-already-zoomed-exit same '' -- send-keys -t PANE Escape
  case_run switch-mode-already-zoomed-flag same '' -- display-message -p -t PANE '#{window_zoomed_flag}'
  run_both resize-pane -Z -t PANE
  lifetime_window_close switch-mode-zoom-lifetime-restored
}

switch_lifetime_self_checks() {
  lifetime_window
  lifetime_split
  self_check_run switch-mode-kill-control switch-mode -k -t PANE
  self_check_expect 'switch -k accepts the flag and opens the same mode' exit=0 stdout=0 stderr=0 screen=0 state=0
  run_both copy-mode -q -t PANE
  lifetime_split
  tmux_inner_command switch-mode -k -t "$(active_pane tmux)"
  zz_command switch-mode -t "$(active_pane zz)"
  run_both send-keys -t PANE Escape
  self_check_run switch-mode-kill-sabotage list-panes -t "=$INNER_SESSION:2" -F '#{pane_index}'
  self_check_expect 'omitting -k preserves the source pane on one side' exit=0 stdout=1 stderr=0 state=1
  lifetime_window_close switch-mode-kill-sabotage-restored

  lifetime_window
  self_check_run switch-mode-zoom-control switch-mode -Z -t PANE
  self_check_expect 'switch -Z accepts the flag and opens the same mode' exit=0 stdout=0 stderr=0 screen=0 state=0
  run_both copy-mode -q -t PANE
  tmux_inner_command switch-mode -Z -t "$(active_pane tmux)"
  zz_command switch-mode -t "$(active_pane zz)"
  lifetime_split
  run_both resize-pane -Z -t PANE
  self_check_run switch-mode-zoom-equivalence display-message -p -t PANE '#{window_zoomed_flag}'
  self_check_expect 'both modes show the zoom before exit' exit=0 stdout=0 stderr=0 screen=0 state=0
  run_both send-keys -t PANE Escape
  self_check_run switch-mode-zoom-sabotage display-message -p -t PANE '#{window_zoomed_flag}'
  self_check_expect 'omitting -Z keeps a zoom the mode should restore' exit=0 stdout=1 stderr=0 state=1
  lifetime_window_close switch-mode-zoom-sabotage-restored
}

# --- the roster's cases -----------------------------------------------------
INTERACTIVE_REFRESH='clients.interactive-refresh, accepted: every zz client renders itself from published frames, so the pan and redraw-adjustment family stays loudly unsupported'
LOCK_PROGRAM='DECIDED options.lock-program: decided 2026-09-14 by fabrico under the superset principle; the desktop session owns locking. The pin runs lock-command on the client tty; zz accepts the CLI and stores lock-command and lock-after-time without arming a terminal locker'
RICH_CAPTURE='capture.rich-transports, accepted: zz captures the terminal worker retained UTF-8 text snapshot, not the pin grid and input parser'
CAPTURE_FLAGS='DECIDED capture.rich-transports, refused with a measurement 2026-09-15: -F prints six grid_line flags and zz does not retain the full set as line facts. D is a dead pane line, X an extended cell line and H a hyperlink line, all tmux grid bookkeeping; O and P are the OSC 133 marks libghostty records on cells but does not publish per line; W is the wrap flag that the terminal grid now exposes. The workload it would serve is a script reading which rows are output, prompt or continuation; that wants a line-fact channel out of the terminal worker, not a sixth text transform. decided 2026-09-15 by the orchestrator under fabrico'"'"'s TUI parity contract of 2026-09-09; reversible'
CAPTURE_LINKS='DECIDED capture.rich-transports, refused with a measurement 2026-09-15: -H prints each line OSC 8 URIs and zz has no hyperlink to print. On a row the pin marks HX, a zz capture -e emits the text with no OSC 8 at all, so the retained snapshot did not keep the link. The workload it would serve is a script harvesting the URLs on a screen; that wants hyperlinks retained and published by the terminal worker first. decided 2026-09-15 by the orchestrator under fabrico'"'"'s TUI parity contract of 2026-09-09; reversible'
CAPTURE_PENDING='DECIDED capture.rich-transports, refused with a measurement 2026-09-15: -P prints the bytes the pin parser has read and not yet completed, input_pending(wp->ictx). libghostty-vt publishes no parser-pending buffer, so zz cannot answer it and an empty answer would be a fake channel that matched only because the buffer is almost always empty. The workload it would serve is debugging a half-written escape sequence. decided 2026-09-15 by the orchestrator under fabrico'"'"'s TUI parity contract of 2026-09-09; reversible'
CAPTURE_GRID='DECIDED capture.rich-transports, refused with a measurement 2026-09-15: -R dumps the pin internal grid - a header G <sx>x<sy> (<hsize>/<hlimit>), then per line L <yy> (<n>) flags=<string>[<hex>] <cellused>/<cellsize>, then one C line per column carrying that cell colour, attribute and link ids. Measured at 40x8 that is 329 lines for eight rows. zz has no hsize/hlimit pair, no per-line cellused and cellsize, and no grid flag word: building them inside zz would be inventing tmux internals to make bytes match. The workload it would serve is a tmux regression test reading another tmux grid. decided 2026-09-15 by the orchestrator under fabrico'"'"'s TUI parity contract of 2026-09-09; reversible'
CAPTURE_CHARSET='DECIDED capture charset provenance: decided 2026-09-15 by the orchestrator under fabrico'"'"'s TUI parity contract of 2026-09-09; reversible. At 80x24 ESC(0qqqESC(B gives literal \016qqq\017 under -C -e on the pin and UTF-8 box drawing on zz; without -e the pin emits qqq while zz still emits box drawing. Ghostty maps the source charset byte to Unicode before storing the cell and retains no charset bit. The workload is replaying original DEC line drawing bytes; ordinary Unicode text capture remains asserted'
CAPTURE_TABS='DECIDED 2026-09-18 (fabrico): zz capture-pane returns the spaces a tab left on screen. Since tmux 3.4 the pin marks every cell a tab produced (GRID_FLAG_TAB) and prints a literal \t for it under any capture flags (grid.c:1202), and once an edit removes the head of such a tab it drops the padding cells that stay behind. zz keeps no tab provenance in its terminal grid: tracking it cost up to +68% CPU on output that overwrites tab-bearing rows and made every later edit keep the span honest. Recorded in knowledge/designs/tui-parity.md, amendment 2026-09-18'
LOG_IDENTITY='DECIDED 2026-09-14: zz keeps device-<n> for a client with no tty of its own, where the pin prints client-<pid>. Measured 2026-09-14 on both sides: the pin names ANY tty-bearing client by that tty, including the attached terminal client whose attach-session row reads /dev/pts/<n>, and zz named none of them - it spelled every row by the device name the client sent, which for an interactive client is the hostname. That half is closed: the server log now names a client by its tty whenever it has one. What stays is the clientless CLI, which names a process that has already exited by the time anyone reads the log while device-<n> is the spelling every zz target, chooser row and #{client_name} uses. The pin also reprints each command through args_print, so capture-pane -pa comes back as capture-pane -ap. Registered, not masked'
SERVER_ACCESS='protocol.socket-acl, accepted as a permanent exclusion: the daemon socket is the invoking user at mode 0600, so zz keeps no peer identity and every other form of the command - the list, the lookups, the owner test, the flag conflicts, the deny of an entry that is not there and the no-action form - answers exactly as the pin does, measured 2026-09-15. Only admitting a second identity diverges - semantic:multi-user-socket-acl, the permanent exclusion this gap exists for: the pin stores the entry and exits 0, zz refuses it'
CLIENT_TREE_CLIENTLESS='clients.interactive-refresh, accepted: a chooser is per client in zz, so a clientless CLI answers the same attached-client error choose-tree and choose-buffer answer, while the pin exits 0 with no output and, alone among the three, opens no mode either: cmd_choose_tree_exec returns CMD_RETURN_NORMAL before window_pane_set_mode when server_client_how_many() == 0 (cmd-choose-tree.c), so the exit status and the error text are what diverge here, measured 2026-09-14. The raw TUI opens the pin client mode on prefix D, asserted whole in compat/tui-choosers.sh as client-tree-open'

refresh_client_cases() {
  case_run refresh-bare same '' -- refresh-client
  case_run refresh-status same '' -- refresh-client -S
  case_run refresh-target same '' -- refresh-client -t CLIENT
  case_run refresh-missing-target same '' -- refresh-client -t /dev/zzcc-nope
  case_run refresh-flag-set same '' -- refresh-client -f no-output
  case_run refresh-flag-clear same '' -- refresh-client -F no-output
  case_run refresh-flag-restore same '' -- refresh-client -f ''
  case_run refresh-control-pane same '' -- refresh-client -A '%0:on'
  case_run refresh-control-subscribe same '' -- refresh-client -B 'name:%*:#{window_id}'
  case_run refresh-control-size same '' -- refresh-client -C 90x30
  case_run refresh-missing-argument same '' -- refresh-client -r
  case_run refresh-pan-up record "$INTERACTIVE_REFRESH" -- refresh-client -U
  case_run refresh-pan-down record "$INTERACTIVE_REFRESH" -- refresh-client -D
  case_run refresh-pan-left record "$INTERACTIVE_REFRESH" -- refresh-client -L
  case_run refresh-pan-right record "$INTERACTIVE_REFRESH" -- refresh-client -R
  case_run refresh-pan-cursor record "$INTERACTIVE_REFRESH" -- refresh-client -c
  case_run refresh-clipboard record "$INTERACTIVE_REFRESH" -- refresh-client -l
  case_run refresh-adjustment record "$INTERACTIVE_REFRESH" -- refresh-client 5
  restore_case refresh-restored
}

capture_pane_cases() {
  case_run capture-range same '' -- capture-pane -p -t PANE -S 0 -E 2
  case_run capture-past-last-row same '' -- capture-pane -p -t PANE -S 0 -E 5
  case_run capture-escape same '' -- capture-pane -p -e -t PANE -S 0 -E 2
  case_run capture-join same '' -- capture-pane -p -J -t PANE -S 0 -E 2
  case_run capture-trailing same '' -- capture-pane -p -T -t PANE -S 0 -E 2
  case_run capture-reversed same '' -- capture-pane -p -t PANE -S 2 -E 0
  case_run capture-history same '' -- capture-pane -p -t PANE -S - -E 0
  case_run capture-quiet-missing same '' -- capture-pane -p -q -t %99
  case_run capture-loud-missing same '' -- capture-pane -p -t %99
  case_run capture-buffer same '' -- capture-pane -b zzcap -t PANE -S 0 -E 2
  case_run capture-buffer-shown same '' -- show-buffer -b zzcap
  case_run capture-buffer-deleted same '' -- delete-buffer -b zzcap
  case_run capture-default-range same '' -- capture-pane -p -t PANE
  case_run capture-default-range-escape same '' -- capture-pane -p -e -t PANE
  case_run capture-preserve-trailing same '' -- capture-pane -p -N -t PANE -S 0 -E 0
  case_run capture-preserve-trailing-range same '' -- capture-pane -p -N -t PANE -S 0 -E 2
  case_run capture-preserve-trailing-positions same '' -- capture-pane -p -N -T -t PANE -S 0 -E 2
  case_run capture-preserve-trailing-default same '' -- capture-pane -p -N -t PANE
  case_run capture-mode-screen same '' -- capture-pane -p -M -t PANE -S 0 -E 2
  case_run capture-mode-default same '' -- capture-pane -p -M -t PANE
  case_run capture-alternate same '' -- capture-pane -p -a -t PANE
  case_run capture-alternate-quiet same '' -- capture-pane -p -a -q -t PANE
  case_run capture-control same '' -- capture-pane -p -C -t PANE -S 0 -E 2
  case_run capture-control-escape same '' -- capture-pane -p -C -e -t PANE -S 0 -E 2
  case_run capture-control-buffer same '' -- capture-pane -C -b zzcap -t PANE -S 0 -E 2
  case_run capture-control-buffer-shown same '' -- show-buffer -b zzcap
  case_run capture-control-buffer-deleted same '' -- delete-buffer -b zzcap
  case_run capture-line-numbers same '' -- capture-pane -p -L -t PANE -S 0 -E 2
  case_run capture-line-numbers-history same '' -- capture-pane -p -L -t PANE -S - -E 0
  case_run capture-line-numbers-join same '' -- capture-pane -p -L -J -t PANE -S 0 -E 2
  case_run capture-line-numbers-trailing same '' -- capture-pane -p -L -N -t PANE -S 0 -E 2
  case_run capture-line-numbers-reversed same '' -- capture-pane -p -L -t PANE -S 2 -E 0
  case_run capture-control-line-numbers same '' -- capture-pane -p -C -L -t PANE -S 0 -E 2
  case_run capture-line-numbers-missing same '' -- capture-pane -p -L -t %99
  case_run capture-flags record "$CAPTURE_FLAGS" -- capture-pane -p -F -t PANE -S 0 -E 2
  case_run capture-hyperlinks record "$CAPTURE_LINKS" -- capture-pane -p -H -t PANE -S 0 -E 2
  case_run capture-pending record "$CAPTURE_PENDING" -- capture-pane -p -P -t PANE
  case_run capture-grid record "$CAPTURE_GRID" -- capture-pane -p -R -t PANE
  restore_case capture-restored
}

capture_scene_ready() {
  side_command "$1" capture-pane -p -t '=zzcap-rich:win' | grep -Fq NEXT
}

capture_scene_changed() {
  capture_scene_ready zz &&
    [ "$(zz_command capture-pane -p -e -t '=zzcap-rich:win')" != "$(tmux_inner_command capture-pane -p -e -t '=zzcap-rich:win')" ]
}

rich_capture_case() {
  local name="$1" payload="$2" disposition="$3" reason="$4"
  shift 4
  local command side changed size="${CAPTURE_SIZE:-80x24}"
  printf -v command 'printf %%b %q; exec sleep 600' "$payload"
  run_on_both new-session -d -s zzcap-rich -n win -x "${size%x*}" -y "${size#*x}" "$command"
  run_on_both select-pane -t '=zzcap-rich:win' -T "$PANE_TITLE"
  for side in tmux zz; do
    wait_for "$side printed $name" capture_scene_ready "$side"
    [ "$(side_command "$side" display-message -p -t '=zzcap-rich:win' '#{pane_width}x#{pane_height}')" = "$size" ] ||
      die "$side capture scene is not $size"
  done
  if [ "$SELF_CHECK" -eq 1 ]; then
    self_check_run "$name-equivalence" capture-pane -p -t '=zzcap-rich:win' "$@"
    self_check_expect "$name exact bytes before sabotage" exit=0 stdout=0 stderr=0
    if [ "$name" = capture-low-indexed-colour ]; then
      changed="${payload/38;5;1/31}"
    elif [ "$name" = capture-edited-tab-overwrite-background ]; then
      changed="${payload/41m/42m}"
    elif [[ "$name" == capture-edited-tab-ich-* ]]; then
      changed="${payload/@/m}"
    elif [[ "$name" == capture-edited-tab-dch-* ]]; then
      changed="${payload/\\033\[P/\\033[m}"
      changed="${changed/2P/2m}"
      changed="${changed/5P/5m}"
      changed="${changed/80P/80m}"
    elif [[ "$name" == capture-edited-tab-ech-* ]]; then
      changed="${payload/DEF/XYZ}"
    elif [[ "$name" == capture-edited-tab-il-* ]]; then
      changed="${payload/\\033\[L/\\033[m}"
    elif [[ "$name" == capture-edited-tab-dl-* ]]; then
      changed="${payload/\\033\[M/\\033[m}"
    elif [[ "$name" == capture-edited-tab-scroll-* ]]; then
      changed="${payload/\\n/}"
    elif [ "$name" = capture-real-history ]; then
      changed="${payload//H/Z}"
    elif [[ "$name" == capture-erased-* ]]; then
      changed="${payload/NEXT/NEXT-X}"
    elif [[ "$name" == capture-wide-* ]]; then
      changed="${payload/A/Z}"
    else
      changed="X$payload"
    fi
    printf -v command 'printf %%b %q; exec sleep 600' "$changed"
    zz_command respawn-pane -k -t '=zzcap-rich:win' "$command" >/dev/null ||
      die 'zz refused capture scene sabotage'
    wait_for "zz printed $name sabotage" capture_scene_changed
    self_check_run "$name-sabotage" capture-pane -p -t '=zzcap-rich:win' "$@"
    self_check_expect "$name one-sided scene change" exit=0 stdout=1 stderr=0
  else
    case_run "$name" "$disposition" "$reason" -- capture-pane -p -t '=zzcap-rich:win' "$@"
  fi
  run_on_both kill-session -t '=zzcap-rich'
}

edited_tab_capture_cases() {
  rich_capture_case capture-edited-tab-overwrite-background '\033[44mABC\tDEF\033[0m\r\033[6G\033[41mX\033[0m\033[5;1HNEXT' same '' -C -e -S 0 -E 4
  rich_capture_case capture-edited-tab-ich-off-line '\033[73GABC\tZ\r\033[70G\033[6@\033[5;1HNEXT' same '' -C -S 0 -E 4
  rich_capture_case capture-edited-tab-dch-entire 'ABC\tDEF\r\033[4G\033[5P\033[5;1HNEXT' same '' -C -S 0 -E 4
  rich_capture_case capture-edited-tab-dch-off-line 'ABC\tDEF\r\033[80P\033[5;1HNEXT' same '' -C -S 0 -E 4
  rich_capture_case capture-edited-tab-dch-overwrite 'ABC\tDEF\r\033[5G\033[P\033[6GX\033[5;1HNEXT' same '' -C -S 0 -E 4
  rich_capture_case capture-edited-tab-ech-entire 'ABC\tDEF\r\033[4G\033[5X\033[5;1HNEXT' same '' -C -S 0 -E 4
  rich_capture_case capture-edited-tab-il-off-region '\033[2;3r\033[3;1HABC\tDEF\033[2;1H\033[L\033[r\033[5;1HNEXT' same '' -C -S 0 -E 4
  rich_capture_case capture-edited-tab-dl-tab 'TOP\r\nABC\tDEF\r\nBOTTOM\033[2;1H\033[M\033[5;1HNEXT' same '' -C -S 0 -E 4
  rich_capture_case capture-edited-tab-scroll-off-region '\033[2;4r\033[2;1HABC\tDEF\033[4;1H\n\033[r\033[5;1HNEXT' same '' -C -S 0 -E 4
  [ "$SELF_CHECK" -eq 0 ] || return 0
  rich_capture_case capture-tab-trailing 'ABC\t\r\nNEXT' record "$CAPTURE_TABS" -C -S 0 -E 4
  rich_capture_case capture-tab-internal 'ABC\tDEF\r\nNEXT' record "$CAPTURE_TABS" -C -S 0 -E 4
  rich_capture_case capture-tab-wide '界\t\r\nNEXT' record "$CAPTURE_TABS" -C -S 0 -E 4
  rich_capture_case capture-edited-tab-ich-middle 'ABC\tDEF\r\033[5G\033[@X\033[5;1HNEXT' record "$CAPTURE_TABS" -C -S 0 -E 4
  rich_capture_case capture-edited-tab-ich-before 'ABC\tDEF\r\033[2G\033[2@\033[5;1HNEXT' record "$CAPTURE_TABS" -C -S 0 -E 4
  rich_capture_case capture-edited-tab-ich-head 'ABC\tDEF\r\033[4G\033[@\033[5;1HNEXT' record "$CAPTURE_TABS" -C -S 0 -E 4
  rich_capture_case capture-edited-tab-ich-truncate '\033[73GABC\tZ\r\033[70G\033[2@\033[5;1HNEXT' record "$CAPTURE_TABS" -C -S 0 -E 4
  rich_capture_case capture-edited-tab-ich-overwrite 'ABC\tDEF\r\033[5G\033[@X\033[7GY\033[5;1HNEXT' record "$CAPTURE_TABS" -C -S 0 -E 4
  rich_capture_case capture-edited-tab-dch-middle 'ABC\tDEF\r\033[5G\033[P\033[5;1HNEXT' record "$CAPTURE_TABS" -C -S 0 -E 4
  rich_capture_case capture-edited-tab-dch-before 'ABC\tDEF\r\033[2G\033[2P\033[5;1HNEXT' record "$CAPTURE_TABS" -C -S 0 -E 4
  rich_capture_case capture-edited-tab-dch-head 'ABC\tDEF\r\033[4G\033[P\033[5;1HNEXT' record "$CAPTURE_TABS" -C -S 0 -E 4
  rich_capture_case capture-edited-tab-ech-middle 'ABC\tDEF\r\033[5G\033[2X\033[5;1HNEXT' record "$CAPTURE_TABS" -C -S 0 -E 4
  rich_capture_case capture-edited-tab-ech-head 'ABC\tDEF\r\033[4G\033[X\033[5;1HNEXT' record "$CAPTURE_TABS" -C -S 0 -E 4
  rich_capture_case capture-edited-tab-il-shift 'TOP\r\nABC\tDEF\r\nBOTTOM\033[2;1H\033[L\033[5;1HNEXT' record "$CAPTURE_TABS" -C -S 0 -E 4
  rich_capture_case capture-edited-tab-dl-shift 'TOP\r\nABC\tDEF\r\nBOTTOM\033[1;1H\033[M\033[5;1HNEXT' record "$CAPTURE_TABS" -C -S 0 -E 4
  rich_capture_case capture-edited-tab-scroll-up '\033[2;4r\033[3;1HABC\tDEF\033[4;1H\n\033[r\033[5;1HNEXT' record "$CAPTURE_TABS" -C -S 0 -E 4
  rich_capture_case capture-edited-tab-scroll-down '\033[2;4r\033[2;1HABC\tDEF\033[2;1H\033M\033[r\033[5;1HNEXT' record "$CAPTURE_TABS" -C -S 0 -E 4
}

erased_wide_capture_cases() {
  local scene payload
  for scene in line display clear region; do
    case "$scene" in
    line) payload='\033[H\033[2J\033[41m\033[2K\033[0m\r\nNEXT' ;;
    display) payload='\033[H\033[2J\033[41m\033[2J\033[0m\r\nNEXT' ;;
    clear) payload='\033[H\033[2J\033[41m\033[H\033[2J\033[0m\r\nNEXT' ;;
    region) payload='\033[H\033[2J\033[2;4r\033[2;1H\033[41m\033[2K\033[0m\r\nNEXT\033[r' ;;
    esac
    CAPTURE_SIZE=100x30 rich_capture_case "capture-erased-100x30-$scene-escape" "$payload" same '' -C -e -S 0 -E 4
    CAPTURE_SIZE=100x30 rich_capture_case "capture-erased-100x30-$scene-padding" "$payload" same '' -e -N -S 0 -E 4
  done
}

rich_capture_cases() {
  local wrap history='' row
  printf -v wrap '%*s' 170 ''
  wrap="${wrap// /A}"
  rich_capture_case capture-colour-wrap "\033[31m${wrap}\033[0m\r\nNEXT" same '' -L -e -J -S 0 -E 3
  rich_capture_case capture-real-wrap "${wrap}\r\nNEXT" same '' -L -J -S 0 -E 3
  for ((row = 0; row < 35; row++)); do
    printf -v history '%sH%02d\\r\\n' "$history" "$row"
  done
  rich_capture_case capture-real-history "${history}NEXT" same '' -L -S -3 -E 0
  rich_capture_case capture-named-colour '\033[31mRED\033[0m\r\nNEXT' same '' -C -e -S 0 -E 0
  rich_capture_case capture-named-background '\033[32;44mCOLOUR\033[0m\r\nNEXT' same '' -C -e -S 0 -E 0
  rich_capture_case capture-bright-colours '\033[91;104mBRIGHT\033[0m\r\nNEXT' same '' -C -e -S 0 -E 0
  rich_capture_case capture-indexed-colour '\033[38;5;196mINDEX\033[0m\r\nNEXT' same '' -C -e -S 0 -E 0
  rich_capture_case capture-rgb-colour '\033[38;2;1;2;3mRGB\033[0m\r\nNEXT' same '' -C -e -S 0 -E 0
  rich_capture_case capture-bold '\033[1mBOLD\033[0m\r\nNEXT' same '' -C -e -S 0 -E 0
  rich_capture_case capture-underline '\033[4mUNDER\033[0m\r\nNEXT' same '' -C -e -S 0 -E 0
  rich_capture_case capture-attribute-reset '\033[1;4;31mONE\033[22mTWO\033[0m\r\nNEXT' same '' -C -e -S 0 -E 0
  rich_capture_case capture-unicode-lines '───\r\nNEXT' same '' -C -S 0 -E 0
  rich_capture_case capture-erased-background-join '\033[41m\033[2K\033[0m\r\nNEXT' same '' -L -e -J -S 0 -E 4
  rich_capture_case capture-erased-background-trim '\033[41m\033[2K\033[0m\r\nNEXT' same '' -e -T -S 0 -E 4
  rich_capture_case capture-erased-display-join '\033[41m\033[2J\033[0m\r\nNEXT' same '' -L -e -J -S 0 -E 4
  rich_capture_case capture-erased-display-trim '\033[41m\033[2J\033[0m\r\nNEXT' same '' -e -T -S 0 -E 4
  printf -v wrap '%*s' 79 ''
  wrap="${wrap// /A}"
  rich_capture_case capture-wide-wrap-escape "\033[31m${wrap}界界\033[0mNEXT" same '' -C -e -S 0 -E 4
  rich_capture_case capture-wide-wrap-padding "\033[31m${wrap}界界\033[0mNEXT" same '' -e -N -S 0 -E 4
  rich_capture_case capture-wide-wrap-join "\033[31m${wrap}界界\033[0mNEXT" same '' -L -e -J -S 0 -E 4
  erased_wide_capture_cases
  edited_tab_capture_cases
  rich_capture_case capture-low-indexed-colour '\033[38;5;1mRED\033[0m\r\nNEXT' same '' -C -e -S 0 -E 0
  if [ "$SELF_CHECK" -eq 0 ]; then
    rich_capture_case capture-charset-text '\033(0qqq\033(B\r\nNEXT' record "$CAPTURE_CHARSET" -C -S 0 -E 0
    rich_capture_case capture-charset-escape '\033(0qqq\033(B\r\nNEXT' record "$CAPTURE_CHARSET" -C -e -S 0 -E 0
  fi
}

buffer_stream_cases() {
  CASE_STDIN='piped-payload'
  case_run buffer-load-stdin same '' -- load-buffer -b zzpiped -
  case_run buffer-show-named same '' -- show-buffer -b zzpiped
  case_run buffer-save-stdout same '' -- save-buffer -b zzpiped -
  case_run buffer-save-append same '' -- save-buffer -a -b zzpiped -
  case_run buffer-show-missing same '' -- show-buffer -b zzcc-nope
  case_run buffer-delete same '' -- delete-buffer -b zzpiped
  case_run buffer-show-empty same '' -- show-buffer
  CASE_STDIN='set -g @zzcc-stream one'
  case_run stream-source-file same '' -- source-file -
  CASE_STDIN=''
  case_run stream-source-file-effect same '' -- show-options -gqv @zzcc-stream
  CASE_STDIN='typed-into-the-pane'
  case_run stream-display-message same '' -- display-message -I
  case_run stream-split-window same '' -- split-window -I -t PANE
  CASE_STDIN=''
  restore_extra_panes stream-split-window-restored
}

message_hook_cases() {
  case_run hooks-show same '' -- show-hooks
  case_run hooks-show-global same '' -- show-hooks -g
  case_run hooks-show-window same '' -- show-hooks -w
  case_run hooks-show-pane same '' -- show-hooks -p
  case_run hooks-show-before same '' -- show-hooks -B
  case_run hooks-show-one same '' -- show-hooks -g after-lock-server
  case_run hooks-show-missing same '' -- show-hooks -g no-such-hook
  case_run hooks-show-target same '' -- show-hooks -t PANE
  case_run messages-log record "$LOG_IDENTITY" -- show-messages
  case_run messages-jobs same '' -- show-messages -J
  CASE_NORMALIZE="$PER_PROCESS_NUMBERS"
  case_run messages-terminals same '' -- show-messages -T
  CASE_NORMALIZE="$PER_PROCESS_NUMBERS"
  case_run messages-terminals-target same '' -- show-messages -T -t CLIENT
  CASE_NORMALIZE="$PER_PROCESS_NUMBERS"
  case_run messages-terminals-missing-target same '' -- show-messages -T -t /dev/zzcc-nope
  CASE_NORMALIZE="$PER_PROCESS_NUMBERS"
  case_run messages-jobs-and-terminals same '' -- show-messages -JT
  message_live_job_cases
}

# A -J table that is not empty. What puts a row in the pin's is format_job_get:
# one job tree per client plus one for a clientless expansion, keyed by the
# format tag and the command, and job_print_summary walks job.c `all_jobs` over
# all of them. One #() in status-left with one client attached is therefore ONE
# row on the pin, and arming the same one on zz is the only way to compare a
# live table. The job's fd and pid belong to its own child, so they go through
# the same PER_PROCESS_NUMBERS substitution on both sides; the command beside
# them does not. `sleep` outlives the case, so what the status draws is the
# pin's own `<'cmd' not ready>` placeholder on both sides.
LIVE_JOB_COMMAND='#(sleep 40; echo zzcc-job)'

job_rows_are() {
  [ "$(side_command "$1" show-messages -J 2>/dev/null | grep -c '^Job ')" = "$2" ]
}

job_status_pending() {
  capture_plain "$1" | tail -n 1 | grep -Fq "<'sleep 40"
}

message_live_job_cases() {
  local interval
  interval="$(tmux_inner_command show-options -gv status-interval)"
  CASE_LABEL=messages-jobs-live
  set_on_both status-interval 1
  set_on_both status-left "$LIVE_JOB_COMMAND"
  wait_for 'the pin armed one format job' job_rows_are tmux 1
  wait_for 'zz armed one format job' job_rows_are zz 1
  wait_for 'the pin drew its pending format job' job_status_pending tmux
  wait_for 'zz drew its pending format job' job_status_pending zz
  CASE_NORMALIZE="$PER_PROCESS_NUMBERS"
  case_run messages-jobs-live same '' -- show-messages -J
  set_on_both status-left L
  set_on_both status-interval "$interval"
  restore_case messages-jobs-live-restored
}

lock_target_cases() {
  local spec name target command
  for spec in 'window|=cli:win' 'pane|=cli:win.0' 'pane-id|%0' \
    'missing-window|=cli:nosuchwin' 'missing-pane-id|%99' 'missing-pane|=cli:win.9' \
    'missing-session|=nosuch:win' 'exact-pane-name|=%0' 'exact-session|=cl:win' \
    'session-pane-id|cli:.%1' 'empty-exact-window|cli:=' 'empty-exact-session|=:'; do
    name="${spec%%|*}"
    target="${spec#*|}"
    run_on_both set-option -gu @zzcc-locked
    case_run "lock-target-session-$name" same '' -- lock-session -t "$target"
    case_run "lock-target-hook-$name" same '' -- show-options -gqv @zzcc-locked
    case_run "lock-target-has-session-$name" same '' -- has-session -t "$target"
    case_run "lock-target-list-windows-$name" same '' -- list-windows -t "$target" -F '#{window_index}:#{window_name}'
  done
  for spec in 'empty-exact-window-pane|cli:=.0' 'empty-exact-window-pane-id|cli:=.%0'; do
    name="${spec%%|*}"
    target="${spec#*|}"
    for command in lock-session has-session list-windows; do
      case_run "lock-target-$name-$command" same '' -- "$command" -t "$target"
    done
  done
  run_on_both new-session -d -s zzcc-foreign -n foreign "$INNER_SHELL"
  local foreign_pane
  foreign_pane="$(tmux_inner_command display-message -p -t '=zzcc-foreign:' '#{pane_id}')"
  for command in lock-session has-session list-windows; do
    case_run "lock-target-global-pane-$command" same '' -- "$command" -t ":.$foreign_pane"
  done
  run_on_both kill-session -t '=zzcc-foreign'
  run_on_both set-option -gw pane-base-index 1
  case_run lock-target-base-index-lock-session same '' -- lock-session -t '=cli:win.1'
  case_run lock-target-base-index-has-session same '' -- has-session -t '=cli:win.1'
  case_run lock-target-base-index-list-windows same '' -- list-windows -t '=cli:win.1'
  run_on_both set-option -gw pane-base-index 0
}

lock_cases() {
  run_on_both set-hook -g after-lock-server 'set -g @zzcc-locked yes'
  case_run lock-server cli "$LOCK_PROGRAM" -- lock-server
  restore_case lock-server-restored
  case_run lock-server-hook same '' -- show-options -gv @zzcc-locked
  case_run lock-server-arity same '' -- lock-server zzcc-extra
  case_run lock-server-unknown-flag same '' -- lock-server -t zzcc-nope
  run_on_both set-option -gu @zzcc-locked
  case_run lock-session cli "$LOCK_PROGRAM" -- lock-session -t "=$INNER_SESSION"
  restore_case lock-session-restored
  case_run lock-session-no-server-hook same '' -- show-options -gqv @zzcc-locked
  run_on_both set-option -gu @zzcc-locked
  case_run lock-session-current cli "$LOCK_PROGRAM" -- lock-session
  restore_case lock-session-current-restored
  case_run lock-session-current-no-server-hook same '' -- show-options -gqv @zzcc-locked
  case_run lock-session-missing same '' -- lock-session -t zzcc-nope
  case_run lock-session-arity same '' -- lock-session zzcc-extra
  run_on_both set-option -gu @zzcc-locked
  case_run lock-client same '' -- lock-client -t CLIENT
  restore_case lock-client-restored
  case_run lock-client-no-server-hook same '' -- show-options -gqv @zzcc-locked
  case_run lock-client-current cli "$LOCK_PROGRAM" -- lock-client
  restore_case lock-client-current-restored
  case_run lock-client-missing same '' -- lock-client -t /dev/zzcc-nope
  case_run lock-client-arity same '' -- lock-client zzcc-extra
  case_run lock-client-missing-argument same '' -- lock-client -t
  case_run lock-hook-session-name same '' -- set-hook -g after-lock-session 'set -g @zzcc-locked no'
  case_run lock-hook-client-name same '' -- set-hook -g after-lock-client 'set -g @zzcc-locked no'
  case_run lock-after-time-store same '' -- show-options -g lock-after-time
  case_run lock-command-store same '' -- show-options -g lock-command
  case_run lock-after-time-invalid same '' -- set-option -g lock-after-time zzcc-nope
  case_run lock-command-session-set same '' -- set-option -t "$INNER_SESSION" lock-command zzcc-locker
  case_run lock-command-session-store same '' -- show-options -t "$INNER_SESSION" lock-command
  case_run lock-after-time-session-set same '' -- set-option -t "$INNER_SESSION" lock-after-time 45
  case_run lock-after-time-session-store same '' -- show-options -t "$INNER_SESSION" lock-after-time
  case_run lock-command-session-unset same '' -- set-option -t "$INNER_SESSION" -u lock-command
  case_run lock-after-time-session-unset same '' -- set-option -t "$INNER_SESSION" -u lock-after-time
  case_run lock-session-options-restored same '' -- show-options -t "$INNER_SESSION" lock-command
  lock_target_cases
}

client_tool_cases() {
  CASE_NEEDLE_MODE=1
  case_run client-tree-open record "$CLIENT_TREE_CLIENTLESS" -- choose-client -t PANE
  restore_case client-tree-closed
  case_run client-tree-unknown-flag same '' -- choose-client -Q -t PANE
  case_run client-tree-bad-sort same '' -- choose-client -O zzcc-nope -t PANE
  case_run client-tree-usage same '' -- choose-client -t PANE one two
  CASE_NEEDLE_MODE=1
  CASE_CLOCK_FACE=1
  case_run clock-mode-open same '' -- clock-mode -t PANE
  restore_case clock-mode-closed
  CASE_CLOCK_FACE=1
  case_run clock-injected-open same '' -- clock-mode -t PANE
  case_run clock-injected-key same '' -- send-keys -t PANE x
  case_run clock-injected-shell same '' -- capture-pane -p -t PANE
  CASE_CLOCK_FACE=1
  case_run clock-injected-pair-open same '' -- clock-mode -t PANE
  case_run clock-injected-pair same '' -- send-keys -t PANE xy
  case_run clock-injected-pair-shell same '' -- capture-pane -p -t PANE
  case_run clock-injected-pair-clear same '' -- send-keys -t PANE C-u
  run_both clock-mode -t PANE
  case_run clock-injected-byte same '' -- send-keys -H -t PANE ff
  run_both clock-mode -t PANE
  case_run clock-injected-prefix same '' -- send-prefix -t PANE
  run_both clock-mode -t PANE
  case_run clock-injected-backtab same '' -- send-keys -t PANE BTab
  run_both clock-mode -t PANE
  case_run clock-injected-keypad same '' -- send-keys -t PANE KPEnter
  run_both switch-mode -t PANE
  case_run switch-injected-escape same '' -- send-keys -H -t PANE 1b
  CASE_CLOCK_FACE=1
  case_run clock-cancel-open same '' -- clock-mode -t PANE
  case_run clock-cancel-all same '' -- copy-mode -q -t PANE
  case_run clock-cancel-shell same '' -- capture-pane -p -t PANE
  CASE_CLOCK_FACE=1
  case_run stack-clock same '' -- clock-mode -t PANE
  case_run stack-switch same '' -- switch-mode -t PANE
  case_run stack-depth same '' -- display-message -p -t PANE '#{pane_in_mode}/#{pane_mode}'
  local side
  for side in tmux zz; do
    tmux_outer_command send-keys -t "=$OUTER_SESSION:$side" Escape
  done
  CASE_CLOCK_FACE=1
  case_run stack-restored-clock same '' -- display-message -p -t PANE '#{pane_in_mode}/#{pane_mode}'
  case_run stack-terminal same '' -- send-keys -t PANE x
  case_run stack-terminal-shell same '' -- capture-pane -p -t PANE
  run_both copy-mode -t PANE
  CASE_CLOCK_FACE=1
  case_run clock-over-copy record 'DECIDED 2026-09-17 (fabrico): copy mode stays per client so two clients can scroll independently; the pin keeps one mode stack per pane. TUI-014 follows the dated copy-mode amendment in knowledge/designs/tui-parity.md.' -- clock-mode -t PANE
  for side in tmux zz; do
    tmux_outer_command send-keys -t "=$OUTER_SESSION:$side" x
  done
  case_run clock-over-copy-key record 'DECIDED 2026-09-17 (fabrico): copy mode stays per client so two clients can scroll independently; the pin keeps one mode stack per pane. TUI-014 follows the dated copy-mode amendment in knowledge/designs/tui-parity.md.' -- display-message -p -t PANE '#{pane_in_mode}/#{pane_mode}'
  run_both copy-mode -q -t PANE
  restore_case clock-over-copy-restored
  run_both clock-mode -t PANE
  case_run copy-over-clock record 'DECIDED 2026-09-17 (fabrico): copy mode stays per client so two clients can scroll independently; the pin keeps one mode stack per pane. TUI-014 follows the dated copy-mode amendment in knowledge/designs/tui-parity.md.' -- copy-mode -t PANE
  run_both copy-mode -q -t PANE
  restore_case copy-over-clock-restored
  set_window_on_both clock-mode-colour '#ff00aa'
  set_window_on_both clock-mode-style 12
  CASE_NEEDLE_MODE=1
  CASE_CLOCK_FACE=1
  case_run clock-mode-twelve same '' -- clock-mode -t PANE
  restore_case clock-mode-twelve-closed
  local clock_style
  for clock_style in 24-with-seconds 12-with-seconds; do
    set_window_on_both clock-mode-style "$clock_style"
    CASE_CLOCK_FACE=2
    case_run "clock-mode-$clock_style" same '' -- clock-mode -t PANE
    restore_case "clock-mode-$clock_style-closed"
  done
  run_on_both set-option -gwu clock-mode-colour
  run_on_both set-option -gwu clock-mode-style
  CASE_NEEDLE_MODE=1
  case_run customize-mode-open same '' -- customize-mode -t PANE
  restore_case customize-mode-closed
  customize_fix_cases
  CASE_NEEDLE_MODE=1
  case_run switch-mode same '' -- switch-mode -t PANE
  restore_case switch-mode-closed
  case_run switch-mode-format same '' -- switch-mode -F 'REVIEW-#{session_name}' -t PANE
  restore_case switch-mode-format-closed
  case_run switch-mode-template same '' -- switch-mode -t PANE 'set-option -g @review-command "%%"'
  case_run switch-mode-control-j same '' -- send-keys -t PANE C-j
  case_run switch-mode-linefeed same '' -- send-keys -H -t PANE 0a
  for side in tmux zz; do
    tmux_outer_command send-keys -t "=$OUTER_SESSION:$side" Enter
  done
  case_run switch-mode-template-result same '' -- show-options -gv @review-command
  run_on_both set-option -gu @review-command
  switch_lifetime_cases
  CASE_GRID_CELLS=1
  case_run switch-mode-history-cells same '' -- switch-mode -w -t PANE
  CASE_GRID_CELLS=0
  restore_case switch-mode-history-closed
  attach_both_at 80 24
  CASE_NEEDLE_MODE=1
  case_run switch-mode-windows same '' -- switch-mode -w -t PANE
  restore_case switch-mode-windows-closed
  attach_both_at 80 24
  run_on_both new-session -d -s alpha -n "$WINDOW_NAME" -x 80 -y 24 "$INNER_SHELL"
  run_on_both new-session -d -s zulu -n "$WINDOW_NAME" -x 80 -y 24 "$INNER_SHELL"
  case_run switch-mode-duplicate-windows same '' -- switch-mode -w -t PANE
  restore_case switch-mode-duplicate-windows-closed
  case_run switch-mode-window-order same '' -- switch-mode -w -F '#{session_name}:#{window_name}' -t PANE
  restore_case switch-mode-window-order-closed
  run_on_both kill-session -t '=alpha'
  run_on_both kill-session -t '=zulu'
  restore_case switch-mode-window-order-restored
  switch_tail_style_cases
  case_run server-access-bare same '' -- server-access
  case_run server-access-formatted same '' -- server-access '#{?#{==:1,1},nobody,root}'
  case_run server-access-user same '' -- server-access -w zzcc-nobody
  case_run server-access-list same '' -- server-access -l
  case_run server-access-owner same '' -- server-access -a "$SERVER_OWNER"
  case_run server-access-unknown-group same '' -- server-access -g zzcc-nogroup
  case_run server-access-both-actions same '' -- server-access -g -a -d "$SERVER_GROUP"
  case_run server-access-both-rights same '' -- server-access -g -r -w "$SERVER_GROUP"
  case_run server-access-deny same '' -- server-access -g -d "$SERVER_GROUP"
  case_run server-access-no-action same '' -- server-access -g "$SERVER_GROUP"
  case_run server-access-add record "$SERVER_ACCESS" -- server-access -g -a "$SERVER_GROUP"
  drop_pin_access_entry server-access-restored
  restore_case client-tools-restored
}

native_usage_run() {
  local status=0
  zz_command "$@" >"$SCRATCH_DIR/native.out" 2>"$SCRATCH_DIR/native.err" </dev/null || status=$?
  printf '%s\n' "$status" >"$SCRATCH_DIR/native.rc"
}

native_usage_matches() {
  printf '%s\n' "$1" >"$SCRATCH_DIR/native.expected"
  [ "$(cat "$SCRATCH_DIR/native.rc")" = 2 ] &&
    [ ! -s "$SCRATCH_DIR/native.out" ] &&
    cmp -s "$SCRATCH_DIR/native.err" "$SCRATCH_DIR/native.expected"
}

native_usage_case() {
  local name="$1" expected="$2"
  shift 2
  native_usage_run "$@"
  CHECKS=$((CHECKS + 1))
  if native_usage_matches "$expected"; then
    printf 'ok    %s: native usage exits 2, exact stdout and stderr\n' "$name"
  else
    FAILURES=$((FAILURES + 1))
    printf 'DIFF  %s: native usage expected exit 2, got %s\n' "$name" "$(cat "$SCRATCH_DIR/native.rc")"
    cat "$SCRATCH_DIR/native.out" "$SCRATCH_DIR/native.err"
  fi
}

usage_contract_cases() {
  native_usage_case native-verb-usage 'reload-config does not take arguments' reload-config extra
  native_usage_case native-bound-usage 'agent-send does not support --bogus' \
    bind-key F9 agent-send --bogus
  native_usage_case native-json-format-usage 'command list-panes: --json cannot be combined with -F' \
    list-panes --json -F x
  native_usage_case native-option-usage 'unsupported command: set-option -a history-trickle' \
    set-option -a history-trickle 1
  case_run tmux-unknown-flag same '' -- list-panes -Z
  case_run tmux-unknown-command same '' -- zzcc-unknown-command
}

run_cases() {
  printf 'stock client-command roster at %sx%s (pin %s)\n' \
    "$COLUMNS_UNDER_TEST" "$ROWS_UNDER_TEST" "$(basename -- "$TMUX_BIN")"
  attach_both_at 80 24
  case_run baseline same '' -- display-message -p -t PANE '#{window_index}.#{pane_index}'
  usage_contract_cases
  refresh_client_cases
  capture_pane_cases
  rich_capture_cases
  buffer_stream_cases
  message_hook_cases
  lock_cases
  client_tool_cases
  # LAST, and nothing may follow it: cmd-detach-client.c sends SIGTSTP to the
  # client process, so the pin's attached client stops and its session loses
  # its client for the rest of the run. The measurement is worth a case; a
  # stopped client underneath every later case is not.
  SUSPENDED_ZZ_PID="$(zz_command list-clients -F '#{client_pid}')"
  case_run suspend-client same '' -- suspend-client
  wait_for 'the raw client is stopped' client_process_stopped "$SUSPENDED_ZZ_PID"

  if [ "$FAILURES" -ne 0 ]; then
    printf '%s of %s asserted comparisons differ, %s recorded (%s for a sibling lane, %s)\n' \
      "$FAILURES" "$CHECKS" "$RECORDS" "$SIBLINGS" "$(owner_tally)"
    exit 1
  fi
  printf 'all %s asserted comparisons identical, %s recorded not asserted (%s for a sibling lane, %s)\n' \
    "$CHECKS" "$RECORDS" "$SIBLINGS" "$(owner_tally)"
}

# --- self-check -------------------------------------------------------------
SELF_CHECK_FAILURES=0

self_check_expect() {
  local name="$1"
  shift
  local outcome=ok pair channel want got
  for pair in "$@"; do
    channel="${pair%%=*}"
    want="${pair##*=}"
    case "$channel" in
    exit) got="$LAST_EXIT_DIFFERED" ;;
    stdout) got="$LAST_STDOUT_DIFFERED" ;;
    stderr) got="$LAST_STDERR_DIFFERED" ;;
    screen) got="$LAST_SCREEN_DIFFERED" ;;
    state) got="$LAST_STATE_DIFFERED" ;;
    *) die "unknown self-check channel $channel" ;;
    esac
    if [ "$got" != "$want" ]; then
      outcome="$channel reported $got, expected $want"
      break
    fi
  done
  if [ "$outcome" = ok ]; then
    printf 'ok    self-check %s\n' "$name"
    return 0
  fi
  SELF_CHECK_FAILURES=$((SELF_CHECK_FAILURES + 1))
  printf 'FAIL  self-check %s: %s\n' "$name" "$outcome"
}

client_process_stopped() {
  ps -o stat= -p "$1" | grep -q '^T'
}

self_check_run() {
  local name="$1"
  shift
  CASE_LABEL="$name"
  run_both "$@"
  settle_screen zz ''
  settle_screen tmux ''
  compare_channels "$name" || true
}

zz_status_left_is() {
  [ "$(zz_command display-message -p '#{status-left}' 2>/dev/null)" = "$1" ]
}

zz_cursor_is() {
  [ "$(cursor_tuple zz)" = "$1" ]
}
zz_cursor_is_not() {
  [ "$(cursor_tuple zz)" != "$1" ]
}

lock_target_sabotages() {
  local command pane target expected_stdout
  for target in 'cli:=' '=:' 'cli:=.0' 'cli:=.%0'; do
    for command in lock-session has-session list-windows; do
      self_check_run "$command-$target-equivalence" "$command" -t "$target"
      self_check_expect "$command accepts $target" exit=0 stdout=0 stderr=0
      printf '1\n' >"$SCRATCH_DIR/zz.rc"
      printf "can't find window: \n" >"$SCRATCH_DIR/zz.err"
      compare_channels "$command-$target-rejection-sabotage" || true
      self_check_expect "$command rejects $target on zz only" exit=1 stderr=1
    done
  done
  run_on_both set-option -gw pane-base-index 1
  for command in lock-session has-session list-windows; do
    self_check_run "$command-base-index-equivalence" "$command" -t '=cli:win.1'
    self_check_expect "$command resolves pane-base-index 1" exit=0 stdout=0 stderr=0
    zz_command set-option -gw pane-base-index 2 >/dev/null
    self_check_run "$command-base-index-sabotage" "$command" -t '=cli:win.1'
    expected_stdout=0
    [ "$command" != list-windows ] || expected_stdout=1
    self_check_expect "$command loses configured pane 1 on zz only" exit=1 "stdout=$expected_stdout" stderr=1
    zz_command set-option -gw pane-base-index 1 >/dev/null
  done
  run_on_both set-option -gw pane-base-index 0
  zz_command new-window -d -t "=$INNER_SESSION" -n zzcc-target "$INNER_SHELL" >/dev/null ||
    die 'zz refused the one-sided window target'
  pane="$(zz_command display-message -p -t "=$INNER_SESSION:zzcc-target" '#{pane_id}')"
  for target in "=$INNER_SESSION:zzcc-target" "$pane" ":.$pane"; do
    for command in lock-session has-session list-windows; do
      self_check_run "$command-$target-sabotage" "$command" -t "$target"
      expected_stdout=0
      [ "$command" != list-windows ] || expected_stdout=1
      self_check_expect "$command target $target exists on zz only" exit=1 "stdout=$expected_stdout" stderr=1
    done
  done
  zz_command kill-window -t "=$INNER_SESSION:zzcc-target" >/dev/null || die 'zz refused kill-window'
  zz_command split-window -d -t "=$INNER_SESSION:$WINDOW_NAME" "$INNER_SHELL" >/dev/null ||
    die 'zz refused the one-sided pane index'
  for command in lock-session has-session list-windows; do
    self_check_run "$command-pane-component-sabotage" "$command" -t "=$INNER_SESSION:$WINDOW_NAME.1"
    expected_stdout=0
    [ "$command" != list-windows ] || expected_stdout=1
    self_check_expect "$command pane index exists on zz only" exit=1 "stdout=$expected_stdout" stderr=1
  done
  zz_command kill-pane -t "=$INNER_SESSION:$WINDOW_NAME.1" >/dev/null || die 'zz refused kill-pane'
  settle_screen zz ''
}

run_self_check() {
  printf 'self-check: one deliberate one-sided difference per channel, plus two equivalences\n'
  attach_both_at 80 24

  # The equivalence first: with nothing planted the same command on both sides
  # must report nothing, or every sabotage below would be satisfied by a
  # comparison that always reports.
  self_check_run equivalence-before display-message -p -t PANE '#{window_index}.#{pane_index}'
  self_check_expect 'equivalence: the same command on both sides' \
    exit=0 stdout=0 stderr=0 screen=0 state=0

  local usage_case
  for usage_case in refresh-missing-argument client-tree-unknown-flag client-tree-usage tmux-unknown-flag tmux-unknown-command; do
    case "$usage_case" in
      refresh-missing-argument) run_both refresh-client -r ;;
      client-tree-unknown-flag) run_both choose-client -Q -t PANE ;;
      client-tree-usage) run_both choose-client -t PANE one two ;;
      tmux-unknown-flag) run_both list-panes -Z ;;
      tmux-unknown-command) run_both zzcc-unknown-command ;;
    esac
    compare_channels "$usage_case-control" || true
    self_check_expect "$usage_case control" exit=0 stdout=0 stderr=0 screen=0 state=0
    printf '2\n' >"$SCRATCH_DIR/zz.rc"
    compare_channels "$usage_case-sabotage" || true
    self_check_expect "$usage_case rejects exit 2" exit=1 stdout=0 stderr=0 screen=0 state=0
  done
  local native_case expected
  for native_case in native-verb-usage native-bound-usage native-json-format-usage native-option-usage; do
    case "$native_case" in
      native-verb-usage)
        native_usage_run reload-config extra
        expected='reload-config does not take arguments'
        ;;
      native-option-usage)
        native_usage_run set-option -a history-trickle 1
        expected='unsupported command: set-option -a history-trickle'
        ;;
      native-bound-usage)
        native_usage_run bind-key F9 agent-send --bogus
        expected='agent-send does not support --bogus'
        ;;
      native-json-format-usage)
        native_usage_run list-panes --json -F x
        expected='command list-panes: --json cannot be combined with -F'
        ;;
    esac
    if native_usage_matches "$expected"; then
      printf 'ok    self-check %s control\n' "$native_case"
    else
      SELF_CHECK_FAILURES=$((SELF_CHECK_FAILURES + 1))
      printf 'FAIL  self-check %s control\n' "$native_case"
    fi
    printf '1\n' >"$SCRATCH_DIR/native.rc"
    if native_usage_matches "$expected"; then
      SELF_CHECK_FAILURES=$((SELF_CHECK_FAILURES + 1))
      printf 'FAIL  self-check %s accepted exit 1\n' "$native_case"
    else
      printf 'ok    self-check %s rejects exit 1\n' "$native_case"
    fi
  done

  # stdout: a buffer that exists on the zz side only, listed by name. The exit
  # status and stderr are the same on both sides and must not be reported.
  zz_command set-buffer -b zzcc-sabotage SABOTAGE >/dev/null || die 'zz refused set-buffer'
  self_check_run stdout-sabotage list-buffers -F '#{buffer_name}'
  self_check_expect 'stdout, a buffer on one side only' exit=0 stdout=1 stderr=0

  # exit status and stderr: showing that same buffer, which one side does not
  # have. stdout differs too - that is what an error instead of a payload
  # means - and the two channels under test have to be reported by themselves.
  # stdout again, with no buffer behind it: an access entry the pin alone holds,
  # listed by server-access -l. Adding one is the single form zz refuses, so the
  # entry can only be planted on the pin, and a zz -l that answered the wrong
  # owner would be caught here rather than by the buffer sabotage above.
  tmux_inner_command server-access -g -a "$SERVER_GROUP" >/dev/null ||
    die 'the pin refused server-access -a'
  self_check_run access-sabotage server-access -l
  self_check_expect 'stdout, an access entry on the pin only' exit=0 stdout=1 stderr=0
  tmux_inner_command server-access -g -d "$SERVER_GROUP" >/dev/null ||
    die 'the pin refused server-access -d'

  self_check_run access-format-equivalence server-access '#{?#{==:1,1},nobody,root}'
  self_check_expect 'formatted identity resolves before lookup' exit=0 stdout=0 stderr=0
  zz_command server-access '#{?#{==:1,1},zzcc-missing,root}' >"$SCRATCH_DIR/zz.out" 2>"$SCRATCH_DIR/zz.err" && rc=0 || rc=$?
  printf '%s\n' "$rc" >"$SCRATCH_DIR/zz.rc"
  compare_channels access-format-sabotage || true
  self_check_expect 'one-sided formatted identity failure changes stderr and status' exit=1 stdout=0 stderr=1

  self_check_run exit-sabotage show-buffer -b zzcc-sabotage
  self_check_expect 'exit and stderr, a buffer missing on one side' exit=1 stderr=1
  zz_command delete-buffer -b zzcc-sabotage >/dev/null || die 'zz refused delete-buffer'

  # session state: a second session on the zz side only. It is in no status
  # row of the attached session, so the screen must not be reported and the
  # three CLI channels of an unrelated command must not move.
  zz_command new-session -d -s zzcc-sab -x 80 -y 24 "$INNER_SHELL" >/dev/null ||
    die 'zz refused new-session'
  self_check_run state-sabotage display-message -p -t PANE '#{window_index}.#{pane_index}'
  self_check_expect 'state, a session on one side only' \
    exit=0 stdout=0 stderr=0 screen=0 state=1
  zz_command kill-session -t zzcc-sab >/dev/null || die 'zz refused kill-session'

  # the attached screen and the session state together: window_clock_mode open
  # on the zz pane alone. The pin's pane stays live, so the mode surface has to
  # be reported through the screen and `#{pane_in_mode}`/`#{pane_mode}` through
  # the state, while the three CLI channels of an unrelated command stay still.
  # Ending it with one key is `window_clock_key`, so the same sabotage proves
  # the key route the mode is torn down by.
  zz_command clock-mode -t "$(active_pane zz)" >/dev/null || die 'zz refused clock-mode'
  wait_for 'the one-sided clock' pane_in_mode zz 1
  self_check_run clock-sabotage display-message -p -t PANE '#{window_index}.#{pane_index}'
  self_check_expect 'screen and state, a clock on one side only' \
    exit=0 stdout=0 stderr=0 screen=1 state=1
  tmux_outer_command send-keys -t "=$OUTER_SESSION:zz" q ||
    die 'the outer tmux refused send-keys'
  wait_for 'the one-sided clock ended' pane_in_mode zz 0

  local clock_style
  for clock_style in 24-with-seconds 12-with-seconds; do
    set_window_on_both clock-mode-style "$clock_style"
    run_both clock-mode -t PANE
    zz_command set-option -gw clock-mode-colour red >/dev/null
    CASE_CLOCK_FACE=2
    self_check_run "seconds-$clock_style-sabotage" display-message -p -t PANE '#{pane_in_mode}/#{pane_mode}'
    self_check_expect "$clock_style one-sided colour changes the synchronized screen" \
      exit=0 stdout=0 stderr=0 screen=1 state=0
    CASE_CLOCK_FACE=0
    run_both copy-mode -q -t PANE
    run_on_both set-option -gwu clock-mode-colour
  done
  run_on_both set-option -gwu clock-mode-style

  run_both clock-mode -t PANE
  run_both send-keys -t PANE x
  zz_command clock-mode -t "$(active_pane zz)" >/dev/null
  self_check_run injected-clock-sabotage capture-pane -p -t PANE
  self_check_expect 'injected key bypass leaves a one-sided clock' \
    exit=0 stdout=0 stderr=0 screen=1 state=1
  zz_command copy-mode -q -t "$(active_pane zz)" >/dev/null
  zz_command send-keys -t "$(active_pane zz)" x >/dev/null
  self_check_run injected-shell-sabotage capture-pane -p -t PANE
  self_check_expect 'a one-sided leaked key reaches shell bytes and screen' \
    exit=0 stdout=1 stderr=0 screen=1 state=0
  tmux_inner_command send-keys -t "$(active_pane tmux)" x >/dev/null
  run_both send-keys -t PANE C-u
  run_both clock-mode -t PANE
  run_both copy-mode -q -t PANE
  zz_command clock-mode -t "$(active_pane zz)" >/dev/null
  self_check_run cancel-clock-sabotage capture-pane -p -t PANE
  self_check_expect 'generic cancellation leaves a one-sided clock' \
    exit=0 stdout=0 stderr=0 screen=1 state=1
  zz_command copy-mode -q -t "$(active_pane zz)" >/dev/null

  run_both clock-mode -t PANE
  run_both switch-mode -t PANE
  tmux_inner_command copy-mode -q -t "$(active_pane tmux)" >/dev/null
  tmux_inner_command switch-mode -t "$(active_pane tmux)" >/dev/null
  self_check_run stack-sabotage display-message -p -t PANE '#{pane_in_mode}/#{pane_mode}'
  self_check_expect 'a missing suspended mode changes the stack count' \
    exit=0 stdout=1 stderr=0 screen=0 state=1
  run_both copy-mode -q -t PANE

  # the same for the other pane mode, which is not a clock: window_switch_mode
  # on the zz pane alone, whose rows and prompt are a different surface over the
  # same per-pane state. Escape ends it, which is the one key window_switch_key
  # answers with window_pane_reset_mode rather than swallowing into its prompt.
  zz_command switch-mode -t "$(active_pane zz)" >/dev/null || die 'zz refused switch-mode'
  wait_for 'the one-sided switch mode' pane_in_mode zz 1
  self_check_run switch-sabotage display-message -p -t PANE '#{window_index}.#{pane_index}'
  self_check_expect 'screen and state, a switch mode on one side only' \
    exit=0 stdout=0 stderr=0 screen=1 state=1
  tmux_outer_command send-keys -t "=$OUTER_SESSION:zz" Escape ||
    die 'the outer tmux refused send-keys'
  wait_for 'the one-sided switch mode ended' pane_in_mode zz 0

  run_both switch-mode -F 'REVIEW-#{session_name}' -t PANE
  zz_command copy-mode -q -t "$(active_pane zz)" >/dev/null
  zz_command switch-mode -F 'WRONG-#{session_name}' -t "$(active_pane zz)" >/dev/null
  self_check_run switch-format-sabotage display-message -p -t PANE '#{pane_in_mode}/#{pane_mode}'
  self_check_expect 'a discarded switch format changes the rows' \
    exit=0 stdout=0 stderr=0 screen=1 state=0
  run_both copy-mode -q -t PANE

  run_both switch-mode -t PANE 'set-option -g @review-command yes'
  zz_command copy-mode -q -t "$(active_pane zz)" >/dev/null
  zz_command switch-mode -t "$(active_pane zz)" 'set-option -g @review-command wrong' >/dev/null
  for side in tmux zz; do
    tmux_outer_command send-keys -t "=$OUTER_SESSION:$side" Enter
    wait_for "the $side template mode ended" pane_in_mode "$side" 0
  done
  self_check_run switch-template-sabotage show-options -gv @review-command
  self_check_expect 'a changed Enter template changes its command effect' \
    exit=0 stdout=1 stderr=0 screen=0 state=0
  run_on_both set-option -gu @review-command

  run_on_both new-session -d -s alpha -n "$WINDOW_NAME" -x 80 -y 24 "$INNER_SHELL"
  run_on_both new-session -d -s zulu -n "$WINDOW_NAME" -x 80 -y 24 "$INNER_SHELL"
  run_both switch-mode -w -F '#{session_name}:#{window_name}' -t PANE
  zz_command copy-mode -q -t "$(active_pane zz)" >/dev/null
  zz_command switch-mode -w -F '#{?#{==:#{session_name},alpha},cli,#{?#{==:#{session_name},cli},alpha,#{session_name}}}:#{window_name}' -t "$(active_pane zz)" >/dev/null
  self_check_run switch-order-sabotage display-message -p -t PANE '#{pane_in_mode}/#{pane_mode}'
  self_check_expect 'swapped tied window labels change the screen' \
    exit=0 stdout=0 stderr=0 screen=1 state=0
  run_both copy-mode -q -t PANE
  run_on_both kill-session -t '=alpha'
  run_on_both kill-session -t '=zulu'

  run_on_both set-option -g @review-identity nobody
  zz_command set-option -g @review-identity zzcc-missing-identity >/dev/null
  self_check_run formatted-identity-sabotage server-access '#{@review-identity}'
  self_check_expect 'a wrong expanded identity reaches exit and stderr' \
    exit=1 stdout=0 stderr=1 screen=0 state=0
  run_on_both set-option -gu @review-identity

  # the attached screen: one space typed at the zz client's prompt. capture-pane
  # trims trailing blanks, so this reaches the comparison through the cursor,
  # which is part of the same channel. Nothing else moves.
  local zz_before
  zz_before="$(cursor_tuple zz)"
  tmux_outer_command send-keys -t "=$OUTER_SESSION:zz" Space ||
    die 'the outer tmux refused send-keys'
  wait_for 'the one-sided space' zz_cursor_is_not "$zz_before"
  self_check_run screen-sabotage display-message -p -t PANE '#{window_index}.#{pane_index}'
  self_check_expect 'screen, one column at one prompt' \
    exit=0 stdout=0 stderr=0 screen=1 state=0
  tmux_outer_command send-keys -t "=$OUTER_SESSION:zz" BSpace ||
    die 'the outer tmux refused send-keys'
  wait_for 'the space withdrawn' zz_cursor_is "$zz_before"

  # The normalizer, in three steps. Each client sits on its own pts, so the
  # same format really does print two different numbers, the comparison has to
  # report that without CASE_NORMALIZE, and CASE_NORMALIZE has to collapse it.
  self_check_run normalize-off list-clients -F 'tty #{client_tty}'
  self_check_expect 'two pts numbers differ while nothing normalizes them' \
    exit=0 stdout=1 stderr=0 screen=0 state=0
  CASE_NORMALIZE="$PER_PROCESS_NUMBERS"
  self_check_run normalize-on list-clients -F 'tty #{client_tty}'
  CASE_NORMALIZE=''
  self_check_expect 'the same two numbers compare equal once normalized' \
    exit=0 stdout=0 stderr=0 screen=0 state=0

  # And the substitution must not swallow the line it sits in: a session only
  # the zz side has, printed beside a pts path the substitution does collapse.
  zz_command new-session -d -s zzcc-norm -x 80 -y 24 "$INNER_SHELL" >/dev/null ||
    die 'zz refused new-session'
  CASE_NORMALIZE="$PER_PROCESS_NUMBERS"
  self_check_run normalize-keeps-the-line \
    list-sessions -F '#{session_name} /dev/pts/#{session_windows}'
  CASE_NORMALIZE=''
  self_check_expect 'a one-sided line beside a normalized number is still reported' \
    exit=0 stdout=1 stderr=0
  zz_command kill-session -t zzcc-norm >/dev/null || die 'zz refused kill-session'

  # The live -J table: one #() armed in status-left on the zz side alone. The
  # two tables then hold a different number of rows, and PER_PROCESS_NUMBERS
  # collapses the fd and the pid but not the command beside them, so the
  # difference has to reach stdout. The status draws the job on one side too,
  # which is the screen channel doing its own job.
  local interval
  interval="$(zz_command show-options -gv status-interval)"
  zz_command set-option -g status-interval 1 >/dev/null ||
    die 'zz refused set-option -g status-interval'
  zz_command set-option -g status-left "$LIVE_JOB_COMMAND" >/dev/null ||
    die 'zz refused set-option -g status-left'
  wait_for 'the one-sided format job' job_rows_are zz 1
  wait_for 'the one-sided pending format job on the status row' job_status_pending zz
  CASE_NORMALIZE="$PER_PROCESS_NUMBERS"
  self_check_run live-job-sabotage show-messages -J
  CASE_NORMALIZE=''
  self_check_expect 'a format job armed on one side only' exit=0 stdout=1 stderr=0 screen=1
  zz_command set-option -g status-left L >/dev/null ||
    die 'zz refused set-option -g status-left'
  zz_command set-option -g status-interval "$interval" >/dev/null ||
    die 'zz refused set-option -g status-interval'
  wait_for 'the one-sided format job withdrawn from the status' zz_status_left_is L

  zz_command new-session -d -s zzcc-sab-lock -x 80 -y 24 "$INNER_SHELL" >/dev/null ||
    die 'zz refused new-session'
  self_check_run lock-target-sabotage lock-session -t =zzcc-sab-lock:0
  self_check_expect 'the lock target on one side only' exit=1 stdout=0 stderr=1
  zz_command kill-session -t zzcc-sab-lock >/dev/null || die 'zz refused kill-session'

  zz_command set-hook -g after-lock-server 'set -g @zzcc-sab-lock yes' >/dev/null ||
    die 'zz refused set-hook'
  self_check_run lock-hook-sabotage-fire lock-server
  self_check_run lock-hook-sabotage show-options -gqv @zzcc-sab-lock
  self_check_expect 'the after-lock-server hook armed on one side only' \
    exit=0 stdout=1 stderr=0
  zz_command set-hook -gu after-lock-server >/dev/null || die 'zz refused set-hook -gu'
  local lock_command
  run_on_both set-hook -g after-lock-server 'set -g @zzcc-sab-negative yes'
  for lock_command in lock-session lock-client; do
    run_on_both set-option -gu @zzcc-sab-negative
    if [ "$lock_command" = lock-session ]; then
      self_check_run "$lock_command-negative-fire" lock-session -t "=$INNER_SESSION:$WINDOW_NAME"
    else
      self_check_run "$lock_command-negative-fire" lock-client -t CLIENT
    fi
    self_check_run "$lock_command-negative-before" show-options -gqv @zzcc-sab-negative
    self_check_expect "$lock_command does not fire the server hook" exit=0 stdout=0 stderr=0
    zz_command lock-server >/dev/null || die 'zz refused inappropriate hook sabotage'
    self_check_run "$lock_command-negative-sabotage" show-options -gqv @zzcc-sab-negative
    self_check_expect "$lock_command inappropriate server hook on zz only" exit=0 stdout=1 stderr=0
  done
  run_on_both set-hook -gu after-lock-server
  run_on_both set-option -gu @zzcc-sab-negative
  zz_command set-option -gu @zzcc-sab-lock >/dev/null || die 'zz refused set-option -gu'

  zz_command set-option -g status-left LOCK-SABOTAGE >/dev/null
  self_check_run lock-client-screen-sabotage lock-client -t CLIENT
  self_check_expect 'lock-client one-sided status row' exit=0 stdout=0 stderr=0 screen=1
  zz_command set-option -g status-left L >/dev/null

  zz_before="$(cursor_tuple zz)"
  tmux_outer_command send-keys -t "=$OUTER_SESSION:zz" zzq ||
    die 'the outer tmux refused send-keys'
  wait_for 'the one-sided glyphs for the numbered capture' zz_cursor_is_not "$zz_before"
  self_check_run capture-line-numbers-sabotage capture-pane -p -L -t PANE -S 0 -E 2
  self_check_expect 'a numbered capture of three columns on one side only' \
    exit=0 stdout=1 stderr=0
  tmux_outer_command send-keys -t "=$OUTER_SESSION:zz" BSpace BSpace BSpace ||
    die 'the outer tmux refused send-keys'
  wait_for 'the glyphs withdrawn again' zz_cursor_is "$zz_before"

  rich_capture_cases
  lock_target_sabotages
  run_both customize-mode -t PANE
  side_command zz send-keys -t "$(active_pane zz)" Right
  self_check_run customize-sabotage display-message -p -t PANE '#{pane_in_mode}/#{pane_mode}'
  self_check_expect 'customize expansion on one side changes the decoded tree' exit=0 stdout=0 stderr=0 screen=1 state=0
  run_both copy-mode -q -t PANE

  SUSPENDED_ZZ_PID="$(zz_command list-clients -F '#{client_pid}')"
  zz_command suspend-client
  wait_for 'the sabotaged raw client stops' client_process_stopped "$SUSPENDED_ZZ_PID"
  self_check_run suspend-sabotage display-message -p -t PANE '#{pane_in_mode}/#{pane_mode}'
  self_check_expect 'suspension on one side changes its screen and attachment state' exit=0 stdout=0 stderr=0 screen=1 state=1
  kill -CONT "$SUSPENDED_ZZ_PID"
  wait_for 'the suspended raw client resumes' client_attached zz
  SUSPENDED_ZZ_PID=""

  switch_lifetime_self_checks
  customize_fix_self_checks
  switch_tail_self_checks
  self_check_run switch-tail-short-control switch-mode -w -F '#[bg=red]#{window_name}#[default]' -t PANE
  self_check_expect 'short styled rows clear their allocated tail' exit=0 stdout=0 stderr=0 screen=0 state=0
  run_both copy-mode -q -t PANE

  self_check_run equivalence-after display-message -p -t PANE '#{window_index}.#{pane_index}'
  self_check_expect 'equivalence: every sabotage withdrawn' \
    exit=0 stdout=0 stderr=0 screen=0 state=0

  if [ "$SELF_CHECK_FAILURES" -ne 0 ]; then
    printf '%s self-check expectations unmet\n' "$SELF_CHECK_FAILURES"
    exit 1
  fi
  printf 'self-check complete: every sabotage was caught in its own channel and both equivalences passed\n'
}

write_attach zz "$SCRATCH_DIR/attach-zz.sh"
write_attach tmux "$SCRATCH_DIR/attach-tmux.sh"

zz_command -f /dev/null daemon >"$SCRATCH_DIR/zz-daemon.out" 2>"$SCRATCH_DIR/zz-daemon.err" &
ZZ_PID=$!
wait_for "zz daemon socket" test -S "$ZZ_SOCKET"

if [ "$SELF_CHECK" -eq 1 ]; then
  run_self_check
else
  run_cases
fi
