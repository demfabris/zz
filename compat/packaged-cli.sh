#!/usr/bin/env bash
set -eEuo pipefail
set +B

usage() {
  printf 'usage: compat/packaged-cli.sh [APP_BUNDLE]\n' >&2
}

if [ "$#" -gt 1 ]; then
  usage
  exit 2
fi

if [ "$(uname -s)" != "Darwin" ]; then
  printf 'error: packaged CLI smoke requires macOS\n' >&2
  exit 2
fi

APP_INPUT="${1:-dist/zz/zz.app}"
if [ ! -d "$APP_INPUT" ]; then
  printf 'error: app bundle is missing: %s\n' "$APP_INPUT" >&2
  exit 2
fi

APP_DIR="$(cd -- "$(dirname -- "$APP_INPUT")" && pwd)"
APP_SOURCE="$APP_DIR/$(basename -- "$APP_INPUT")"
TMUX_BIN="$(command -v tmux)"
SCRATCH_DIR="$(mktemp -d '/tmp/zz packaged cli.XXXXXX')"
TOKEN="${SCRATCH_DIR##*.}"
APP="$SCRATCH_DIR/installed zz.app"
CLI="$APP/Contents/MacOS/cli"
ZZ="$APP/Contents/MacOS/zz"
SOCKETS=()
OUTER_NAMES=()

packaged_command() {
  local socket="$1"
  shift
  env -u TMUX -u TMUX_PANE -u ZZ_SESSION -u ZZ_PANE \
    HOME="$SCRATCH_DIR/home" XDG_CONFIG_HOME="$SCRATCH_DIR/config" \
    ZZ_SOCKET="$socket" "$CLI" -S "$socket" "$@"
}

outer_command() {
  local name="$1"
  shift
  env -u TMUX -u TMUX_PANE TMUX_TMPDIR=/tmp \
    HOME="$SCRATCH_DIR/home" XDG_CONFIG_HOME="$SCRATCH_DIR/config" \
    "$TMUX_BIN" -L "$name" "$@"
}

cleanup() {
  local status=$?
  local name
  local socket
  trap - EXIT ERR INT TERM
  set +e
  for name in "${OUTER_NAMES[@]-}"; do
    [ -n "$name" ] || continue
    outer_command "$name" kill-server >/dev/null 2>&1
  done
  for socket in "${SOCKETS[@]-}"; do
    [ -n "$socket" ] || continue
    packaged_command "$socket" kill-server >/dev/null 2>&1
    rm -f -- "$socket" "$socket.identity" "$socket.lock"
  done
  rm -rf -- "$SCRATCH_DIR"
  exit "$status"
}

fail_case() {
  local name="$1"
  local message="$2"
  local outer="$3"
  trap - ERR
  set +e
  printf 'error: %s: %s\n' "$name" "$message" >&2
  printf 'outer pane:\n' >&2
  outer_command "$outer" capture-pane -p -S - -t driver:0.0 >&2 || printf '<unavailable>\n' >&2
  exit 1
}

write_client_command() {
  local destination="$1"
  local socket="$2"
  shift 2
  {
    printf '#!/usr/bin/env bash\n'
    printf 'exec env -u TMUX -u TMUX_PANE -u ZZ_SESSION -u ZZ_PANE HOME=%q XDG_CONFIG_HOME=%q ZZ_SOCKET=%q %q' \
      "$SCRATCH_DIR/home" "$SCRATCH_DIR/config" "$socket" "$CLI"
    if [ "$#" -gt 0 ]; then
      printf ' %q' "$@"
    fi
    printf '\n'
  } >"$destination"
  chmod +x "$destination"
}

start_attached_client() {
  local name="$1"
  local socket="$2"
  local outer="$3"
  local width="$4"
  local height="$5"
  shift 5
  local runner="$SCRATCH_DIR/$name"
  local runner_command

  OUTER_NAMES+=("$outer")
  write_client_command "$runner" "$socket" "$@"
  printf -v runner_command '/bin/bash %q' "$runner"
  env -u TMUX -u TMUX_PANE TMUX_TMPDIR=/tmp \
    HOME="$SCRATCH_DIR/home" XDG_CONFIG_HOME="$SCRATCH_DIR/config" \
    "$TMUX_BIN" -L "$outer" -f /dev/null new-session -d \
    -x "$width" -y "$height" -s driver
  outer_command "$outer" set-option -g remain-on-exit on
  outer_command "$outer" respawn-pane -k -t driver:0.0 "$runner_command"
}

wait_for_client_value() {
  local name="$1"
  local socket="$2"
  local outer="$3"
  local format="$4"
  local expected="$5"
  local value=""
  local attempt

  for ((attempt = 0; attempt < 200; attempt++)); do
    value="$(packaged_command "$socket" list-clients -F "$format" 2>/dev/null || true)"
    if [ "$value" = "$expected" ]; then
      return 0
    fi
    if [ "$(outer_command "$outer" display-message -p -t driver:0.0 '#{pane_dead}' 2>/dev/null || true)" = 1 ]; then
      fail_case "$name" "packaged CLI exited while waiting for client value $expected" "$outer"
    fi
    sleep 0.05
  done
  fail_case "$name" "client value was ${value:-<empty>}, expected $expected" "$outer"
}

wait_for_client_pattern() {
  local name="$1"
  local socket="$2"
  local outer="$3"
  local format="$4"
  local pattern="$5"
  local value=""
  local attempt

  for ((attempt = 0; attempt < 200; attempt++)); do
    value="$(packaged_command "$socket" list-clients -F "$format" 2>/dev/null || true)"
    if [[ "$value" =~ $pattern ]]; then
      return 0
    fi
    if [ "$(outer_command "$outer" display-message -p -t driver:0.0 '#{pane_dead}' 2>/dev/null || true)" = 1 ]; then
      fail_case "$name" "packaged CLI exited while waiting for client value matching $pattern" "$outer"
    fi
    sleep 0.05
  done
  fail_case "$name" "client value was ${value:-<empty>}, expected pattern $pattern" "$outer"
}

wait_for_outer_marker() {
  local name="$1"
  local outer="$2"
  local marker="$3"
  local output=""
  local attempt

  for ((attempt = 0; attempt < 200; attempt++)); do
    output="$(outer_command "$outer" capture-pane -p -S - -t driver:0.0 2>/dev/null || true)"
    if grep -Fq -- "$marker" <<<"$output"; then
      return 0
    fi
    sleep 0.05
  done
  fail_case "$name" "outer screen did not show $marker" "$outer"
}

wait_for_pane_marker() {
  local name="$1"
  local socket="$2"
  local outer="$3"
  local target="$4"
  local marker="$5"
  local output=""
  local attempt

  for ((attempt = 0; attempt < 200; attempt++)); do
    output="$(packaged_command "$socket" capture-pane -p -J -S - -t "$target" 2>/dev/null || true)"
    if grep -Fq -- "$marker" <<<"$output"; then
      return 0
    fi
    sleep 0.05
  done
  fail_case "$name" "terminal pane did not show $marker" "$outer"
}

wait_for_outer_exit() {
  local name="$1"
  local outer="$2"
  local expected_status="$3"
  local notice="$4"
  local dead=""
  local status=""
  local output=""
  local attempt

  for ((attempt = 0; attempt < 200; attempt++)); do
    dead="$(outer_command "$outer" display-message -p -t driver:0.0 '#{pane_dead}' 2>/dev/null || true)"
    [ "$dead" = 1 ] && break
    sleep 0.05
  done
  if [ "$dead" != 1 ]; then
    fail_case "$name" "packaged CLI did not exit" "$outer"
  fi
  status="$(outer_command "$outer" display-message -p -t driver:0.0 '#{pane_dead_status}')"
  if [ "$status" != "$expected_status" ]; then
    fail_case "$name" "exit status was $status, expected $expected_status" "$outer"
  fi
  if [ -n "$notice" ]; then
    output="$(outer_command "$outer" capture-pane -p -S - -t driver:0.0)"
    if ! grep -Fq -- "$notice" <<<"$output"; then
      fail_case "$name" "output did not contain $notice" "$outer"
    fi
  fi
}

detach_outer_client() {
  local name="$1"
  local outer="$2"
  local session="$3"

  outer_command "$outer" send-keys -t driver:0.0 "C-\\"
  wait_for_outer_exit "$name" "$outer" 0 "[detached (from session $session)]"
}

run_case() {
  local name="$1"
  local existing="$2"
  local expected_client="$3"
  local expected_sessions="$4"
  shift 4
  local socket="/tmp/zzp-$TOKEN-${#SOCKETS[@]}.sock"
  local outer="zzpo-$TOKEN-${#OUTER_NAMES[@]}"
  local runner="$SCRATCH_DIR/$name"
  local runner_command
  local clients=""
  local sessions=""
  local attempt

  SOCKETS+=("$socket")
  OUTER_NAMES+=("$outer")
  if [ "$existing" = yes ]; then
    packaged_command "$socket" -f /dev/null new-session -d -s existing
  fi
  write_client_command "$runner" "$socket" "$@"
  printf -v runner_command '/bin/bash %q' "$runner"
  env -u TMUX -u TMUX_PANE TMUX_TMPDIR=/tmp \
    HOME="$SCRATCH_DIR/home" XDG_CONFIG_HOME="$SCRATCH_DIR/config" \
    "$TMUX_BIN" -L "$outer" -f /dev/null new-session -d -x 80 -y 24 -s driver
  outer_command "$outer" set-option -g remain-on-exit on
  outer_command "$outer" respawn-pane -k -t driver:0.0 "$runner_command"

  for ((attempt = 0; attempt < 200; attempt++)); do
    clients="$(packaged_command "$socket" list-clients -F '#{client_session}' 2>/dev/null || true)"
    if [ "$clients" = "$expected_client" ]; then
      break
    fi
    if [ "$(outer_command "$outer" display-message -p -t driver:0.0 '#{pane_dead}' 2>/dev/null || true)" = 1 ]; then
      fail_case "$name" "packaged CLI exited before attaching" "$outer"
    fi
    sleep 0.05
  done
  if [ "$clients" != "$expected_client" ]; then
    fail_case "$name" "client session was ${clients:-<empty>}, expected $expected_client" "$outer"
  fi

  sessions="$(packaged_command "$socket" list-sessions -F '#{session_name}' | LC_ALL=C sort)"
  if [ "$sessions" != "$expected_sessions" ]; then
    fail_case "$name" "sessions were ${sessions:-<empty>}, expected $expected_sessions" "$outer"
  fi

  outer_command "$outer" kill-server
  packaged_command "$socket" kill-server
  printf 'packaged CLI %s: PASS\n' "$name"
}

run_no_sessions_case() {
  local name="attach-empty"
  local socket="/tmp/zzp-$TOKEN-${#SOCKETS[@]}.sock"
  local outer="zzpo-$TOKEN-${#OUTER_NAMES[@]}"
  local runner="$SCRATCH_DIR/$name"
  local runner_command
  local dead=""
  local status=""
  local output=""
  local sessions=""
  local attempt

  SOCKETS+=("$socket")
  OUTER_NAMES+=("$outer")
  write_client_command "$runner" "$socket" attach
  printf -v runner_command '/bin/bash %q' "$runner"
  env -u TMUX -u TMUX_PANE TMUX_TMPDIR=/tmp \
    HOME="$SCRATCH_DIR/home" XDG_CONFIG_HOME="$SCRATCH_DIR/config" \
    "$TMUX_BIN" -L "$outer" -f /dev/null new-session -d -x 80 -y 24 -s driver
  outer_command "$outer" set-option -g remain-on-exit on
  outer_command "$outer" respawn-pane -k -t driver:0.0 "$runner_command"

  for ((attempt = 0; attempt < 200; attempt++)); do
    dead="$(outer_command "$outer" display-message -p -t driver:0.0 '#{pane_dead}' 2>/dev/null || true)"
    [ "$dead" = 1 ] && break
    sleep 0.05
  done
  if [ "$dead" != 1 ]; then
    fail_case "$name" "packaged CLI did not exit" "$outer"
  fi
  status="$(outer_command "$outer" display-message -p -t driver:0.0 '#{pane_dead_status}')"
  if [ "$status" != 1 ]; then
    fail_case "$name" "exit status was $status, expected 1" "$outer"
  fi
  output="$(outer_command "$outer" capture-pane -p -S - -t driver:0.0)"
  case "$output" in
  *'no sessions'*) ;;
  *) fail_case "$name" "output did not contain no sessions" "$outer" ;;
  esac
  sessions="$(packaged_command "$socket" list-sessions -F '#{session_name}' 2>/dev/null || true)"
  if [ -n "$sessions" ]; then
    fail_case "$name" "sessions were $sessions, expected none" "$outer"
  fi

  outer_command "$outer" kill-server
  packaged_command "$socket" kill-server
  printf 'packaged CLI %s: PASS\n' "$name"
}

run_detached_size_case() {
  local name="new-detached-size"
  local socket="/tmp/zzp-$TOKEN-${#SOCKETS[@]}.sock"
  local outer="zzpo-$TOKEN-${#OUTER_NAMES[@]}"
  local size

  SOCKETS+=("$socket")
  start_attached_client "$name" "$socket" "$outer" 80 24 \
    -f /dev/null new -d -x 93 -y 29 -s sized
  wait_for_outer_exit "$name" "$outer" 0 ""
  size="$(packaged_command "$socket" display-message -p -t sized:0.0 \
    '#{window_width}x#{window_height}|#{pane_width}x#{pane_height}')"
  if [ "$size" != '93x29|93x29' ]; then
    fail_case "$name" "detached size was $size, expected 93x29|93x29" "$outer"
  fi

  outer_command "$outer" kill-server
  packaged_command "$socket" kill-server
  printf 'packaged CLI %s: PASS\n' "$name"
}

run_attached_size_case() {
  local name="attach-terminal-size"
  local socket="/tmp/zzp-$TOKEN-${#SOCKETS[@]}.sock"
  local outer="zzpo-$TOKEN-${#OUTER_NAMES[@]}"
  local outer_size

  SOCKETS+=("$socket")
  packaged_command "$socket" -f /dev/null new-session -d -s sized
  start_attached_client "$name" "$socket" "$outer" 97 31 attach -t sized
  outer_size="$(outer_command "$outer" display-message -p -t driver:0.0 \
    '#{pane_width}x#{pane_height}')"
  if [ "$outer_size" != '97x31' ]; then
    fail_case "$name" "outer PTY was $outer_size, expected 97x31" "$outer"
  fi
  wait_for_client_value "$name" "$socket" "$outer" \
    '#{client_width}x#{client_height}|#{client_session}' '97x31|sized'
  detach_outer_client "$name" "$outer" sized

  outer_command "$outer" kill-server
  packaged_command "$socket" kill-server
  printf 'packaged CLI %s: PASS\n' "$name"
}

run_read_only_case() {
  local name="attach-read-only"
  local socket="/tmp/zzp-$TOKEN-${#SOCKETS[@]}.sock"
  local outer="zzpo-$TOKEN-${#OUTER_NAMES[@]}"
  local pane_output
  local screen_output

  SOCKETS+=("$socket")
  packaged_command "$socket" -f /dev/null new-session -d -s readonly
  start_attached_client "$name" "$socket" "$outer" 80 24 attach -r -t readonly
  wait_for_client_value "$name" "$socket" "$outer" \
    '#{client_session}|#{client_flags}|#{client_width}x#{client_height}' \
    'readonly|attached,read-only|80x24'

  outer_command "$outer" send-keys -l -t driver:0.0 'printf PACKAGED_READONLY_INPUT_BAD'
  outer_command "$outer" send-keys -t driver:0.0 Enter C-b '['
  wait_for_client_pattern "$name" "$socket" "$outer" '#{client_key_table}' \
    '^copy-mode(-vi)?$'
  outer_command "$outer" send-keys -t driver:0.0 q
  wait_for_client_value "$name" "$socket" "$outer" '#{client_key_table}' root

  packaged_command "$socket" send-keys -l -t readonly:0.0 \
    'printf PACKAGED_READONLY_OUTPUT_OK'
  packaged_command "$socket" send-keys -t readonly:0.0 Enter
  wait_for_pane_marker "$name" "$socket" "$outer" readonly:0.0 \
    PACKAGED_READONLY_OUTPUT_OK
  wait_for_outer_marker "$name" "$outer" PACKAGED_READONLY_OUTPUT_OK
  pane_output="$(packaged_command "$socket" capture-pane -p -J -S - -t readonly:0.0)"
  screen_output="$(outer_command "$outer" capture-pane -p -S - -t driver:0.0)"
  if grep -Fq PACKAGED_READONLY_INPUT_BAD <<<"$pane_output"; then
    fail_case "$name" "read-only terminal input reached the pane" "$outer"
  fi
  if grep -Fq PACKAGED_READONLY_INPUT_BAD <<<"$screen_output"; then
    fail_case "$name" "read-only terminal input was visibly echoed" "$outer"
  fi
  detach_outer_client "$name" "$outer" readonly

  outer_command "$outer" kill-server
  packaged_command "$socket" kill-server
  printf 'packaged CLI %s: PASS\n' "$name"
}

run_detach_eviction_case() {
  local name="attach-detach-eviction"
  local socket="/tmp/zzp-$TOKEN-${#SOCKETS[@]}.sock"
  local victim_outer="zzpo-$TOKEN-${#OUTER_NAMES[@]}"
  local evictor_outer

  SOCKETS+=("$socket")
  packaged_command "$socket" -f /dev/null new-session -d -s eviction
  start_attached_client "$name-victim" "$socket" "$victim_outer" 80 24 \
    attach -t eviction
  wait_for_client_value "$name" "$socket" "$victim_outer" \
    '#{client_width}x#{client_height}|#{client_session}' '80x24|eviction'

  evictor_outer="zzpo-$TOKEN-${#OUTER_NAMES[@]}"
  start_attached_client "$name-evictor" "$socket" "$evictor_outer" 90 27 \
    attach -d -t eviction
  wait_for_outer_exit "$name" "$victim_outer" 0 '[detached (from session eviction)]'
  wait_for_client_value "$name" "$socket" "$evictor_outer" \
    '#{client_width}x#{client_height}|#{client_session}' '90x27|eviction'
  detach_outer_client "$name" "$evictor_outer" eviction

  outer_command "$victim_outer" kill-server
  outer_command "$evictor_outer" kill-server
  packaged_command "$socket" kill-server
  printf 'packaged CLI %s: PASS\n' "$name"
}

trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

mkdir -p "$SCRATCH_DIR/home" "$SCRATCH_DIR/config"
cargo xtask verify-cef-bundle "$APP_SOURCE"
if ! cp -cR "$APP_SOURCE" "$APP" 2>/dev/null; then
  ditto "$APP_SOURCE" "$APP"
fi

for required in \
  "$CLI" \
  "$ZZ" \
  "$APP/Contents/Frameworks/Chromium Embedded Framework.framework/Chromium Embedded Framework" \
  "$APP/Contents/Resources/CEF_LICENSE.txt"; do
  if [ ! -s "$required" ]; then
    printf 'error: copied bundle file is missing or empty: %s\n' "$required" >&2
    exit 1
  fi
done
case "$CLI" in
*' '*) ;;
*)
  printf 'error: copied packaged CLI path does not contain spaces: %s\n' "$CLI" >&2
  exit 1
  ;;
esac
/usr/bin/codesign --verify --deep --strict "$APP"
if [ "$("$CLI" -V)" != 'tmux 3.8-zz' ]; then
  printf 'error: copied packaged CLI did not execute the bundled zz binary\n' >&2
  exit 1
fi

run_case bare-empty no 0 0
run_case bare-existing yes existing existing
run_case new-empty no created created new -s created
run_case new-existing yes created $'created\nexisting' new -s created
run_no_sessions_case
run_case attach-existing yes existing existing attach -t existing
run_detached_size_case
run_attached_size_case
run_read_only_case
run_detach_eviction_case

printf 'packaged CLI compatibility: PASS\n'
