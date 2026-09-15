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
# clock-mode [-t]           clock mode on the target pane     hard-rejected                  CHILD TUI-014
# customize-mode [-kNZ]     customize mode on the pane        hard-rejected                  CHILD TUI-014
#   [-F -f -t]
# switch-mode [-kswZ]       switches an open mode in place    hard-rejected                  CHILD TUI-014
#   [-F -t] [command]
# suspend-client [-t]       SIGTSTP to the client process     hard-rejected                  CHILD TUI-014
# server-access [-adglrw]   socket access control list        hard-rejected                  CHILD TUI-014, zz
#   [-t] [user|group]                                                                         has no socket ACL
# lock-server               locks every client, runs          validates, empty execution,    PROVED (CLI) +
#                             lock-command on each tty          after-lock-server fires       DECLARED (screen),
#                                                                                             TUI-015
# lock-session [-t]         locks that session's clients      same                           PROVED + DECLARED
# lock-client [-t]          locks that one client             same                           PROVED + DECLARED
# lock-server/-session/     too many arguments, unknown       same                           PROVED
#   -client arity and -t     flag, -t without an argument
# lock-session -t with a    resolves the session through      matches the whole string as a  NOT A LOCK FACT and no
#   window or pane suffix     the whole session:window.pane     session name                   case here: the same
#                             grammar                                                          bytes come back from
#                                                                                              has-session, so it is
#                                                                                              the shared target
#                                                                                              grammar. Measured in
#                                                                                              TUI-015's evidence
# after-lock-session        neither is a hook name: the pin   same                           PROVED
# after-lock-client           answers `invalid option`
# lock-after-time           arms a per-client server timer    store-only at both scopes,     DECLARED, TUI-015
#                                                               and an invalid value is
#                                                               refused the pin's way
# lock-command              spawned on the client tty         store-only at both scopes;     DECLARED, the pin's
#                                                               the pin's own default is a     default is whatever
#                                                               build-time choice              configure found
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
# capture-pane -C           backslashes doubled, and with     same                           PROVED
#                             -e every escape written \033
# capture-pane -L           each line numbered from the       same                           PROVED
#                             history size, negative in
#                             history, and with -J the
#                             number of every joined row
#                             inside the joined line
# capture-pane -F -H -P -R  line flags, the line OSC 8        loudly unsupported             DECLARED, TUI-017
#                             URIs, the pending input                                         capture.rich-transports,
#                             buffer and the whole internal                                   each with the workload
#                             grid                                                            its refusal names
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
CASE_STDIN=''

# Two spellings a command prints belong to the process that printed them and no
# two servers can share them: the pts number the kernel gave a client, and the
# fd and pid of a job's own child. A case that prints either sets
# CASE_NORMALIZE, and the SAME substitution runs over both sides, so nothing
# one-sided is hidden - the text around the number is still compared byte for
# byte, which the self-check's normalized-stdout sabotage proves.
CASE_NORMALIZE=''
PER_PROCESS_NUMBERS='s|/dev/pts/[0-9][0-9]*|/dev/pts/N|g;s|fd=[0-9][0-9]*|fd=N|g;s|pid=[0-9][0-9]*|pid=N|g'

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

compare_channels() {
  local name="$1"
  local zz_rc tmux_rc zz_state tmux_state zz_screen tmux_screen zz_cursor tmux_cursor
  zz_rc="$(cat "$SCRATCH_DIR/zz.rc")"
  tmux_rc="$(cat "$SCRATCH_DIR/tmux.rc")"
  zz_state="$(state_of zz)"
  tmux_state="$(state_of tmux)"
  zz_screen="$(styled_screen_of zz)"
  tmux_screen="$(styled_screen_of tmux)"
  zz_cursor="$(cursor_tuple zz)"
  tmux_cursor="$(cursor_tuple tmux)"
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
  capture-control | capture-flags | capture-hyperlinks | capture-line-numbers | capture-pending | capture-grid)
    printf 'TUI-017'
    ;;
  stream-source-file | stream-source-file-effect | stream-display-message | stream-split-window)
    printf 'TUI-018'
    ;;
  lock-server | lock-session | lock-client | lock-client-current)
    printf 'TUI-015'
    ;;
  clock-mode-open | customize-mode-open | switch-mode | suspend-client | server-access-bare | server-access-user)
    printf 'TUI-014'
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
}

# The pin's mode commands leave a mode open on the pane, and its lock draws over
# the client. q ends a mode, and the screen both sides show once everything has
# settled is asserted: the pin has to give the pane back exactly, and zz, which
# opened nothing, has to be where it already was.
# Escape first and q only if the mode is still up: mode_tree_key answers q and
# a prompt inside a mode answers Escape, and a q typed at a shell prompt that
# already came back would be a change of its own.
end_pin_mode() {
  local attempt poll key
  for ((attempt = 0; attempt < 3; attempt++)); do
    for key in Escape q; do
      pane_in_mode tmux 0 && return 0
      tmux_outer_command send-keys -t "=$OUTER_SESSION:tmux" "$key" ||
        die 'the outer tmux refused send-keys'
      for ((poll = 0; poll < 20; poll++)); do
        pane_in_mode tmux 0 && return 0
        sleep 0.05
      done
    done
  done
  pane_in_mode tmux 0
}

restore_case() {
  local name="$1"
  CASE_LABEL="$name"
  if pane_in_mode tmux 1; then
    local before_tmux
    before_tmux="$(styled_screen_of tmux)"
    end_pin_mode || true
    wait_for "the pin mode ended for $name" pane_in_mode tmux 0
    settle_screen tmux "$before_tmux"
  fi
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

# --- the roster's cases -----------------------------------------------------
NATIVE_CLIENT_TOOLS='commands.native-client-tools, accepted: the pin paints client chrome inside the target pane and zz answers each intent with a native surface. The raw TUI half is TUI-014'
INTERACTIVE_REFRESH='clients.interactive-refresh, accepted: every zz client renders itself from published frames, so the pan and redraw-adjustment family stays loudly unsupported'
LOCK_PROGRAM='options.lock-program, accepted: the pin spawns lock-command on the client tty and a daemon that only publishes frames cannot run a program on a client terminal'
RICH_CAPTURE='capture.rich-transports, accepted: zz captures the terminal worker retained UTF-8 text snapshot, not the pin grid and input parser'
CONTROL_ESCAPE='capture.rich-transports: -C answers the pin over the plain snapshot, and -C -e rides on -e, whose vendored formatter in crates/zz-terminal/src/session.rs spells a colour 38;5;1 where the pin spells it 31 and keeps one trailing cell the pin trims. That residue is this obligation clause 2, not the transport'
NUMBERED_TRAILING='the numbers are identical and the rows are not: -L over -N rides on the -N residue this obligation clause 2 owns, where the pin pads to the pane edge and zz stops at the last written cell. Measured 2026-09-15 at 80x24, the three numbered rows differ only in their trailing spaces. The case flips to same when clause 2 lands'
CAPTURE_FLAGS='capture.rich-transports, refused with a measurement 2026-09-15: -F prints six grid_line flags and zz has none of them as a line fact. D is a dead pane line, X an extended cell line and H a hyperlink line, all tmux grid bookkeeping; O and P are the OSC 133 marks libghostty records on cells but does not publish per line; W is the wrap the formatter consumes and never reports. The workload it would serve is a script reading which rows are output, prompt or continuation; that wants a line-fact channel out of the terminal worker, not a sixth text transform'
CAPTURE_LINKS='capture.rich-transports, refused with a measurement 2026-09-15: -H prints each line OSC 8 URIs and zz has no hyperlink to print. On a row the pin marks HX, a zz capture -e emits the text with no OSC 8 at all, so the retained snapshot did not keep the link. The workload it would serve is a script harvesting the URLs on a screen; that wants hyperlinks retained and published by the terminal worker first'
CAPTURE_PENDING='capture.rich-transports, refused with a measurement 2026-09-15: -P prints the bytes the pin parser has read and not yet completed, input_pending(wp->ictx). libghostty-vt publishes no parser-pending buffer, so zz cannot answer it and an empty answer would be a fake channel that matched only because the buffer is almost always empty. The workload it would serve is debugging a half-written escape sequence'
CAPTURE_GRID='capture.rich-transports, refused with a measurement 2026-09-15: -R dumps the pin internal grid - a header G <sx>x<sy> (<hsize>/<hlimit>), then per line L <yy> (<n>) flags=<string>[<hex>] <cellused>/<cellsize>, then one C line per column carrying that cell colour, attribute and link ids. Measured at 40x8 that is 329 lines for eight rows. zz has no hsize/hlimit pair, no per-line cellused and cellsize, and no grid flag word: building them inside zz would be inventing tmux internals to make bytes match. The workload it would serve is a tmux regression test reading another tmux grid. Decided 2026-09-15 by the orchestrator under fabrico TUI parity contract of 2026-09-09; reversible'
LOG_IDENTITY='DECIDED 2026-09-14: zz keeps device-<n> for a client with no tty of its own, where the pin prints client-<pid>. Measured 2026-09-14 on both sides: the pin names ANY tty-bearing client by that tty, including the attached terminal client whose attach-session row reads /dev/pts/<n>, and zz named none of them - it spelled every row by the device name the client sent, which for an interactive client is the hostname. That half is closed: the server log now names a client by its tty whenever it has one. What stays is the clientless CLI, which names a process that has already exited by the time anyone reads the log while device-<n> is the spelling every zz target, chooser row and #{client_name} uses. The pin also reprints each command through args_print, so capture-pane -pa comes back as capture-pane -ap. Registered, not masked'
SERVER_ACCESS='zz has no multi-user socket access list: the daemon socket is the invoking user, so there is no user or group to add, and TUI-014 carries the refusal shape'
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

# The lock family's whole CLI surface: the three commands, their arity and flag
# errors, every target form, the one hook name that exists, and both options at
# both scopes. cmd-lock-server.c gives all three CMD_AFTERHOOK, so the pin fires
# after-lock-<name> and only after-lock-server is a hook name - the other two are
# `invalid option`, which is what makes lock-server-hook's marker a lock-server
# fact rather than a lock fact. The option cases set a session-scope value and
# unset it again while no lock runs between them, so nothing downstream can be
# spawned onto the pin's client tty but `true`.
lock_cases() {
  run_on_both set-hook -g after-lock-server 'set -g @zzcc-locked yes'
  case_run lock-server cli "$LOCK_PROGRAM" -- lock-server
  restore_case lock-server-restored
  case_run lock-server-hook same '' -- show-options -gv @zzcc-locked
  case_run lock-server-arity same '' -- lock-server zzcc-extra
  case_run lock-server-unknown-flag same '' -- lock-server -t zzcc-nope
  case_run lock-session cli "$LOCK_PROGRAM" -- lock-session -t "=$INNER_SESSION"
  restore_case lock-session-restored
  case_run lock-session-current cli "$LOCK_PROGRAM" -- lock-session
  restore_case lock-session-current-restored
  case_run lock-session-missing same '' -- lock-session -t zzcc-nope
  case_run lock-session-arity same '' -- lock-session zzcc-extra
  case_run lock-client cli "$LOCK_PROGRAM" -- lock-client -t CLIENT
  restore_case lock-client-restored
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
}

client_tool_cases() {
  CASE_NEEDLE_MODE=1
  case_run client-tree-open record "$CLIENT_TREE_CLIENTLESS" -- choose-client -t PANE
  restore_case client-tree-closed
  case_run client-tree-unknown-flag same '' -- choose-client -Q -t PANE
  case_run client-tree-bad-sort same '' -- choose-client -O zzcc-nope -t PANE
  case_run client-tree-usage same '' -- choose-client -t PANE one two
  CASE_NEEDLE_MODE=1
  case_run clock-mode-open record "$NATIVE_CLIENT_TOOLS" -- clock-mode -t PANE
  restore_case clock-mode-closed
  CASE_NEEDLE_MODE=1
  case_run customize-mode-open record "$NATIVE_CLIENT_TOOLS" -- customize-mode -t PANE
  restore_case customize-mode-closed
  case_run switch-mode record "$NATIVE_CLIENT_TOOLS" -- switch-mode -t PANE
  restore_case switch-mode-closed
  case_run server-access-bare record "$SERVER_ACCESS" -- server-access
  case_run server-access-user record "$SERVER_ACCESS" -- server-access -w zzcc-nobody
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
  buffer_stream_cases
  message_hook_cases
  lock_cases
  client_tool_cases
  # LAST, and nothing may follow it: cmd-detach-client.c sends SIGTSTP to the
  # client process, so the pin's attached client stops and its session loses
  # its client for the rest of the run. The measurement is worth a case; a
  # stopped client underneath every later case is not.
  case_run suspend-client record "$NATIVE_CLIENT_TOOLS" -- suspend-client

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

  # The lock family, twice, because its three commands print nothing on either
  # side and a comparison that only reads their stdout would pass while zz did
  # nothing at all. First its target validation: a session the zz side alone
  # has, so lock-session succeeds there and the pin cannot find it. Then its
  # hook: after-lock-server armed on the zz side only, run, and the marker it
  # writes read back - the lock itself is silent on both, so the marker is the
  # channel a sabotage can reach.
  zz_command new-session -d -s zzcc-sab-lock -x 80 -y 24 "$INNER_SHELL" >/dev/null ||
    die 'zz refused new-session'
  self_check_run lock-target-sabotage lock-session -t zzcc-sab-lock
  self_check_expect 'the lock target on one side only' exit=1 stdout=0 stderr=1
  zz_command kill-session -t zzcc-sab-lock >/dev/null || die 'zz refused kill-session'

  zz_command set-hook -g after-lock-server 'set -g @zzcc-sab-lock yes' >/dev/null ||
    die 'zz refused set-hook'
  self_check_run lock-hook-sabotage-fire lock-server
  self_check_run lock-hook-sabotage show-options -gqv @zzcc-sab-lock
  self_check_expect 'the after-lock-server hook armed on one side only' \
    exit=0 stdout=1 stderr=0
  zz_command set-hook -gu after-lock-server >/dev/null || die 'zz refused set-hook -gu'
  zz_command set-option -gu @zzcc-sab-lock >/dev/null || die 'zz refused set-option -gu'

  # The numbered capture, whose whole output is one transform of a capture that
  # already matched: one glyph typed at the zz prompt has to move a numbered
  # line the way it moves the plain one, so the -L bytes are compared and not
  # just produced. It has to be a glyph and not the space the screen sabotage
  # uses, because capture-pane without -N trims the trailing blank away and
  # there would be nothing in the bytes to catch. It is withdrawn before the
  # equivalence below.
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

  # The second equivalence: with every sabotage withdrawn the comparison is
  # silent again, so none of the seven above was a difference the scene kept.
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
