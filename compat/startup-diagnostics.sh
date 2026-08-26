#!/usr/bin/env bash
set -eEuo pipefail
set +B

PIN="d77c9dc6aa021e4bc61f0da128c591af695e6466"
TMUX_VERSION="tmux next-3.8"
ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
TMUX_FETCH_SCRIPT="$ROOT_DIR/compat/fetch-tmux.sh"
TMUX_BUILD_STAMP="$ROOT_DIR/compat/.cache/tmux-build.stamp"
PROBE_TIMEOUT_SECONDS=0.5
POLL_DEADLINE_SECONDS=10

usage() {
  printf 'usage: compat/startup-diagnostics.sh [ZZ_BIN [TMUX_BIN]]\n' >&2
}

if [ "$#" -gt 2 ]; then
  usage
  exit 2
fi

ZZ_INPUT="${1:-$ROOT_DIR/target/debug/zz}"
TMUX_INPUT="${2:-$ROOT_DIR/compat/.cache/tmux-src/tmux}"

resolve_binary() {
  local binary="$1"
  local directory

  case "$binary" in
  */*)
    [ -x "$binary" ] || return 1
    directory="$(cd -- "$(dirname -- "$binary")" && pwd)"
    printf '%s/%s\n' "$directory" "$(basename -- "$binary")"
    ;;
  *)
    command -v "$binary"
    ;;
  esac
}

if ! ZZ_BIN="$(resolve_binary "$ZZ_INPUT")"; then
  printf 'error: zz binary is not executable: %s\n' "$ZZ_INPUT" >&2
  exit 2
fi
if ! TMUX_BIN="$(resolve_binary "$TMUX_INPUT")"; then
  printf 'error: tmux binary is not executable: %s\n' "$TMUX_INPUT" >&2
  exit 2
fi

TMUX_SOURCE_DIR="$(cd -- "$(dirname -- "$TMUX_BIN")" && pwd)"
if ! ORACLE_ROOT="$(git -C "$TMUX_SOURCE_DIR" rev-parse --show-toplevel 2>/dev/null)"; then
  printf 'error: tmux source checkout is unavailable beside %s\n' "$TMUX_BIN" >&2
  exit 2
fi
ORACLE_ROOT="$(cd -- "$ORACLE_ROOT" && pwd)"
if [ "$TMUX_SOURCE_DIR" != "$ORACLE_ROOT" ] || [ "$TMUX_BIN" != "$ORACLE_ROOT/tmux" ]; then
  printf 'error: tmux binary must be the checkout-root binary: %s/tmux\n' \
    "$ORACLE_ROOT" >&2
  exit 2
fi
if ! ORACLE_HEAD="$(git -C "$TMUX_SOURCE_DIR" rev-parse HEAD 2>/dev/null)"; then
  printf 'error: tmux source HEAD is unavailable: %s\n' "$TMUX_SOURCE_DIR" >&2
  exit 2
fi
if [ "$ORACLE_HEAD" != "$PIN" ]; then
  printf 'error: tmux pin is %s, expected %s\n' "$ORACLE_HEAD" "$PIN" >&2
  exit 2
fi
if ! ORACLE_DIRTY="$(git -C "$TMUX_SOURCE_DIR" status --porcelain \
  --untracked-files=all 2>/dev/null)"; then
  printf 'error: tmux source status is unavailable: %s\n' "$TMUX_SOURCE_DIR" >&2
  exit 2
fi
if [ -n "$ORACLE_DIRTY" ]; then
  printf 'error: tmux source checkout is dirty: %s\n' "$TMUX_SOURCE_DIR" >&2
  exit 2
fi
if [ "$("$TMUX_BIN" -V 2>/dev/null || true)" != "$TMUX_VERSION" ]; then
  printf "error: pinned tmux must report '%s'\n" "$TMUX_VERSION" >&2
  exit 2
fi
if [ ! -f "$TMUX_BUILD_STAMP" ]; then
  printf 'error: tmux build stamp is unavailable: %s\n' "$TMUX_BUILD_STAMP" >&2
  exit 2
fi
if ! FETCH_CHECKSUM="$(cksum <"$TMUX_FETCH_SCRIPT")" || \
  ! BINARY_CHECKSUM="$(cksum <"$TMUX_BIN")"; then
  printf 'error: failed to checksum the tmux fetch recipe or binary\n' >&2
  exit 2
fi
EXPECTED_STAMP="$(printf 'commit=%s\nversion=%s\nscript-cksum=%s\nbinary-cksum=%s\n' \
  "$PIN" "$TMUX_VERSION" "$FETCH_CHECKSUM" "$BINARY_CHECKSUM")"
if ! ACTUAL_STAMP="$(cat "$TMUX_BUILD_STAMP")"; then
  printf 'error: tmux build stamp is unreadable: %s\n' "$TMUX_BUILD_STAMP" >&2
  exit 2
fi
if [ "$ACTUAL_STAMP" != "$EXPECTED_STAMP" ]; then
  printf 'error: tmux build stamp does not attest executable: %s\n' "$TMUX_BIN" >&2
  exit 2
fi

if command -v timeout >/dev/null 2>&1; then
  TIMEOUT_BIN="$(command -v timeout)"
elif command -v gtimeout >/dev/null 2>&1; then
  TIMEOUT_BIN="$(command -v gtimeout)"
else
  printf 'error: GNU timeout is required\n' >&2
  exit 2
fi

SCRATCH_DIR="$(mktemp -d /tmp/zz-startup-diagnostics.XXXXXX)"
TOKEN="${SCRATCH_DIR##*.}"
ZZ_HOME="$SCRATCH_DIR/home-zz"
ZZ_CONFIG_HOME="$SCRATCH_DIR/config-zz"
TMUX_HOME="$SCRATCH_DIR/home-tmux"
TMUX_CONFIG_HOME="$SCRATCH_DIR/config-tmux"
OUTER_HOME="$SCRATCH_DIR/home-outer"
OUTER_CONFIG_HOME="$SCRATCH_DIR/config-outer"
OUTPUT_DIR="$SCRATCH_DIR/output"
FIXTURE_DIR="$SCRATCH_DIR/fixtures"
ZZ_SOCKETS=()
TMUX_LABELS=()
OUTER_LABELS=()
CURRENT_CASE="setup"
CAPTURE_RC=0
SIDE_TARGET=""
CASES_RUN=0
EXPECTED_CASES=7

mkdir -p "$ZZ_HOME" "$ZZ_CONFIG_HOME" "$TMUX_HOME" "$TMUX_CONFIG_HOME" \
  "$OUTER_HOME" "$OUTER_CONFIG_HOME" "$OUTPUT_DIR" "$FIXTURE_DIR"

run_side_with_timeout() {
  local timeout_seconds="$1"
  local side="$2"
  local target="$3"
  shift 3

  if [ "$side" = zz ]; then
    "$TIMEOUT_BIN" "$timeout_seconds" env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION \
      -u ZZ_PANE -u EDITOR -u VISUAL LC_ALL=C HOME="$ZZ_HOME" \
      XDG_CONFIG_HOME="$ZZ_CONFIG_HOME" TMUX_TMPDIR=/tmp \
      "$ZZ_BIN" --socket "$target" "$@"
  else
    "$TIMEOUT_BIN" "$timeout_seconds" env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION \
      -u ZZ_PANE -u EDITOR -u VISUAL LC_ALL=C HOME="$TMUX_HOME" \
      XDG_CONFIG_HOME="$TMUX_CONFIG_HOME" TMUX_TMPDIR=/tmp \
      "$TMUX_BIN" -L "$target" "$@"
  fi
}

run_side() {
  run_side_with_timeout 15 "$@"
}

run_outer_with_timeout() {
  local timeout_seconds="$1"
  local label="$2"
  shift 2

  "$TIMEOUT_BIN" "$timeout_seconds" env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION \
    -u ZZ_PANE -u EDITOR -u VISUAL LC_ALL=C HOME="$OUTER_HOME" \
    XDG_CONFIG_HOME="$OUTER_CONFIG_HOME" TMUX_TMPDIR=/tmp \
    "$TMUX_BIN" -L "$label" "$@"
}

run_outer() {
  run_outer_with_timeout 15 "$@"
}

prepare_target() {
  local side="$1"
  local suffix="$2"

  if [ "$side" = zz ]; then
    SIDE_TARGET="/tmp/zzsd-$TOKEN-$suffix.sock"
    ZZ_SOCKETS+=("$SIDE_TARGET")
  else
    SIDE_TARGET="zzsd-$TOKEN-$suffix"
    TMUX_LABELS+=("$SIDE_TARGET")
  fi
}

capture_side() {
  local side="$1"
  local target="$2"
  local stdout="$3"
  local stderr="$4"
  shift 4

  if run_side "$side" "$target" "$@" </dev/null >"$stdout" 2>"$stderr"; then
    CAPTURE_RC=0
  else
    CAPTURE_RC=$?
  fi
}

fail() {
  trap - ERR
  printf 'FAIL: %s: %s\n' "$CURRENT_CASE" "$*" >&2
  exit 1
}

unexpected_failure() {
  local line="$1"
  local status="$2"
  trap - ERR
  printf 'FAIL: %s: unexpected failure at line %s (exit %s)\n' \
    "$CURRENT_CASE" "$line" "$status" >&2
  exit "$status"
}

cleanup() {
  local status=$?
  local label
  local socket

  trap - EXIT ERR INT TERM
  set +e
  for label in "${OUTER_LABELS[@]-}"; do
    [ -n "$label" ] || continue
    run_outer "$label" kill-server >/dev/null 2>&1
    rm -f -- "/tmp/tmux-$(id -u)/$label"
  done
  for socket in "${ZZ_SOCKETS[@]-}"; do
    [ -n "$socket" ] || continue
    run_side zz "$socket" kill-server >/dev/null 2>&1
    rm -f -- "$socket" "$socket.identity" "$socket.lock"
  done
  for label in "${TMUX_LABELS[@]-}"; do
    [ -n "$label" ] || continue
    run_side tmux "$label" kill-server >/dev/null 2>&1
    rm -f -- "/tmp/tmux-$(id -u)/$label"
  done
  rm -rf -- "$SCRATCH_DIR"
  exit "$status"
}

trap cleanup EXIT
trap 'unexpected_failure "$LINENO" "$?"' ERR
trap 'exit 130' INT
trap 'exit 143' TERM

assert_rc() {
  local side="$1"
  local expected="$2"

  if [ "$CAPTURE_RC" -ne "$expected" ]; then
    fail "$side exit was $CAPTURE_RC, expected $expected"
  fi
}

assert_empty() {
  local side="$1"
  local stream="$2"
  local file="$3"

  if [ -s "$file" ]; then
    printf '%s %s:\n' "$side" "$stream" >&2
    sed -n '1,120p' "$file" >&2
    fail "$side $stream was not empty"
  fi
}

normalize_control() {
  local source="$1"
  local destination="$2"

  awk -v prefix="$SCRATCH_DIR" '
    function replace_literal(value, needle, replacement, position, output) {
      output = ""
      while ((position = index(value, needle)) != 0) {
        output = output substr(value, 1, position - 1) replacement
        value = substr(value, position + length(needle))
      }
      return output value
    }
    {
      sub(/\r$/, "")
      line = replace_literal($0, prefix, "<SCRATCH>")
      if (line ~ /^%window-add @[0-9]+$/ ||
          line ~ /^%window-close @[0-9]+$/ ||
          line ~ /^%window-renamed @[0-9]+ / ||
          line ~ /^%window-pane-changed @[0-9]+ %[0-9]+$/ ||
          line ~ /^%unlinked-window-(add|close) @[0-9]+$/ ||
          line ~ /^%unlinked-window-renamed @[0-9]+ / ||
          line == "%sessions-changed" ||
          line ~ /^%session-changed \$[0-9]+ / ||
          line ~ /^%session-renamed \$[0-9]+ / ||
          line ~ /^%session-window-changed \$[0-9]+ @[0-9]+$/ ||
          line ~ /^%client-session-changed [^ ]+ \$[0-9]+ /) {
        next
      }
      if (line ~ /^%(begin|end|error) [0-9]+ [0-9]+ [0-9]+$/) {
        split(line, fields, " ")
        print fields[1] " TIME NUMBER " fields[4]
        next
      }
      print line
    }
  ' "$source" >"$destination"
}

assert_files_equal() {
  local label="$1"
  local expected="$2"
  local actual="$3"
  local difference="$OUTPUT_DIR/difference"

  if ! diff -u --label "expected $label" --label "actual $label" \
    "$expected" "$actual" >"$difference"; then
    sed -n '1,200p' "$difference" >&2
    fail "$label differed"
  fi
}

assert_control() {
  local side="$1"
  local raw="$2"
  local expected="$3"
  local normalized="$raw.normalized"

  normalize_control "$raw" "$normalized"
  assert_files_equal "$side control transcript" "$expected" "$normalized"
}

stop_side() {
  local side="$1"
  local target="$2"
  local deadline
  local endpoint
  local status

  run_side "$side" "$target" kill-server >/dev/null 2>&1 || true
  deadline=$((SECONDS + POLL_DEADLINE_SECONDS))
  if [ "$side" = zz ]; then
    endpoint="$target"
    while [ "$SECONDS" -lt "$deadline" ]; do
      [ ! -S "$endpoint" ] && return 0
      sleep 0.05
    done
  else
    endpoint="/tmp/tmux-$(id -u)/$target"
    while [ "$SECONDS" -lt "$deadline" ]; do
      if run_side_with_timeout "$PROBE_TIMEOUT_SECONDS" tmux "$target" \
        list-sessions >/dev/null 2>&1; then
        status=0
      else
        status=$?
      fi
      if [ "$status" -ne 0 ] && [ "$status" -ne 124 ]; then
        rm -f -- "$endpoint"
        return 0
      fi
      sleep 0.05
    done
  fi
  fail "$side server did not stop within 10 seconds"
}

pass_case() {
  CASES_RUN=$((CASES_RUN + 1))
  printf 'PASS: %s\n' "$CURRENT_CASE"
}

case_initial_control() {
  local directory="$FIXTURE_DIR/initial"
  local root="$directory/root.conf"
  local child="$directory/child.conf"
  local expected="$OUTPUT_DIR/initial.expected"
  local side
  local target
  local stdout
  local stderr

  CURRENT_CASE="initial Control cold start"
  mkdir -p "$directory"
  printf '%s\n' 'display-message -p NESTED_CAUSE' >"$child"
  printf "display-message -p DIRECT_CAUSE\nsource-file '%s'\n" "$child" >"$root"
  printf '%s\n' \
    '%config-error <SCRATCH>/fixtures/initial/root.conf:1: DIRECT_CAUSE' \
    '%config-error <SCRATCH>/fixtures/initial/child.conf:1: NESTED_CAUSE' \
    '%begin TIME NUMBER 0' \
    '%end TIME NUMBER 0' \
    '%exit' >"$expected"

  for side in zz tmux; do
    prepare_target "$side" initial
    target="$SIDE_TARGET"
    stdout="$OUTPUT_DIR/initial.$side.stdout"
    stderr="$OUTPUT_DIR/initial.$side.stderr"
    capture_side "$side" "$target" "$stdout" "$stderr" \
      -f "$root" -C new-session -s initial-control
    assert_rc "$side" 0
    assert_empty "$side" stderr "$stderr"
    assert_control "$side" "$stdout" "$expected"
    stop_side "$side" "$target"
  done
  assert_files_equal "initial differential" \
    "$OUTPUT_DIR/initial.zz.stdout.normalized" \
    "$OUTPUT_DIR/initial.tmux.stdout.normalized"
  pass_case
}

case_detached_late_attach() {
  local directory="$FIXTURE_DIR/detached"
  local root="$directory/root.conf"
  local child="$directory/child.conf"
  local list_expected="$OUTPUT_DIR/detached.list.expected"
  local first_expected="$OUTPUT_DIR/detached.first.expected"
  local second_expected="$OUTPUT_DIR/detached.second.expected"
  local side
  local target
  local stdout
  local stderr

  CURRENT_CASE="detached launch and late Control attach"
  mkdir -p "$directory"
  printf '%s\n' 'display-message -p DETACHED_NESTED' >"$child"
  printf "display-message -p DETACHED_DIRECT\nsource-file '%s'\n" "$child" >"$root"
  printf '%s\n' \
    '%begin TIME NUMBER 0' \
    'detached-cold' \
    '%end TIME NUMBER 0' \
    '%exit' >"$list_expected"
  printf '%s\n' \
    '%begin TIME NUMBER 0' \
    '%config-error <SCRATCH>/fixtures/detached/root.conf:1: DETACHED_DIRECT' \
    '%config-error <SCRATCH>/fixtures/detached/child.conf:1: DETACHED_NESTED' \
    '%end TIME NUMBER 0' \
    '%exit' >"$first_expected"
  printf '%s\n' \
    '%begin TIME NUMBER 0' \
    '%end TIME NUMBER 0' \
    '%exit' >"$second_expected"

  for side in zz tmux; do
    prepare_target "$side" detached
    target="$SIDE_TARGET"
    stdout="$OUTPUT_DIR/detached.$side.start.stdout"
    stderr="$OUTPUT_DIR/detached.$side.start.stderr"
    capture_side "$side" "$target" "$stdout" "$stderr" \
      -f "$root" new-session -d -s detached-cold
    assert_rc "$side" 0
    assert_empty "$side" stdout "$stdout"
    assert_empty "$side" stderr "$stderr"

    stdout="$OUTPUT_DIR/detached.$side.list.stdout"
    stderr="$OUTPUT_DIR/detached.$side.list.stderr"
    capture_side "$side" "$target" "$stdout" "$stderr" \
      -C list-sessions -F '#{session_name}'
    assert_rc "$side" 0
    assert_empty "$side" stderr "$stderr"
    assert_control "$side" "$stdout" "$list_expected"

    stdout="$OUTPUT_DIR/detached.$side.first.stdout"
    stderr="$OUTPUT_DIR/detached.$side.first.stderr"
    capture_side "$side" "$target" "$stdout" "$stderr" \
      -C attach-session -t =detached-cold
    assert_rc "$side" 0
    assert_empty "$side" stderr "$stderr"
    assert_control "$side" "$stdout" "$first_expected"

    stdout="$OUTPUT_DIR/detached.$side.second.stdout"
    stderr="$OUTPUT_DIR/detached.$side.second.stderr"
    capture_side "$side" "$target" "$stdout" "$stderr" \
      -C attach-session -t =detached-cold
    assert_rc "$side" 0
    assert_empty "$side" stderr "$stderr"
    assert_control "$side" "$stdout" "$second_expected"
    stop_side "$side" "$target"
  done
  assert_files_equal "detached list differential" \
    "$OUTPUT_DIR/detached.zz.list.stdout.normalized" \
    "$OUTPUT_DIR/detached.tmux.list.stdout.normalized"
  assert_files_equal "detached first attach differential" \
    "$OUTPUT_DIR/detached.zz.first.stdout.normalized" \
    "$OUTPUT_DIR/detached.tmux.first.stdout.normalized"
  assert_files_equal "detached second attach differential" \
    "$OUTPUT_DIR/detached.zz.second.stdout.normalized" \
    "$OUTPUT_DIR/detached.tmux.second.stdout.normalized"
  pass_case
}

case_list_output_discarded() {
  local directory="$FIXTURE_DIR/list-output"
  local root="$directory/root.conf"
  local expected="$OUTPUT_DIR/list-output.expected"
  local side
  local target
  local stdout
  local stderr

  CURRENT_CASE="startup list output is discarded"
  mkdir -p "$directory"
  printf '%s\n' \
    'new-session -d -s list-seed' \
    'list-sessions -F "SHOULD_NOT_LEAK_#{session_name}"' \
    'display-message -p LIST_DISPLAY_CAUSE' >"$root"
  printf '%s\n' \
    '%config-error <SCRATCH>/fixtures/list-output/root.conf:3: LIST_DISPLAY_CAUSE' \
    '%begin TIME NUMBER 0' \
    '%end TIME NUMBER 0' \
    '%exit' >"$expected"

  for side in zz tmux; do
    prepare_target "$side" list
    target="$SIDE_TARGET"
    stdout="$OUTPUT_DIR/list-output.$side.stdout"
    stderr="$OUTPUT_DIR/list-output.$side.stderr"
    capture_side "$side" "$target" "$stdout" "$stderr" \
      -f "$root" -C new-session -d -s list-command
    assert_rc "$side" 0
    assert_empty "$side" stderr "$stderr"
    if grep -Fq SHOULD_NOT_LEAK "$stdout"; then
      fail "$side leaked list-sessions startup output"
    fi
    assert_control "$side" "$stdout" "$expected"
    stop_side "$side" "$target"
  done
  assert_files_equal "list-output differential" \
    "$OUTPUT_DIR/list-output.zz.stdout.normalized" \
    "$OUTPUT_DIR/list-output.tmux.stdout.normalized"
  pass_case
}

case_failure_root_ordering() {
  local directory="$FIXTURE_DIR/ordering"
  local root_one="$directory/root-one.conf"
  local root_parse="$directory/root-parse.conf"
  local missing_root="$directory/missing-root.conf"
  local root_two="$directory/root-two.conf"
  local nested="$directory/nested.conf"
  local nested_missing="$directory/nested-missing.conf"
  local expected="$OUTPUT_DIR/ordering.expected"
  local side
  local target
  local stdout
  local stderr

  CURRENT_CASE="failure and explicit-root ordering"
  mkdir -p "$directory"
  printf '%s\n' 'display-message -p ROOT_ONE' >"$root_one"
  printf '%s\n' 'display-message -p PARSE_NEVER' 'set @bad \400' >"$root_parse"
  printf "display-message -p NESTED_BEFORE\nsource-file '%s'\ndisplay-message -p NESTED_AFTER\n" \
    "$nested_missing" >"$nested"
  printf "new-session -d -s order-seed\ndisplay-message -p ROOT_TWO_BEFORE\nkill-session -t =missing-runtime\nsource-file '%s'\ndisplay-message -p ROOT_TWO_AFTER\n" \
    "$nested" >"$root_two"
  printf '%s\n' \
    '%config-error <SCRATCH>/fixtures/ordering/root-parse.conf:2: invalid octal escape' \
    '%config-error <SCRATCH>/fixtures/ordering/missing-root.conf: No such file or directory' \
    '%config-error <SCRATCH>/fixtures/ordering/root-one.conf:1: ROOT_ONE' \
    '%config-error <SCRATCH>/fixtures/ordering/root-two.conf:2: ROOT_TWO_BEFORE' \
    "%config-error <SCRATCH>/fixtures/ordering/root-two.conf:3: can't find session: missing-runtime" \
    '%config-error <SCRATCH>/fixtures/ordering/nested.conf:1: NESTED_BEFORE' \
    '%config-error <SCRATCH>/fixtures/ordering/nested.conf:2: No such file or directory: <SCRATCH>/fixtures/ordering/nested-missing.conf' \
    '%config-error <SCRATCH>/fixtures/ordering/nested.conf:3: NESTED_AFTER' \
    '%config-error <SCRATCH>/fixtures/ordering/root-two.conf:5: ROOT_TWO_AFTER' \
    '%begin TIME NUMBER 0' \
    '%end TIME NUMBER 0' \
    '%exit' >"$expected"

  for side in zz tmux; do
    prepare_target "$side" ordering
    target="$SIDE_TARGET"
    stdout="$OUTPUT_DIR/ordering.$side.stdout"
    stderr="$OUTPUT_DIR/ordering.$side.stderr"
    capture_side "$side" "$target" "$stdout" "$stderr" \
      -f "$root_one" -f "$root_parse" -f "$missing_root" -f "$root_two" \
      -C new-session -d -s order-command
    assert_rc "$side" 0
    assert_empty "$side" stderr "$stderr"
    assert_control "$side" "$stdout" "$expected"
    stop_side "$side" "$target"
  done
  assert_files_equal "ordering differential" \
    "$OUTPUT_DIR/ordering.zz.stdout.normalized" \
    "$OUTPUT_DIR/ordering.tmux.stdout.normalized"
  pass_case
}

case_multiline_cause() {
  local directory="$FIXTURE_DIR/multiline"
  local root="$directory/root.conf"
  local expected="$OUTPUT_DIR/multiline.expected"
  local side
  local target
  local stdout
  local stderr
  local prefixes

  CURRENT_CASE="multiline cause prefixing"
  mkdir -p "$directory"
  printf 'display-message -p "MULTI_FIRST\nMULTI_SECOND"\n' >"$root"
  printf '%s\n' \
    '%config-error <SCRATCH>/fixtures/multiline/root.conf:2: MULTI_FIRST' \
    'MULTI_SECOND' \
    '%begin TIME NUMBER 0' \
    '%end TIME NUMBER 0' \
    '%exit' >"$expected"

  for side in zz tmux; do
    prepare_target "$side" multiline
    target="$SIDE_TARGET"
    stdout="$OUTPUT_DIR/multiline.$side.stdout"
    stderr="$OUTPUT_DIR/multiline.$side.stderr"
    capture_side "$side" "$target" "$stdout" "$stderr" \
      -f "$root" -C new-session -d -s multiline-command
    assert_rc "$side" 0
    assert_empty "$side" stderr "$stderr"
    assert_control "$side" "$stdout" "$expected"
    prefixes="$(awk '/^%config-error / { count++ } END { print count + 0 }' \
      "$stdout.normalized")"
    if [ "$prefixes" -ne 1 ]; then
      fail "$side used $prefixes config-error prefixes for one multiline cause"
    fi
    stop_side "$side" "$target"
  done
  assert_files_equal "multiline differential" \
    "$OUTPUT_DIR/multiline.zz.stdout.normalized" \
    "$OUTPUT_DIR/multiline.tmux.stdout.normalized"
  pass_case
}

case_restart_redelivery() {
  local directory="$FIXTURE_DIR/restart"
  local root="$directory/root.conf"
  local child="$directory/child.conf"
  local expected="$OUTPUT_DIR/restart.expected"
  local side
  local target
  local generation
  local stdout
  local stderr

  CURRENT_CASE="daemon restart redelivery"
  mkdir -p "$directory"
  printf '%s\n' 'display-message -p RESTART_NESTED' >"$child"
  printf "display-message -p RESTART_DIRECT\nsource-file '%s'\n" "$child" >"$root"
  printf '%s\n' \
    '%begin TIME NUMBER 0' \
    '%config-error <SCRATCH>/fixtures/restart/root.conf:1: RESTART_DIRECT' \
    '%config-error <SCRATCH>/fixtures/restart/child.conf:1: RESTART_NESTED' \
    '%end TIME NUMBER 0' \
    '%exit' >"$expected"

  for side in zz tmux; do
    prepare_target "$side" restart
    target="$SIDE_TARGET"
    for generation in 1 2; do
      stdout="$OUTPUT_DIR/restart.$side.$generation.start.stdout"
      stderr="$OUTPUT_DIR/restart.$side.$generation.start.stderr"
      capture_side "$side" "$target" "$stdout" "$stderr" \
        -f "$root" new-session -d -s restart-session
      assert_rc "$side" 0
      assert_empty "$side" stdout "$stdout"
      assert_empty "$side" stderr "$stderr"

      stdout="$OUTPUT_DIR/restart.$side.$generation.attach.stdout"
      stderr="$OUTPUT_DIR/restart.$side.$generation.attach.stderr"
      capture_side "$side" "$target" "$stdout" "$stderr" \
        -C attach-session -t =restart-session
      assert_rc "$side" 0
      assert_empty "$side" stderr "$stderr"
      assert_control "$side" "$stdout" "$expected"
      stop_side "$side" "$target"
    done
    assert_files_equal "$side restart generations" \
      "$OUTPUT_DIR/restart.$side.1.attach.stdout.normalized" \
      "$OUTPUT_DIR/restart.$side.2.attach.stdout.normalized"
  done
  assert_files_equal "restart first differential" \
    "$OUTPUT_DIR/restart.zz.1.attach.stdout.normalized" \
    "$OUTPUT_DIR/restart.tmux.1.attach.stdout.normalized"
  assert_files_equal "restart second differential" \
    "$OUTPUT_DIR/restart.zz.2.attach.stdout.normalized" \
    "$OUTPUT_DIR/restart.tmux.2.attach.stdout.normalized"
  pass_case
}

write_interactive_runner() {
  local side="$1"
  local target="$2"
  local destination="$3"

  printf '#!/usr/bin/env bash\n' >"$destination"
  if [ "$side" = zz ]; then
    printf 'exec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL LC_ALL=C HOME=%q XDG_CONFIG_HOME=%q TMUX_TMPDIR=/tmp %q --socket %q attach-session -t =interactive-session\n' \
      "$ZZ_HOME" "$ZZ_CONFIG_HOME" "$ZZ_BIN" "$target" >>"$destination"
  else
    printf 'exec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL LC_ALL=C HOME=%q XDG_CONFIG_HOME=%q TMUX_TMPDIR=/tmp %q -L %q attach-session -t =interactive-session\n' \
      "$TMUX_HOME" "$TMUX_CONFIG_HOME" "$TMUX_BIN" "$target" >>"$destination"
  fi
  chmod +x "$destination"
}

interactive_mode_value() {
  local side="$1"
  local target="$2"

  if [ "$side" = zz ]; then
    run_side_with_timeout "$PROBE_TIMEOUT_SECONDS" zz "$target" \
      list-clients -F '#{client_session}|#{client_key_table}' 2>/dev/null || true
  else
    run_side_with_timeout "$PROBE_TIMEOUT_SECONDS" tmux "$target" \
      display-message -p -t =interactive-session:0.0 '#{pane_in_mode}' \
      2>/dev/null || true
  fi
}

wait_for_interactive_mode() {
  local side="$1"
  local target="$2"
  local expected
  local actual=""
  local deadline

  if [ "$side" = zz ]; then
    expected='interactive-session|copy-mode'
  else
    expected=1
  fi
  deadline=$((SECONDS + POLL_DEADLINE_SECONDS))
  while [ "$SECONDS" -lt "$deadline" ]; do
    actual="$(interactive_mode_value "$side" "$target")"
    if [ "$actual" = "$expected" ]; then
      return 0
    fi
    sleep 0.05
  done
  fail "$side interactive mode was ${actual:-<empty>}, expected $expected"
}

wait_for_located_rows() {
  local side="$1"
  local outer="$2"
  local direct="$3"
  local nested="$4"
  local screen="$OUTPUT_DIR/interactive.$side.screen"
  local positions
  local deadline

  deadline=$((SECONDS + POLL_DEADLINE_SECONDS))
  while [ "$SECONDS" -lt "$deadline" ]; do
    run_outer_with_timeout "$PROBE_TIMEOUT_SECONDS" "$outer" \
      capture-pane -p -S - -t "=driver:$side" >"$screen" 2>/dev/null || true
    positions="$(awk -v direct="$direct" -v nested="$nested" '
      index($0, direct) { direct_count++; direct_line = NR }
      index($0, nested) { nested_count++; nested_line = NR }
      END {
        if (direct_count == 1 && nested_count == 1 && direct_line < nested_line) {
          print direct_line ":" nested_line
        }
      }
    ' "$screen")"
    if [ -n "$positions" ]; then
      return 0
    fi
    sleep 0.05
  done
  printf '%s interactive screen:\n' "$side" >&2
  sed -n '1,120p' "$screen" >&2
  fail "$side did not show both located startup rows once and in order"
}

case_interactive_delivery() {
  local directory="$FIXTURE_DIR/interactive"
  local root="$directory/root.conf"
  local child="$directory/child.conf"
  local expected="$OUTPUT_DIR/interactive.control.expected"
  local outer="zzsdo-$TOKEN-interactive"
  local side
  local target
  local stdout
  local stderr
  local runner
  local direct_row="$root:1: INTERACTIVE_DIRECT"
  local nested_row="$child:1: INTERACTIVE_NESTED"
  local zz_target
  local tmux_target

  CURRENT_CASE="Interactive delivery and global drain"
  mkdir -p "$directory"
  printf '%s\n' 'display-message -p INTERACTIVE_NESTED' >"$child"
  printf "display-message -p INTERACTIVE_DIRECT\nsource-file '%s'\n" "$child" >"$root"
  printf '%s\n' \
    '%begin TIME NUMBER 0' \
    '%end TIME NUMBER 0' \
    '%exit' >"$expected"

  for side in zz tmux; do
    prepare_target "$side" interactive
    target="$SIDE_TARGET"
    if [ "$side" = zz ]; then
      zz_target="$target"
    else
      tmux_target="$target"
    fi
    stdout="$OUTPUT_DIR/interactive.$side.start.stdout"
    stderr="$OUTPUT_DIR/interactive.$side.start.stderr"
    capture_side "$side" "$target" "$stdout" "$stderr" \
      -f "$root" new-session -d -s interactive-session
    assert_rc "$side" 0
    assert_empty "$side" stdout "$stdout"
    assert_empty "$side" stderr "$stderr"
    runner="$SCRATCH_DIR/attach-$side"
    write_interactive_runner "$side" "$target" "$runner"
  done

  OUTER_LABELS+=("$outer")
  run_outer "$outer" -f /dev/null new-session -d -x 220 -y 30 \
    -s driver -n zz "$SCRATCH_DIR/attach-zz"
  run_outer "$outer" set-option -g remain-on-exit on
  run_outer "$outer" new-window -d -t =driver -n tmux "$SCRATCH_DIR/attach-tmux"

  wait_for_interactive_mode zz "$zz_target"
  wait_for_interactive_mode tmux "$tmux_target"
  wait_for_located_rows zz "$outer" "$direct_row" "$nested_row"
  wait_for_located_rows tmux "$outer" "$direct_row" "$nested_row"

  for side in zz tmux; do
    if [ "$side" = zz ]; then
      target="$zz_target"
    else
      target="$tmux_target"
    fi
    stdout="$OUTPUT_DIR/interactive.$side.control.stdout"
    stderr="$OUTPUT_DIR/interactive.$side.control.stderr"
    capture_side "$side" "$target" "$stdout" "$stderr" \
      -C attach-session -t =interactive-session
    assert_rc "$side" 0
    assert_empty "$side" stderr "$stderr"
    assert_control "$side" "$stdout" "$expected"
  done
  assert_files_equal "Interactive drain differential" \
    "$OUTPUT_DIR/interactive.zz.control.stdout.normalized" \
    "$OUTPUT_DIR/interactive.tmux.control.stdout.normalized"

  stop_side zz "$zz_target"
  stop_side tmux "$tmux_target"
  run_outer "$outer" kill-server >/dev/null 2>&1 || true
  pass_case
}

case_initial_control
case_detached_late_attach
case_list_output_discarded
case_failure_root_ordering
case_multiline_cause
case_restart_redelivery
case_interactive_delivery

CURRENT_CASE="completion"
if [ "$CASES_RUN" -ne "$EXPECTED_CASES" ]; then
  fail "ran $CASES_RUN cases, expected $EXPECTED_CASES"
fi
printf 'startup-diagnostics compatibility: PASS (%s/%s cases)\n' \
  "$CASES_RUN" "$EXPECTED_CASES"
