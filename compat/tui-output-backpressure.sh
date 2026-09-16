#!/usr/bin/env bash
# Prolonged terminal backpressure: does an attached client stay live, stay
# bounded, and hand the terminal back clean?
#
# compat/scenarios/smoke/tui-client-input-backpressure.txt already asks the
# short question: with the pty saturated for a second or two, does a bound key
# still act? Both binaries pass it. tui.client-output-queue-budget is the long
# question the smoke scenario cannot ask, because its undrained window is
# derived from the kernel's pty buffer and stops well short of the client's own
# queue: with a terminal that reads NOTHING for tens of seconds, does the client
# keep answering keys, and what does it cost in memory to do so?
#
# THE TERMINAL THAT STOPS READING. An outer pinned tmux always drains its
# panes, so the fixture puts compat/tui-output-relay.py between the two: the
# relay runs in the outer pane, hands the client a pty at the pane's size, and
# copies both directions until SIGUSR1 tells it to stop copying master to
# stdout. Keys keep flowing while it holds, so what is undrained is the
# client's terminal and not its input. SIGUSR2 resumes and everything the
# client wrote reaches the outer pane, which decodes it: the final screen of
# this fixture is a decoded screen like every other one in the campaign, and
# the relay is transparent to it.
#
# THE REFERENCE. tty.c is the behaviour being matched, not guessed at:
# tty_block_maybe discards the whole out buffer once it holds more than
# TTY_BLOCK_START (1 + sx*sy*8 bytes, 15361 at 80x24), sets TTY_BLOCK so every
# later tty_puts is dropped too, and arms a 100 ms timer; tty_timer_callback
# sets CLIENT_ALLREDRAWFLAGS every interval and, once less than TTY_BLOCK_STOP
# (1 + sx*sy/8) was discarded in one interval, clears the flag and calls
# tty_invalidate, which repaints from the screen. The pin therefore never waits
# on its tty from its event loop, pays a constant amount of memory for a viewer
# who is not reading, and shows a correct screen when reading resumes. Those
# three are what this fixture asserts, on both binaries, with the same driver.
#
# DECLARED PARAMETERS, printed by every run:
#   --undrained SECONDS   how long the relay holds, default 45, the window the
#                         cycle-18 residual measurement used and the one the
#                         registry records zz failing at.
#   --deadline SECONDS    the response bound for a bound key delivered while
#                         undrained, default 1.000. Derivation: the registry
#                         records pinned tmux d77c9dc6 acting on F5 in 0.050 s
#                         at 45 s undrained, and the same pin runs here as a
#                         side of this fixture, so the deadline is 20x the
#                         reference latency and the pin's own column proves on
#                         every run that it is achievable on this box.
#   --rss-growth KB       how much RSS a side may gain between its drained
#                         baseline and the end of the undrained window,
#                         default 16384. The client's paint queue is bounded by
#                         crates/zz-tui/src/writer.rs QUEUE_BUDGET, 4 MiB; the
#                         bound here is four times that, which is loose enough
#                         that allocator behaviour is not the subject and tight
#                         enough that an unbounded queue cannot pass.
#   --key-timeout SECONDS how long a press is waited for before it is a
#                         non-answer, default 12, the registry's number.
# The two sides are run one after the other, never at once: the flood is CPU
# work and a shared box would otherwise put one side's latency inside the
# other's.
#
# WHAT IS ASSERTED PER SIDE
#   drained key      acts within the deadline (the canary: a binding that never
#                    worked would make the undrained press meaningless).
#   undrained key    acts within the deadline, after --undrained seconds of a
#                    terminal reading nothing.
#   memory           RSS growth over the same window stays under --rss-growth.
#   final screen     after the relay resumes and the pane is put back to a
#                    fixed marker, both sides' screens are compared cell for
#                    cell through the outer tmux, plus the asserted cursor
#                    tuple. Cursor shape, blink and colour are RECORDED, the
#                    standing divergence tui-screen-diff.sh records too, and so
#                    is the status row: see compare_final_screens for the
#                    measurement that puts that row's one spelling difference
#                    before this fixture's change rather than in it.
#   teardown         detach while undrained, resume, and then two things: the
#                    outer pane after the client exits holds exactly the
#                    relay's RELAY-DONE and nothing else, twice, a second
#                    apart, so a paint that outlived the client is a row of its
#                    own; and the client wrote no ESCAPE byte after it left the
#                    alternate screen, so a paint that outlived the terminal
#                    restoration is caught even though the relay's own clear
#                    would have wiped it off the grid. Both binaries print a
#                    detach line after the restore, which is text and not a
#                    paint, so the assertion counts escapes and not bytes.
#
# SETTLES. Every wait is bounded and on an observable. The undrained window is
# a declared parameter, not a settle. The pty being full is not slept for
# either: the relay publishes FIONREAD on the master into its status file, and
# the fixture waits for that to go flat, which is the kernel saying the client
# is blocked. The final screen settles the way tui-screen-diff.sh settles, on
# the marker being present AND the screen unchanged between two polls.
#
# --self-check sabotages the two assertions this fixture adds that no other
# fixture makes: it runs the teardown case with the relay writing a paint after
# the restoration (which the teardown assertion must catch) and the undrained
# case with the deadline set below the latency just measured (which the
# response assertion must catch). A fixture that only passes has proved
# nothing. The third sabotage is not in here because it is a different binary:
# run this fixture against a build from before the drop path and the undrained
# key never lands.
set -eEuo pipefail

usage() {
  printf 'usage: compat/tui-output-backpressure.sh [--undrained S] [--deadline S]\n' >&2
  printf '       [--rss-growth KB] [--key-timeout S] [--side zz|tmux|both]\n' >&2
  printf '       [--self-check] [ZZ_BIN [TMUX_BIN]]\n' >&2
}

COMPAT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd -- "$COMPAT_DIR/.." && pwd)"
UNDRAINED=45
DEADLINE=1.000
RSS_GROWTH=16384
KEY_TIMEOUT=12
SIDES="both"
SELF_CHECK=0
POSITIONAL=()
while [ "$#" -gt 0 ]; do
  case "$1" in
  --undrained)
    UNDRAINED="$2"
    shift 2
    ;;
  --deadline)
    DEADLINE="$2"
    shift 2
    ;;
  --rss-growth)
    RSS_GROWTH="$2"
    shift 2
    ;;
  --key-timeout)
    KEY_TIMEOUT="$2"
    shift 2
    ;;
  --side)
    SIDES="$2"
    shift 2
    ;;
  --self-check)
    SELF_CHECK=1
    shift
    ;;
  -h | --help)
    usage
    exit 0
    ;;
  -*)
    usage
    exit 2
    ;;
  *)
    POSITIONAL+=("$1")
    shift
    ;;
  esac
done
[ "${#POSITIONAL[@]}" -le 2 ] || {
  usage
  exit 2
}

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
ZZ_BIN="$(resolve_binary "$ZZ_INPUT")" || {
  printf 'error: zz binary not found: %s\n' "$ZZ_INPUT" >&2
  exit 2
}
TMUX_BIN="$(resolve_binary "$TMUX_INPUT")" || {
  printf 'error: tmux binary not found: %s\n' "$TMUX_INPUT" >&2
  exit 2
}
RELAY="$COMPAT_DIR/tui-output-relay.py"
[ -f "$RELAY" ] || {
  printf 'error: relay missing: %s\n' "$RELAY" >&2
  exit 2
}

COLUMNS_UNDER_TEST=80
ROWS_UNDER_TEST=24
SCRATCH_DIR="$(mktemp -d /tmp/zzbp.XXXXXX)"
TOKEN="${SCRATCH_DIR##*.}"
OUTER_SOCKET_NAME="zzbpo-$TOKEN"
INNER_SOCKET_NAME="zzbpi-$TOKEN"
ZZ_SOCKET="/tmp/zzbp-$TOKEN.sock"
OUTER_SESSION="driver"
INNER_SESSION="backpressure"
WINDOW_NAME="win"
PANE_TITLE="bptitle"
INNER_SHELL="ENV= PS1='\$ ' exec /bin/sh"
ZZ_HOME="$SCRATCH_DIR/zz-home"
TMUX_HOME="$SCRATCH_DIR/tmux-home"
OUTER_HOME="$SCRATCH_DIR/outer-home"
ZZ_LOG_DIR="$SCRATCH_DIR/zz-logs"
ZZ_PID=""
FAILURES=0
CHECKS=0
RECORDS=0
DONE_MARKER="RELAY-DONE"
declare -A REPORT=()
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
  rm -f -- "$ZZ_SOCKET" "/tmp/tmux-$(id -u)/$OUTER_SOCKET_NAME" \
    "/tmp/tmux-$(id -u)/$INNER_SOCKET_NAME"
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

now_seconds() {
  printf '%s\n' "${EPOCHREALTIME/,/.}"
}
elapsed_since() {
  awk -v start="$1" -v end="$2" 'BEGIN { printf "%.3f\n", end - start }'
}
within() {
  awk -v value="$1" -v bound="$2" 'BEGIN { exit !(value <= bound) }'
}

wait_for() {
  local label="$1"
  local attempt
  shift
  for ((attempt = 0; attempt < 400; attempt++)); do
    if "$@" >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.05
  done
  die "$label did not happen within 20 seconds"
}

capture_screen() {
  tmux_outer_command capture-pane -p -e -S 0 -E "$((ROWS_UNDER_TEST - 1))" \
    -t "=$OUTER_SESSION:$1"
}
capture_plain() {
  tmux_outer_command capture-pane -p -S 0 -E "$((ROWS_UNDER_TEST - 1))" \
    -t "=$OUTER_SESSION:$1"
}
CURSOR_ASSERTED_FORMAT='#{cursor_x},#{cursor_y} flag=#{cursor_flag} pane=#{pane_width}x#{pane_height}'
CURSOR_RECORDED_FORMAT='shape=#{cursor_shape} blinking=#{cursor_blinking} colour=#{cursor_colour}'
cursor_tuple() {
  tmux_outer_command display-message -p -t "=$OUTER_SESSION:$1" "$CURSOR_ASSERTED_FORMAT"
}
recorded_cursor_tuple() {
  tmux_outer_command display-message -p -t "=$OUTER_SESSION:$1" "$CURSOR_RECORDED_FORMAT"
}

# The settle tui-screen-diff.sh proved out: the marker on the screen AND the
# screen unchanged between two polls, both observable, the whole thing bounded.
wait_settled() {
  local side="$1"
  local marker="$2"
  local previous=""
  local current attempt
  for ((attempt = 0; attempt < 400; attempt++)); do
    current="$(capture_plain "$side" 2>/dev/null || true)"
    if [ -n "$previous" ] && [ "$current" = "$previous" ] &&
      printf '%s' "$current" | grep -Fq "$marker"; then
      return 0
    fi
    previous="$current"
    sleep 0.05
  done
  die "$side did not settle on $marker within 20 seconds"
}

side_home() {
  case "$1" in
  zz) printf '%s\n' "$ZZ_HOME" ;;
  tmux) printf '%s\n' "$TMUX_HOME" ;;
  esac
}
status_path() { printf '%s/%s.relay-status\n' "$SCRATCH_DIR" "$1"; }
pid_path() { printf '%s/%s.relay-pid\n' "$SCRATCH_DIR" "$1"; }
relay_pid_path() { printf '%s/%s.relay-self\n' "$SCRATCH_DIR" "$1"; }
mark_path() { printf '%s/%s.key-mark\n' "$SCRATCH_DIR" "$1"; }
flood_path() { printf '%s/%s.flooding\n' "$SCRATCH_DIR" "$1"; }

write_attach() {
  local side="$1"
  local late_paint="$2"
  local destination="$SCRATCH_DIR/attach-$side.sh"
  local relay_flag=""
  [ "$late_paint" -eq 0 ] || relay_flag=" --late-paint"

  {
    printf '#!/usr/bin/env bash\n'
    printf 'printf %%s "$$" >%q\n' "$(relay_pid_path "$side")"
    if [ "$side" = zz ]; then
      printf 'exec python3 %q%s %q %q -- env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL -u XDG_STATE_HOME HOME=%q XDG_CONFIG_HOME=%q ZZ_LOG_DIR=%q TMUX_TMPDIR=/tmp TERM=xterm-256color %q --socket %q attach-session -t %q\n' \
        "$RELAY" "$relay_flag" "$(status_path "$side")" "$(pid_path "$side")" \
        "$ZZ_HOME" "$ZZ_HOME/config" "$ZZ_LOG_DIR" "$ZZ_BIN" "$ZZ_SOCKET" "=$INNER_SESSION"
    else
      printf 'exec python3 %q%s %q %q -- env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL -u XDG_STATE_HOME HOME=%q XDG_CONFIG_HOME=%q TMUX_TMPDIR=/tmp TERM=xterm-256color %q -L %q attach-session -t %q\n' \
        "$RELAY" "$relay_flag" "$(status_path "$side")" "$(pid_path "$side")" \
        "$TMUX_HOME" "$TMUX_HOME/config" "$TMUX_BIN" "$INNER_SOCKET_NAME" "=$INNER_SESSION"
    fi
  } >"$destination"
  chmod +x "$destination"
  printf '%s\n' "$destination"
}

relay_status_field() {
  local side="$1"
  local field="$2"
  local line
  line="$(cat "$(status_path "$side")" 2>/dev/null || true)"
  awk -v field="$field" '{ for (i = 1; i <= NF; i++) { split($i, kv, "="); if (kv[1] == field) print kv[2] } }' \
    <<<"$line"
}
relay_hold() {
  local side="$1"
  local signal="$2"
  local pid
  pid="$(cat "$(relay_pid_path "$side")" 2>/dev/null || true)"
  [ -n "$pid" ] || die "$side relay pid never appeared"
  kill -"$signal" "$pid" || die "$side relay did not take SIG$signal"
}
client_pid() {
  cat "$(pid_path "$1")" 2>/dev/null || true
}
client_rss() {
  local pid
  pid="$(client_pid "$1")"
  [ -n "$pid" ] || return 1
  awk '/^VmRSS:/ { print $2 }' "/proc/$pid/status" 2>/dev/null
}

client_attached() {
  [ "$(side_command "$1" list-clients -F '#{client_session}' 2>/dev/null)" = "$INNER_SESSION" ]
}
active_pane() {
  side_command "$1" list-panes -t "=$INNER_SESSION" -F '#{pane_active} #{pane_id}' |
    awk '$1 == 1 { print $2; exit }'
}

# The pty is full when the kernel stops handing the relay more bytes to hold:
# FIONREAD on the master, flat between two polls, with the flood still running.
# That is the observable this fixture waits on instead of sleeping.
wait_for_full_pty() {
  local side="$1"
  local previous=-1
  local current attempt
  for ((attempt = 0; attempt < 400; attempt++)); do
    current="$(relay_status_field "$side" avail)"
    if [[ "$current" =~ ^[0-9]+$ ]] && [ "$current" -gt 1024 ] &&
      [ "$current" = "$previous" ]; then
      REPORT["$side.pty_full_at"]="$current"
      return 0
    fi
    previous="$current"
    sleep 0.05
  done
  die "$side pty never filled while the relay held it; last avail=${current:-<none>}"
}

press_and_time() {
  local side="$1"
  local mark
  local started
  local attempt
  mark="$(mark_path "$side")"
  rm -f -- "$mark"
  started="$(now_seconds)"
  tmux_outer_command send-keys -t "=$OUTER_SESSION:$side" F5 ||
    die "$side outer send-keys F5 failed"
  for ((attempt = 0; attempt < KEY_TIMEOUT * 50; attempt++)); do
    if [ -e "$mark" ]; then
      elapsed_since "$started" "$(now_seconds)"
      return 0
    fi
    sleep 0.02
  done
  printf 'none\n'
}

start_flood() {
  local side="$1"
  local flood
  flood="$(flood_path "$side")"
  rm -f -- "$flood"
  side_command "$side" respawn-pane -k -t "$(active_pane "$side")" \
    "sh -c 'touch $flood; exec base64 /dev/urandom'" ||
    die "$side could not start its flood"
  wait_for "$side flood" test -e "$flood"
}
stop_flood() {
  local side="$1"
  side_command "$side" respawn-pane -k -t "$(active_pane "$side")" "$INNER_SHELL" ||
    die "$side could not stop its flood"
}

pin_dynamic_values() {
  local side="$1"
  side_command "$side" set-option -g status-right '' || die "$side refused status-right"
  side_command "$side" set-option -g status-left L || die "$side refused status-left"
  side_command "$side" set-option -g automatic-rename off || die "$side refused automatic-rename"
  side_command "$side" set-option -g status-keys emacs || die "$side refused status-keys"
  side_command "$side" bind-key -n F5 run-shell -b "touch $(mark_path "$side")" ||
    die "$side refused its F5 binding"
}

create_inner_session() {
  local side="$1"
  if [ "$side" = zz ]; then
    zz_command new-session -d -s "$INNER_SESSION" -n "$WINDOW_NAME" \
      -x "$COLUMNS_UNDER_TEST" -y "$ROWS_UNDER_TEST" "$INNER_SHELL" ||
      die "could not create the zz session"
  else
    tmux_inner_command -f /dev/null new-session -d -s "$INNER_SESSION" -n "$WINDOW_NAME" \
      -x "$COLUMNS_UNDER_TEST" -y "$ROWS_UNDER_TEST" "$INNER_SHELL" ||
      die "could not create the tmux session"
  fi
  pin_dynamic_values "$side"
}

attach_side() {
  local side="$1"
  local late_paint="${2:-0}"
  local script
  script="$(write_attach "$side" "$late_paint")"
  rm -f -- "$(status_path "$side")" "$(pid_path "$side")" "$(relay_pid_path "$side")"
  if tmux_outer_command has-session -t "=$OUTER_SESSION" >/dev/null 2>&1; then
    tmux_outer_command new-window -d -n "$side" "$script" ||
      die "could not start the $side relay window"
  else
    tmux_outer_command -f /dev/null new-session -d -s "$OUTER_SESSION" -n "$side" \
      -x "$COLUMNS_UNDER_TEST" -y "$ROWS_UNDER_TEST" "$script" ||
      die "could not start the outer session"
    tmux_outer_command set-option -g status off || die "outer tmux refused status off"
    tmux_outer_command set-option -g remain-on-exit on || die "outer tmux refused remain-on-exit"
  fi
  wait_for "$side relay pid" test -s "$(relay_pid_path "$side")"
  wait_for "$side client attached" client_attached "$side"
  side_command "$side" select-pane -t "=$INNER_SESSION:0.0" -T "$PANE_TITLE" ||
    die "$side refused select-pane -T"
}

# --- the measured window ---------------------------------------------------

run_undrained_case() {
  local side="$1"
  local drained backlogged baseline peak sample growth
  local started

  side_command "$side" send-keys -t "$(active_pane "$side")" \
    "printf 'MARK-%s\\n' ready" Enter || die "$side refused send-keys"
  wait_settled "$side" MARK-ready

  drained="$(press_and_time "$side")"
  baseline="$(client_rss "$side")" || die "$side has no client rss"

  start_flood "$side"
  relay_hold "$side" USR1
  wait_for_full_pty "$side"

  peak="$baseline"
  started="$(now_seconds)"
  while within "$(elapsed_since "$started" "$(now_seconds)")" "$UNDRAINED"; do
    sample="$(client_rss "$side" || true)"
    if [ -n "$sample" ] && [ "$sample" -gt "$peak" ]; then
      peak="$sample"
    fi
    sleep 0.25
  done

  backlogged="$(press_and_time "$side")"
  sample="$(client_rss "$side" || true)"
  if [ -n "$sample" ] && [ "$sample" -gt "$peak" ]; then
    peak="$sample"
  fi
  growth=$((peak - baseline))

  relay_hold "$side" USR2
  stop_flood "$side"

  REPORT["$side.drained"]="$drained"
  REPORT["$side.backlogged"]="$backlogged"
  REPORT["$side.rss_baseline"]="$baseline"
  REPORT["$side.rss_peak"]="$peak"
  REPORT["$side.rss_growth"]="$growth"

  printf '%-5s drained=%s undrained=%s rss_baseline=%s kB rss_peak=%s kB growth=%s kB\n' \
    "$side" "$drained" "$backlogged" "$baseline" "$peak" "$growth"

  CHECKS=$((CHECKS + 1))
  if [ "$drained" = none ] || ! within "$drained" "$DEADLINE"; then
    FAILURES=$((FAILURES + 1))
    printf 'FAIL  %s drained key did not act within %s s (got %s)\n' "$side" "$DEADLINE" "$drained"
  else
    printf 'ok    %s drained key acted in %s s\n' "$side" "$drained"
  fi

  CHECKS=$((CHECKS + 1))
  if [ "$backlogged" = none ] || ! within "$backlogged" "$DEADLINE"; then
    FAILURES=$((FAILURES + 1))
    printf 'FAIL  %s undrained key did not act within %s s after %s s undrained (got %s)\n' \
      "$side" "$DEADLINE" "$UNDRAINED" "$backlogged"
  else
    printf 'ok    %s undrained key acted in %s s after %s s undrained\n' \
      "$side" "$backlogged" "$UNDRAINED"
  fi

  CHECKS=$((CHECKS + 1))
  if [ "$growth" -gt "$RSS_GROWTH" ]; then
    FAILURES=$((FAILURES + 1))
    printf 'FAIL  %s grew %s kB over the undrained window, bound is %s kB\n' \
      "$side" "$growth" "$RSS_GROWTH"
  else
    printf 'ok    %s grew %s kB over the undrained window, bound is %s kB\n' \
      "$side" "$growth" "$RSS_GROWTH"
  fi
}

inner_pane_has() {
  side_command "$1" capture-pane -p -t "$(active_pane "$1")" 2>/dev/null | grep -Fq "$2"
}

settle_final_screen() {
  local side="$1"
  wait_for "$side respawned shell prompt" inner_pane_has "$side" '$'
  side_command "$side" send-keys -t "$(active_pane "$side")" \
    "printf '\\033[2J\\033[3J\\033[H'" Enter || die "$side refused the clear"
  side_command "$side" send-keys -t "$(active_pane "$side")" \
    "printf 'MARK-%s\\n' final" Enter || die "$side refused send-keys"
  wait_settled "$side" MARK-final
}

compare_final_screens() {
  local zz_rows tmux_rows zz_cursor tmux_cursor index differing status
  mapfile -t zz_rows < <(capture_screen zz)
  mapfile -t tmux_rows < <(capture_screen tmux)
  zz_cursor="$(cursor_tuple zz)"
  tmux_cursor="$(cursor_tuple tmux)"

  RECORDS=$((RECORDS + 1))
  printf 'note  recorded cursor attributes after recovery\n'
  printf '        tmux: %s\n' "$(recorded_cursor_tuple tmux)"
  printf '        zz:   %s\n' "$(recorded_cursor_tuple zz)"

  # THE STATUS ROW IS RECORDED, NOT ASSERTED, and this is measured rather than
  # waived by omission. At 80x24 with status-left L and status-right empty,
  # both binaries paint the same characters in the same colours and differ in
  # ONE spelling: after the reset that ends the window name, zz re-states the
  # default foreground (\e[38;2;13;13;13m) before the background and the pin
  # states only the background. That is the recorded explicit-default-
  # foreground divergence of the daemon's frame representation, which has no
  # owner this cycle; measured 2026-09-09 on this box against a build from
  # BEFORE this fixture's drop path and again after it, byte for byte the same
  # difference, so it is not something recovery introduced. Everything above
  # the status row is asserted.
  status=$((ROWS_UNDER_TEST - 1))
  if [ "${zz_rows[status]-}" = "${tmux_rows[status]-}" ]; then
    printf 'note  status row after recovery identical, the record can close\n'
  else
    RECORDS=$((RECORDS + 1))
    printf 'note  status row after recovery differs (explicit default foreground, no owner)\n'
    printf '        tmux: %s\n' "$(printf '%s' "${tmux_rows[status]-}" | cat -v)"
    printf '        zz:   %s\n' "$(printf '%s' "${zz_rows[status]-}" | cat -v)"
  fi

  differing=-1
  for ((index = 0; index < status; index++)); do
    if [ "${zz_rows[index]-}" != "${tmux_rows[index]-}" ]; then
      differing="$index"
      break
    fi
  done

  CHECKS=$((CHECKS + 1))
  if [ "$differing" -lt 0 ] && [ "$zz_cursor" = "$tmux_cursor" ]; then
    printf 'ok    recovered screen identical on both sides above the status row, cursor %s\n' \
      "$zz_cursor"
    return 0
  fi
  FAILURES=$((FAILURES + 1))
  printf 'FAIL  recovered screen differs\n'
  if [ "$differing" -ge 0 ]; then
    printf '      first differing row %s of %s\n' "$differing" "$status"
    printf '        tmux: %s\n' "$(printf '%s' "${tmux_rows[differing]-}" | cat -v)"
    printf '        zz:   %s\n' "$(printf '%s' "${zz_rows[differing]-}" | cat -v)"
  else
    printf '      all %s rows above the status row identical\n' "$status"
  fi
  printf '      asserted cursor tmux: %s\n' "$tmux_cursor"
  printf '      asserted cursor zz:   %s\n' "$zz_cursor"
}

# --- detach under backlog --------------------------------------------------

relay_exited() {
  [ "$(relay_status_field "$1" exited)" = 1 ]
}
non_empty_rows() {
  capture_plain "$1" | sed -e 's/\r$//' -e 's/[[:space:]]*$//' | grep -v '^$' || true
}
# The restored screen carries no style of its own, so any SGR at all in the
# decoded capture is a paint that outlived the restoration even when it painted
# nothing but coloured blanks.
styled_rows() {
  capture_screen "$1" | grep -c $'\033' || true
}
no_client_attached() {
  [ -z "$(side_command "$1" list-clients -F '#{client_name}' 2>/dev/null)" ]
}

run_teardown_case() {
  local side="$1"
  local expect="$2"
  local first second styled restores tail
  local screen_caught=0
  local tail_caught=0

  start_flood "$side"
  relay_hold "$side" USR1
  wait_for_full_pty "$side"

  tmux_outer_command send-keys -t "=$OUTER_SESSION:$side" C-b d ||
    die "$side outer send-keys detach failed"
  relay_hold "$side" USR2
  wait_for "$side client detached" no_client_attached "$side"
  wait_for "$side relay handed the terminal back" relay_exited "$side"
  wait_settled "$side" "$DONE_MARKER"

  first="$(non_empty_rows "$side")"
  sleep 1
  second="$(non_empty_rows "$side")"
  styled="$(styled_rows "$side")"
  restores="$(relay_status_field "$side" restores)"
  tail="$(relay_status_field "$side" tail)"
  escapes="$(relay_status_field "$side" escapes)"

  if [ "$first" != "$second" ]; then
    screen_caught=1
    printf '      %s outer screen kept changing after the client exited\n' "$side"
  elif [ "$first" != "$DONE_MARKER" ]; then
    screen_caught=1
    printf '      %s outer screen after the client exited was not just %s:\n' "$side" "$DONE_MARKER"
    printf '%s\n' "$first" | cat -v | sed 's/^/        /'
  elif [ "$styled" -ne 0 ]; then
    screen_caught=1
    printf '      %s left %s styled rows on the terminal the relay had cleared\n' "$side" "$styled"
  fi

  # The escape count is a byte-level observable on purpose: "after the terminal
  # was handed back" has no expression in the decoded grid, because the outer
  # tmux would have decoded a late paint into the same cells the relay then
  # cleared. Both binaries print a detach line after the restore and that is
  # text, not a paint; a paint carries escapes. Measured 2026-09-09: pinned
  # tmux d77c9dc6 never leaves an alternate screen at all (restores=0), so its
  # stream has no restoration point to be after and its tail is recorded.
  if [[ "$restores" =~ ^[0-9]+$ ]] && [ "$restores" -gt 0 ]; then
    if [ "$escapes" -ne 0 ]; then
      tail_caught=1
      printf '      %s wrote %s escape bytes in the %s bytes after leaving the alternate screen: %s\n' \
        "$side" "$escapes" "$tail" "$(cat "$(status_path "$side").tail" 2>/dev/null || true)"
    fi
  else
    RECORDS=$((RECORDS + 1))
    printf 'note  %s never left an alternate screen (restores=%s), tail recorded at %s bytes, %s of them escapes\n' \
      "$side" "${restores:-<none>}" "${tail:-<none>}" "${escapes:-<none>}"
  fi

  stop_flood "$side"
  tmux_outer_command kill-window -t "=$OUTER_SESSION:$side" >/dev/null 2>&1 || true

  if [ "$expect" = clean ]; then
    CHECKS=$((CHECKS + 1))
    if [ "$screen_caught" -eq 0 ] && [ "$tail_caught" -eq 0 ]; then
      printf 'ok    %s handed the terminal back with nothing queued behind it (restores=%s tail=%s escapes=%s)\n' \
        "$side" "$restores" "$tail" "$escapes"
      return 0
    fi
    FAILURES=$((FAILURES + 1))
    printf 'FAIL  %s left a paint on the terminal it had restored\n' "$side"
    return 0
  fi
  if [ "$screen_caught" -eq 1 ] && [ "$tail_caught" -eq 1 ]; then
    printf 'ok    sabotage caught in both channels: a paint after the restore and a paint after the marker\n'
    return 0
  fi
  FAILURES=$((FAILURES + 1))
  printf 'FAIL  sabotage missed: screen_caught=%s tail_caught=%s\n' "$screen_caught" "$tail_caught"
  return 0
}

run_side() {
  local side="$1"
  create_inner_session "$side"
  attach_side "$side"
  run_undrained_case "$side"
  settle_final_screen "$side"
}

# --- run -------------------------------------------------------------------

zz_command -f /dev/null daemon >"$SCRATCH_DIR/zz-daemon.out" 2>"$SCRATCH_DIR/zz-daemon.err" &
ZZ_PID=$!
wait_for "zz daemon socket" test -S "$ZZ_SOCKET"

printf 'prolonged output backpressure (pin %s)\n' "$(basename -- "$TMUX_BIN")"
printf 'undrained=%s s deadline=%s s rss-growth-bound=%s kB key-timeout=%s s size=%sx%s\n' \
  "$UNDRAINED" "$DEADLINE" "$RSS_GROWTH" "$KEY_TIMEOUT" "$COLUMNS_UNDER_TEST" "$ROWS_UNDER_TEST"

if [ "$SELF_CHECK" -eq 1 ]; then
  printf 'self-check: two deliberate sabotages, each caught in its own assertion\n'
  # The teardown sabotage runs on zz because it is the side with a restoration
  # to be after: the pin leaves no alternate screen, so its escape tail has no
  # starting point and the fixture records it instead of asserting it.
  create_inner_session zz
  attach_side zz 1
  run_teardown_case zz sabotaged

  attach_side zz 0
  run_undrained_case zz
  DEADLINE="$(awk -v value="${REPORT[zz.backlogged]}" 'BEGIN { printf "%.4f", value / 2 }')"
  printf 'sabotage: the deadline halved to %s s, below the latency just measured\n' "$DEADLINE"
  FAILURES_BEFORE="$FAILURES"
  run_undrained_case zz
  if [ "$FAILURES" -gt "$FAILURES_BEFORE" ]; then
    printf 'ok    sabotage caught: the response bound fires when the deadline is halved\n'
    FAILURES="$FAILURES_BEFORE"
  else
    printf 'FAIL  sabotage missed: a halved deadline still passed\n'
    FAILURES=$((FAILURES_BEFORE + 1))
  fi
  if [ "$FAILURES" -ne 0 ]; then
    printf '%s sabotages were not caught\n' "$FAILURES"
    exit 1
  fi
  printf 'self-check complete: every sabotage was caught in its own assertion\n'
  exit 0
fi

case "$SIDES" in
both)
  run_side tmux
  run_side zz
  compare_final_screens
  run_teardown_case tmux clean
  run_teardown_case zz clean
  ;;
zz | tmux)
  run_side "$SIDES"
  run_teardown_case "$SIDES" clean
  ;;
*) die "unknown side: $SIDES" ;;
esac

if [ "$FAILURES" -ne 0 ]; then
  printf '%s of %s assertions failed, %s recorded\n' "$FAILURES" "$CHECKS" "$RECORDS"
  exit 1
fi
printf 'all %s assertions passed, %s recorded\n' "$CHECKS" "$RECORDS"
