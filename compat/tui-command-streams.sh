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
# display-message -I          PaneInput   streamed into a pane with no process
# split-window -I             PaneInput   builds that pane, then streams into it
# load-buffer -               Argument    the bytes become a paste buffer
# save-buffer - / -a -        (stdout)    the buffer's bytes, exactly, to stdout
# ---------------------------------------------------------------------------
#
# FOUR CHANNELS PER CASE. exit status, stdout bytes and stderr bytes come from
# the invocation; the state is a fixed set of list-* formats plus the option
# values under test plus the first rows of every pane, which is where a
# PaneInput sink's bytes land. `same` asserts all four. `decided` prints a
# difference that is a recorded product decision and asserts nothing about it.
# `record` asserts nothing and has to say why; a recorded case holds the
# obligation's clause open.
#
# THE TARGET CAN ALSO DISAPPEAR while the caller still holds stdin open. The
# pin's callback checks `wp == NULL` before it looks at end of file, so a pane
# killed mid-stream exits 1, prints nothing more and drops the rest of the
# caller's `\;` chain. Four cells cover it: both readers, and a writer that
# closes its end or writes more bytes once the pane is gone.
#
# THE ONE DECIDED DIFFERENCE is the bound on a sink that holds its payload.
# Pinned tmux accumulates a source-file - or load-buffer - payload in 16 KiB
# chunks with no total limit; zz reads at most MAX_AGENT_SEND_BYTES (1 MiB) and
# refuses the whole invocation at the reader, before the daemon sees a byte.
# Bulk file transfer through a command client is a workload zz does not serve,
# because an unbounded accumulation lets one caller grow daemon memory without
# limit. Every case at and below the cap is asserted, so what is decided is the
# bound and nothing else. A PaneInput sink holds nothing: it streams chunks into
# the pane with backpressure and no total cap, like the pin, and
# stream-display-fast asserts a payload over the cap.
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
# NOT report.
set -eEuo pipefail

usage() {
  printf 'usage: compat/tui-command-streams.sh [--self-check] [--execution-check] [--matrix] [--matrix-list] [ZZ_BIN [TMUX_BIN]]\n' >&2
  printf '       ZZ_BIN=path TMUX_BIN=path compat/tui-command-streams.sh\n' >&2
}

COMPAT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd -- "$COMPAT_DIR/.." && pwd)"
SELF_CHECK=0
EXECUTION_CHECK=0
MATRIX_CHECK=0
MATRIX_LIST=0
POSITIONAL=()
for argument in "$@"; do
  case "$argument" in
  --self-check) SELF_CHECK=1 ;;
  --execution-check) EXECUTION_CHECK=1 ;;
  --matrix) MATRIX_CHECK=1 ;;
  --matrix-list) MATRIX_LIST=1 ;;
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
ENVIRONMENT=0
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
  scrubbed "$TMUX_BIN" -L "$INNER_SOCKET_NAME-matrix" kill-server >/dev/null 2>&1
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
PROBE_OPTIONS=(@zzcs-after @zzcs-replay @zzcs-one @zzcs-two @zzcs-three @zzcs-parse @zzcs-cancel @zzcs-before @zzcs-control)

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
  local zz_rc tmux_rc zz_state tmux_state side
  zz_rc="$(cat "$SCRATCH_DIR/zz.rc")"
  tmux_rc="$(cat "$SCRATCH_DIR/tmux.rc")"
  if [ "${CASE_STATE_FILE:-0}" = 1 ]; then
    zz_state="$(cat "$SCRATCH_DIR/zz.matrix-state")"
    tmux_state="$(cat "$SCRATCH_DIR/tmux.matrix-state")"
  else
    zz_state="$(state_of zz)"
    tmux_state="$(state_of tmux)"
  fi
  if [ -n "${ZZ_STREAM_PROBES:-}" ]; then
    {
      printf 'case=%s\nzz_exit=%s\ntmux_exit=%s\n' "$name" "$zz_rc" "$tmux_rc"
      for side in zz tmux; do
        printf '%s stdout hex\n' "$side"
        xxd -p "$SCRATCH_DIR/$side.out"
        printf '%s stderr hex\n' "$side"
        xxd -p "$SCRATCH_DIR/$side.err"
      done
      printf 'zz state\n%s\ntmux state\n%s\n' "$zz_state" "$tmux_state"
    } >>"$ZZ_STREAM_PROBES"
  fi
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

# A zz side that produced nothing at all where the pin produced bytes did not
# lose a comparison, it never ran: under concurrent load on this box a starved
# command client answers with an empty stream. That is an environment failure,
# reported the way a wait that never reached its execution point already is,
# and never a parity difference.
stream_output_starved() {
  [ ! -s "$SCRATCH_DIR/zz.out" ] && [ -s "$SCRATCH_DIR/tmux.out" ]
}

report_stream_environment_failure() {
  ENVIRONMENT=$((ENVIRONMENT + 1))
  printf 'env   %s: the zz side produced no stdout where the pin produced %s bytes\n' \
    "$1" "$(wc -c <"$SCRATCH_DIR/tmux.out")"
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
      if stream_output_starved; then
        report_stream_environment_failure "$name"
      else
        printf 'DIFF  %s\n' "$name"
      fi
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
    for pane in $(side_command "$side" list-panes -t "=$SESSION:$WINDOW_NAME" -F '#{pane_index} #{pane_id}' | awk '$1 > 0 {print $2}'); do
      side_command "$side" kill-pane -t "$pane" >/dev/null 2>&1 || true
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
BOUND_DECISION="pinned tmux accumulates a source-file or load-buffer payload in 16 KiB chunks with no total limit and zz refuses one larger than MAX_AGENT_SEND_BYTES ($STREAM_CAP_BYTES) at the reader, before the daemon sees a byte: bulk file transfer through a command client is a workload zz does not serve, because an unbounded stream lets one caller grow daemon memory without limit; $DECISION"

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

review_alias_case() {
  local name="$1" body="$2" sabotage="$3" payload="$4" file="$5" channel="$6" side
  if [ "$name" = alias-c-locale-binary-unterminated-stdout ]; then
    local LC_ALL=C
    export LC_ALL
  fi
  for side in zz tmux; do
    side_command "$side" set -gu @zzcs-after >/dev/null
    side_command "$side" set -gu @zzcs-replay >/dev/null
    local selected="$body"
    if [ "$SELF_CHECK" -eq 1 ] && [ "$side" = zz ] && [[ "$name" != *binary-unterminated-stdout ]]; then
      selected="$sabotage"
    fi
    side_command "$side" set -s 'command-alias[82]' "zzcs-review=$selected" >/dev/null ||
      die "$side refused $name alias"
    if [ "$file" = file-tail ]; then
      side_command "$side" set -s 'command-alias[83]' \
        'zzcs-tail=display-message -p ignored ; set -g @zzcs-after yes' >/dev/null ||
        die "$side refused tail alias"
    fi
  done
  stdin_from_file "$payload"
  local -a command=(zzcs-review)
  if [[ "$file" = file* ]]; then
    printf 'zzcs-review\n' >"$SCRATCH_DIR/replay.conf"
    if [ "$file" = file-tail ]; then
      printf 'zzcs-tail\n' >>"$SCRATCH_DIR/replay.conf"
    fi
    command=(source-file "$SCRATCH_DIR/replay.conf")
  fi
  if [ "$SELF_CHECK" -eq 1 ]; then
    self_check_run "$name" "${command[@]}"
    self_check_expect "$name: altered alias member on zz" "$channel=1"
  else
    case_run "$name" same '' -- "${command[@]}"
  fi
  for side in zz tmux; do
    side_command "$side" set -gu @zzcs-after >/dev/null
    side_command "$side" set -gu @zzcs-replay >/dev/null
    side_command "$side" delete-buffer -b zzcsreview >/dev/null 2>&1 || true
  done
}

startup_config_stream_case() {
  local side config="$SCRATCH_DIR/startup.conf" rc
  local -a base
  for side in zz tmux; do
    printf 'source-file -\nset -g @boot-ready yes\n' >"$config"
    if [ "$SELF_CHECK" -eq 1 ] && [ "$side" = zz ]; then
      printf 'set -g @boot-input applied\nset -g @boot-ready yes\n' >"$config"
    fi
    if [ "$side" = zz ]; then
      base=("$ZZ_BIN" --socket "${ZZ_SOCKET%.sock}-boot.sock")
    else
      base=("$TMUX_BIN" -L "$INNER_SOCKET_NAME-boot")
    fi
    set +e
    printf 'set -g @boot-input applied\n' |
      scrubbed HOME="$SCRATCH_DIR/boot-$side" XDG_CONFIG_HOME="$SCRATCH_DIR/boot-$side/config" \
      "${base[@]}" -f "$config" new-session -d -s boot 'sleep 60' \
      >"$SCRATCH_DIR/$side.out" 2>"$SCRATCH_DIR/$side.err"
    rc=$?
    set -e
    printf '%s\n' "$rc" >"$SCRATCH_DIR/$side.rc"
    scrubbed "${base[@]}" show-options -gqv @boot-input >>"$SCRATCH_DIR/$side.out"
    scrubbed "${base[@]}" show-options -gqv @boot-ready >>"$SCRATCH_DIR/$side.out"
    scrubbed "${base[@]}" kill-server >/dev/null 2>&1 || true
  done
  compare_channels daemon-start-config-refuses-stdin || true
  if [ "$SELF_CHECK" -eq 1 ]; then
    self_check_expect 'daemon-start-config-refuses-stdin: applied payload on zz' stdout=1
  else
    CHECKS=$((CHECKS + 1))
    if [ "$LAST_EXIT_DIFFERED$LAST_STDOUT_DIFFERED$LAST_STDERR_DIFFERED$LAST_STATE_DIFFERED" = 0000 ] &&
      [ "$(cat "$SCRATCH_DIR/tmux.out")" = yes ] && [ "$(cat "$SCRATCH_DIR/tmux.rc")" = 0 ]; then
      printf 'ok    daemon-start-config-refuses-stdin\n'
    else
      FAILURES=$((FAILURES + 1))
      printf 'DIFF  daemon-start-config-refuses-stdin\n'
    fi
  fi
}

review_alias_cases() {
  printf 'set -g @zzcs-replay applied\n' >"$SCRATCH_DIR/replay-input.conf"
  printf 'a\377\000z' >"$SCRATCH_DIR/unterminated.bin"
  local reader name
  for name in source buffer pane; do
    case "$name" in
    source) reader='source-file -' ;;
    buffer) reader='load-buffer -b zzcsreview -' ;;
    pane) reader="split-window -I -d -t =$SESSION:$WINDOW_NAME.0" ;;
    esac
    review_alias_case "alias-$name-spent-source-continues" \
      "$reader ; source-file - ; display-message -p after ; set -g @zzcs-after yes" \
      "$reader ; source-file -" "$SCRATCH_DIR/replay-input.conf" direct stdout
    if [ "$SELF_CHECK" -eq 1 ]; then
      review_alias_case "alias-$name-spent-source-continues-state" \
        "$reader ; source-file - ; display-message -p after ; set -g @zzcs-after yes" \
        "$reader ; source-file - ; display-message -p after" "$SCRATCH_DIR/replay-input.conf" direct state
    fi
    if [ "$name" = pane ]; then
      drop_extra_panes alias-pane-restored >/dev/null
    fi
  done
  review_alias_case alias-missing-source-aborts \
    'display-message -p before ; source-file /tmp/zz018-no-such-config ; display-message -p after ; set -g @zzcs-after yes' \
    'display-message -p before ; source-file -q /tmp/zz018-no-such-config ; display-message -p after ; set -g @zzcs-after yes' \
    "$SCRATCH_DIR/replay-input.conf" direct state
  review_alias_case file-replay-single-reader 'source-file -' \
    'display-message -p dropped' "$SCRATCH_DIR/replay-input.conf" file state
  review_alias_case file-replay-nonreader-then-reader \
    'set -g @zzcs-after yes ; source-file -' \
    'set -g @zzcs-after yes' "$SCRATCH_DIR/replay-input.conf" file state
  review_alias_case file-replay-two-readers 'source-file - ; source-file -' \
    'source-file -' "$SCRATCH_DIR/replay-input.conf" file stderr
  review_alias_case alias-binary-unterminated-stdout \
    'load-buffer -b zzcsreview - ; save-buffer -b zzcsreview -' \
    'load-buffer -b zzcsreview - ; set-buffer -b zzcsreview changed ; save-buffer -b zzcsreview -' \
    "$SCRATCH_DIR/unterminated.bin" direct stdout
  review_alias_case alias-c-locale-binary-unterminated-stdout \
    'load-buffer -b zzcsreview - ; save-buffer -b zzcsreview -' \
    'load-buffer -b zzcsreview - ; save-buffer -b zzcsreview -' \
    "$SCRATCH_DIR/unterminated.bin" direct stdout
  review_alias_case file-replay-binary-unterminated-stdout \
    'load-buffer -b zzcsreview - ; save-buffer -b zzcsreview -' \
    'load-buffer -b zzcsreview - ; save-buffer -b zzcsreview -' \
    "$SCRATCH_DIR/unterminated.bin" file stdout
  review_alias_case file-replay-binary-then-alias-print \
    'load-buffer -b zzcsreview - ; save-buffer -b zzcsreview -' \
    'load-buffer -b zzcsreview - ; save-buffer -b zzcsreview - ; save-buffer -b zzcsreview -' \
    "$SCRATCH_DIR/unterminated.bin" file-tail stderr
  review_alias_case alias-binary-then-print \
    'load-buffer -b zzcsreview - ; save-buffer -b zzcsreview - ; display-message -p tail' \
    'load-buffer -b zzcsreview - ; display-message -p changed' \
    "$SCRATCH_DIR/unterminated.bin" direct stdout
}

review_pending_case() {
  local name="$1" shape="$2" channel="$3" side pid rc before attempt
  local -a base command
  for side in zz tmux; do
    side_command "$side" set -gu @zzcs-before >/dev/null
    side_command "$side" set -gu @zzcs-cancel >/dev/null
    side_command "$side" set -gu @zzcs-replay >/dev/null
    printf 'set -g @zzcs-replay yes\n' >"$SCRATCH_DIR/unused.conf"
    local body='set -g @zzcs-before yes ; source-file - ; set -g @zzcs-cancel yes'
    if [ "$SELF_CHECK" -eq 1 ] && [ "$side" = zz ]; then
      case "$shape" in
      unused-open|unused-large) printf 'source-file -\nset -g @zzcs-replay yes\n' >"$SCRATCH_DIR/unused.conf" ;;
      alias-order|file-order) body='source-file - ; set -g @zzcs-before yes ; set -g @zzcs-cancel yes' ;;
      esac
    fi
    side_command "$side" set -s 'command-alias[84]' "zzcs-pending=$body" >/dev/null
    command=(zzcs-pending)
    case "$shape" in
    unused-*) command=(source-file "$SCRATCH_DIR/unused.conf") ;;
    file-order) printf 'zzcs-pending\n' >"$SCRATCH_DIR/pending.conf"; command=(source-file "$SCRATCH_DIR/pending.conf") ;;
    esac
    if [ "$side" = zz ]; then
      base=(env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE
        HOME="$ZZ_HOME" XDG_CONFIG_HOME="$ZZ_HOME/config" ZZ_LOG_DIR="$ZZ_LOG_DIR"
        "$ZZ_BIN" --socket "$ZZ_SOCKET")
    else
      base=(env -u TMUX -u TMUX_PANE TMUX_TMPDIR=/tmp HOME="$TMUX_HOME"
        XDG_CONFIG_HOME="$TMUX_HOME/config" "$TMUX_BIN" -L "$INNER_SOCKET_NAME")
    fi
    if [ "$shape" = unused-large ]; then
      set +e
      "${base[@]}" "${command[@]}" <"$SCRATCH_DIR/over-cap.bin" >"$SCRATCH_DIR/$side.out" 2>"$SCRATCH_DIR/$side.err"
      rc=$?
      set -e
    else
      mkfifo "$SCRATCH_DIR/pending-$side"
      exec 9<>"$SCRATCH_DIR/pending-$side"
      if [ "$shape" = unused-open ]; then
        (exec timeout 3 "${base[@]}" "${command[@]}" <"$SCRATCH_DIR/pending-$side" 9>&-) >"$SCRATCH_DIR/$side.out" 2>"$SCRATCH_DIR/$side.err" &
      else
        (exec "${base[@]}" "${command[@]}" <"$SCRATCH_DIR/pending-$side" 9>&-) >"$SCRATCH_DIR/$side.out" 2>"$SCRATCH_DIR/$side.err" &
      fi
      pid=$!
      if [ "$shape" != unused-open ]; then
        printf 'set -g @zzcs-replay payload' >&9
        before=''
        for ((attempt = 0; attempt < 60; attempt++)); do
          before="$(side_command "$side" show-options -gqv @zzcs-before)"
          [ "$before" = yes ] && break
          sleep 0.05
        done
        printf 'before-eof=%s\n' "$before" >"$SCRATCH_DIR/$side.order"
        sleep 0.15
        kill -TERM "$pid" || die "$side pending reader exited before SIGTERM"
      fi
      set +e
      wait "$pid"
      rc=$?
      set -e
      exec 9>&-
      rm "$SCRATCH_DIR/pending-$side"
      if [ "$shape" != unused-open ]; then
        cat "$SCRATCH_DIR/$side.order" >>"$SCRATCH_DIR/$side.out"
      fi
    fi
    if [ "$SELF_CHECK" -eq 1 ] && [ "$side" = zz ]; then
      case "$shape" in
      term-status) rc=143 ;;
      term-payload) side_command "$side" set -g @zzcs-replay payload >/dev/null ;;
      term-tail) side_command "$side" set -g @zzcs-cancel yes >/dev/null ;;
      esac
    fi
    printf '%s\n' "$rc" >"$SCRATCH_DIR/$side.rc"
  done
  compare_channels "$name" || true
  if [ "$SELF_CHECK" -eq 1 ]; then
    self_check_expect "$name: $shape sabotage" "$channel=1"
  else
    CHECKS=$((CHECKS + 1))
    local oracle=1
    case "$shape" in
    unused-*) [ "$(tmux_command show-options -gqv @zzcs-replay)" = yes ] || oracle=0 ;;
    *)
      [ "$(cat "$SCRATCH_DIR/tmux.out")" = before-eof=yes ] || oracle=0
      [ -z "$(tmux_command show-options -gqv @zzcs-replay)" ] || oracle=0
      [ -z "$(tmux_command show-options -gqv @zzcs-cancel)" ] || oracle=0
      ;;
    esac
    if [ "$LAST_EXIT_DIFFERED$LAST_STDOUT_DIFFERED$LAST_STDERR_DIFFERED$LAST_STATE_DIFFERED" = 0000 ] &&
      [ "$(cat "$SCRATCH_DIR/tmux.rc")" = 0 ] && [ "$oracle" = 1 ]; then
      printf 'ok    %s\n' "$name"
    else
      FAILURES=$((FAILURES + 1))
      printf 'DIFF  %s\n' "$name"
    fi
  fi
  for side in zz tmux; do
    side_command "$side" set -gu @zzcs-before >/dev/null
    side_command "$side" set -gu @zzcs-cancel >/dev/null
    side_command "$side" set -gu @zzcs-replay >/dev/null
  done
}

review_control_case() {
  local name="$1" channel="$2" side rc body
  for side in zz tmux; do
    side_command "$side" set -gu @zzcs-control >/dev/null
    body='source-file - ; display-message -p after ; set -g @zzcs-control yes'
    if [ "$SELF_CHECK" -eq 1 ] && [ "$side" = zz ]; then
      case "$channel" in
      stdout) body='source-file - ; set -g @zzcs-control yes' ;;
      state) body='source-file - ; display-message -p after' ;;
      esac
    fi
    side_command "$side" set -s 'command-alias[85]' "zzcs-control=$body" >/dev/null
    set +e
    side_command "$side" -C zzcs-control </dev/null >"$SCRATCH_DIR/$side.control" 2>"$SCRATCH_DIR/$side.err"
    rc=$?
    set -e
    printf '%s\n' "$rc" >"$SCRATCH_DIR/$side.rc"
    sed -E 's/^(%begin|%end|%error) [0-9]+ [0-9]+ /\1 TIME ID /' "$SCRATCH_DIR/$side.control" >"$SCRATCH_DIR/$side.out"
  done
  compare_channels "$name" || true
  if [ "$SELF_CHECK" -eq 1 ]; then
    self_check_expect "$name: removed continuation on zz" "$channel=1"
  else
    CHECKS=$((CHECKS + 1))
    if [ "$LAST_EXIT_DIFFERED$LAST_STDOUT_DIFFERED$LAST_STDERR_DIFFERED$LAST_STATE_DIFFERED" = 0000 ]; then
      printf 'ok    %s\n' "$name"
    else
      FAILURES=$((FAILURES + 1))
      printf 'DIFF  %s\n' "$name"
    fi
  fi
  for side in zz tmux; do
    side_command "$side" set -gu @zzcs-control >/dev/null
  done
}

review_execution_cases() {
  review_pending_case file-unused-open-stdin unused-open exit
  review_pending_case file-unused-oversized-stdin unused-large exit
  review_pending_case alias-preceding-state-before-eof alias-order stdout
  review_pending_case file-preceding-state-before-eof file-order stdout
  review_pending_case pending-source-sigterm-status term-status exit
  review_pending_case pending-source-sigterm-payload-unapplied term-payload state
  review_pending_case pending-source-sigterm-tail-unapplied term-tail state
  review_control_case control-source-read-error-continues-output stdout
  review_control_case control-source-read-error-continues-state state
}

# A PaneInput sink parses each chunk into the pane as it arrives. Every sample
# is taken while the caller still holds its stdin open, after a SIGTERM, or
# after EOF, and each one reads the first pane row with its attributes plus the
# cursor, so a sink that buffers until EOF shows a blank row at 0:0. The fast
# shape reads the whole screen and leaves #{history_size} out: past history-limit
# the retained row count is semantic:history-limit-product-default, not this
# channel.
pane_stream_sample() {
  local side="$1" pane="$2" label="$3" rows="${4:-0}" facts=' cursor=#{cursor_x}:#{cursor_y} history=#{history_size}'
  if [ "$rows" != 0 ]; then facts=' cursor=#{cursor_x}:#{cursor_y}'; fi
  printf '%s=' "$label"
  side_command "$side" capture-pane -e -p -t "$pane" -S 0 -E "$rows" 2>&1 | cat -v | tr -d '\000' |
    awk '{ rows[NR] = $0 } END { last = 0; for (i = 1; i <= NR; i++) if (rows[i] != "") last = i; for (i = 1; i <= last; i++) printf "%s|", rows[i] }'
  side_command "$side" display-message -p -t "$pane" "$facts"
}

pane_stream_wait() {
  local side="$1" pane="$2" text="$3" attempt
  for ((attempt = 0; attempt < 60; attempt++)); do
    side_command "$side" capture-pane -p -t "$pane" -S 0 -E 23 2>/dev/null | grep -Fq -- "$text" && return 0
    sleep 0.05
  done
  return 1
}

# NAME DESTINATION SHAPE
#   partial  12 styled bytes, sampled while stdin stays open, then EOF
#   eof      a first write sampled open, a second write completed by EOF
#   term     12 styled bytes, then SIGTERM with stdin still open
#   slow     six writes 150 ms apart, sampled after the third and at EOF
#   fast     a numbered payload over the 1 MiB cap written as fast as the pipe takes it
#   utf8     a multibyte character and an escape sequence cut between writes
# DESTINATION is the pane the bytes reach: an existing empty pane through
# `display-message -I`, a pane `split-window -I` builds, or `split-print`, the
# same split asking for `-P`, whose caller stdout is read while stdin is still
# open so a server that holds the printed line until the command ends is caught.
pane_stream_case() {
  local name="$1" destination="$2" shape="$3" side pid rc pane attempt sabotage oracle=1 piece
  local -a base command
  CASE_LABEL="stream-$name"
  CASE_STATE_FILE=1
  for side in zz tmux; do
    if [ "$side" = zz ]; then
      base=(env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE HOME="$ZZ_HOME" XDG_CONFIG_HOME="$ZZ_HOME/config" ZZ_LOG_DIR="$ZZ_LOG_DIR" "$ZZ_BIN" --socket "$ZZ_SOCKET")
    else
      base=(env -u TMUX -u TMUX_PANE TMUX_TMPDIR=/tmp HOME="$TMUX_HOME" XDG_CONFIG_HOME="$TMUX_HOME/config" "$TMUX_BIN" -L "$INNER_SOCKET_NAME")
    fi
    sabotage=0
    if [ "$SELF_CHECK" = 1 ] && [ "$side" = zz ]; then sabotage=1; fi
    if [ "$destination" = display ]; then
      pane="$(side_command "$side" split-window -d -t "=$SESSION:$WINDOW_NAME.0" -P -F '#{pane_id}' '')"
      command=(display-message -I -t "$pane")
    elif [ "$destination" = split-print ]; then
      pane=''
      command=(split-window -I -d -P -F '#{pane_index} #{?pane_pid,has-pid,no-pid}' -t "=$SESSION:$WINDOW_NAME.0")
    else
      pane=''
      command=(split-window -I -d -t "=$SESSION:$WINDOW_NAME.0")
    fi
    rm -f "$SCRATCH_DIR/stream-fifo"
    mkfifo "$SCRATCH_DIR/stream-fifo"
    exec 9<>"$SCRATCH_DIR/stream-fifo"
    (exec "${base[@]}" "${command[@]}" <"$SCRATCH_DIR/stream-fifo" 9>&-) >"$SCRATCH_DIR/$side.out" 2>"$SCRATCH_DIR/$side.err" &
    pid=$!
    if [ -z "$pane" ]; then
      for ((attempt = 0; attempt < 100; attempt++)); do
        pane="$(side_command "$side" list-panes -t "=$SESSION:$WINDOW_NAME" -F '#{pane_index} #{pane_id}' 2>/dev/null | awk '$1 == 1 { print $2 }')"
        [ -n "$pane" ] && break
        sleep 0.03
      done
      [ -n "$pane" ] || oracle=0
    fi
    : >"$SCRATCH_DIR/$side.matrix-state"
    case "$shape" in
    partial|term)
      if [ "$sabotage" = 0 ]; then printf '\033[31mRED\033[0m' >&9; fi
      pane_stream_wait "$side" "$pane" RED || [ "$sabotage" = 1 ] || oracle=0
      pane_stream_sample "$side" "$pane" open >>"$SCRATCH_DIR/$side.matrix-state"
      pane_stream_open_stdout "$side" "$destination" >>"$SCRATCH_DIR/$side.matrix-state"
      if [ "$shape" = term ]; then
        kill -TERM "$pid" 2>/dev/null || oracle=0
      else
        if [ "$sabotage" = 1 ]; then printf '\033[31mRED\033[0m' >&9; fi
        exec 9>&-
      fi
      ;;
    eof)
      printf 'FIRST-' >&9
      pane_stream_wait "$side" "$pane" FIRST- || oracle=0
      pane_stream_sample "$side" "$pane" open >>"$SCRATCH_DIR/$side.matrix-state"
      if [ "$sabotage" = 0 ]; then printf '\033[1mSECOND\033[0m\r\nTHIRD' >&9; fi
      exec 9>&-
      ;;
    slow)
      for piece in 1 2 3 4 5 6; do
        if [ "$sabotage" = 0 ] || [ "$piece" != 5 ]; then printf 'slow%s-' "$piece" >&9; fi
        sleep 0.15
        if [ "$piece" = 3 ]; then
          pane_stream_wait "$side" "$pane" slow3- || oracle=0
          pane_stream_sample "$side" "$pane" third >>"$SCRATCH_DIR/$side.matrix-state"
        fi
      done
      exec 9>&-
      ;;
    fast)
      if [ "$sabotage" = 1 ]; then
        head -c "$(($(wc -c <"$SCRATCH_DIR/stream-fast.bin") - 16384))" "$SCRATCH_DIR/stream-fast.bin" >&9
      else
        cat "$SCRATCH_DIR/stream-fast.bin" >&9
      fi
      exec 9>&-
      ;;
    utf8)
      printf 'A\303' >&9
      sleep 0.3
      if [ "$sabotage" = 1 ]; then printf 'B\033[3' >&9; else printf '\251B\033[3' >&9; fi
      sleep 0.3
      printf '1mRED\033[0mC' >&9
      pane_stream_wait "$side" "$pane" RED || oracle=0
      pane_stream_sample "$side" "$pane" open >>"$SCRATCH_DIR/$side.matrix-state"
      exec 9>&-
      ;;
    esac
    for ((attempt = 0; attempt < 1200; attempt++)); do
      kill -0 "$pid" 2>/dev/null || break
      sleep 0.05
    done
    if kill -0 "$pid" 2>/dev/null; then
      kill -KILL "$pid" 2>/dev/null
      oracle=0
    fi
    set +e
    wait "$pid"
    rc=$?
    set -e
    exec 9>&-
    rm -f "$SCRATCH_DIR/stream-fifo"
    printf '%s\n' "$rc" >"$SCRATCH_DIR/$side.rc"
    settle_state "$side"
    if [ "$shape" = fast ]; then
      pane_stream_sample "$side" "$pane" final 23 >>"$SCRATCH_DIR/$side.matrix-state"
    else
      pane_stream_sample "$side" "$pane" final >>"$SCRATCH_DIR/$side.matrix-state"
    fi
    printf 'panes=%s\n' "$(side_command "$side" list-panes -t "=$SESSION:$WINDOW_NAME" -F x | wc -l)" >>"$SCRATCH_DIR/$side.matrix-state"
    side_command "$side" kill-pane -t "$pane" >/dev/null 2>&1 || true
  done
  compare_channels "$CASE_LABEL" || true
  case "$shape" in
  partial|term) grep -Fq 'open=^[[31mRED' "$SCRATCH_DIR/tmux.matrix-state" || oracle=0 ;;
  fast) grep -Fq '180000| cursor=' "$SCRATCH_DIR/tmux.matrix-state" || oracle=0 ;;
  esac
  if [ "$SELF_CHECK" = 1 ]; then
    if [ "$oracle" != 1 ]; then
      SELF_CHECK_FAILURES=$((SELF_CHECK_FAILURES + 1))
      printf 'FAIL  self-check %s did not reach its required execution point\n' "$CASE_LABEL"
    fi
    self_check_expect "$CASE_LABEL: $shape delivery sabotage" state=1
  else
    CHECKS=$((CHECKS + 1))
    if [ "$LAST_EXIT_DIFFERED$LAST_STDOUT_DIFFERED$LAST_STDERR_DIFFERED$LAST_STATE_DIFFERED" = 0000 ] && [ "$oracle" = 1 ]; then
      printf 'ok    %s\n' "$CASE_LABEL"
    else
      FAILURES=$((FAILURES + 1))
      if stream_output_starved; then
        oracle=0
        report_stream_environment_failure "$CASE_LABEL"
      else
        printf 'DIFF  %s\n' "$CASE_LABEL"
      fi
    fi
  fi
  CASE_STATE_FILE=0
}

# `-P` is `cmdq_print`, which the pin writes through to the caller's stdout
# while the input callback is still open, so the new pane's line is readable
# before the stream ends. Only the shapes that carry -P read it; the others
# print nothing here and stay byte-for-byte what they were.
pane_stream_open_stdout() {
  local side="$1" destination="$2"
  [ "$destination" = split-print ] || return 0
  printf 'open-stdout=%s\n' "$(cat "$SCRATCH_DIR/$side.out" | cat -v | tr '\n' '|')"
}

# The target pane disappears while the caller's stream is still open.
# `window_pane_input_callback` finds no pane, so the pin sets `c->retval = 1`
# and `CLIENT_EXIT` and cancels the read: the invocation exits 1, prints
# nothing of what came after it and the rest of the chain never runs. AFTER is
# what the writer does once the pane is gone - close its end, or write more
# bytes into a stream nobody reads any more.
pane_death_case() {
  local name="$1" destination="$2" after="$3" side pid rc pane attempt sabotage oracle=1
  local -a base command
  CASE_LABEL="stream-$name"
  CASE_STATE_FILE=1
  for side in zz tmux; do
    if [ "$side" = zz ]; then
      base=(env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE HOME="$ZZ_HOME" XDG_CONFIG_HOME="$ZZ_HOME/config" ZZ_LOG_DIR="$ZZ_LOG_DIR" "$ZZ_BIN" --socket "$ZZ_SOCKET")
    else
      base=(env -u TMUX -u TMUX_PANE TMUX_TMPDIR=/tmp HOME="$TMUX_HOME" XDG_CONFIG_HOME="$TMUX_HOME/config" "$TMUX_BIN" -L "$INNER_SOCKET_NAME")
    fi
    sabotage=0
    if [ "$SELF_CHECK" = 1 ] && [ "$side" = zz ]; then sabotage=1; fi
    side_command "$side" set-option -gu @zzcs-after >/dev/null 2>&1 || true
    if [ "$destination" = display ]; then
      pane="$(side_command "$side" split-window -d -t "=$SESSION:$WINDOW_NAME.0" -P -F '#{pane_id}' '')"
      command=(display-message -I -t "$pane")
    else
      pane=''
      command=(split-window -I -d -t "=$SESSION:$WINDOW_NAME.0")
    fi
    command+=(';' display-message -p tail-out ';' set-option -g @zzcs-after yes)
    rm -f "$SCRATCH_DIR/stream-fifo"
    mkfifo "$SCRATCH_DIR/stream-fifo"
    exec 9<>"$SCRATCH_DIR/stream-fifo"
    (exec "${base[@]}" "${command[@]}" <"$SCRATCH_DIR/stream-fifo" 9>&-) >"$SCRATCH_DIR/$side.out" 2>"$SCRATCH_DIR/$side.err" &
    pid=$!
    if [ -z "$pane" ]; then
      for ((attempt = 0; attempt < 100; attempt++)); do
        pane="$(side_command "$side" list-panes -t "=$SESSION:$WINDOW_NAME" -F '#{pane_index} #{pane_id}' 2>/dev/null | awk '$1 == 1 { print $2 }')"
        [ -n "$pane" ] && break
        sleep 0.03
      done
      [ -n "$pane" ] || oracle=0
    fi
    printf '\033[31mRED\033[0m' >&9
    pane_stream_wait "$side" "$pane" RED || oracle=0
    if [ "$sabotage" = 0 ]; then
      side_command "$side" kill-pane -t "$pane" >/dev/null 2>&1 || oracle=0
    fi
    if [ "$after" = write ]; then printf 'AFTER-' >&9; fi
    exec 9>&-
    for ((attempt = 0; attempt < 1200; attempt++)); do
      kill -0 "$pid" 2>/dev/null || break
      sleep 0.05
    done
    if kill -0 "$pid" 2>/dev/null; then
      kill -KILL "$pid" 2>/dev/null
      oracle=0
    fi
    set +e
    wait "$pid"
    rc=$?
    set -e
    rm -f "$SCRATCH_DIR/stream-fifo"
    printf '%s\n' "$rc" >"$SCRATCH_DIR/$side.rc"
    settle_state "$side"
    : >"$SCRATCH_DIR/$side.matrix-state"
    {
      printf 'after=%s\n' "$(side_command "$side" show-options -gqv @zzcs-after)"
      printf 'panes=%s\n' "$(side_command "$side" list-panes -t "=$SESSION:$WINDOW_NAME" -F x | wc -l)"
    } >>"$SCRATCH_DIR/$side.matrix-state"
  done
  compare_channels "$CASE_LABEL" || true
  [ "$(cat "$SCRATCH_DIR/tmux.rc")" = 1 ] || oracle=0
  if [ "$SELF_CHECK" = 1 ]; then
    if [ "$oracle" != 1 ]; then
      SELF_CHECK_FAILURES=$((SELF_CHECK_FAILURES + 1))
      printf 'FAIL  self-check %s did not reach its required execution point\n' "$CASE_LABEL"
    fi
    self_check_expect "$CASE_LABEL: the pane survives the stream" exit=1 stdout=1 state=1
  else
    CHECKS=$((CHECKS + 1))
    if [ "$LAST_EXIT_DIFFERED$LAST_STDOUT_DIFFERED$LAST_STDERR_DIFFERED$LAST_STATE_DIFFERED" = 0000 ] && [ "$oracle" = 1 ]; then
      printf 'ok    %s\n' "$CASE_LABEL"
    else
      FAILURES=$((FAILURES + 1))
      if stream_output_starved; then
        oracle=0
        report_stream_environment_failure "$CASE_LABEL"
      else
        printf 'DIFF  %s\n' "$CASE_LABEL"
      fi
    fi
  fi
  CASE_STATE_FILE=0
}

review_pane_death_cases() {
  local row
  for row in 'display-kill-close display close' 'display-kill-write display write' \
    'split-kill-close split close' 'split-kill-write split write'; do
    set -- $row
    if [[ -n "${ZZ_STREAM_MATRIX_FILTER:-}" && ! "stream-$1" =~ $ZZ_STREAM_MATRIX_FILTER ]]; then continue; fi
    pane_death_case "$1" "$2" "$3"
  done
  drop_extra_panes pane-death-restored
}

review_stream_delivery_cases() {
  local row
  for row in 'display-partial display partial' 'display-eof display eof' 'display-term display term' \
    'display-slow display slow' 'display-fast display fast' 'display-utf8 display utf8' \
    'split-partial split partial' 'split-term split term' 'split-utf8 split utf8' \
    'split-print-partial split-print partial' 'split-print-term split-print term'; do
    set -- $row
    if [[ -n "${ZZ_STREAM_MATRIX_FILTER:-}" && ! "stream-$1" =~ $ZZ_STREAM_MATRIX_FILTER ]]; then continue; fi
    pane_stream_case "$1" "$2" "$3"
  done
}

stream_matrix() {
  cat <<'MATRIX'
direct-buffer-spent direct cli spent buffer none state same
direct-split-no-space direct cli open split-small none exit same
direct-split-pending direct cli open split-zoom pending state same
direct-buffer-closed direct cli closed buffer none stderr same
direct-closed-unused direct cli closed-unused source none state same
alias-buffer-spent alias cli spent buffer none state same
alias-split-no-space alias cli open split-small none exit same
alias-split-pending alias cli open split-zoom pending state same
alias-buffer-closed alias cli closed buffer none stderr same
alias-closed-unused alias cli closed-unused source none state same
file-buffer-spent file cli spent buffer none state same
file-split-no-space file cli open split-small none exit same
file-split-pending file cli open split-zoom pending state same
file-buffer-closed file cli closed buffer none stderr same
file-closed-unused file cli closed-unused source none state same
direct-split-zoom-no-space direct cli open split-zoom-small none state same
direct-split-keep-zoom-no-space direct cli open split-keep-zoom-small none state same
direct-split-horizontal-no-space direct cli open split-narrow none exit same
direct-split-bad-size direct cli open split-bad-size none exit same
direct-split-bad-percent direct cli open split-bad-percent none exit same
direct-split-command direct cli open split-command none exit same
direct-split-style direct cli open split-style pending state same
direct-split-active-style direct cli open split-active-style pending state same
direct-split-border-style direct cli open split-border-style pending state same
direct-display-bad-flag direct cli open display-bad-flag none exit same
direct-display-too-many direct cli open display-too-many none exit same
direct-split-pending-zoom direct cli open split-keep-zoom pending state same
direct-display-closed direct cli closed display-empty none stderr same
direct-split-closed direct cli closed split-normal none stderr same
alias-eof alias cli eof source none state same
file-eof file cli eof source none state same
attached-term-idle-before direct attached used source idle-before exit same
attached-term-before direct attached used source before exit same
attached-term-after direct attached used source after exit same
attached-term-after-two direct attached spent source after exit same
attached-term-idle-after direct attached used source idle-after exit same
attached-term-file-after file attached used source after exit same
direct-use direct cli used source none state same
alias-use alias cli used source none state same
file-use file cli used source none state same
startup-use startup cli used source none state same
direct-control direct control used source none stdout same
alias-control alias control used source none stdout same
file-control file control used source none stdout same
direct-attached direct attached used source none exit same
alias-attached alias attached used source none exit same
file-attached file attached used source none stdout same
direct-unused direct cli open-unused source none exit same
alias-unused alias cli open-unused source none exit same
file-unused file cli open-unused source none exit same
startup-unused startup cli open-unused source none state same
direct-spent direct cli spent source none state same
alias-spent alias cli spent source none state same
file-spent file cli spent source none state same
direct-closed direct cli closed source none stderr same
alias-closed alias cli closed source none stderr same
file-closed file cli closed source none stderr same
direct-eof direct cli eof source none state same
direct-large-unused direct cli oversized-unused source none exit same
alias-large-unused alias cli oversized-unused source none exit same
file-large-unused file cli oversized-unused source none exit same
startup-large-unused startup cli oversized-unused source none state same
direct-large-used direct cli oversized-used source none stderr decided
direct-display-valid direct cli used display-empty none state same
direct-display-running direct cli open display-running none exit same
alias-display-running alias cli open display-running none exit same
file-display-running file cli open display-running none exit same
direct-display-missing direct cli open display-missing none exit same
alias-display-missing alias cli open display-missing none exit same
file-display-missing file cli open display-missing none exit same
direct-split-missing direct cli open split-missing none exit same
alias-split-missing alias cli open split-missing none exit same
file-split-missing file cli open split-missing none exit same
direct-fail-after-read direct cli used source-missing none state same
alias-fail-after-read alias cli used source-missing none state same
file-fail-after-read file cli used source-missing none state same
direct-error-newline direct cli used source-missing-newline none stderr same
alias-error-newline alias cli used source-missing-newline none stderr same
file-error-newline file cli used source-missing-newline none stderr same
direct-term-before direct cli used source before exit same
alias-term-before alias cli used source before exit same
file-term-before file cli used source before exit same
direct-term-during direct cli open source during exit same
alias-term-during alias cli open source during exit same
file-term-during file cli open source during exit same
direct-term-after direct cli used source after exit same
alias-term-after alias cli used source after exit same
file-term-after file cli used source after exit same
direct-term-after-error direct cli spent source after exit same
alias-term-after-error alias cli spent source after exit same
file-term-after-error file cli spent source after exit same
direct-display-spent direct cli spent display-empty none state same
alias-display-spent alias cli spent display-empty none state same
file-display-spent file cli spent display-empty none state same
direct-split-spent direct cli spent split-normal none state same
alias-split-spent alias cli spent split-normal none state same
file-split-spent file cli spent split-normal none state same
direct-source-writeonly direct cli writeonly source none stderr same
alias-source-writeonly alias cli writeonly source none stderr same
file-source-writeonly file cli writeonly source none stderr same
direct-buffer-writeonly direct cli writeonly buffer none stderr same
alias-buffer-writeonly alias cli writeonly buffer none stderr same
file-buffer-writeonly file cli writeonly buffer none stderr same
direct-display-writeonly direct cli writeonly display-empty none state same
alias-display-writeonly alias cli writeonly display-empty none state same
file-display-writeonly file cli writeonly display-empty none state same
direct-split-writeonly direct cli writeonly split-normal none state same
alias-split-writeonly alias cli writeonly split-normal none state same
file-split-writeonly file cli writeonly split-normal none state same
direct-source-directory direct cli directory source none stderr same
alias-source-directory alias cli directory source none stderr same
file-source-directory file cli directory source none stderr same
direct-buffer-directory direct cli directory buffer none stderr same
alias-buffer-directory alias cli directory buffer none stderr same
file-buffer-directory file cli directory buffer none stderr same
direct-display-directory direct cli directory display-empty none state same
alias-display-directory alias cli directory display-empty none state same
file-display-directory file cli directory display-empty none state same
direct-split-directory direct cli directory split-normal none state same
alias-split-directory alias cli directory split-normal none state same
file-split-directory file cli directory split-normal none state same
alias-attached-term-before alias attached used source before stdout same
alias-attached-term-after alias attached used source after stdout same
alias-control-term-before alias control used source before stdout same
alias-control-term-after alias control used source after stdout same
file-control-term-before file control used source before stdout same
file-control-term-after file control used source after stdout same
MATRIX
}

matrix_body() {
  local argument
  for argument in "$@"; do
    if [ "$argument" = ';' ]; then
      printf '; '
    else
      printf "'%s' " "${argument//\'/\'\\\'\'}"
    fi
  done
  printf '\n'
}

matrix_attached() {
  local side="$1" body="$2" outer="$INNER_SOCKET_NAME-matrix" wrapper="$SCRATCH_DIR/attached-$side.sh"
  local attempt pane
  {
    printf '#!/usr/bin/env bash\n'
    printf '(printf "%%s\\n" "$BASHPID" > %q; exec ' "$SCRATCH_DIR/$side.pid"
    printf '%q ' "${MATRIX_BASE[@]}" -CC attach-session -f no-output -t "=$SESSION"
    printf '2> %q)\n' "$SCRATCH_DIR/$side.err"
    printf 'printf "%%s\\n" "$?" > %q\n' "$SCRATCH_DIR/$side.rc"
    printf 'touch %q\n' "$SCRATCH_DIR/$side.done"
  } >"$wrapper"
  rm -f "$SCRATCH_DIR/$side.done" "$SCRATCH_DIR/attached-start"
  : >"$SCRATCH_DIR/$side.control"
  pane="$(scrubbed "$TMUX_BIN" -L "$outer" -f /dev/null new-session -d -s matrix -x 80 -y 24 -P -F '#{pane_id}' "while [ ! -f $SCRATCH_DIR/attached-start ]; do sleep 0.02; done; bash $wrapper; sleep 1")"
  scrubbed "$TMUX_BIN" -L "$outer" pipe-pane -O -t "$pane" "cat > $SCRATCH_DIR/$side.control"
  touch "$SCRATCH_DIR/attached-start"
  for ((attempt=0; attempt<100; attempt++)); do
    if rg -q '%session-changed' "$SCRATCH_DIR/$side.control" 2>/dev/null; then break; fi
    sleep 0.03
  done
  [ "$attempt" != 100 ] || oracle=0
  scrubbed "$TMUX_BIN" -L "$outer" send-keys -t "$pane" -l 'display-message -p MATRIX-BEGIN'
  scrubbed "$TMUX_BIN" -L "$outer" send-keys -t "$pane" Enter
  for ((attempt=0; attempt<100; attempt++)); do
    if rg -q $'^MATRIX-BEGIN\r?$' "$SCRATCH_DIR/$side.control"; then break; fi
    sleep 0.03
  done
  [ "$attempt" != 100 ] || oracle=0
  if [ "$signal" != idle-before ]; then
    scrubbed "$TMUX_BIN" -L "$outer" send-keys -t "$pane" -l "$body"
    scrubbed "$TMUX_BIN" -L "$outer" send-keys -t "$pane" Enter
    for ((attempt=0; attempt<100; attempt++)); do
      if [ "$signal" = before ] || [ "$signal" = after ]; then
        [ -f "$ready" ] && break
      elif [ "$(side_command "$side" show-options -gqv @zzcs-matrix-tail)" = yes ]; then break
      fi
      sleep 0.03
    done
    [ "$attempt" != 100 ] || oracle=0
  fi
  if [ "$signal" = none ]; then
    scrubbed "$TMUX_BIN" -L "$outer" send-keys -t "$pane" Enter
  else
    sleep 0.05
    if [ "$SELF_CHECK" = 1 ] && [ "${MATRIX_MUTATION:-0}" = 1 ] && [ "$side" = zz ]; then
      kill -KILL "$(cat "$SCRATCH_DIR/$side.pid")" || oracle=0
    else
      kill -TERM "$(cat "$SCRATCH_DIR/$side.pid")" || oracle=0
    fi
  fi
  for ((attempt=0; attempt<100; attempt++)); do
    [ -f "$SCRATCH_DIR/$side.done" ] && break
    sleep 0.03
  done
  [ "$attempt" != 100 ] || oracle=0
  if [ ! -f "$SCRATCH_DIR/$side.done" ]; then printf '124\n' >"$SCRATCH_DIR/$side.rc"; fi
  if [ "$signal" != none ]; then sleep 1.1; fi
  sleep 0.05
  scrubbed "$TMUX_BIN" -L "$outer" kill-server >/dev/null 2>&1 || true
  if [ -n "${ZZ_STREAM_PROBES:-}" ]; then
    mkdir -p "${ZZ_STREAM_PROBES}.raw"
    cp "$SCRATCH_DIR/$side.control" "${ZZ_STREAM_PROBES}.raw/$CASE_LABEL-$side.control"
  fi
  sed $'s/\r$//' "$SCRATCH_DIR/$side.control" | sed -n '/^MATRIX-BEGIN$/,$p' |
    sed -E 's/^(%begin|%end|%error) [0-9]+ [0-9]+ /\1 TIME ID /' >"$SCRATCH_DIR/$side.out"
}

matrix_case() {
  local name="$1" invocation="$2" caller="$3" input="$4" destination="$5" signal="$6" sabotage="$7" mode="$8"
  local side option body rc pid guard attempt ready pane target config before after oracle=1 drop_before
  local -a MATRIX_BASE reader command
  CASE_LABEL="matrix-$name"
  CASE_STATE_FILE=1
  for side in zz tmux; do
    if [ "$side" = zz ]; then
      MATRIX_BASE=(env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE HOME="$ZZ_HOME" XDG_CONFIG_HOME="$ZZ_HOME/config" ZZ_LOG_DIR="$ZZ_LOG_DIR" "$ZZ_BIN" --socket "$ZZ_SOCKET")
    else
      MATRIX_BASE=(env -u TMUX -u TMUX_PANE TMUX_TMPDIR=/tmp HOME="$TMUX_HOME" XDG_CONFIG_HOME="$TMUX_HOME/config" "$TMUX_BIN" -L "$INNER_SOCKET_NAME")
    fi
    for option in input before tail; do side_command "$side" set -gu "@zzcs-matrix-$option" >/dev/null; done
    ready="$SCRATCH_DIR/$side.ready"
    rm -f "$ready"
    printf 'set -g @zzcs-matrix-input yes\n' >"$SCRATCH_DIR/matrix-input"
    reader=(source-file -)
    case "$destination" in
    buffer) reader=(load-buffer -b zzcs-matrix -) ;;
    split-missing) reader=(split-window -I -d -t %99999) ;;
    split-*)
      side_command "$side" new-window -d -t "=$SESSION" -n matrix-split 'sleep 60' >/dev/null
      target="=$SESSION:matrix-split"
      reader=(split-window -I -d -t "$target.0")
      printf 'MATRIX-INPUT\n' >"$SCRATCH_DIR/matrix-input"
      case "$destination" in
      split-small) side_command "$side" resize-window -t "$target" -x 80 -y 2 >/dev/null ;;
      split-narrow) side_command "$side" resize-window -t "$target" -x 2 -y 24 >/dev/null; reader+=(-h) ;;
      split-bad-size) reader+=(-l invalid) ;;
      split-bad-percent) reader+=(-p invalid) ;;
      split-command) reader+=(true) ;;
      split-style) reader+=(-s invalid-style) ;;
      split-active-style) reader+=(-S invalid-style) ;;
      split-border-style) reader+=(-R invalid-style) ;;
      split-zoom|split-keep-zoom|split-zoom-small|split-keep-zoom-small)
        side_command "$side" split-window -d -t "$target.0" 'sleep 60' >/dev/null
        if [[ "$destination" = *-small ]]; then side_command "$side" resize-window -t "$target" -x 80 -y 4 >/dev/null; fi
        side_command "$side" resize-pane -Z -t "$target.0" >/dev/null
        if [[ "$destination" = split-keep-zoom* ]]; then reader+=(-Z); fi
        ;;
      esac
      ;;
    display-bad-flag) reader=(display-message -I -Q) ;;
    display-too-many) reader=(display-message -I one two) ;;
    source-missing) printf 'set -g @zzcs-matrix-input yes\nsource-file /tmp/zzcs-matrix-missing\n' >"$SCRATCH_DIR/matrix-input" ;;
    source-missing-newline) printf "set -g @zzcs-matrix-input yes\nsource-file '/tmp/zzcs-matrix-missing\n'\n" >"$SCRATCH_DIR/matrix-input" ;;
    display-empty)
      pane="$(side_command "$side" split-window -d -t "=$SESSION:$WINDOW_NAME.0" -P -F '#{pane_id}' '')"
      reader=(display-message -I -t "$pane")
      printf 'MATRIX-INPUT\n' >"$SCRATCH_DIR/matrix-input"
      ;;
    display-running) reader=(display-message -I -t "=$SESSION:$WINDOW_NAME.0") ;;
    display-missing) reader=(display-message -I -t %99999) ;;
    split-missing) reader=(split-window -I -d -t %99999) ;;
    esac
    case "$input" in
    *unused) reader=(set -g @zzcs-matrix-input unused) ;;
    eof) : >"$SCRATCH_DIR/matrix-input" ;;
    oversized-used)
      head -c "$STREAM_CAP_BYTES" /dev/zero | tr '\0' '#' >>"$SCRATCH_DIR/matrix-input"
      printf '\n' >>"$SCRATCH_DIR/matrix-input"
      ;;
    esac
    before=yes
    after=MATRIX-AFTER
    drop_before=0
    if [ "$SELF_CHECK" = 1 ] && [ "${MATRIX_MUTATION:-0}" = 1 ] && [ "$side" = zz ]; then
      case "$sabotage" in
      state) if [ "$signal" != pending ]; then before=SABOTAGE; fi ;;
      stdout)
        after=MATRIX-SABOTAGE
        if [ "$signal" = before ] || [ "$signal" = after ]; then drop_before=1; fi
        ;;
      stderr) reader=(source-file /tmp/zzcs-sabotage-missing) ;;
      exit)
        if [ "$signal" = none ]; then
          if [ "$caller" = attached ]; then reader=(set -g @zzcs-matrix-input unused)
          else reader=(source-file -)
          fi
        fi
        ;;
      esac
    fi
    command=(set -g @zzcs-matrix-before "$before" ';')
    if [ "$drop_before" = 1 ]; then command=(); fi
    if [ "$signal" = before ]; then command+=(run-shell "printf ready > $ready; sleep 1" ';'); fi
    command+=("${reader[@]}" ';')
    if [ "$input" = spent ]; then command+=("${reader[@]}" ';'); fi
    if [ "$signal" = after ]; then command+=(run-shell "printf ready > $ready; sleep 1" ';'); fi
    if [ "$invocation" != startup ]; then command+=(display-message -p "$after" ';'); fi
    command+=(set -g @zzcs-matrix-tail yes)
    body="$(matrix_body "${command[@]}")"
    config="$SCRATCH_DIR/matrix.conf"
    case "$invocation" in
    alias)
      side_command "$side" set -s 'command-alias[86]' "zzcs-matrix=$body" >/dev/null
      command=(zzcs-matrix)
      body=zzcs-matrix
      ;;
    file)
      printf '%s\n' "$body" >"$config"
      command=(source-file "$config")
      body="source-file $config"
      ;;
    startup)
      printf '%s\n' "$body" >"$config"
      if [ "$side" = zz ]; then
        MATRIX_BASE=(env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE HOME="$ZZ_HOME" XDG_CONFIG_HOME="$ZZ_HOME/config" "$ZZ_BIN" --socket "${ZZ_SOCKET%.sock}-matrix.sock")
      else
        MATRIX_BASE=(env -u TMUX -u TMUX_PANE TMUX_TMPDIR=/tmp HOME="$TMUX_HOME" XDG_CONFIG_HOME="$TMUX_HOME/config" "$TMUX_BIN" -L "$INNER_SOCKET_NAME-matrix-boot")
      fi
      command=(-f "$config" new-session -d -s matrix 'sleep 60')
      ;;
    esac
    if [ "$caller" = attached ]; then
      matrix_attached "$side" "$body"
    else
      if [ "$caller" = control ]; then command=(-C "${command[@]}"); fi
      case "$input" in
      open*)
        mkfifo "$SCRATCH_DIR/matrix-fifo"
        exec 9<>"$SCRATCH_DIR/matrix-fifo"
        if [ "$signal" = during ]; then printf 'set -g @zzcs-matrix-input pending' >&9; fi
        (exec "${MATRIX_BASE[@]}" "${command[@]}" <"$SCRATCH_DIR/matrix-fifo" 9>&-) >"$SCRATCH_DIR/$side.out" 2>"$SCRATCH_DIR/$side.err" &
        ;;
      closed*)
        (exec "${MATRIX_BASE[@]}" "${command[@]}" 0<&-) >"$SCRATCH_DIR/$side.out" 2>"$SCRATCH_DIR/$side.err" &
        ;;
      writeonly)
        : >"$SCRATCH_DIR/matrix-writeonly"
        (exec "${MATRIX_BASE[@]}" "${command[@]}" 0>"$SCRATCH_DIR/matrix-writeonly") >"$SCRATCH_DIR/$side.out" 2>"$SCRATCH_DIR/$side.err" &
        ;;
      directory)
        (exec "${MATRIX_BASE[@]}" "${command[@]}" 0<"$SCRATCH_DIR") >"$SCRATCH_DIR/$side.out" 2>"$SCRATCH_DIR/$side.err" &
        ;;
      oversized-unused)
        (exec "${MATRIX_BASE[@]}" "${command[@]}" <"$SCRATCH_DIR/over-cap.bin") >"$SCRATCH_DIR/$side.out" 2>"$SCRATCH_DIR/$side.err" &
        ;;
      *)
        if [ "$caller" = control ]; then : >"$SCRATCH_DIR/matrix-input"; fi
        (exec "${MATRIX_BASE[@]}" "${command[@]}" <"$SCRATCH_DIR/matrix-input") >"$SCRATCH_DIR/$side.out" 2>"$SCRATCH_DIR/$side.err" &
        ;;
      esac
      pid=$!
      (exec 9>&-; sleep 5; kill -KILL "$pid" 2>/dev/null || true) &
      guard=$!
      if [ "$signal" = pending ]; then
        for ((attempt=0; attempt<100; attempt++)); do
          [ "$(side_command "$side" show-options -gqv @zzcs-matrix-before)" = yes ] && break
          sleep 0.02
        done
        [ "$attempt" != 100 ] || oracle=0
        sleep 0.1
        side_command "$side" display-message -p -t "$target" '#{window_zoomed_flag}:#{window_panes}' >"$SCRATCH_DIR/$side.pending"
        if [ "$SELF_CHECK" = 1 ] && [ "${MATRIX_MUTATION:-0}" = 1 ] && [ "$side" = zz ]; then
          side_command "$side" resize-pane -Z -t "$target.0" >/dev/null
          side_command "$side" display-message -p -t "$target" '#{window_zoomed_flag}:#{window_panes}' >"$SCRATCH_DIR/$side.pending"
        fi
        cat "$SCRATCH_DIR/matrix-input" >&9
        exec 9>&-
      elif [ "$signal" != none ]; then
        for ((attempt=0; attempt<100; attempt++)); do
          if [ "$signal" = during ]; then
            [ "$(side_command "$side" show-options -gqv @zzcs-matrix-before)" = yes ] && break
          else
            [ -f "$ready" ] && break
          fi
          sleep 0.02
        done
        if [ "$attempt" = 100 ]; then oracle=0; fi
        sleep 0.05
        if [ "$SELF_CHECK" = 1 ] && [ "${MATRIX_MUTATION:-0}" = 1 ] && [ "$side" = zz ]; then
          kill -KILL "$pid" 2>/dev/null || oracle=0
        else
          kill -TERM "$pid" 2>/dev/null || oracle=0
        fi
      fi
      set +e
      wait "$pid"
      rc=$?
      if [ "$signal" = pending ] && [ "$rc" != 0 ]; then oracle=0; fi
      kill "$guard" 2>/dev/null
      wait "$guard" 2>/dev/null
      set -e
      printf '%s\n' "$rc" >"$SCRATCH_DIR/$side.rc"
      case "$input" in open*) exec 9>&-; rm "$SCRATCH_DIR/matrix-fifo" ;; esac
      if [ "$signal" != none ]; then sleep 1.1; fi
      if [ "$caller" = control ]; then
        mv "$SCRATCH_DIR/$side.out" "$SCRATCH_DIR/$side.control"
        sed -E 's/^(%begin|%end|%error) [0-9]+ [0-9]+ /\1 TIME ID /' "$SCRATCH_DIR/$side.control" >"$SCRATCH_DIR/$side.out"
      fi
    fi
    {
      for option in input before tail; do
        printf '%s=%s\n' "$option" "$("${MATRIX_BASE[@]}" show-options -gqv "@zzcs-matrix-$option")"
      done
      if [ "$signal" = pending ]; then printf 'pending='; cat "$SCRATCH_DIR/$side.pending"; fi
      if [[ "$destination" = split-* && "$destination" != split-missing ]]; then side_command "$side" display-message -p -t "$target" 'final=#{window_zoomed_flag}:#{window_panes}'; fi
      if [ "$destination" = buffer ]; then side_command "$side" show-buffer -b zzcs-matrix 2>/dev/null || true; fi
      if [ "$destination" = display-empty ]; then side_command "$side" capture-pane -p -t "$pane" -S 0 -E 0; fi
    } >"$SCRATCH_DIR/$side.matrix-state"
    if [ "$invocation" = startup ]; then "${MATRIX_BASE[@]}" kill-server >/dev/null 2>&1 || true; fi
    if [[ "$destination" = split-* && "$destination" != split-missing ]]; then side_command "$side" kill-window -t "$target" >/dev/null; fi
    if [ "$destination" = buffer ]; then side_command "$side" delete-buffer -b zzcs-matrix >/dev/null 2>&1 || true; fi
    if [ "$destination" = display-empty ]; then side_command "$side" kill-pane -t "$pane" >/dev/null; fi
  done
  compare_channels "$CASE_LABEL" || true
  if [ "$mode" = decided ]; then
    if [ "$SELF_CHECK" = 0 ]; then
      DECIDED=$((DECIDED+1))
      printf 'note  %s decided:TUI-018: %s\n' "$CASE_LABEL" "$BOUND_DECISION"
    fi
  elif [ "$mode" = record ]; then
    RECORDS=$((RECORDS+1))
    printf 'note  %s owner:TUI-018: closed fd 0 is sanitized to /dev/null by Rust before application entry; the pin instead fails in libevent after the preceding command (exit 1, evsig_cb Bad file descriptor), so the tail state differs\n' "$CASE_LABEL"
  elif [ "$SELF_CHECK" = 1 ]; then
    if [ "${MATRIX_MUTATION:-0}" = 0 ]; then
      if [ "$LAST_EXIT_DIFFERED$LAST_STDOUT_DIFFERED$LAST_STDERR_DIFFERED$LAST_STATE_DIFFERED" != 0000 ] || [ "$oracle" != 1 ]; then
        SELF_CHECK_FAILURES=$((SELF_CHECK_FAILURES+1))
        printf 'FAIL  self-check %s baseline differs before sabotage\n' "$CASE_LABEL"
      fi
      MATRIX_MUTATION=1 matrix_case "$@"
      return
    fi
    if [ "$oracle" != 1 ]; then
      SELF_CHECK_FAILURES=$((SELF_CHECK_FAILURES+1))
      printf 'FAIL  self-check %s did not reach its required execution point\n' "$CASE_LABEL"
    fi
    self_check_expect "$CASE_LABEL: $sabotage execution sabotage" "$sabotage=1"
  else
    CHECKS=$((CHECKS+1))
    if [ "$LAST_EXIT_DIFFERED$LAST_STDOUT_DIFFERED$LAST_STDERR_DIFFERED$LAST_STATE_DIFFERED" = 0000 ] && [ "$oracle" = 1 ]; then
      printf 'ok    %s\n' "$CASE_LABEL"
    else
      FAILURES=$((FAILURES+1))
      if stream_output_starved; then
        oracle=0
        report_stream_environment_failure "$CASE_LABEL"
      else
        printf 'DIFF  %s\n' "$CASE_LABEL"
      fi
    fi
  fi
  CASE_STATE_FILE=0
  for side in zz tmux; do
    for option in input before tail; do side_command "$side" set -gu "@zzcs-matrix-$option" >/dev/null; done
  done
}

run_stream_matrix() {
  local name invocation caller input destination signal sabotage mode
  while read -r name invocation caller input destination signal sabotage mode; do
    if [[ -n "${ZZ_STREAM_MATRIX_FILTER:-}" && ! "$name" =~ $ZZ_STREAM_MATRIX_FILTER ]]; then continue; fi
    matrix_case "$name" "$invocation" "$caller" "$input" "$destination" "$signal" "$sabotage" "$mode"
  done < <(stream_matrix)
}

write_payloads() {
  printf 'a\303\251b\377c\000d\n' >"$SCRATCH_DIR/binary.bin"
  awk 'BEGIN { for (i = 1; i <= 180000; i++) printf "%06d ", i }' >"$SCRATCH_DIR/stream-fast.bin"
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
  review_alias_cases
  startup_config_stream_case
  review_execution_cases
  review_stream_delivery_cases
  review_pane_death_cases
  run_stream_matrix
  bound_cases

  if [ "$ENVIRONMENT" -ne 0 ]; then
    printf 'environment: %s of those cells produced no zz output at all where the pin produced bytes, which is starvation on this box and not a parity difference\n' \
      "$ENVIRONMENT"
  fi
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
  if [[ "$name" = *binary-unterminated-stdout ]]; then
    printf '\n' >>"$SCRATCH_DIR/zz.out"
  fi
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

  review_alias_cases
  startup_config_stream_case
  review_execution_cases
  review_stream_delivery_cases
  review_pane_death_cases
  run_stream_matrix

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

if [ "$MATRIX_LIST" = 1 ]; then
  stream_matrix
  exit 0
fi
if [ "$MATRIX_CHECK" = 1 ]; then
  build_scene
  write_payloads
  review_stream_delivery_cases
  review_pane_death_cases
  run_stream_matrix
  printf 'matrix: %s asserted, %s failures, %s recorded:TUI-018, %s decided:TUI-018, %s self-check failures\n' "$CHECKS" "$FAILURES" "$RECORDS" "$DECIDED" "$SELF_CHECK_FAILURES"
  [ "$FAILURES" = 0 ] && [ "$SELF_CHECK_FAILURES" = 0 ]
  exit $?
fi
if [ "$EXECUTION_CHECK" -eq 1 ]; then
  build_scene
  write_payloads
  review_execution_cases
  review_stream_delivery_cases
  review_pane_death_cases
  printf 'execution-check: %s asserted, %s failures, %s self-check failures\n' "$CHECKS" "$FAILURES" "$SELF_CHECK_FAILURES"
  [ "$FAILURES" -eq 0 ] && [ "$SELF_CHECK_FAILURES" -eq 0 ]
elif [ "$SELF_CHECK" -eq 1 ]; then
  run_self_check
else
  run_cases
fi
