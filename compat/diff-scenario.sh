#!/usr/bin/env bash
# Runs one command corpus against zz and pinned tmux, comparing topology and
# geometry after every command.
set -euo pipefail
set +B

COMPAT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
SCENARIOS_DIR="$COMPAT_DIR/scenarios"
RESULTS_DIR="$COMPAT_DIR/results"
STRICT_GEOMETRY=0

if [ "${1:-}" = "--strict-geometry" ]; then
  STRICT_GEOMETRY=1
  shift
fi

if [ "$#" -ne 3 ]; then
  echo "usage: compat/diff-scenario.sh [--strict-geometry] SCENARIO ZZ_BIN TMUX_BIN" >&2
  exit 2
fi

SCENARIO_FILE="$1"
ZZ_BIN="$2"
TMUX_BIN="$3"

[ -f "$SCENARIO_FILE" ] || {
  echo "error: scenario not found: $SCENARIO_FILE" >&2
  exit 2
}
[ -x "$ZZ_BIN" ] || {
  echo "error: zz binary is not executable: $ZZ_BIN" >&2
  exit 2
}
[ -x "$TMUX_BIN" ] || {
  echo "error: tmux binary is not executable: $TMUX_BIN" >&2
  exit 2
}

SCENARIO_FILE="$(cd -- "$(dirname -- "$SCENARIO_FILE")" && pwd)/$(basename -- "$SCENARIO_FILE")"
case "$SCENARIO_FILE" in
"$SCENARIOS_DIR"/*) scenario_relative="${SCENARIO_FILE#"$SCENARIOS_DIR"/}" ;;
*) scenario_relative="$(basename -- "$SCENARIO_FILE")" ;;
esac
scenario_name="${scenario_relative%.txt}"
safe_name="${scenario_name//\//-}"
safe_name="${safe_name//[^[:alnum:]_.-]/-}"

LOG_FILE="$RESULTS_DIR/$scenario_name.log"
mkdir -p "$RESULTS_DIR" "$(dirname -- "$LOG_FILE")"
: >"$LOG_FILE"

SCRATCH_DIR="$(mktemp -d "$RESULTS_DIR/.$safe_name.XXXXXX")"
ZZ_HOME="$SCRATCH_DIR/home"
ZZ_CONFIG_HOME="$SCRATCH_DIR/config"
ZZ_SOCKET="/tmp/zzc-$$.sock"
TMUX_SOCKET_NAME="zzc-$$"
DAEMON_STDOUT="$SCRATCH_DIR/zz-daemon.stdout"
DAEMON_STDERR="$SCRATCH_DIR/zz-daemon.stderr"
ZZ_PID=""

mkdir -p "$ZZ_HOME" "$ZZ_CONFIG_HOME"

die() {
  printf 'HARNESS ERROR: %s\n' "$*" >>"$LOG_FILE"
  printf 'error: %s\n' "$*" >&2
  exit 2
}

zz_command() {
  HOME="$ZZ_HOME" XDG_CONFIG_HOME="$ZZ_CONFIG_HOME" \
    "$ZZ_BIN" --socket "$ZZ_SOCKET" "$@"
}

tmux_command() {
  "$TMUX_BIN" -L "$TMUX_SOCKET_NAME" "$@"
}

side_command() {
  local side="$1"
  shift
  case "$side" in
  zz) zz_command "$@" ;;
  tmux) tmux_command "$@" ;;
  *) die "unknown comparison side: $side" ;;
  esac
}

cleanup() {
  local status=$?
  trap - EXIT
  set +e
  if [ -x "$ZZ_BIN" ]; then
    zz_command kill-server >/dev/null 2>&1
  fi
  if [ -x "$TMUX_BIN" ]; then
    tmux_command kill-server >/dev/null 2>&1
  fi
  rm -f -- "${TMUX_TMPDIR:-/tmp}/tmux-$(id -u)/$TMUX_SOCKET_NAME"
  rm -f -- "$ZZ_SOCKET"
  if [ -n "$ZZ_PID" ]; then
    kill "$ZZ_PID" >/dev/null 2>&1
    wait "$ZZ_PID" >/dev/null 2>&1
  fi
  rm -rf -- "$SCRATCH_DIR"
  exit "$status"
}

trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

append_stream() {
  local label="$1"
  local file="$2"
  printf '%s\n' "$label" >>"$LOG_FILE"
  if [ -s "$file" ]; then
    sed 's/^/    /' "$file" >>"$LOG_FILE"
  else
    printf '    <empty>\n' >>"$LOG_FILE"
  fi
}

run_setup() {
  local side="$1"
  local label="$2"
  shift 2
  local stdout_file="$SCRATCH_DIR/setup.stdout"
  local stderr_file="$SCRATCH_DIR/setup.stderr"
  local rc

  : >"$stdout_file"
  : >"$stderr_file"
  if side_command "$side" "$@" >"$stdout_file" 2>"$stderr_file"; then
    return 0
  else
    rc=$?
  fi

  printf 'SETUP FAILED: %s (exit %d)\n' "$label" "$rc" >>"$LOG_FILE"
  append_stream "stdout:" "$stdout_file"
  append_stream "stderr:" "$stderr_file"
  return "$rc"
}

query_side() {
  local side="$1"
  local destination="$2"
  local errors="$3"
  local label="$4"
  shift 4
  local stderr_file="$SCRATCH_DIR/query.stderr"
  local rc

  : >"$destination"
  : >"$stderr_file"
  if side_command "$side" "$@" >"$destination" 2>"$stderr_file"; then
    return 0
  else
    rc=$?
  fi

  {
    printf '%s exited %d\n' "$label" "$rc"
    if [ -s "$stderr_file" ]; then
      sed 's/^/    /' "$stderr_file"
    else
      printf '    <empty stderr>\n'
    fi
  } >>"$errors"
  return 1
}

collect_topology() {
  local side="$1"
  local snapshot="$2"
  local errors="$3"
  local sessions_file="$SCRATCH_DIR/$side-topo-sessions"
  local windows_file="$SCRATCH_DIR/$side-topo-windows"
  local panes_file="$SCRATCH_DIR/$side-topo-panes"
  local failed=0
  local window_index

  : >"$snapshot"
  : >"$errors"
  printf 'LIST-SESSIONS\n' >>"$snapshot"
  if query_side "$side" "$sessions_file" "$errors" "list-sessions" \
    list-sessions -F '#{session_name}:#{session_windows}'; then
    cat "$sessions_file" >>"$snapshot"
  else
    printf '<query failed>\n' >>"$snapshot"
    failed=1
  fi

  printf 'LIST-WINDOWS w\n' >>"$snapshot"
  if query_side "$side" "$windows_file" "$errors" "list-windows -t w" \
    list-windows -t w -F '#{window_index}:#{window_name}:#{window_active}:#{window_panes}'; then
    cat "$windows_file" >>"$snapshot"
    while IFS=: read -r window_index _; do
      [ -n "$window_index" ] || continue
      printf 'LIST-PANES w:%s\n' "$window_index" >>"$snapshot"
      if query_side "$side" "$panes_file" "$errors" "list-panes -t w:$window_index" \
        list-panes -t "w:$window_index" -F '#{pane_index}:#{pane_active}'; then
        cat "$panes_file" >>"$snapshot"
      else
        printf '<query failed>\n' >>"$snapshot"
        failed=1
      fi
    done <"$windows_file"
  else
    printf '<query failed>\n' >>"$snapshot"
    failed=1
  fi

  return "$failed"
}

collect_geometry() {
  local side="$1"
  local snapshot="$2"
  local errors="$3"
  local windows_file="$SCRATCH_DIR/$side-geo-windows"
  local panes_file="$SCRATCH_DIR/$side-geo-panes"
  local failed=0
  local window_index

  : >"$snapshot"
  : >"$errors"
  printf 'LIST-WINDOWS w\n' >>"$snapshot"
  if query_side "$side" "$windows_file" "$errors" "list-windows -t w geometry" \
    list-windows -t w -F '#{window_index}:#{window_width}x#{window_height}:#{window_layout}'; then
    # Pane numbers in a layout string are opaque ids the parser ignores, and the zz
    # daemon's auto-session shifts allocation by one, so the diff compares only the
    # structural body: strip the checksum and the leaf pane ids from both sides.
    sed -E 's/^([0-9]+:[0-9]+x[0-9]+:)[0-9a-f]{4},/\1/; s/([0-9]+x[0-9]+,[0-9]+,[0-9]+),[0-9]+([],}]|$)/\1\2/g' \
      "$windows_file" >"$windows_file.norm"
    mv "$windows_file.norm" "$windows_file"
    cat "$windows_file" >>"$snapshot"
    while IFS=: read -r window_index _; do
      [ -n "$window_index" ] || continue
      printf 'LIST-PANES w:%s\n' "$window_index" >>"$snapshot"
      if query_side "$side" "$panes_file" "$errors" "list-panes -t w:$window_index geometry" \
        list-panes -t "w:$window_index" -F '#{pane_index}:#{pane_width}x#{pane_height}'; then
        cat "$panes_file" >>"$snapshot"
      else
        printf '<query failed>\n' >>"$snapshot"
        failed=1
      fi
    done <"$windows_file"
  else
    printf '<query failed>\n' >>"$snapshot"
    failed=1
  fi

  return "$failed"
}

compare_snapshot() {
  local class="$1"
  local step="$2"
  local zz_snapshot="$3"
  local tmux_snapshot="$4"
  local diff_file="$SCRATCH_DIR/$class.diff"
  local rc

  if diff -u \
    --label "zz $class step $step" \
    --label "tmux $class step $step" \
    "$zz_snapshot" "$tmux_snapshot" >"$diff_file"; then
    printf '%s: clean\n' "$class" >>"$LOG_FILE"
    return 0
  else
    rc=$?
  fi

  [ "$rc" -eq 1 ] || die "diff failed while comparing $class at step $step"
  printf '%s: divergence\n' "$class" >>"$LOG_FILE"
  cat "$diff_file" >>"$LOG_FILE"
  return 1
}

{
  printf '# Scenario: %s\n' "$scenario_name"
  printf '# Source: %s\n' "$SCENARIO_FILE"
  if [ "$STRICT_GEOMETRY" -eq 1 ]; then
    printf '# Strict geometry: yes\n'
  else
    printf '# Strict geometry: no\n'
  fi
} >>"$LOG_FILE"

zz_command daemon >"$DAEMON_STDOUT" 2>"$DAEMON_STDERR" &
ZZ_PID=$!

socket_ready=0
for ((attempt = 0; attempt < 200; attempt++)); do
  if [ -S "$ZZ_SOCKET" ]; then
    socket_ready=1
    break
  fi
  if ! kill -0 "$ZZ_PID" 2>/dev/null; then
    append_stream "zz daemon stderr:" "$DAEMON_STDERR"
    die "zz daemon exited before creating $ZZ_SOCKET"
  fi
  sleep 0.05
done

if [ "$socket_ready" -ne 1 ]; then
  append_stream "zz daemon stderr:" "$DAEMON_STDERR"
  die "zz daemon did not create $ZZ_SOCKET within 10 seconds"
fi

run_setup zz "zz new-session -d -s w" new-session -d -s w ||
  die "could not create zz scenario session"
run_setup zz "zz kill-session -t 0" kill-session -t 0 ||
  die "could not remove zz auto-session"
# Window 0's default name is process-derived in tmux and index-derived in zz;
# pin it so #{window_name} diffs mean something.
run_setup zz "zz rename-window -t w:0 main" rename-window -t w:0 main ||
  die "could not rename the zz scenario window"

tmux_setup_stdout="$SCRATCH_DIR/tmux-setup.stdout"
tmux_setup_stderr="$SCRATCH_DIR/tmux-setup.stderr"
if ! "$TMUX_BIN" -L "$TMUX_SOCKET_NAME" -f /dev/null \
  new-session -d -s w >"$tmux_setup_stdout" 2>"$tmux_setup_stderr"; then
  append_stream "tmux setup stdout:" "$tmux_setup_stdout"
  append_stream "tmux setup stderr:" "$tmux_setup_stderr"
  die "could not create tmux scenario session"
fi
run_setup tmux "tmux rename-window -t w:0 main" rename-window -t w:0 main ||
  die "could not rename the tmux scenario window"

steps=0
topo_divergences=0
geo_divergences=0

while IFS= read -r raw_line || [ -n "$raw_line" ]; do
  raw_line="${raw_line%$'\r'}"
  line="${raw_line#"${raw_line%%[![:space:]]*}"}"
  line="${line%"${line##*[![:space:]]}"}"
  [ -n "$line" ] || continue
  [[ "$line" == \#* ]] && continue

  run_zz=1
  run_tmux=1
  command_text="$line"
  case "$line" in
  zz-only:*)
    run_tmux=0
    command_text="${line#zz-only:}"
    ;;
  tmux-only:*)
    run_zz=0
    command_text="${line#tmux-only:}"
    ;;
  esac
  command_text="${command_text#"${command_text%%[![:space:]]*}"}"
  command_text="${command_text%"${command_text##*[![:space:]]}"}"
  [ -n "$command_text" ] || die "empty command after side prefix: $line"

  case "$command_text" in
  *'$'* | *'`'* | *';'* | *'&'* | *'|'* | *'<'* | *'>'*)
    die "unsupported shell metacharacter in scenario command: $command_text"
    ;;
  esac

  command_args=()
  if ! eval "command_args=($command_text)"; then
    die "could not parse scenario command: $command_text"
  fi
  [ "${#command_args[@]}" -gt 0 ] || die "empty parsed command: $command_text"

  steps=$((steps + 1))
  topo_step_diverged=0
  geo_step_diverged=0
  zz_stdout="$SCRATCH_DIR/step-$steps.zz.stdout"
  zz_stderr="$SCRATCH_DIR/step-$steps.zz.stderr"
  tmux_stdout="$SCRATCH_DIR/step-$steps.tmux.stdout"
  tmux_stderr="$SCRATCH_DIR/step-$steps.tmux.stderr"
  : >"$zz_stdout"
  : >"$zz_stderr"
  : >"$tmux_stdout"
  : >"$tmux_stderr"

  if [ "$run_zz" -eq 1 ]; then
    if zz_command "${command_args[@]}" >"$zz_stdout" 2>"$zz_stderr"; then
      zz_rc=0
    else
      zz_rc=$?
    fi
  else
    zz_rc=-1
  fi

  if [ "$run_tmux" -eq 1 ]; then
    if tmux_command "${command_args[@]}" >"$tmux_stdout" 2>"$tmux_stderr"; then
      tmux_rc=0
    else
      tmux_rc=$?
    fi
  else
    tmux_rc=-1
  fi

  {
    printf '\n## Step %d\n' "$steps"
    printf 'COMMAND: %s\n' "$line"
    if [ "$run_zz" -eq 1 ]; then
      printf 'zz exit: %d\n' "$zz_rc"
    else
      printf 'zz exit: skipped\n'
    fi
    if [ "$run_tmux" -eq 1 ]; then
      printf 'tmux exit: %d\n' "$tmux_rc"
    else
      printf 'tmux exit: skipped\n'
    fi
  } >>"$LOG_FILE"
  append_stream "zz stderr:" "$zz_stderr"
  append_stream "tmux stderr:" "$tmux_stderr"
  if [ -s "$zz_stdout" ]; then
    append_stream "zz stdout:" "$zz_stdout"
  fi
  if [ -s "$tmux_stdout" ]; then
    append_stream "tmux stdout:" "$tmux_stdout"
  fi

  if [ "$run_zz" -eq 1 ] && [ "$run_tmux" -eq 1 ]; then
    if { [ "$zz_rc" -eq 0 ] && [ "$tmux_rc" -ne 0 ]; } ||
      { [ "$zz_rc" -ne 0 ] && [ "$tmux_rc" -eq 0 ]; }; then
      printf 'COMMAND EXIT-CLASS: divergence\n' >>"$LOG_FILE"
      topo_step_diverged=1
    elif [ "$zz_rc" -ne 0 ]; then
      printf 'COMMAND EXIT-CLASS: both nonzero\n' >>"$LOG_FILE"
    else
      printf 'COMMAND EXIT-CLASS: clean\n' >>"$LOG_FILE"
    fi
  else
    printf 'COMMAND EXIT-CLASS: side-specific setup\n' >>"$LOG_FILE"
  fi

  zz_topo="$SCRATCH_DIR/step-$steps.zz.topo"
  tmux_topo="$SCRATCH_DIR/step-$steps.tmux.topo"
  zz_topo_errors="$SCRATCH_DIR/step-$steps.zz.topo-errors"
  tmux_topo_errors="$SCRATCH_DIR/step-$steps.tmux.topo-errors"
  if collect_topology zz "$zz_topo" "$zz_topo_errors"; then
    zz_topo_ok=1
  else
    zz_topo_ok=0
  fi
  if collect_topology tmux "$tmux_topo" "$tmux_topo_errors"; then
    tmux_topo_ok=1
  else
    tmux_topo_ok=0
  fi
  if [ "$zz_topo_ok" -ne 1 ] || [ "$tmux_topo_ok" -ne 1 ]; then
    printf 'TOPO QUERY: failure\n' >>"$LOG_FILE"
    append_stream "zz TOPO query errors:" "$zz_topo_errors"
    append_stream "tmux TOPO query errors:" "$tmux_topo_errors"
    topo_step_diverged=1
  fi
  if ! compare_snapshot TOPO "$steps" "$zz_topo" "$tmux_topo"; then
    topo_step_diverged=1
  fi

  zz_geo="$SCRATCH_DIR/step-$steps.zz.geo"
  tmux_geo="$SCRATCH_DIR/step-$steps.tmux.geo"
  zz_geo_errors="$SCRATCH_DIR/step-$steps.zz.geo-errors"
  tmux_geo_errors="$SCRATCH_DIR/step-$steps.tmux.geo-errors"
  if collect_geometry zz "$zz_geo" "$zz_geo_errors"; then
    zz_geo_ok=1
  else
    zz_geo_ok=0
  fi
  if collect_geometry tmux "$tmux_geo" "$tmux_geo_errors"; then
    tmux_geo_ok=1
  else
    tmux_geo_ok=0
  fi
  if [ "$zz_geo_ok" -ne 1 ] || [ "$tmux_geo_ok" -ne 1 ]; then
    printf 'GEO QUERY: failure\n' >>"$LOG_FILE"
    append_stream "zz GEO query errors:" "$zz_geo_errors"
    append_stream "tmux GEO query errors:" "$tmux_geo_errors"
    geo_step_diverged=1
  fi
  if ! compare_snapshot GEO "$steps" "$zz_geo" "$tmux_geo"; then
    geo_step_diverged=1
  fi

  if [ "$topo_step_diverged" -eq 1 ]; then
    topo_divergences=$((topo_divergences + 1))
  fi
  if [ "$geo_step_diverged" -eq 1 ]; then
    geo_divergences=$((geo_divergences + 1))
  fi
done <"$SCENARIO_FILE"

[ "$steps" -gt 0 ] || die "scenario contains no commands: $SCENARIO_FILE"

printf '\nSUMMARY steps=%d topo_divergences=%d geo_divergences=%d\n' \
  "$steps" "$topo_divergences" "$geo_divergences" >>"$LOG_FILE"

printf '%s: %d step(s), %d TOPO divergence(s), %d GEO divergence(s)\n' \
  "$scenario_name" "$steps" "$topo_divergences" "$geo_divergences"

if [ "$topo_divergences" -gt 0 ]; then
  exit 1
fi
if [ "$STRICT_GEOMETRY" -eq 1 ] && [ "$geo_divergences" -gt 0 ]; then
  exit 1
fi
