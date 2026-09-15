#!/usr/bin/env bash
# The caller stream forms: every command that moves bytes between the invoking
# process's standard streams and the server, compared on exit status, stdout
# bytes, stderr bytes and the state each one leaves behind.
#
# TUI-018 asks for one bounded command-stream channel and for a comparison of
# each form against pinned tmux d77c9dc6, including that the pin's
# `source-file -` really APPLIES its stdin rather than reading and dropping it.
# knowledge/designs/command-stream-channel.md is the channel; this file is the
# measurement.
#
# THE ROSTER
# ---------------------------------------------------------------------------
# form                        sink        what it does with the caller's bytes
# source-file -               Config      parsed and applied as a file named -
# display-message -I          PaneInput   written into a pane with no process
# split-window -I             PaneInput   builds that pane, then writes into it
# load-buffer -               Argument    the bytes become a paste buffer
# save-buffer - / -a -        (stdout)    the buffer's bytes, exactly, to stdout
# ---------------------------------------------------------------------------
#
# NO ATTACHED CLIENT, AND WHY. window_pane_start_input returns before it reads
# anything when the invoking client has a session (window.c), so the pin reads
# the caller's stdin only for a CLIENTLESS client - the way a script invokes
# it, which is the only way these forms work at all. An attached client would
# change nothing this file measures, and the attached screen for the same
# commands is asserted in compat/tui-client-commands.sh, which does run both
# binaries under one outer pinned tmux. Everything else follows the usual
# driver: isolated HOME and XDG_CONFIG_HOME per side, short /tmp sockets,
# bounded wait_for on an observable, a trap that reaps what it started.
#
# FOUR CHANNELS PER CASE. exit status, stdout bytes and stderr bytes come from
# the invocation; the state is a fixed set of list-* formats plus the option
# values under test plus the first rows of every pane, which is where a
# PaneInput sink's bytes land. `same` asserts all four. `decided` prints a
# difference that is a recorded product decision and asserts nothing about it.
# `record` asserts nothing and has to say why; a recorded case holds the
# obligation's clause open.
#
# THE ONE DECIDED DIFFERENCE is the bound. Pinned tmux streams a caller payload
# in acknowledged 16 KiB chunks with no total limit; zz reads at most
# MAX_AGENT_SEND_BYTES (1 MiB) and refuses the whole invocation at the reader,
# before the daemon sees a byte. Bulk file transfer through a command client is
# a workload zz does not serve, because an unbounded stream lets one caller grow
# daemon memory without limit. Every case at and below the cap is asserted, so
# what is decided is the bound and nothing else.
#
# CONTROLLED DYNAMIC VALUES, set identically on both sides:
#   the inner shell   ENV= PS1='$ ' exec /bin/sh: no rc file, and a prompt with
#                     no host, user, path or clock.
#   pane pids         a pid is never compared, only whether the pane has one:
#                     that is what "holds no process" means and the number
#                     differs by construction.
#   default-shell     the pin fills an empty pane's #{pane_current_command}
#                     from its build-time default shell and zz leaves it empty;
#                     that belongs to split-window -E, which predates this
#                     channel, so the state format below reads pane_dead and
#                     the presence of a pid instead.
#
# --self-check runs the driver against a deliberate one-sided difference in
# each channel - stdout, stderr with the exit status, the session state and the
# pane content that carries a PaneInput sink's proof - and requires the
# comparison to catch each one in that channel, plus two equivalences it must
# NOT report. Alias cases remove one reader or change one byte on the zz side
# and check the specific exit, stderr, stdout or option-state channel.
set -eEuo pipefail

usage() {
  printf 'usage: compat/tui-command-streams.sh [--self-check] [ZZ_BIN [TMUX_BIN]]\n' >&2
  printf '       ZZ_BIN=path TMUX_BIN=path compat/tui-command-streams.sh\n' >&2
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

# crates/zz-protocol/src/message.rs: MAX_AGENT_SEND_BYTES.
STREAM_CAP_BYTES=1048576
COLUMNS_UNDER_TEST=80
ROWS_UNDER_TEST=24
SCRATCH_DIR="$(mktemp -d /tmp/zzcs.XXXXXX)"
TOKEN="${SCRATCH_DIR##*.}"
INNER_SOCKET_NAME="zzcst-$TOKEN"
ZZ_SOCKET="/tmp/zzcs-$TOKEN.sock"
SESSION="cs"
WINDOW_NAME="win"
ZZ_HOME="$SCRATCH_DIR/zz-home"
TMUX_HOME="$SCRATCH_DIR/tmux-home"
ZZ_LOG_DIR="$SCRATCH_DIR/zz-logs"
CASE_LABEL=""
ZZ_PID=""
FAILURES=0
CHECKS=0
RECORDS=0
DECIDED=0
SIBLINGS=0
LAST_EXIT_DIFFERED=0
LAST_STDOUT_DIFFERED=0
LAST_STDERR_DIFFERED=0
LAST_STATE_DIFFERED=0
INNER_SHELL="ENV= PS1='\$ ' exec /bin/sh"
DECISION='decided 2026-09-14 by the orchestrator under fabrico'"'"'s TUI parity contract of 2026-09-09; reversible'
mkdir -p "$ZZ_HOME" "$TMUX_HOME" "$ZZ_LOG_DIR"

scrubbed() {
  env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL \
    -u XDG_STATE_HOME -u ZZ_LOG_DIR \
    TMUX_TMPDIR=/tmp "$@"
}
zz_command() {
  scrubbed HOME="$ZZ_HOME" XDG_CONFIG_HOME="$ZZ_HOME/config" ZZ_LOG_DIR="$ZZ_LOG_DIR" \
    "$ZZ_BIN" --socket "$ZZ_SOCKET" "$@"
}
tmux_command() {
  scrubbed HOME="$TMUX_HOME" XDG_CONFIG_HOME="$TMUX_HOME/config" \
    "$TMUX_BIN" -L "$INNER_SOCKET_NAME" "$@"
}
side_command() {
  local side="$1"
  shift
  case "$side" in
  zz) zz_command "$@" ;;
  tmux) tmux_command "$@" ;;
  esac
}

cleanup() {
  local status=$?
  trap - EXIT ERR INT TERM
  set +e
  zz_command kill-server >/dev/null 2>&1
  tmux_command kill-server >/dev/null 2>&1
  if [ -n "$ZZ_PID" ]; then
    kill "$ZZ_PID" >/dev/null 2>&1
    wait "$ZZ_PID" >/dev/null 2>&1
  fi
  rm -f -- "$ZZ_SOCKET" "/tmp/tmux-$(id -u)/$INNER_SOCKET_NAME"
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
    printf -- '--- %s state ---\n' "$side" >&2
    state_of "$side" >&2 2>&1 || true
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

# The options the config sink writes, read back by name so a case can prove the
# stream was applied and not merely accepted.
PROBE_OPTIONS=(@zzcs-one @zzcs-two @zzcs-three @zzcs-parse @zzcs-cancel)

# Everything a caller stream can move, read the same way from both servers. A
# pane's pid is never printed, only whether it has one: that is what an empty
# pane is, and the number differs by construction.
state_of() {
  local side="$1"
  local option pane
  side_command "$side" list-sessions -F 'S #{session_name} #{session_windows}' 2>&1
  side_command "$side" list-panes -a \
    -F 'P #{session_name}:#{window_index}.#{pane_index} dead=#{pane_dead} process=#{?pane_pid,yes,no} #{pane_width}x#{pane_height}' 2>&1
  side_command "$side" list-buffers -F 'B #{buffer_name} #{buffer_size}' 2>&1
  for option in "${PROBE_OPTIONS[@]}"; do
    printf 'O %s %s\n' "$option" "$(side_command "$side" show-options -gqv "$option" 2>&1)"
  done
  for pane in $(side_command "$side" list-panes -a -F '#{pane_index}' 2>/dev/null); do
    printf 'C %s %s\n' "$pane" "$(capture_rows "$side" "$pane")"
  done
}

# The first three rows of one pane, with rows that are blank at the end of the
# range dropped. Inside an explicit range the pin returns every row and zz
# stops at the last written one; that is capture.rich-transports, which
# TUI-017 owns and compat/tui-client-commands.sh records as
# capture-preserve-trailing. Measured here on 2026-09-14 and left out of the
# comparison so this file does not carry a second copy of another obligation's
# case: what a PaneInput sink writes is on the rows that are not blank.
capture_rows() {
  side_command "$1" capture-pane -p -t "=$SESSION:$WINDOW_NAME.$2" -S 0 -E 2 2>&1 | cat -v |
    tr -d '\000' |
    awk '{ rows[NR] = $0 }
      END {
        last = 0
        for (i = 1; i <= NR; i++) {
          if (rows[i] != "") {
            last = i
          }
        }
        for (i = 1; i <= last; i++) {
          printf "%s|", rows[i]
        }
      }'
}

pane_count_is() {
  [ "$(side_command "$1" list-panes -a -F x 2>/dev/null | wc -l)" = "$2" ]
}
pane_shows() {
  side_command "$1" capture-pane -p -t "=$SESSION:$WINDOW_NAME.$2" -S 0 -E 2 2>/dev/null |
    grep -Fq "$3"
}

# A state is settled when two consecutive reads are equal. A PaneInput sink
# hands its bytes to the pane's parser, so the state a case compares is the one
# that stopped moving, never a sleep.
settle_state() {
  local side="$1"
  local previous="" current attempt
  for ((attempt = 0; attempt < 120; attempt++)); do
    current="$(state_of "$side" 2>&1)"
    if [ -n "$previous" ] && [ "$current" = "$previous" ]; then
      return 0
    fi
    previous="$current"
    sleep 0.05
  done
  printf 'note  %s: the %s state never settled within 6 seconds\n' "${CASE_LABEL:-scene}" "$side"
  return 0
}

build_scene() {
  local side
  zz_command -f /dev/null new-session -d -s "$SESSION" -n "$WINDOW_NAME" \
    -x "$COLUMNS_UNDER_TEST" -y "$ROWS_UNDER_TEST" "$INNER_SHELL" ||
    die "could not create the zz session"
  tmux_command -f /dev/null new-session -d -s "$SESSION" -n "$WINDOW_NAME" \
    -x "$COLUMNS_UNDER_TEST" -y "$ROWS_UNDER_TEST" "$INNER_SHELL" ||
    die "could not create the tmux session"
  for side in zz tmux; do
    side_command "$side" set-option -g automatic-rename off ||
      die "$side refused set-option automatic-rename"
    side_command "$side" set-option -g status off ||
      die "$side refused set-option status"
    wait_for "the $side shell printed its prompt" pane_shows "$side" 0 '$'
  done
  settle_state zz
  settle_state tmux
}

# --- running one case -------------------------------------------------------
#
# CASE_STDIN_FILE names the file a case pipes into both binaries; empty means
# /dev/null, which is a caller that closed its standard input before writing.
CASE_STDIN_FILE=''

run_both() {
  local side argument rc
  local -a arguments
  for side in zz tmux; do
    arguments=()
    for argument in "$@"; do
      case "$argument" in
      SESSION) arguments+=("=$SESSION") ;;
      PANE0) arguments+=("=$SESSION:$WINDOW_NAME.0") ;;
      PANE1) arguments+=("=$SESSION:$WINDOW_NAME.1") ;;
      *) arguments+=("$argument") ;;
      esac
    done
    set +e
    if [ -n "$CASE_STDIN_FILE" ]; then
      side_command "$side" "${arguments[@]}" \
        >"$SCRATCH_DIR/$side.out" 2>"$SCRATCH_DIR/$side.err" <"$CASE_STDIN_FILE"
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
  local zz_rc tmux_rc zz_state tmux_state
  zz_rc="$(cat "$SCRATCH_DIR/zz.rc")"
  tmux_rc="$(cat "$SCRATCH_DIR/tmux.rc")"
  zz_state="$(state_of zz)"
  tmux_state="$(state_of tmux)"
  LAST_EXIT_DIFFERED=0
  LAST_STDOUT_DIFFERED=0
  LAST_STDERR_DIFFERED=0
  LAST_STATE_DIFFERED=0
  [ "$zz_rc" = "$tmux_rc" ] || LAST_EXIT_DIFFERED=1
  cmp -s "$SCRATCH_DIR/zz.out" "$SCRATCH_DIR/tmux.out" || LAST_STDOUT_DIFFERED=1
  cmp -s "$SCRATCH_DIR/zz.err" "$SCRATCH_DIR/tmux.err" || LAST_STDERR_DIFFERED=1
  [ "$zz_state" = "$tmux_state" ] || LAST_STATE_DIFFERED=1
  if [ "$LAST_EXIT_DIFFERED" -eq 0 ] && [ "$LAST_STDOUT_DIFFERED" -eq 0 ] &&
    [ "$LAST_STDERR_DIFFERED" -eq 0 ] && [ "$LAST_STATE_DIFFERED" -eq 0 ]; then
    return 0
  fi
  printf '      case %s\n' "$name"
  [ "$LAST_EXIT_DIFFERED" -eq 0 ] ||
    printf '      exit tmux: %s  zz: %s\n' "$tmux_rc" "$zz_rc"
  if [ "$LAST_STDOUT_DIFFERED" -eq 1 ]; then
    printf '      stdout differs (tmux %s bytes, zz %s bytes)\n' \
      "$(wc -c <"$SCRATCH_DIR/tmux.out")" "$(wc -c <"$SCRATCH_DIR/zz.out")"
    diff <(cat -v "$SCRATCH_DIR/tmux.out") <(cat -v "$SCRATCH_DIR/zz.out") |
      sed -e 's/^/        /' | head -20
  fi
  if [ "$LAST_STDERR_DIFFERED" -eq 1 ]; then
    printf '      stderr differs\n'
    diff <(cat -v "$SCRATCH_DIR/tmux.err") <(cat -v "$SCRATCH_DIR/zz.err") |
      sed -e 's/^/        /' | head -20
  fi
  if [ "$LAST_STATE_DIFFERED" -eq 1 ]; then
    printf '      state differs\n'
    diff <(printf '%s\n' "$tmux_state") <(printf '%s\n' "$zz_state") |
      sed -e 's/^/        /' | head -30
  fi
  return 1
}

# NAME MODE REASON -- command...
#   same     all four channels asserted
#   decided  a difference this campaign decided to keep; asserts nothing and
#            the reason has to carry the decision sentence
#   record   nothing asserted, the reason says why
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
  run_both "$@"
  settle_state zz
  settle_state tmux
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
  decided)
    DECIDED=$((DECIDED + 1))
    case "$reason" in
    *"$DECISION"*) ;;
    *) die "decided case $name does not carry the decision sentence" ;;
    esac
    if [ "$same" -eq 0 ]; then
      printf 'note  %s is identical on all four channels, the decision can close\n' "$name"
    else
      printf 'note  %s decided, not asserted: %s\n' "$name" "$reason"
    fi
    ;;
  record)
    RECORDS=$((RECORDS + 1))
    [ -n "$reason" ] || die "recorded case $name says nothing about why"
    if [ "$same" -eq 0 ]; then
      printf 'note  %s is identical on all four channels, the record can close\n' "$name"
    else
      printf 'note  %s recorded, not asserted: %s\n' "$name" "$reason"
    fi
    ;;
  *) die "unknown case mode $mode" ;;
  esac
  CASE_STDIN_FILE=''
}

stdin_from() {
  printf '%s' "$1" >"$SCRATCH_DIR/stdin.bin"
  CASE_STDIN_FILE="$SCRATCH_DIR/stdin.bin"
}
stdin_from_file() {
  CASE_STDIN_FILE="$1"
}

# Kill the pane a -I case built, on both sides, so the next case reads the
# state the roster expects.
drop_extra_panes() {
  local name="$1"
  local side pane
  CASE_LABEL="$name"
  for side in zz tmux; do
    for pane in $(side_command "$side" list-panes -a -F '#{pane_index}' | awk '$1 > 0'); do
      side_command "$side" kill-pane -t "=$SESSION:$WINDOW_NAME.$pane" >/dev/null 2>&1 || true
    done
    wait_for "the extra $side pane is gone for $name" pane_count_is "$side" 1
  done
  settle_state zz
  settle_state tmux
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

# --- the roster's cases -----------------------------------------------------
BOUND_DECISION="pinned tmux streams a caller payload in acknowledged 16 KiB chunks with no total limit and zz refuses one larger than MAX_AGENT_SEND_BYTES ($STREAM_CAP_BYTES) at the reader, before the daemon sees a byte: bulk file transfer through a command client is a workload zz does not serve, because an unbounded stream lets one caller grow daemon memory without limit; $DECISION"

config_sink_cases() {
  stdin_from 'set -g @zzcs-one alpha'
  case_run source-file-applies same '' -- source-file -
  case_run source-file-applied-value same '' -- show-options -gqv @zzcs-one
  case_run source-file-closed-stdin same '' -- source-file -
  stdin_from 'bogus-zzcs-command
'
  case_run source-file-unknown-command same '' -- source-file -
  stdin_from 'set -g @zzcs-two beta
set -g @zzcs-three'
  case_run source-file-cancelled-midline same '' -- source-file -
  stdin_from 'set -g @zzcs-parse delta'
  case_run source-file-parse-only same '' -- source-file -n -
  stdin_from 'set -g @zzcs-parse delta'
  case_run source-file-verbose same '' -- source-file -v -
  stdin_from 'set -g @zzcs-cancel epsilon'
  case_run source-file-quiet same '' -- source-file -q -
  stdin_from 'set -g @zzcs-one zeta'
  case_run source-file-twice-one-stream same '' -- source-file - -
  case_run source-file-twice-one-stream-value same '' -- show-options -gqv @zzcs-one
  stdin_from 'set -g @zzcs-one eta'
  case_run source-file-dash-and-missing-path same '' -- source-file - /nonexistent-zzcs.conf
  stdin_from ''
  case_run source-file-empty-stream same '' -- source-file -
}

pane_input_sink_cases() {
  stdin_from 'REFUSED'
  case_run display-message-into-a-running-pane same '' -- display-message -I -t PANE0
  stdin_from 'FIRST-WRITE
'
  case_run split-window-builds-and-writes same '' -- split-window -I -d -t PANE0
  stdin_from 'SECOND-WRITE
'
  case_run display-message-into-the-empty-pane same '' -- display-message -I -t PANE1
  stdin_from "$(printf 'BYTES-\303\251-\377-\000-END\r\n')"
  case_run display-message-binary-bytes same '' -- display-message -I -t PANE1
  case_run display-message-closed-stdin same '' -- display-message -I -t PANE1
  case_run display-message-missing-target same '' -- display-message -I -t %99
  drop_extra_panes pane-input-restored
  stdin_from 'PRINTED
'
  case_run split-window-prints-the-new-pane same '' -- \
    split-window -I -d -P -F '#{pane_index} #{?pane_pid,has-pid,no-pid}' -t PANE0
  drop_extra_panes split-window-print-restored
  stdin_from 'NEVER
'
  case_run split-window-with-a-command same '' -- split-window -I -d -t PANE0 true
  case_run split-window-closed-stdin same '' -- split-window -I -d -t PANE0
  drop_extra_panes split-window-restored
}

buffer_stream_cases() {
  stdin_from_file "$SCRATCH_DIR/binary.bin"
  case_run load-buffer-binary-stdin same '' -- load-buffer -b zzcsbin -
  case_run save-buffer-binary-stdout same '' -- save-buffer -b zzcsbin -
  case_run save-buffer-binary-append same '' -- save-buffer -a -b zzcsbin -
  case_run save-buffer-missing-buffer same '' -- save-buffer -b zzcs-nope -
  case_run buffer-deleted same '' -- delete-buffer -b zzcsbin
}

bound_cases() {
  stdin_from_file "$SCRATCH_DIR/at-cap.bin"
  case_run load-buffer-at-the-cap same '' -- load-buffer -b zzcscap -
  case_run load-buffer-at-the-cap-saved same '' -- save-buffer -b zzcscap -
  case_run load-buffer-at-the-cap-deleted same '' -- delete-buffer -b zzcscap
  stdin_from_file "$SCRATCH_DIR/at-cap.conf"
  case_run source-file-at-the-cap same '' -- source-file -
  stdin_from_file "$SCRATCH_DIR/over-cap.bin"
  case_run load-buffer-over-the-cap decided "$BOUND_DECISION" -- load-buffer -b zzcsover -
  case_run load-buffer-over-the-cap-listed decided "$BOUND_DECISION" -- list-buffers -F '#{buffer_name}'
  side_command zz delete-buffer -b zzcsover >/dev/null 2>&1 || true
  side_command tmux delete-buffer -b zzcsover >/dev/null 2>&1 || true
  stdin_from_file "$SCRATCH_DIR/over-cap.conf"
  case_run source-file-over-the-cap decided "$BOUND_DECISION" -- source-file -
  case_run source-file-over-the-cap-value decided "$BOUND_DECISION" -- show-options -gqv @zzcs-one
}

install_stream_aliases() {
  local body='zzcs-twostream=source-file - ; source-file -'
  local single='zzcs-onestream=source-file -'
  side_command zz set -s 'command-alias[77]' "$body" >/dev/null ||
    die 'zz refused the two-member command-alias'
  side_command tmux set -s 'command-alias[77]' "$body" >/dev/null ||
    die 'tmux refused the two-member command-alias'
  side_command zz set -s 'command-alias[78]' "$single" >/dev/null ||
    die 'zz refused the single-member command-alias'
  side_command tmux set -s 'command-alias[78]' "$single" >/dev/null ||
    die 'tmux refused the single-member command-alias'
  local side
  for side in zz tmux; do
    side_command "$side" set -s 'command-alias[79]' \
      'zzcs-buffer-source=load-buffer -b zzcsalias - ; source-file -' >/dev/null ||
      die "$side refused the buffer/source alias"
    side_command "$side" set -s 'command-alias[80]' \
      'zzcs-buffer-twice=load-buffer -b zzcsalias - ; load-buffer -b zzcssecond - ; display-message -p after-error' >/dev/null ||
      die "$side refused the two-buffer alias"
    side_command "$side" set -s 'command-alias[81]' \
      'zzcs-buffer-last=display-message -p before ; load-buffer -b zzcsalias -' >/dev/null ||
      die "$side refused the buffer-last alias"
  done
}

alias_group_cases() {
  install_stream_aliases
  stdin_from 'set -g @zzcs-one kappa'
  case_run source-file-alias-group-two-members same '' -- zzcs-twostream
  case_run source-file-alias-group-two-members-value same '' -- show-options -gqv @zzcs-one

  stdin_from 'set -g @zzcs-one lambda'
  case_run source-file-alias-single-member same '' -- zzcs-onestream
  case_run source-file-alias-single-member-value same '' -- show-options -gqv @zzcs-one

  stdin_from_file "$SCRATCH_DIR/binary.bin"
  case_run load-buffer-alias-group-source-second same '' -- zzcs-buffer-source
  case_run load-buffer-alias-group-source-second-value same '' -- save-buffer -b zzcsalias -
  stdin_from_file "$SCRATCH_DIR/binary.bin"
  case_run load-buffer-alias-group-two-readers same '' -- zzcs-buffer-twice
  case_run load-buffer-alias-group-two-readers-value same '' -- save-buffer -b zzcsalias -
  stdin_from_file "$SCRATCH_DIR/binary.bin"
  case_run load-buffer-alias-group-reader-last same '' -- zzcs-buffer-last
  case_run load-buffer-alias-group-reader-last-value same '' -- save-buffer -b zzcsalias -
  zz_command delete-buffer -b zzcsalias >/dev/null || die 'zz refused alias buffer cleanup'
  tmux_command delete-buffer -b zzcsalias >/dev/null || die 'tmux refused alias buffer cleanup'
}

write_payloads() {
  printf 'a\303\251b\377c\000d\n' >"$SCRATCH_DIR/binary.bin"
  # The cap counts bytes, so these are built by size and never by line count.
  head -c "$STREAM_CAP_BYTES" /dev/zero | tr '\0' 'x' >"$SCRATCH_DIR/at-cap.bin"
  head -c "$((STREAM_CAP_BYTES + 1))" /dev/zero | tr '\0' 'x' >"$SCRATCH_DIR/over-cap.bin"
  {
    printf 'set -g @zzcs-one theta\n'
    head -c "$((STREAM_CAP_BYTES - 24))" /dev/zero | tr '\0' '#'
    printf '\n'
  } >"$SCRATCH_DIR/at-cap.conf"
  {
    printf 'set -g @zzcs-one iota\n'
    head -c "$STREAM_CAP_BYTES" /dev/zero | tr '\0' '#'
    printf '\n'
  } >"$SCRATCH_DIR/over-cap.conf"
  [ "$(wc -c <"$SCRATCH_DIR/at-cap.conf")" -le "$STREAM_CAP_BYTES" ] ||
    die 'the at-cap config payload is over the cap'
  [ "$(wc -c <"$SCRATCH_DIR/over-cap.conf")" -gt "$STREAM_CAP_BYTES" ] ||
    die 'the over-cap config payload is not over the cap'
}

run_cases() {
  printf 'caller stream forms at %sx%s (pin %s, cap %s bytes)\n' \
    "$COLUMNS_UNDER_TEST" "$ROWS_UNDER_TEST" "$(basename -- "$TMUX_BIN")" "$STREAM_CAP_BYTES"
  build_scene
  write_payloads
  config_sink_cases
  pane_input_sink_cases
  buffer_stream_cases
  alias_group_cases
  bound_cases

  if [ "$FAILURES" -ne 0 ]; then
    printf '%s of %s asserted comparisons differ, %s recorded not asserted, %s decided (%s for a sibling lane)\n' \
      "$FAILURES" "$CHECKS" "$RECORDS" "$DECIDED" "$SIBLINGS"
    exit 1
  fi
  printf 'all %s asserted comparisons identical, %s recorded not asserted, %s decided (%s for a sibling lane)\n' \
    "$CHECKS" "$RECORDS" "$DECIDED" "$SIBLINGS"
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
  settle_state zz
  settle_state tmux
  compare_channels "$name" || true
  CASE_STDIN_FILE=''
}

zz_pane_shows() {
  pane_shows zz "$1" "$2"
}

self_check_alias_buffer_value() {
  local name="$1"
  LC_ALL=C tr a z <"$SCRATCH_DIR/binary.bin" >"$SCRATCH_DIR/sabotage.bin"
  zz_command load-buffer -b zzcsalias - <"$SCRATCH_DIR/sabotage.bin" ||
    die 'zz refused the buffer value sabotage'
  self_check_run "$name" save-buffer -b zzcsalias -
  self_check_expect "$name: one byte changed on zz" exit=0 stdout=1 stderr=0 state=0
  zz_command load-buffer -b zzcsalias - <"$SCRATCH_DIR/binary.bin" ||
    die 'zz refused to restore the buffer bytes'
}

run_self_check() {
  printf 'self-check: one deliberate one-sided difference per channel, plus two equivalences\n'
  build_scene
  write_payloads

  # The equivalence first: with nothing planted the same command on both sides
  # must report nothing, or every sabotage below would be satisfied by a
  # comparison that always reports.
  self_check_run equivalence-before display-message -p -t PANE0 '#{pane_index}'
  self_check_expect 'equivalence: the same command on both sides' \
    exit=0 stdout=0 stderr=0 state=0

  # stdout: an option that only the zz side carries, read by name. The exit
  # status and stderr are the same on both sides and must not be reported, and
  # the option is outside PROBE_OPTIONS so the state does not move either.
  zz_command set-option -g @zzcs-sabotage SABOTAGE >/dev/null || die 'zz refused set-option'
  self_check_run stdout-sabotage show-options -gqv @zzcs-sabotage
  self_check_expect 'stdout, an option on one side only' exit=0 stdout=1 stderr=0 state=0

  # exit status and stderr: saving a buffer one side does not have. stdout
  # differs too - that is what an error instead of a payload means - and the
  # two channels under test have to be reported by themselves.
  self_check_run exit-sabotage save-buffer -b zzcs-sabotage-missing -
  self_check_expect 'exit and stderr, a buffer missing on both sides' \
    exit=0 stdout=0 stderr=0 state=0
  zz_command set-buffer -b zzcs-sabotage SABOTAGE >/dev/null || die 'zz refused set-buffer'
  self_check_run exit-sabotage-present show-buffer -b zzcs-sabotage
  self_check_expect 'exit and stderr, a buffer one side does not have' exit=1 stderr=1
  zz_command delete-buffer -b zzcs-sabotage >/dev/null || die 'zz refused delete-buffer'
  zz_command set-option -gu @zzcs-sabotage >/dev/null || die 'zz refused set-option -u'

  # session state: a second session on the zz side only. No CLI channel of an
  # unrelated command may move.
  zz_command new-session -d -s zzcs-sab -x 80 -y 24 "$INNER_SHELL" >/dev/null ||
    die 'zz refused new-session'
  self_check_run state-sabotage display-message -p -t PANE0 '#{pane_index}'
  self_check_expect 'state, a session on one side only' \
    exit=0 stdout=0 stderr=0 state=1
  zz_command kill-session -t zzcs-sab >/dev/null || die 'zz refused kill-session'

  # pane content, which is where a PaneInput sink's proof lives: bytes written
  # into the zz side's pane and not the pin's. The channel that must report it
  # is the state, and no CLI channel may move.
  zz_command split-window -E -d -t "=$SESSION:$WINDOW_NAME.0" >/dev/null ||
    die 'zz refused split-window -E'
  tmux_command split-window -E -d -t "=$SESSION:$WINDOW_NAME.0" >/dev/null ||
    die 'tmux refused split-window -E'
  printf 'ONE-SIDED\n' >"$SCRATCH_DIR/stdin.bin"
  zz_command display-message -I -t "=$SESSION:$WINDOW_NAME.1" <"$SCRATCH_DIR/stdin.bin" ||
    die 'zz refused display-message -I'
  wait_for 'the one-sided pane bytes' zz_pane_shows 1 ONE-SIDED
  self_check_run content-sabotage display-message -p -t PANE0 '#{pane_index}'
  self_check_expect 'pane content, bytes written into one side only' \
    exit=0 stdout=0 stderr=0 state=1
  drop_extra_panes content-sabotage-withdrawn >/dev/null

  install_stream_aliases
  zz_command set -s 'command-alias[77]' 'zzcs-twostream=source-file -' >/dev/null ||
    die 'zz refused the one-reader sabotage'
  stdin_from 'set -g @zzcs-one kappa'
  self_check_run source-file-alias-group-two-members zzcs-twostream
  self_check_expect 'source-file-alias-group-two-members: second reader removed on zz' \
    exit=1 stdout=0 stderr=1 state=0
  zz_command set -g @zzcs-one sabotage >/dev/null || die 'zz refused the value sabotage'
  self_check_run source-file-alias-group-two-members-value show-options -gqv @zzcs-one
  self_check_expect 'source-file-alias-group-two-members-value: zz option changed' \
    exit=0 stdout=1 stderr=0 state=1
  zz_command set -gu @zzcs-one >/dev/null || die 'zz refused to restore the option'
  tmux_command set -gu @zzcs-one >/dev/null || die 'tmux refused to restore the option'

  zz_command set -s 'command-alias[79]' 'zzcs-buffer-source=load-buffer -b zzcsalias -' >/dev/null ||
    die 'zz refused the buffer/source sabotage'
  stdin_from_file "$SCRATCH_DIR/binary.bin"
  self_check_run load-buffer-alias-group-source-second zzcs-buffer-source
  self_check_expect 'load-buffer-alias-group-source-second: source reader removed on zz' \
    exit=1 stdout=0 stderr=1 state=0
  self_check_alias_buffer_value load-buffer-alias-group-source-second-value

  zz_command set -s 'command-alias[80]' \
    'zzcs-buffer-twice=load-buffer -b zzcsalias - ; display-message -p after-error' >/dev/null ||
    die 'zz refused the second buffer reader sabotage'
  stdin_from_file "$SCRATCH_DIR/binary.bin"
  self_check_run load-buffer-alias-group-two-readers zzcs-buffer-twice
  self_check_expect 'load-buffer-alias-group-two-readers: second reader removed on zz' \
    exit=1 stdout=0 stderr=1 state=0
  self_check_alias_buffer_value load-buffer-alias-group-two-readers-value

  zz_command set -s 'command-alias[81]' \
    'zzcs-buffer-last=display-message -p sabotage ; load-buffer -b zzcsalias -' >/dev/null ||
    die 'zz refused the buffer-last stdout sabotage'
  stdin_from_file "$SCRATCH_DIR/binary.bin"
  self_check_run load-buffer-alias-group-reader-last zzcs-buffer-last
  self_check_expect 'load-buffer-alias-group-reader-last: first member output changed on zz' \
    exit=0 stdout=1 stderr=0 state=0
  self_check_alias_buffer_value load-buffer-alias-group-reader-last-value
  zz_command delete-buffer -b zzcsalias >/dev/null || die 'zz refused alias buffer cleanup'
  tmux_command delete-buffer -b zzcsalias >/dev/null || die 'tmux refused alias buffer cleanup'
  install_stream_aliases

  # The second equivalence: with every sabotage withdrawn the comparison is
  # silent again, so none of the four above was a difference the scene kept.
  self_check_run equivalence-after display-message -p -t PANE0 '#{pane_index}'
  self_check_expect 'equivalence: every sabotage withdrawn' \
    exit=0 stdout=0 stderr=0 state=0

  if [ "$SELF_CHECK_FAILURES" -ne 0 ]; then
    printf '%s self-check expectations unmet\n' "$SELF_CHECK_FAILURES"
    exit 1
  fi
  printf 'self-check complete: every sabotage was caught in its own channel and both equivalences passed\n'
}

zz_command -f /dev/null daemon >"$SCRATCH_DIR/zz-daemon.out" 2>"$SCRATCH_DIR/zz-daemon.err" &
ZZ_PID=$!
wait_for "zz daemon socket" test -S "$ZZ_SOCKET"

if [ "$SELF_CHECK" -eq 1 ]; then
  run_self_check
else
  run_cases
fi
