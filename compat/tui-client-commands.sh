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
# choose-client [-hikNrZ]   window_pane_set_mode, client      hard-rejected in
#   [-F -f -K -O -t] [tmpl]   mode on the target pane           UNIMPLEMENTED_TMUX_COMMANDS   CHILD TUI-014
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
#                                                                                             CHILD TUI-015
# lock-session [-t]         locks that session's clients      same                           PROVED + DECLARED
# lock-client [-t]          locks that one client             same                           PROVED + DECLARED
# lock-after-time           arms a per-client server timer    store-only                     DECLARED, CHILD TUI-015
# lock-command              spawned on the client tty         store-only; the pin's own      DECLARED, the pin's
#                                                               default is a build-time        default is whatever
#                                                               choice                         configure found
# refresh-client            status jobs rerun, redraw         status render published        PROVED
# refresh-client -S         status jobs rerun, status redraw  status render published        PROVED
# refresh-client -f -F      client flags set                  client flags set               PROVED
# refresh-client -A -B -C   control-client only               control-client only            PROVED
# refresh-client -t         target client, missing-client     same                           PROVED
# refresh-client -c -D -L   pans a terminal client's view     loudly unsupported             DECLARED
#   -R -U -l -r [adjust]                                                                      clients.interactive-refresh
# capture-pane -p -S -E     the requested line range          the same range                 PROVED
# capture-pane -J -q -T     join, quiet, trailing positions   same                           PROVED
# capture-pane -b           fills a named buffer              same                           PROVED
# capture-pane -e           the range with SGR, trimmed       one trailing cell kept         DECLARED, CHILD TUI-017
# capture-pane (no -S -E)   every visible row, trailing       stops at the last written row  DECLARED, CHILD TUI-017
#                             blanks included                                                 capture.rich-transports
# capture-pane -N           trailing spaces to the pane edge  trailing spaces to the last    DECLARED, CHILD TUI-017
#                                                               written cell
# capture-pane -M           the mode screen, the pane when    errors when the pane is in no  DECLARED, CHILD TUI-017
#                             there is no mode                  native mode
# capture-pane -a           `no alternate screen`             `alternate screen is not       DECLARED, CHILD TUI-017
#                                                               active`
# capture-pane -C -F -H     grid internals and the pending    loudly unsupported             DECLARED, CHILD TUI-017
#   -L -P -R                  input parser state                                              capture.rich-transports
# load-buffer -             caller stdin into a buffer        adopted, same                  PROVED
# save-buffer - / -a -      buffer bytes to caller stdout     adopted, same                  PROVED
# show-buffer [-b]          buffer bytes to stdout            same                           PROVED
# source-file -             caller stdin as a config file     loudly unsupported             DECLARED, CHILD TUI-018
#                                                                                             protocol.binary-streams
# display-message -I        caller stdin into the pane        loudly unsupported             DECLARED, CHILD TUI-018
# split-window -I           caller stdin into the new pane    loudly unsupported             DECLARED, CHILD TUI-018
# show-hooks [-Bgpw] [-t]   the hook table                    same                           PROVED
# show-messages             the server log                    same shape, but the invoking   CHILD TUI-016, the log
#                                                               client is named device-<n>     carries client
#                                                               and the pin reprints a         identity
#                                                               command through args_print
# show-messages -J -T       running jobs, known terminals     loudly unsupported             CHILD TUI-016
# show-messages -t          tolerated, log unchanged          loudly unsupported             CHILD TUI-016
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
# DECLARED, NOT PINNED: the server log names its clients, and a clientless CLI
#   is client-<pid> on the pin and device-<n> on zz; the pin also reprints a
#   command through its own argument printer. show-messages is recorded for
#   exactly that and nothing else is masked.
#
# SETTLING. A command that draws nothing still has to be given the chance to
# draw: after every invocation each screen is polled until it is unchanged
# between two consecutive polls, and only then compared. Where the pin opens a
# mode the case waits, bounded, for the pin's own needle first and gives zz the
# same bounded wait made soft. No wait in this file is a sleep.
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
  cmp -s "$SCRATCH_DIR/zz.out" "$SCRATCH_DIR/tmux.out" || LAST_STDOUT_DIFFERED=1
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
    diff <(cat -v "$SCRATCH_DIR/tmux.out") <(cat -v "$SCRATCH_DIR/zz.out") |
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
    RECORDS=$((RECORDS + 1))
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
    RECORDS=$((RECORDS + 1))
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
  CHECKS=$((CHECKS + 1))
  if compare_channels "$name"; then
    printf 'ok    %s\n' "$name"
  else
    FAILURES=$((FAILURES + 1))
    printf 'DIFF  %s\n' "$name"
  fi
}

# `split-window -I` builds a pane on the pin and is refused on zz, so the pin's
# extra pane is killed before the next case reads the state.
restore_pin_pane() {
  local name="$1"
  CASE_LABEL="$name"
  local extra
  extra="$(tmux_inner_command list-panes -t "=$INNER_SESSION:$WINDOW_NAME" \
    -F '#{pane_index} #{pane_id}' | awk '$1 > 0 { print $2 }')"
  if [ -n "$extra" ]; then
    local pane
    for pane in $extra; do
      tmux_inner_command kill-pane -t "$pane" >/dev/null 2>&1 || true
    done
    wait_for "the pin extra pane gone for $name" pin_window_pane_count 1
  fi
  restore_case "$name"
}

pin_window_pane_count() {
  [ "$(tmux_inner_command list-panes -t "=$INNER_SESSION:$WINDOW_NAME" -F x | wc -l)" = "$1" ]
}

# --- the roster's cases -----------------------------------------------------
NATIVE_CLIENT_TOOLS='commands.native-client-tools, accepted: the pin paints client chrome inside the target pane and zz answers each intent with a native surface. The raw TUI half is TUI-014'
INTERACTIVE_REFRESH='clients.interactive-refresh, accepted: every zz client renders itself from published frames, so the pan and redraw-adjustment family stays loudly unsupported'
LOCK_PROGRAM='options.lock-program, accepted: the pin spawns lock-command on the client tty and a daemon that only publishes frames cannot run a program on a client terminal'
RICH_CAPTURE='capture.rich-transports, accepted: zz captures the terminal worker retained UTF-8 text snapshot, not the pin grid and input parser'
BINARY_STREAMS='protocol.binary-streams, accepted: typed UTF-8 arguments are the contract and the remaining - forms stay loudly refused rather than pretending the daemon process is the caller'
LOG_IDENTITY='the server log names the client that ran each command - client-<pid> on the pin, device-<n> on zz - and the pin reprints the command through its own argument printer'
SERVER_ACCESS='zz has no multi-user socket access list: the daemon socket is the invoking user, so there is no user or group to add, and TUI-014 carries the refusal shape'
ESCAPE_TRIM='capture.rich-transports, owner terminal: the -e transform runs through the vendored formatter in crates/zz-terminal/src/session.rs, whose Vt format keeps one trailing cell the pin trims, and that file is outside this lane'
MESSAGES_CHILD='TUI-016: show-messages -J lists the running format jobs and -T the known terminals, neither of which zz publishes yet'

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
  case_run capture-escape record "$ESCAPE_TRIM" -- capture-pane -p -e -t PANE -S 0 -E 2
  case_run capture-join same '' -- capture-pane -p -J -t PANE -S 0 -E 2
  case_run capture-trailing same '' -- capture-pane -p -T -t PANE -S 0 -E 2
  case_run capture-reversed same '' -- capture-pane -p -t PANE -S 2 -E 0
  case_run capture-history same '' -- capture-pane -p -t PANE -S - -E 0
  case_run capture-quiet-missing same '' -- capture-pane -p -q -t %99
  case_run capture-loud-missing same '' -- capture-pane -p -t %99
  case_run capture-buffer same '' -- capture-pane -b zzcap -t PANE -S 0 -E 2
  case_run capture-buffer-shown same '' -- show-buffer -b zzcap
  case_run capture-buffer-deleted same '' -- delete-buffer -b zzcap
  case_run capture-default-range record "$RICH_CAPTURE" -- capture-pane -p -t PANE
  case_run capture-preserve-trailing record "$RICH_CAPTURE" -- capture-pane -p -N -t PANE -S 0 -E 0
  case_run capture-mode-screen record "$RICH_CAPTURE" -- capture-pane -p -M -t PANE -S 0 -E 2
  case_run capture-alternate record "$RICH_CAPTURE" -- capture-pane -p -a -t PANE
  case_run capture-control record "$RICH_CAPTURE" -- capture-pane -p -C -t PANE -S 0 -E 2
  case_run capture-flags record "$RICH_CAPTURE" -- capture-pane -p -F -t PANE -S 0 -E 2
  case_run capture-hyperlinks record "$RICH_CAPTURE" -- capture-pane -p -H -t PANE -S 0 -E 2
  case_run capture-line-numbers record "$RICH_CAPTURE" -- capture-pane -p -L -t PANE -S 0 -E 2
  case_run capture-pending record "$RICH_CAPTURE" -- capture-pane -p -P -t PANE
  case_run capture-grid record "$RICH_CAPTURE" -- capture-pane -p -R -t PANE
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
  case_run stream-source-file record "$BINARY_STREAMS" -- source-file -
  CASE_STDIN=''
  case_run stream-source-file-effect record "$BINARY_STREAMS" -- show-options -gqv @zzcc-stream
  CASE_STDIN='typed-into-the-pane'
  case_run stream-display-message record "$BINARY_STREAMS" -- display-message -I
  case_run stream-split-window record "$BINARY_STREAMS" -- split-window -I -t PANE
  CASE_STDIN=''
  restore_pin_pane stream-split-window-restored
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
  case_run messages-jobs record "$MESSAGES_CHILD" -- show-messages -J
  case_run messages-terminals record "$MESSAGES_CHILD" -- show-messages -T
  case_run messages-target record "$MESSAGES_CHILD" -- show-messages -t CLIENT
}

lock_cases() {
  run_on_both set-hook -g after-lock-server 'set -g @zzcc-locked yes'
  case_run lock-server cli "$LOCK_PROGRAM" -- lock-server
  restore_case lock-server-restored
  case_run lock-server-hook same '' -- show-options -gv @zzcc-locked
  case_run lock-session cli "$LOCK_PROGRAM" -- lock-session -t "=$INNER_SESSION"
  restore_case lock-session-restored
  case_run lock-session-missing same '' -- lock-session -t zzcc-nope
  case_run lock-client cli "$LOCK_PROGRAM" -- lock-client -t CLIENT
  restore_case lock-client-restored
  case_run lock-client-current cli "$LOCK_PROGRAM" -- lock-client
  restore_case lock-client-current-restored
  case_run lock-client-missing same '' -- lock-client -t /dev/zzcc-nope
  case_run lock-after-time-store same '' -- show-options -g lock-after-time
  case_run lock-command-store same '' -- show-options -g lock-command
}

client_tool_cases() {
  CASE_NEEDLE_MODE=1
  case_run client-tree-open record "$NATIVE_CLIENT_TOOLS" -- choose-client -t PANE
  restore_case client-tree-closed
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

run_cases() {
  printf 'stock client-command roster at %sx%s (pin %s)\n' \
    "$COLUMNS_UNDER_TEST" "$ROWS_UNDER_TEST" "$(basename -- "$TMUX_BIN")"
  attach_both_at 80 24
  case_run baseline same '' -- display-message -p -t PANE '#{window_index}.#{pane_index}'
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
    printf '%s of %s asserted comparisons differ, %s recorded (%s for a sibling lane)\n' \
      "$FAILURES" "$CHECKS" "$RECORDS" "$SIBLINGS"
    exit 1
  fi
  printf 'all %s asserted comparisons identical, %s recorded not asserted (%s for a sibling lane)\n' \
    "$CHECKS" "$RECORDS" "$SIBLINGS"
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

  # The second equivalence: with every sabotage withdrawn the comparison is
  # silent again, so none of the four above was a difference the scene kept.
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
