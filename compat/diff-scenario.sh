#!/usr/bin/env bash
# Runs one command corpus against zz and pinned tmux, comparing topology and
# geometry after every command.
set -euo pipefail
set +B

COMPAT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
SCENARIOS_DIR="$COMPAT_DIR/scenarios"
RESULTS_DIR="$COMPAT_DIR/results"
DEFAULT_CORPUS_DIR="$COMPAT_DIR/.cache/plugins"
STRICT_GEOMETRY=0
HARNESS_PATH="$PATH"

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
SMOKE_MODE=0
case "$scenario_relative" in
smoke/*) SMOKE_MODE=1 ;;
esac
if grep -Eq '^[[:space:]]*shim:[[:space:]]*$' "$SCENARIO_FILE"; then
  SMOKE_MODE=1
fi
CORPUS_MODE="$(awk '
  {
    line = $0
    sub(/\r$/, "", line)
    sub(/^[[:space:]]+/, "", line)
    sub(/[[:space:]]+$/, "", line)
    if (line !~ /^corpus:/) next
    count++
    sub(/^corpus:[[:space:]]*/, "", line)
    mode = line
  }
  END {
    if (count > 1 || (count == 1 && mode != "none" && mode != "required")) exit 2
    if (count == 1) print mode
  }
' "$SCENARIO_FILE")" || {
  echo "error: scenario has invalid corpus metadata: $scenario_relative" >&2
  exit 2
}
case "$scenario_relative" in
smoke/*)
  [ -n "$CORPUS_MODE" ] || {
    echo "error: smoke scenario must declare corpus: none or corpus: required: $scenario_relative" >&2
    exit 2
  }
  ;;
*)
  [ -z "$CORPUS_MODE" ] || {
    echo "error: corpus metadata is only valid for smoke scenarios: $scenario_relative" >&2
    exit 2
  }
  ;;
esac

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
CORPUS_DIR="${ZZ_COMPAT_CORPUS:-$DEFAULT_CORPUS_DIR}"
ZZ_SHIM_DIR="$SCRATCH_DIR/zz/bin"
TMUX_SHIM_DIR="$SCRATCH_DIR/tmux/bin"
STAGED_CONF="$ZZ_HOME/.tmux.conf"
EXPECTED_ZZ_WARNINGS="$SCRATCH_DIR/expected.zz.warnings"
EXPECTED_TMUX_WARNINGS="$SCRATCH_DIR/expected.tmux.warnings"

mkdir -p "$ZZ_HOME" "$ZZ_CONFIG_HOME"
: >"$EXPECTED_ZZ_WARNINGS"
: >"$EXPECTED_TMUX_WARNINGS"

die() {
  printf 'HARNESS ERROR: %s\n' "$*" >>"$LOG_FILE"
  printf 'error: %s\n' "$*" >&2
  exit 2
}

# Both CLIs infer state from the invoking environment: a "current pane" (tmux
# via TMUX_PANE in cmd_find_inside_pane, zz via ZZ_PANE) and, for tmux, the
# mode-keys/status-keys defaults (tmux.c sniffs vi out of VISUAL/EDITOR).
# Running the harness from a shell that carries any of those would leak the
# developer's context into the scratch servers, so both sides run scrubbed.
zz_command() {
  if [ "$SMOKE_MODE" -eq 1 ]; then
    env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
      -u EDITOR -u VISUAL \
      HOME="$ZZ_HOME" XDG_CONFIG_HOME="$ZZ_CONFIG_HOME" \
      PATH="$ZZ_SHIM_DIR:$(dirname -- "$ZZ_BIN"):$(dirname -- "$TMUX_BIN"):$HARNESS_PATH" \
      ZZ_SMOKE_CANARY="zz-side-only" ZZ_SMOKE_ZZ_BIN="$ZZ_BIN" \
      ZZ_SMOKE_TMUX_BIN="$TMUX_BIN" \
      ZZ_SMOKE_ZZ_SOCKET="$ZZ_SOCKET" \
      "$ZZ_BIN" --socket "$ZZ_SOCKET" "$@"
  else
    env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
      -u EDITOR -u VISUAL \
      HOME="$ZZ_HOME" XDG_CONFIG_HOME="$ZZ_CONFIG_HOME" \
      "$ZZ_BIN" --socket "$ZZ_SOCKET" "$@"
  fi
}

tmux_command() {
  if [ "$SMOKE_MODE" -eq 1 ]; then
    env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
      -u EDITOR -u VISUAL \
      HOME="$ZZ_HOME" XDG_CONFIG_HOME="$ZZ_CONFIG_HOME" \
      PATH="$TMUX_SHIM_DIR:$(dirname -- "$TMUX_BIN"):$(dirname -- "$ZZ_BIN"):$HARNESS_PATH" \
      ZZ_SMOKE_CANARY="tmux-side-only" ZZ_SMOKE_TMUX_BIN="$TMUX_BIN" \
      ZZ_SMOKE_TMUX_LABEL="$TMUX_SOCKET_NAME" \
      "$TMUX_BIN" -L "$TMUX_SOCKET_NAME" "$@"
  else
    env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
      -u EDITOR -u VISUAL \
      HOME="$ZZ_HOME" XDG_CONFIG_HOME="$ZZ_CONFIG_HOME" \
      "$TMUX_BIN" -L "$TMUX_SOCKET_NAME" "$@"
  fi
}

tmux_start_command() {
  if [ "$SMOKE_MODE" -eq 1 ]; then
    env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
      -u EDITOR -u VISUAL \
      HOME="$ZZ_HOME" XDG_CONFIG_HOME="$ZZ_CONFIG_HOME" \
      PATH="$TMUX_SHIM_DIR:$(dirname -- "$TMUX_BIN"):$(dirname -- "$ZZ_BIN"):$HARNESS_PATH" \
      ZZ_SMOKE_CANARY="tmux-side-only" ZZ_SMOKE_TMUX_BIN="$TMUX_BIN" \
      ZZ_SMOKE_TMUX_LABEL="$TMUX_SOCKET_NAME" \
      "$TMUX_BIN" -L "$TMUX_SOCKET_NAME" -f /dev/null "$@"
  else
    env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
      -u EDITOR -u VISUAL \
      HOME="$ZZ_HOME" XDG_CONFIG_HOME="$ZZ_CONFIG_HOME" \
      "$TMUX_BIN" -L "$TMUX_SOCKET_NAME" -f /dev/null "$@"
  fi
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

prepare_smoke() {
  local plugin
  local plugins=(
    tpm tmux-sensible vim-tmux-navigator tmux-yank tmux-resurrect
    tmux-continuum tmux-fpp oh-my-tmux
  )

  mkdir -p "$ZZ_SHIM_DIR" "$TMUX_SHIM_DIR" "$ZZ_HOME/.tmux/plugins" \
    "$ZZ_HOME/.tmux/bin"
  if [ "$CORPUS_MODE" = "required" ]; then
    [ -d "$CORPUS_DIR" ] || die "smoke corpus is missing: $CORPUS_DIR"
    CORPUS_DIR="$(cd -- "$CORPUS_DIR" && pwd)"
    for plugin in "${plugins[@]}"; do
      [ -d "$CORPUS_DIR/$plugin/.git" ] ||
        die "smoke corpus is missing $CORPUS_DIR/$plugin"
      ln -s "$CORPUS_DIR/$plugin" "$ZZ_HOME/.tmux/plugins/$plugin"
    done
  fi

  printf '%s\n' \
    '#!/usr/bin/env bash' \
    'export ZZ_SMOKE_CANARY=zz-wrapper-only' \
    'export ZZ_SOCKET=/tmp/zz-smoke-wrong.sock' \
    'export TMUX=/tmp/zz-smoke-wrong.sock,0,-1' \
    "exec \"\$ZZ_SMOKE_ZZ_BIN\" --socket \"\$ZZ_SMOKE_ZZ_SOCKET\" \"\$@\"" \
    >"$ZZ_SHIM_DIR/tmux"
  printf '%s\n' \
    '#!/usr/bin/env bash' \
    'export ZZ_SMOKE_CANARY=tmux-wrapper-only' \
    'export ZZ_SOCKET=/tmp/tmux-smoke-wrong.sock' \
    'export TMUX=/tmp/tmux-smoke-wrong.sock,0,-1' \
    "exec \"\$ZZ_SMOKE_TMUX_BIN\" -L \"\$ZZ_SMOKE_TMUX_LABEL\" \"\$@\"" \
    >"$TMUX_SHIM_DIR/tmux"
  printf '%s\n' '#!/usr/bin/env bash' 'exit 0' >"$ZZ_HOME/.tmux/bin/apply-theme"
  chmod +x "$ZZ_SHIM_DIR/tmux" "$TMUX_SHIM_DIR/tmux" \
    "$ZZ_HOME/.tmux/bin/apply-theme"
}

resolve_smoke_path() {
  local argument="$1"

  case "$argument" in
  \~/*) printf '%s/%s\n' "$ZZ_HOME" "${argument#"~/"}" ;;
  /*) printf '%s\n' "$argument" ;;
  *) printf '%s/%s\n' "$(dirname -- "$SCENARIO_FILE")" "$argument" ;;
  esac
}

stage_smoke_file() {
  local source destination

  source="$(resolve_smoke_path "$1")"
  destination="$(resolve_smoke_path "$2")"
  [ -f "$source" ] || die "smoke stage source not found: $source"
  case "$destination" in
  "$ZZ_HOME"/*) ;;
  *) die "smoke stage destination must be under ~/: $2" ;;
  esac
  mkdir -p "$(dirname -- "$destination")"
  cp -- "$source" "$destination"
}

stage_smoke_conf() {
  local source

  source="$(resolve_smoke_path "$1")"
  [ -f "$source" ] || die "smoke config not found: $source"
  if grep -q 'ZZ_SMOKE_CANARY' "$source"; then
    die "smoke config must not depend on ZZ_SMOKE_CANARY: $source"
  fi
  cp -- "$source" "$STAGED_CONF"
}

extract_config_warnings() {
  local source="$1"
  local destination="$2"
  sed -n '/^%config-error /p' "$source" | LC_ALL=C sort -u >"$destination"
}

# Empty %begin/%end pairs are collapsed: the pin frames every config line as
# its own block through a control client's source-file while zz runs the whole
# file in one block (ledgered framing divergence). %error pairs are kept — an
# empty %error body is the pin's exec-time config-failure signal.
normalize_control_stdout() {
  local source="$1"
  local destination="$2"
  sed -E \
    -e '/^%config-error /d' \
    -e 's/^%begin [0-9]+ [0-9]+ ([0-9]+)$/%begin TIME NUMBER \1/' \
    -e 's/^%(end|error) [0-9]+ [0-9]+ ([0-9]+)$/%\1 TIME NUMBER \2/' \
    "$source" |
    awk '
      {
        if (held != "") {
          if ($0 ~ /^%end TIME NUMBER /) { held = ""; next }
          print held
          held = ""
        }
        if ($0 ~ /^%begin TIME NUMBER /) { held = $0; next }
        print
      }
      END { if (held != "") print held }
    ' >"$destination"
}

control_terminator() {
  awk '/^%(end|error) [0-9]+ [0-9]+ [0-9]+$/ { value = $1 } END { print value }' "$1"
}

extract_key_binding() {
  local source="$1"
  local destination="$2"
  local table="$3"
  local key="$4"

  awk -F '|' -v table="$table" -v key="$key" \
    '$1 == table && $2 == key { print }' "$source" >"$destination"
  [ "$(wc -l <"$destination" | tr -d '[:space:]')" = "1" ]
}

compare_expected() {
  local label="$1"
  local step="$2"
  local expected="$3"
  local actual="$4"
  local diff_file="$SCRATCH_DIR/$safe_name-$step-$label.diff"
  local rc

  if diff -u --label "expected $label step $step" --label "actual $label step $step" \
    "$expected" "$actual" >"$diff_file"; then
    printf '%s: clean\n' "$label" >>"$LOG_FILE"
    return 0
  else
    rc=$?
  fi
  [ "$rc" -eq 1 ] || die "diff failed while checking $label at step $step"
  printf '%s: mismatch\n' "$label" >>"$LOG_FILE"
  cat "$diff_file" >>"$LOG_FILE"
  return 1
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

if [ "$SMOKE_MODE" -eq 1 ]; then
  staged_files=()
  while IFS= read -r raw_line || [ -n "$raw_line" ]; do
    raw_line="${raw_line%$'\r'}"
    line="${raw_line#"${raw_line%%[![:space:]]*}"}"
    line="${line%"${line##*[![:space:]]}"}"
    case "$line" in
    corpus:*)
      ;;
    expect-warn:*)
      warning="${line#expect-warn:}"
      warning="${warning#"${warning%%[![:space:]]*}"}"
      side="${warning%%[[:space:]]*}"
      warning="${warning#"$side"}"
      warning="${warning#"${warning%%[![:space:]]*}"}"
      [ -n "$warning" ] || die "expect-warn needs SIDE and warning text: $line"
      case "$side" in
      zz) printf '%%config-error %s\n' "$warning" >>"$EXPECTED_ZZ_WARNINGS" ;;
      tmux) printf '%%config-error %s\n' "$warning" >>"$EXPECTED_TMUX_WARNINGS" ;;
      *) die "expect-warn side must be zz or tmux: $line" ;;
      esac
      ;;
    shim:)
      ;;
    shim:*)
      die "shim does not accept a value: $line"
      ;;
    stage:*)
      staged_files+=("${line#stage:}")
      ;;
    esac
  done <"$SCENARIO_FILE"
  LC_ALL=C sort -u -o "$EXPECTED_ZZ_WARNINGS" "$EXPECTED_ZZ_WARNINGS"
  LC_ALL=C sort -u -o "$EXPECTED_TMUX_WARNINGS" "$EXPECTED_TMUX_WARNINGS"
  prepare_smoke
  for staged in ${staged_files[@]+"${staged_files[@]}"}; do
    read -r stage_source stage_destination stage_extra <<<"$staged"
    [ -n "$stage_source" ] && [ -n "$stage_destination" ] && [ -z "${stage_extra:-}" ] ||
      die "stage needs exactly a source and a destination: stage:$staged"
    stage_smoke_file "$stage_source" "$stage_destination"
  done
fi

zz_command -f /dev/null daemon >"$DAEMON_STDOUT" 2>"$DAEMON_STDERR" &
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
# Window 0's default name is process-derived in tmux and index-derived in zz;
# pin it so #{window_name} diffs mean something.
run_setup zz "zz rename-window -t w:0 main" rename-window -t w:0 main ||
  die "could not rename the zz scenario window"

tmux_setup_stdout="$SCRATCH_DIR/tmux-setup.stdout"
tmux_setup_stderr="$SCRATCH_DIR/tmux-setup.stderr"
if ! tmux_start_command new-session -d -s w \
  >"$tmux_setup_stdout" 2>"$tmux_setup_stderr"; then
  append_stream "tmux setup stdout:" "$tmux_setup_stdout"
  append_stream "tmux setup stderr:" "$tmux_setup_stderr"
  die "could not create tmux scenario session"
fi
run_setup tmux "tmux rename-window -t w:0 main" rename-window -t w:0 main ||
  die "could not rename the tmux scenario window"

steps=0
topo_divergences=0
geo_divergences=0
fmt_divergences=0
out_divergences=0
warn_divergences=0

while IFS= read -r raw_line || [ -n "$raw_line" ]; do
  raw_line="${raw_line%$'\r'}"
  line="${raw_line#"${raw_line%%[![:space:]]*}"}"
  line="${line%"${line##*[![:space:]]}"}"
  [ -n "$line" ] || continue
  [[ "$line" == \#* ]] && continue

  run_zz=1
  run_tmux=1
  is_fmt=0
  is_out=0
  is_conf=0
  is_keys=0
  key_table=""
  key_name=""
  command_text="$line"
  case "$line" in
  corpus:* | shim:* | expect-warn:* | stage:*)
    continue
    ;;
  conf:*)
    [ "$SMOKE_MODE" -eq 1 ] || die "conf is only valid in a smoke scenario"
    is_conf=1
    command_text="${line#conf:}"
    ;;
  keys:*)
    [ "$SMOKE_MODE" -eq 1 ] || die "keys is only valid in a smoke scenario"
    is_keys=1
    command_text="${line#keys:}"
    ;;
  fmt:*)
    is_fmt=1
    command_text="${line#fmt:}"
    ;;
  out:*)
    is_out=1
    command_text="${line#out:}"
    ;;
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
  if [ "$is_fmt" -eq 0 ] && [ "$is_out" -eq 0 ] && [ "$is_conf" -eq 0 ] &&
    [ "$is_keys" -eq 0 ] &&
    [[ "$command_text" == fmt:* || "$command_text" == out:* ]]; then
    die "query lines must run on both sides: $line"
  fi
  [ -n "$command_text" ] || die "empty command after side prefix: $line"

  command_args=()
  if [ "$is_conf" -eq 1 ]; then
    stage_smoke_conf "$command_text"
    command_args=(-C source-file "$STAGED_CONF")
  elif [ "$is_keys" -eq 1 ]; then
    read -r key_table key_name extra <<<"$command_text"
    [ -n "$key_table" ] && [ -n "$key_name" ] && [ -z "${extra:-}" ] ||
      die "keys needs exactly a table and key: $line"
    command_args=(
      list-keys -T "$key_table" -F
      '#{key_table}|#{key_string}|#{key_repeat}|#{key_command}'
    )
  elif [ "$is_fmt" -eq 1 ] || [ "$is_out" -eq 1 ]; then
    case "$command_text" in
    *'$'* | *'`'* | *"'"* | *'"'* | *'#('*)
      die "unsupported content in query line: $command_text"
      ;;
    esac
    if [ "$is_fmt" -eq 1 ]; then
      command_args=(display-message -p "$command_text")
    else
      read -r -a command_args <<<"$command_text"
    fi
  else
    case "$command_text" in
    *'$'* | *'`'* | *';'* | *'&'* | *'|'* | *'<'* | *'>'*)
      die "unsupported shell metacharacter in scenario command: $command_text"
      ;;
    esac
    if ! eval "command_args=($command_text)"; then
      die "could not parse scenario command: $command_text"
    fi
  fi
  [ "${#command_args[@]}" -gt 0 ] || die "empty parsed command: $command_text"

  steps=$((steps + 1))
  topo_step_diverged=0
  geo_step_diverged=0
  fmt_step_diverged=0
  out_step_diverged=0
  warn_step_diverged=0
  key_extract_failed=0
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

  if [ "$is_keys" -eq 1 ]; then
    zz_key_raw="$SCRATCH_DIR/step-$steps.zz.keys.raw"
    tmux_key_raw="$SCRATCH_DIR/step-$steps.tmux.keys.raw"
    mv "$zz_stdout" "$zz_key_raw"
    mv "$tmux_stdout" "$tmux_key_raw"
    if ! extract_key_binding "$zz_key_raw" "$zz_stdout" "$key_table" "$key_name"; then
      key_extract_failed=1
    fi
    if ! extract_key_binding "$tmux_key_raw" "$tmux_stdout" "$key_table" "$key_name"; then
      key_extract_failed=1
    fi
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

  if [ "$is_fmt" -eq 1 ]; then
    if [ "$zz_rc" -ne 0 ] || [ "$tmux_rc" -ne 0 ]; then
      printf 'FMT QUERY: failure\n' >>"$LOG_FILE"
      fmt_step_diverged=1
    elif ! compare_snapshot FMT "$steps" "$zz_stdout" "$tmux_stdout"; then
      fmt_step_diverged=1
    fi
  elif [ "$is_out" -eq 1 ]; then
    if [ "$zz_rc" -ne 0 ] || [ "$tmux_rc" -ne 0 ]; then
      printf 'OUT QUERY: failure\n' >>"$LOG_FILE"
      out_step_diverged=1
    elif ! compare_snapshot OUT "$steps" "$zz_stdout" "$tmux_stdout"; then
      out_step_diverged=1
    fi
  elif [ "$run_zz" -eq 1 ] && [ "$run_tmux" -eq 1 ]; then
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

  if [ "$SMOKE_MODE" -eq 1 ]; then
    if [ "$is_conf" -eq 1 ]; then
      zz_warnings="$SCRATCH_DIR/step-$steps.zz.warnings"
      tmux_warnings="$SCRATCH_DIR/step-$steps.tmux.warnings"
      extract_config_warnings "$zz_stdout" "$zz_warnings"
      extract_config_warnings "$tmux_stdout" "$tmux_warnings"
      if ! compare_expected "WARN zz" "$steps" "$EXPECTED_ZZ_WARNINGS" "$zz_warnings"; then
        warn_step_diverged=1
      fi
      if ! compare_expected "WARN tmux" "$steps" "$EXPECTED_TMUX_WARNINGS" "$tmux_warnings"; then
        warn_step_diverged=1
      fi

      zz_terminator="$(control_terminator "$zz_stdout")"
      tmux_terminator="$(control_terminator "$tmux_stdout")"
      if [ "$zz_terminator" = "%end" ] && [ "$tmux_terminator" = "%end" ]; then
        printf 'WARN terminator: clean (%%end / %%end)\n' >>"$LOG_FILE"
      else
        printf 'WARN terminator: mismatch (zz=%s tmux=%s expected=%%end)\n' \
          "${zz_terminator:-missing}" "${tmux_terminator:-missing}" >>"$LOG_FILE"
        warn_step_diverged=1
      fi

      zz_smoke_stdout="$SCRATCH_DIR/step-$steps.zz.smoke-stdout"
      tmux_smoke_stdout="$SCRATCH_DIR/step-$steps.tmux.smoke-stdout"
      normalize_control_stdout "$zz_stdout" "$zz_smoke_stdout"
      normalize_control_stdout "$tmux_stdout" "$tmux_smoke_stdout"
      if ! compare_snapshot "SMOKE STDOUT" "$steps" \
        "$zz_smoke_stdout" "$tmux_smoke_stdout"; then
        warn_step_diverged=1
      fi
    elif [ "$is_fmt" -eq 0 ] && [ "$is_out" -eq 0 ]; then
      if ! compare_snapshot "SMOKE STDOUT" "$steps" "$zz_stdout" "$tmux_stdout"; then
        warn_step_diverged=1
      fi
    fi
    if ! compare_snapshot "SMOKE STDERR" "$steps" "$zz_stderr" "$tmux_stderr"; then
      warn_step_diverged=1
    fi
    if [ "$key_extract_failed" -eq 1 ]; then
      printf 'SMOKE KEY: expected exactly one %s|%s binding on each side\n' \
        "$key_table" "$key_name" >>"$LOG_FILE"
      warn_step_diverged=1
    fi
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
  if [ "$fmt_step_diverged" -eq 1 ]; then
    fmt_divergences=$((fmt_divergences + 1))
  fi
  if [ "$out_step_diverged" -eq 1 ]; then
    out_divergences=$((out_divergences + 1))
  fi
  if [ "$warn_step_diverged" -eq 1 ]; then
    warn_divergences=$((warn_divergences + 1))
  fi
done <"$SCENARIO_FILE"

[ "$steps" -gt 0 ] || die "scenario contains no commands: $SCENARIO_FILE"

printf '\nSUMMARY steps=%d topo_divergences=%d geo_divergences=%d fmt_divergences=%d out_divergences=%d warn_divergences=%d\n' \
  "$steps" "$topo_divergences" "$geo_divergences" "$fmt_divergences" \
  "$out_divergences" "$warn_divergences" >>"$LOG_FILE"

printf '%s: %d step(s), %d TOPO divergence(s), %d GEO divergence(s), %d FMT divergence(s), %d OUT divergence(s), %d WARN divergence(s)\n' \
  "$scenario_name" "$steps" "$topo_divergences" "$geo_divergences" "$fmt_divergences" \
  "$out_divergences" "$warn_divergences"

if [ "$topo_divergences" -gt 0 ]; then
  exit 1
fi
if [ "$STRICT_GEOMETRY" -eq 1 ] && [ "$geo_divergences" -gt 0 ]; then
  exit 1
fi
if [ "$fmt_divergences" -gt 0 ]; then
  exit 1
fi
if [ "$out_divergences" -gt 0 ]; then
  exit 1
fi
if [ "$warn_divergences" -gt 0 ]; then
  exit 1
fi
