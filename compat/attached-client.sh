#!/usr/bin/env bash
set -eEuo pipefail
set +B

usage() {
  printf 'usage: compat/attached-client.sh [ZZ_BIN [TMUX_BIN]]\n' >&2
  printf '       ZZ_BIN=path TMUX_BIN=path compat/attached-client.sh\n' >&2
}

if [ "$#" -gt 2 ]; then
  usage
  exit 2
fi

ZZ_INPUT="${1:-${ZZ_BIN:-}}"
TMUX_INPUT="${2:-${TMUX_BIN:-}}"

if [ -z "$ZZ_INPUT" ] || [ -z "$TMUX_INPUT" ]; then
  usage
  exit 2
fi

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

TMUX_VERSION="$("$TMUX_BIN" -V 2>/dev/null || true)"
if [ "$TMUX_VERSION" != "tmux next-3.8" ]; then
  printf "error: tmux binary must report 'tmux next-3.8', got: %s\n" "${TMUX_VERSION:-<empty>}" >&2
  exit 2
fi

SCRATCH_DIR="$(mktemp -d /tmp/zza.XXXXXX)"
TOKEN="${SCRATCH_DIR##*.}"
ZZ_HOME="$SCRATCH_DIR/zz-home"
ZZ_CONFIG_HOME="$SCRATCH_DIR/zz-config"
TMUX_HOME="$SCRATCH_DIR/tmux-home"
TMUX_CONFIG_HOME="$SCRATCH_DIR/tmux-config"
OUTER_HOME="$SCRATCH_DIR/outer-home"
OUTER_CONFIG_HOME="$SCRATCH_DIR/outer-config"
ZZ_SOCKET="/tmp/zza-$TOKEN.sock"
INNER_SOCKET_NAME="zzai-$TOKEN"
OUTER_SOCKET_NAME="zzao-$TOKEN"
INNER_SESSION="attached"
CHOOSER_SESSION="chooser-target"
OUTER_SESSION="driver"
INNER_WINDOW_TARGET="=$INNER_SESSION:0"
INNER_PANE_TARGET="=$INNER_SESSION:0.0"
COMMAND_CWD="$SCRATCH_DIR/client [literal]*? cwd"
SESSION_CWD="$SCRATCH_DIR/session [literal]*? cwd"
ENV_SELECTED=ATTACHED_ENV_SELECTED
ENV_MISSING=ATTACHED_ENV_MISSING
ENV_EMPTY=ATTACHED_ENV_EMPTY
ENV_GLOB_A=ATTACHED_ENV_GLOB_A
ENV_GLOB_PATTERN='ATTACHED_ENV_GLOB_*'
ENV_HIDDEN=ATTACHED_ENV_HIDDEN
ENV_HIDDEN_KEEP=ATTACHED_ENV_HIDDEN_KEEP
ENV_KEEP=ATTACHED_ENV_KEEP
SOURCE_CONFIG_DIR="$SESSION_CWD/config-$INNER_SESSION"
SOURCE_CONFIG_DECOY_DIR="$COMMAND_CWD/config-$INNER_SESSION"
SOURCE_DEPTH_DIR="$SCRATCH_DIR/source-depth"
SOURCE_OUTPUT_DIR="$SCRATCH_DIR/o"
SOURCE_OUTPUT_CHILD="$SOURCE_OUTPUT_DIR/c"
SOURCE_OUTPUT_ROOT="$SOURCE_OUTPUT_DIR/r"
SOURCE_OUTPUT_NAVIGATION="$SOURCE_OUTPUT_DIR/n"
DAEMON_STDOUT="$SCRATCH_DIR/zz-daemon.stdout"
DAEMON_STDERR="$SCRATCH_DIR/zz-daemon.stderr"
ZZ_ATTACH="$SCRATCH_DIR/attach-zz"
TMUX_ATTACH="$SCRATCH_DIR/attach-tmux"
ZZ_PID=""

mkdir -p "$ZZ_HOME" "$ZZ_CONFIG_HOME" "$TMUX_HOME" "$TMUX_CONFIG_HOME" \
  "$OUTER_HOME" "$OUTER_CONFIG_HOME" "$SOURCE_CONFIG_DIR" \
  "$SOURCE_CONFIG_DECOY_DIR" "$SOURCE_DEPTH_DIR" "$SOURCE_OUTPUT_DIR"
printf 'set-option -g @attached_source_order glob-first\n' >"$SOURCE_CONFIG_DIR/10.conf"
printf 'set-option -g @attached_source_order glob-second\n' >"$SOURCE_CONFIG_DIR/20.conf"
printf 'set-option -g @attached_source_order client-decoy-first\n' \
  >"$SOURCE_CONFIG_DECOY_DIR/10.conf"
printf 'set-option -g @attached_source_order client-decoy-second\n' \
  >"$SOURCE_CONFIG_DECOY_DIR/20.conf"

printf 'set-option -g @attached_depth_leaf yes\n' >"$SOURCE_DEPTH_DIR/leaf.conf"
printf 'display-message -p ATTACHED_CHILD_ONE\nlist-sessions -F "ATTACHED_CHILD_LIST_#{session_name}"\n' \
  >"$SOURCE_OUTPUT_CHILD"
printf 'display-message -p ATTACHED_ROOT_ONE\nsource-file -v %s\ndisplay-message -p ATTACHED_ROOT_TWO\n' \
  "$SOURCE_OUTPUT_CHILD" >"$SOURCE_OUTPUT_ROOT"
for ((navigation_line = 0; navigation_line < 96; navigation_line++)); do
  if [ "$navigation_line" -eq 35 ] || [ "$navigation_line" -eq 65 ] || \
    [ "$navigation_line" -eq 85 ]; then
    printf 'display-message -p "ATTACHED_NAV_%02d ATTACHED_NAV_MATCH"\n' \
      "$navigation_line"
  else
    printf 'display-message -p ATTACHED_NAV_%02d\n' "$navigation_line"
  fi
done >"$SOURCE_OUTPUT_NAVIGATION"
for ((depth_level = 1; depth_level <= 50; depth_level++)); do
  {
    printf 'set-option -g @attached_depth %s\n' "$depth_level"
    if [ "$depth_level" -lt 50 ]; then
      printf 'source-file %s/f%s.conf\n' "$SOURCE_DEPTH_DIR" "$((depth_level + 1))"
    else
      printf 'source-file %s/leaf.conf\n' "$SOURCE_DEPTH_DIR"
    fi
    printf 'set-option -g @attached_after%s yes\n' "$depth_level"
  } >"$SOURCE_DEPTH_DIR/f$depth_level.conf"
done

zz_command() {
  env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    -u EDITOR -u VISUAL \
    HOME="$ZZ_HOME" XDG_CONFIG_HOME="$ZZ_CONFIG_HOME" TMUX_TMPDIR=/tmp \
    "$ZZ_BIN" --socket "$ZZ_SOCKET" "$@"
}

tmux_inner_command() {
  env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    -u EDITOR -u VISUAL \
    HOME="$TMUX_HOME" XDG_CONFIG_HOME="$TMUX_CONFIG_HOME" TMUX_TMPDIR=/tmp \
    "$TMUX_BIN" -L "$INNER_SOCKET_NAME" "$@"
}

tmux_inner_start() {
  env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    -u EDITOR -u VISUAL \
    HOME="$TMUX_HOME" XDG_CONFIG_HOME="$TMUX_CONFIG_HOME" TMUX_TMPDIR=/tmp \
    "$TMUX_BIN" -L "$INNER_SOCKET_NAME" -f /dev/null "$@"
}

tmux_outer_command() {
  env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    -u EDITOR -u VISUAL \
    HOME="$OUTER_HOME" XDG_CONFIG_HOME="$OUTER_CONFIG_HOME" TMUX_TMPDIR=/tmp \
    "$TMUX_BIN" -L "$OUTER_SOCKET_NAME" "$@"
}

tmux_outer_start() {
  env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    -u EDITOR -u VISUAL \
    HOME="$OUTER_HOME" XDG_CONFIG_HOME="$OUTER_CONFIG_HOME" TMUX_TMPDIR=/tmp \
    "$TMUX_BIN" -L "$OUTER_SOCKET_NAME" -f /dev/null "$@"
}

side_command() {
  local side="$1"
  shift

  case "$side" in
  zz) zz_command "$@" ;;
  tmux) tmux_inner_command "$@" ;;
  *) return 2 ;;
  esac
}

environment_side_command() {
  local side="$1"
  local environment="$2"
  shift 2
  local -a values=(
    -u "$ENV_SELECTED"
    -u "$ENV_MISSING"
    -u "$ENV_EMPTY"
    -u "$ENV_GLOB_A"
    -u "$ENV_HIDDEN"
    -u "$ENV_HIDDEN_KEEP"
    -u "$ENV_KEEP"
  )

  case "$environment" in
  seed)
    values+=(
      "$ENV_SELECTED=seed"
      "$ENV_EMPTY="
      "$ENV_GLOB_A=glob-a"
      "$ENV_HIDDEN=seed-hidden"
      "$ENV_KEEP=client-only"
    )
    ;;
  control)
    values+=(
      "$ENV_SELECTED=control"
      "$ENV_EMPTY="
      "$ENV_GLOB_A=glob-a2"
      "$ENV_HIDDEN=revealed"
    )
    ;;
  changed)
    values+=(
      "$ENV_SELECTED=changed"
      "$ENV_EMPTY="
      "$ENV_GLOB_A=glob-changed"
      "$ENV_HIDDEN=changed-hidden"
    )
    ;;
  client-a)
    values+=("$ENV_SELECTED=client-A")
    ;;
  external)
    values+=("$ENV_SELECTED=external-caller")
    ;;
  missing) ;;
  *) return 2 ;;
  esac

  case "$side" in
  zz)
    env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
      -u EDITOR -u VISUAL "${values[@]}" \
      HOME="$ZZ_HOME" XDG_CONFIG_HOME="$ZZ_CONFIG_HOME" TMUX_TMPDIR=/tmp \
      "$ZZ_BIN" --socket "$ZZ_SOCKET" "$@"
    ;;
  tmux)
    env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
      -u EDITOR -u VISUAL "${values[@]}" \
      HOME="$TMUX_HOME" XDG_CONFIG_HOME="$TMUX_CONFIG_HOME" TMUX_TMPDIR=/tmp \
      "$TMUX_BIN" -L "$INNER_SOCKET_NAME" "$@"
    ;;
  *) return 2 ;;
  esac
}

capture_screen() {
  local side="$1"
  tmux_outer_command capture-pane -p -S - -t "=$OUTER_SESSION:$side"
}

capture_current_screen() {
  local side="$1"
  tmux_outer_command capture-pane -p -t "=$OUTER_SESSION:$side"
}

dump_screen() {
  local side="$1"
  local screen

  printf '%s screen:\n' "$side" >&2
  if screen="$(capture_screen "$side" 2>/dev/null)"; then
    if [ -n "$screen" ]; then
      printf '%s\n' "$screen" >&2
    else
      printf '<empty>\n' >&2
    fi
  else
    printf '<unavailable>\n' >&2
  fi
}

dump_failure() {
  dump_screen zz
  dump_screen tmux
  printf 'zz daemon stderr:\n' >&2
  if [ -s "$DAEMON_STDERR" ]; then
    sed 's/^/    /' "$DAEMON_STDERR" >&2
  else
    printf '    <empty>\n' >&2
  fi
}

fixture_failure() {
  local message="$1"
  trap - ERR
  set +e
  printf 'error: %s\n' "$message" >&2
  dump_failure
  exit 1
}

unexpected_failure() {
  local line="$1"
  local status="$2"
  trap - ERR
  set +e
  printf 'error: unexpected failure at line %s (exit %s)\n' "$line" "$status" >&2
  dump_failure
  exit "$status"
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
  rm -f -- "$ZZ_SOCKET" \
    "/tmp/tmux-$(id -u)/$OUTER_SOCKET_NAME" \
    "/tmp/tmux-$(id -u)/$INNER_SOCKET_NAME"
  rm -rf -- "$SCRATCH_DIR"
  exit "$status"
}

trap cleanup EXIT
trap 'unexpected_failure "$LINENO" "$?"' ERR
trap 'exit 130' INT
trap 'exit 143' TERM

write_attach() {
  local side="$1"
  local destination="$2"
  local session="${3:-$INNER_SESSION}"
  local selected="${4:-root-client}"

  printf '#!/usr/bin/env bash\n' >"$destination"
  printf 'cd -- %q\n' "$COMMAND_CWD" >>"$destination"
  if [ "$side" = "zz" ]; then
    printf 'exec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL %q=%q %q= %q=%q %q=%q HOME=%q XDG_CONFIG_HOME=%q TMUX_TMPDIR=/tmp %q --socket %q attach-session -c %q -t %q\n' \
      "$ENV_SELECTED" "$selected" "$ENV_EMPTY" "$ENV_GLOB_A" root-glob \
      "$ENV_HIDDEN" root-hidden "$ZZ_HOME" "$ZZ_CONFIG_HOME" "$ZZ_BIN" \
      "$ZZ_SOCKET" "$SESSION_CWD" "=$session" >>"$destination"
  else
    printf 'exec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL %q=%q %q= %q=%q %q=%q HOME=%q XDG_CONFIG_HOME=%q TMUX_TMPDIR=/tmp %q -L %q attach-session -c %q -t %q\n' \
      "$ENV_SELECTED" "$selected" "$ENV_EMPTY" "$ENV_GLOB_A" root-glob \
      "$ENV_HIDDEN" root-hidden "$TMUX_HOME" "$TMUX_CONFIG_HOME" "$TMUX_BIN" \
      "$INNER_SOCKET_NAME" "$SESSION_CWD" "=$session" >>"$destination"
  fi
  chmod +x "$destination"
}

wait_for_socket() {
  local attempt

  for ((attempt = 0; attempt < 200; attempt++)); do
    if [ -S "$ZZ_SOCKET" ]; then
      return 0
    fi
    if ! kill -0 "$ZZ_PID" 2>/dev/null; then
      fixture_failure "zz daemon exited before creating $ZZ_SOCKET"
    fi
    sleep 0.05
  done
  fixture_failure "zz daemon did not create $ZZ_SOCKET within 10 seconds"
}

LAST_CLIENT_STATE=""
wait_for_client_state() {
  local side="$1"
  local expected="$2"
  local attempt

  for ((attempt = 0; attempt < 200; attempt++)); do
    LAST_CLIENT_STATE="$(side_command "$side" list-clients -F '#{client_session}|#{client_key_table}' 2>/dev/null || true)"
    if [ "$LAST_CLIENT_STATE" = "$INNER_SESSION|$expected" ]; then
      return 0
    fi
    sleep 0.05
  done
  fixture_failure "$side client state did not become $expected within 10 seconds; last state: ${LAST_CLIENT_STATE:-<empty>}"
}

requested_client_state() {
  local side="$1"
  local output
  local session
  local flags
  local token
  local requested=""
  local -a tokens

  output="$(side_command "$side" list-clients -F '#{client_session}|#{client_flags}' 2>/dev/null || true)"
  if [ -z "$output" ] || [[ "$output" == *$'\n'* ]]; then
    printf '%s\n' "$output"
    return
  fi
  IFS='|' read -r session flags <<<"$output"
  IFS=',' read -r -a tokens <<<"$flags"
  for token in "${tokens[@]}"; do
    case "$token" in
    attached | focused | UTF-8) ;;
    *)
      requested="${requested:+$requested,}$token"
      ;;
    esac
  done
  printf '%s|%s\n' "$session" "$requested"
}

LAST_REQUESTED_CLIENT_STATE=""
wait_for_requested_client_state() {
  local side="$1"
  local expected="$2"
  local attempt

  for ((attempt = 0; attempt < 200; attempt++)); do
    LAST_REQUESTED_CLIENT_STATE="$(requested_client_state "$side")"
    if [ "$LAST_REQUESTED_CLIENT_STATE" = "$expected" ]; then
      return 0
    fi
    sleep 0.05
  done
  fixture_failure "$side requested client state did not become $expected within 10 seconds; last state: ${LAST_REQUESTED_CLIENT_STATE:-<empty>}"
}

wait_for_session_name() {
  local side="$1"
  local expected="$2"
  local attempt

  for ((attempt = 0; attempt < 200; attempt++)); do
    if side_command "$side" list-sessions -F '#{session_name}' 2>/dev/null |
      grep -Fxq -- "$expected"; then
      return 0
    fi
    sleep 0.05
  done
  fixture_failure "$side session $expected did not appear within 10 seconds"
}

attached_client_count() {
  local side="$1"

  side_command "$side" list-clients -F '#{client_session}' 2>/dev/null |
    awk -v session="$INNER_SESSION" '$0 == session { count++ } END { print count + 0 }'
}

wait_for_attached_client_count() {
  local side="$1"
  local expected="$2"
  local attempt
  local count

  for ((attempt = 0; attempt < 200; attempt++)); do
    count="$(attached_client_count "$side" || true)"
    if [ "$count" = "$expected" ]; then
      return 0
    fi
    sleep 0.05
  done
  fixture_failure "$side attached client count did not become $expected within 10 seconds; last count: ${count:-0}"
}

LAST_SESSION_CLIENT_SIZES=""
wait_for_session_client_sizes() {
  local side="$1"
  local session="$2"
  local expected="$3"
  local attempt

  for ((attempt = 0; attempt < 200; attempt++)); do
    LAST_SESSION_CLIENT_SIZES="$(
      side_command "$side" list-clients -t "=$session" \
        -F '#{client_width}x#{client_height}' 2>/dev/null | sort -n || true
    )"
    if [ "$LAST_SESSION_CLIENT_SIZES" = "$expected" ]; then
      return 0
    fi
    sleep 0.05
  done
  fixture_failure "$side session $session client sizes did not become $expected within 10 seconds; last sizes: ${LAST_SESSION_CLIENT_SIZES:-<empty>}"
}

wait_for_outer_pane_size() {
  local target="$1"
  local expected="$2"
  local attempt
  local size

  for ((attempt = 0; attempt < 200; attempt++)); do
    size="$(tmux_outer_command display-message -p -t "$target" '#{pane_width}x#{pane_height}' 2>/dev/null || true)"
    if [ "$size" = "$expected" ]; then
      return 0
    fi
    sleep 0.05
  done
  fixture_failure "outer pane $target did not become $expected within 10 seconds; last size: ${size:-<empty>}"
}

assert_attached_client_count_stays() {
  local side="$1"
  local expected="$2"
  local attempt
  local count

  for ((attempt = 0; attempt < 20; attempt++)); do
    count="$(attached_client_count "$side" || true)"
    if [ "$count" != "$expected" ]; then
      fixture_failure "$side attached client count changed during settle; expected $expected, got ${count:-0}"
    fi
    sleep 0.05
  done
}

LAST_MODE_STATE=""
wait_for_mode_state() {
  local side="$1"
  local expected="$2"
  local expected_value
  local attempt

  if [ "$side" = "zz" ]; then
    wait_for_client_state zz "$expected"
    return 0
  fi

  if [ "$expected" = "copy-mode" ]; then
    expected_value=1
  else
    expected_value=0
  fi

  for ((attempt = 0; attempt < 200; attempt++)); do
    LAST_MODE_STATE="$(tmux_inner_command display-message -p -t "$INNER_SESSION:0.0" '#{pane_in_mode}' 2>/dev/null || true)"
    if [ "$LAST_MODE_STATE" = "$expected_value" ]; then
      return 0
    fi
    sleep 0.05
  done
  fixture_failure "tmux pane mode did not become $expected within 10 seconds; last state: ${LAST_MODE_STATE:-<empty>}"
}

wait_for_output_mode() {
  local side="$1"
  local table="$2"

  if [ "$side" = "zz" ]; then
    wait_for_client_state zz "$table"
  else
    wait_for_mode_state tmux copy-mode
  fi
}

assert_output_mode_stays() {
  local side="$1"
  local table="$2"
  local expected
  local actual
  local attempt

  for ((attempt = 0; attempt < 20; attempt++)); do
    if [ "$side" = "zz" ]; then
      expected="$INNER_SESSION|$table"
      actual="$(side_command zz list-clients -F '#{client_session}|#{client_key_table}' 2>/dev/null || true)"
    else
      expected=1
      actual="$(side_command tmux display-message -p -t "$INNER_PANE_TARGET" '#{pane_in_mode}' 2>/dev/null || true)"
    fi
    if [ "$actual" != "$expected" ]; then
      fixture_failure "$side command-output mode changed during settle; expected $expected, got ${actual:-<empty>}"
    fi
    sleep 0.05
  done
}

wait_for_visible_mode() {
  local side="$1"
  local expected="$2"
  local pattern
  local attempt
  local screen

  case "$side" in
  zz) pattern='(^|[[:space:]])COPY([[:space:]][0-9]+/[0-9]+)?([[:space:]]|$)' ;;
  tmux) pattern='\[[0-9]+/[0-9]+\]' ;;
  *) return 2 ;;
  esac

  for ((attempt = 0; attempt < 200; attempt++)); do
    screen="$(capture_current_screen "$side" 2>/dev/null || true)"
    if [ "$expected" = "copy-mode" ]; then
      if grep -Eq -- "$pattern" <<<"$screen"; then
        return 0
      fi
    elif ! grep -Eq -- "$pattern" <<<"$screen"; then
      return 0
    fi
    sleep 0.05
  done
  fixture_failure "$side screen did not visibly become $expected within 10 seconds"
}

wait_for_marker() {
  local side="$1"
  local marker="$2"
  local attempt
  local screen

  for ((attempt = 0; attempt < 200; attempt++)); do
    screen="$(capture_screen "$side" 2>/dev/null || true)"
    if grep -Fq -- "$marker" <<<"$screen"; then
      return 0
    fi
    sleep 0.05
  done
  fixture_failure "$side screen did not show $marker within 10 seconds"
}

wait_for_current_marker() {
  local side="$1"
  local marker="$2"
  local attempt
  local screen

  for ((attempt = 0; attempt < 200; attempt++)); do
    screen="$(capture_current_screen "$side" 2>/dev/null || true)"
    if grep -Fq -- "$marker" <<<"$screen"; then
      return 0
    fi
    sleep 0.05
  done
  fixture_failure "$side current screen did not show $marker within 10 seconds"
}

wait_for_current_marker_absent() {
  local side="$1"
  local marker="$2"
  local attempt
  local screen

  for ((attempt = 0; attempt < 200; attempt++)); do
    screen="$(capture_current_screen "$side" 2>/dev/null || true)"
    if ! grep -Fq -- "$marker" <<<"$screen"; then
      return 0
    fi
    sleep 0.05
  done
  fixture_failure "$side current screen still showed $marker after 10 seconds"
}

assert_current_marker_absent_for() {
  local side="$1"
  local marker="$2"
  local attempts="$3"
  local attempt
  local screen

  for ((attempt = 0; attempt < attempts; attempt++)); do
    screen="$(capture_current_screen "$side" 2>/dev/null || true)"
    if grep -Fq -- "$marker" <<<"$screen"; then
      fixture_failure "$side current screen exposed $marker while its alert was active"
    fi
    sleep 0.05
  done
}

wait_for_ordered_current_lines() {
  local side="$1"
  shift
  local attempt
  local marker
  local line
  local previous
  local screen
  local ordered

  for ((attempt = 0; attempt < 200; attempt++)); do
    screen="$(capture_current_screen "$side" 2>/dev/null || true)"
    previous=0
    ordered=1
    for marker in "$@"; do
      line="$(awk -v marker="$marker" -v after="$previous" -v side="$side" '
        {
          line = $0
          sub(/\r$/, "", line)
          sub(/^[[:space:]]+/, "", line)
          if (side == "tmux") sub(/[[:space:]]+\[[0-9]+\/[0-9]+\]$/, "", line)
          sub(/[[:space:]]+$/, "", line)
          if (line == marker) {
            count++
            if (NR > after && found == 0) found = NR
          }
        }
        END {
          if (count == 1 && found != 0) print found
        }
      ' <<<"$screen")"
      if [ -z "$line" ]; then
        ordered=0
        break
      fi
      previous="$line"
    done
    if [ "$ordered" -eq 1 ]; then
      return 0
    fi
    sleep 0.05
  done
  fixture_failure "$side command-output view did not show one ordered replay transcript"
}

LAST_SIDE_OUTPUT=""
wait_for_side_output() {
  local side="$1"
  local expected="$2"
  local label="$3"
  local attempt
  shift 3

  for ((attempt = 0; attempt < 200; attempt++)); do
    LAST_SIDE_OUTPUT="$(side_command "$side" "$@" 2>/dev/null || true)"
    if [ "$LAST_SIDE_OUTPUT" = "$expected" ]; then
      return 0
    fi
    sleep 0.05
  done
  fixture_failure "$side $label did not become $expected within 10 seconds; last output: ${LAST_SIDE_OUTPUT:-<empty>}"
}

wait_for_pane_marker() {
  local side="$1"
  local marker="$2"
  local attempt
  local captured

  for ((attempt = 0; attempt < 200; attempt++)); do
    captured="$(side_command "$side" capture-pane -p -J -S - -t "$INNER_PANE_TARGET" 2>/dev/null || true)"
    if grep -Fq -- "$marker" <<<"$captured"; then
      return 0
    fi
    sleep 0.05
  done
  fixture_failure "$side pane did not contain $marker within 10 seconds"
}

pane_flattened_substring_count() {
  local side="$1"
  local marker="$2"
  local captured

  captured="$(side_command "$side" capture-pane -p -J -S - -t "$INNER_PANE_TARGET" 2>/dev/null || true)"
  awk -v marker="$marker" '
    { text = text (NR == 1 ? "" : " ") $0 }
    END {
      gsub(/[[:space:]]+/, "", text)
      gsub(/[[:space:]]+/, "", marker)
      while ((position = index(text, marker)) != 0) {
        count++
        text = substr(text, position + length(marker))
      }
      print count + 0
    }
  ' <<<"$captured"
}

wait_for_new_pane_flattened_substring() {
  local side="$1"
  local marker="$2"
  local baseline="$3"
  local attempt
  local count

  for ((attempt = 0; attempt < 200; attempt++)); do
    count="$(pane_flattened_substring_count "$side" "$marker")"
    if [ "$count" -gt "$baseline" ]; then
      return 0
    fi
    sleep 0.05
  done
  fixture_failure "$side pane did not add flattened text containing $marker within 10 seconds"
}

wait_for_terminal_ready() {
  local side="$1"
  local attempt
  local baseline
  local count

  baseline="$(pane_flattened_substring_count "$side" ATTACHED_TERMINAL_READY)"

  for ((attempt = 0; attempt < 200; attempt++)); do
    tmux_outer_command send-keys -t "$OUTER_SESSION:$side" C-u F12 Enter
    count="$(pane_flattened_substring_count "$side" ATTACHED_TERMINAL_READY)"
    if [ "$count" -gt "$baseline" ]; then
      return 0
    fi
    sleep 0.05
  done
  fixture_failure "$side terminal did not accept a fresh readiness binding within 10 seconds"
}

assert_buffer_parity() {
  local expected="$1"
  local zz_buffers
  local tmux_buffers

  zz_buffers="$(zz_command list-buffers -F '#{buffer_name}')"
  tmux_buffers="$(tmux_inner_command list-buffers -F '#{buffer_name}')"
  if [ "$zz_buffers" != "$tmux_buffers" ]; then
    fixture_failure "buffer lists diverged; zz=${zz_buffers:-<empty>} tmux=${tmux_buffers:-<empty>}"
  fi
  if [ "$zz_buffers" != "$expected" ]; then
    fixture_failure "buffer list did not match expected names; got: ${zz_buffers:-<empty>}"
  fi
}

configure_side() {
  local side="$1"

  side_command "$side" set-option -g prefix2 C-a || return
  side_command "$side" bind-key -n C-g send-keys -l 'printf ATTACHED_ROOT_OK' || return
  side_command "$side" bind-key -n F12 send-keys -l 'printf ATTACHED_TERMINAL_READY' || return
  side_command "$side" bind-key -n F11 send-keys -l $'printf \'%x\\n\' 1515870810\n' || return
  side_command "$side" bind-key x send-keys -l 'printf ATTACHED_PREFIX_OK' || return
  side_command "$side" bind-key y send-keys -l 'printf ATTACHED_PREFIX2_OK' || return
  side_command "$side" bind-key t choose-tree -s -K '#{line}' -O index || return
  side_command "$side" bind-key -n F10 source-file -F \
    'config-#{session_name}/[12]0.conf' || return
  side_command "$side" bind-key -n F9 source-file -F \
    'config-#{session_name}/20.conf' 'config-#{session_name}/10.conf' || return
  side_command "$side" bind-key -n F8 source-file -Fq \
    'config-#{session_name}/missing.conf' 'config-#{session_name}/20.conf' || return
  side_command "$side" bind-key -n F7 source-file -F \
    'config-#{session_name}/missing.conf' || return
  side_command "$side" bind-key -n F6 source-file "$SOURCE_DEPTH_DIR/f1.conf" ||
    return
  side_command "$side" bind-key -n F5 choose-tree -s -f 0 || return
  side_command "$side" bind-key -n F4 choose-buffer -f 0 || return
  side_command "$side" bind-key -n F3 source-file -v "$SOURCE_OUTPUT_ROOT" || return
  side_command "$side" bind-key -n F2 source-file "$SOURCE_OUTPUT_NAVIGATION" || return
  side_command "$side" bind-key -N ZZLK1 -T attached-list a display-message unused || return
  side_command "$side" bind-key Q command-prompt -p ATTACHED_LIST_KEYS_PROMPT || return
  side_command "$side" bind-key R send-keys -l \
    'printf ATTACHED_LIST_KEYS_RESUMED' || return
  side_command "$side" bind-key B send-keys -l \
    'printf ATTACHED_LIST_KEYS_BASE' || return
}

probe_side() {
  local side="$1"

  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" C-g Enter
  wait_for_marker "$side" ATTACHED_ROOT_OK
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" C-b x Enter
  wait_for_marker "$side" ATTACHED_PREFIX_OK
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" C-a y Enter
  wait_for_marker "$side" ATTACHED_PREFIX2_OK
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" C-b '['
  wait_for_mode_state "$side" copy-mode
  wait_for_visible_mode "$side" copy-mode
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" q
  wait_for_mode_state "$side" root
  wait_for_visible_mode "$side" root
}

wait_for_alert_release() {
  local side="$1"
  local marker="$2"
  local ready_baseline="$3"
  local attempt
  local ready_count
  local screen

  for ((attempt = 0; attempt < 100; attempt++)); do
    ready_count="$(pane_flattened_substring_count "$side" ATTACHED_TERMINAL_READY)"
    screen="$(capture_current_screen "$side" 2>/dev/null || true)"
    if [ "$ready_count" -gt "$ready_baseline" ] && grep -Fq -- "$marker" <<<"$screen"; then
      return 0
    fi
    sleep 0.02
  done
  fixture_failure "$side attached input did not dismiss its alert and release $marker"
}

probe_alert_message_lifecycle() {
  local side="$1"
  local alert_window="=$INNER_SESSION:9"
  local alert_pane="=$INNER_SESSION:9.0"
  local zero_alert_window="=$INNER_SESSION:10"
  local zero_alert_pane="=$INNER_SESSION:10.0"
  local timed_marker="ATTACHED_ALERT_TIMED_OUTPUT_$side"
  local repeated_marker="ATTACHED_ALERT_REPEATED_OUTPUT_$side"
  local zero_marker="ATTACHED_ALERT_ZERO_OUTPUT_$side"
  local alert_marker="Bell in window"
  local client_name
  local ready_baseline

  client_name="$(side_command "$side" list-clients -F '#{client_name}')"
  if [ -z "$client_name" ] || [[ "$client_name" == *$'\n'* ]]; then
    fixture_failure "$side did not report exactly one alert target client"
  fi
  side_command "$side" new-window -d -t "$alert_window" -n alert-lifecycle ||
    fixture_failure "$side could not create its alert lifecycle window"
  side_command "$side" new-window -d -t "$zero_alert_window" -n alert-zero ||
    fixture_failure "$side could not create its zero-duration alert window"
  side_command "$side" set-option -g display-time 5000 ||
    fixture_failure "$side could not set its alert display time"
  side_command "$side" set-option -g visual-bell on ||
    fixture_failure "$side could not enable its visual bell"
  side_command "$side" set-option -g bell-action any ||
    fixture_failure "$side could not enable its bell action"
  side_command "$side" set-window-option -t "$alert_window" monitor-bell on ||
    fixture_failure "$side could not monitor its alert window bell"
  side_command "$side" set-window-option -t "$zero_alert_window" monitor-bell on ||
    fixture_failure "$side could not monitor its zero-duration window bell"

  side_command "$side" display-message -d 1500 -N -c "$client_name" \
    -t "$INNER_PANE_TARGET" ATTACHED_ALERT_STICKY ||
    fixture_failure "$side could not arm its sticky message"
  sleep 0.2
  side_command "$side" send-keys -l -t "$alert_pane" "printf '\\007'" ||
    fixture_failure "$side could not type its positive alert trigger"
  side_command "$side" send-keys -t "$alert_pane" Enter ||
    fixture_failure "$side could not run its positive alert trigger"
  wait_for_current_marker "$side" "$alert_marker"
  side_command "$side" send-keys -l -t "$INNER_PANE_TARGET" "printf '$timed_marker\\n'" ||
    fixture_failure "$side could not type its timed alert output"
  side_command "$side" send-keys -t "$INNER_PANE_TARGET" Enter ||
    fixture_failure "$side could not run its timed alert output"
  wait_for_pane_marker "$side" "$timed_marker"
  assert_current_marker_absent_for "$side" "$timed_marker" 36

  ready_baseline="$(pane_flattened_substring_count "$side" ATTACHED_TERMINAL_READY)"
  tmux_outer_command send-keys -t "=$OUTER_SESSION:$side" F12 Enter
  wait_for_alert_release "$side" "$timed_marker" "$ready_baseline"
  wait_for_current_marker_absent "$side" "$alert_marker"

  wait_for_side_output "$side" 1 "alert window bell flag" \
    display-message -p -t "$alert_pane" '#{window_bell_flag}'
  side_command "$side" send-keys -l -t "$alert_pane" "printf '\\007'" ||
    fixture_failure "$side could not type its repeated alert trigger"
  side_command "$side" send-keys -t "$alert_pane" Enter ||
    fixture_failure "$side could not run its repeated alert trigger"
  wait_for_current_marker "$side" "$alert_marker"
  side_command "$side" send-keys -l -t "$INNER_PANE_TARGET" "printf '$repeated_marker\\n'" ||
    fixture_failure "$side could not type its repeated alert output"
  side_command "$side" send-keys -t "$INNER_PANE_TARGET" Enter ||
    fixture_failure "$side could not run its repeated alert output"
  wait_for_pane_marker "$side" "$repeated_marker"
  assert_current_marker_absent_for "$side" "$repeated_marker" 36

  ready_baseline="$(pane_flattened_substring_count "$side" ATTACHED_TERMINAL_READY)"
  tmux_outer_command send-keys -t "=$OUTER_SESSION:$side" F12 Enter
  wait_for_alert_release "$side" "$repeated_marker" "$ready_baseline"
  wait_for_current_marker_absent "$side" "$alert_marker"

  sleep 5.2
  side_command "$side" set-option -g display-time 0 ||
    fixture_failure "$side could not set zero alert duration"
  side_command "$side" send-keys -l -t "$zero_alert_pane" "printf '\\007'" ||
    fixture_failure "$side could not type its zero-duration alert trigger"
  side_command "$side" send-keys -t "$zero_alert_pane" Enter ||
    fixture_failure "$side could not run its zero-duration alert trigger"
  wait_for_current_marker "$side" "$alert_marker"
  side_command "$side" send-keys -l -t "$INNER_PANE_TARGET" "printf '$zero_marker\\n'" ||
    fixture_failure "$side could not type its zero-duration alert output"
  side_command "$side" send-keys -t "$INNER_PANE_TARGET" Enter ||
    fixture_failure "$side could not run its zero-duration alert output"
  wait_for_pane_marker "$side" "$zero_marker"
  assert_current_marker_absent_for "$side" "$zero_marker" 8

  ready_baseline="$(pane_flattened_substring_count "$side" ATTACHED_TERMINAL_READY)"
  tmux_outer_command send-keys -t "=$OUTER_SESSION:$side" F12 Enter
  wait_for_alert_release "$side" "$zero_marker" "$ready_baseline"
  wait_for_current_marker_absent "$side" "$alert_marker"

  side_command "$side" kill-window -t "$alert_window" ||
    fixture_failure "$side could not remove its alert lifecycle window"
  side_command "$side" kill-window -t "$zero_alert_window" ||
    fixture_failure "$side could not remove its zero-duration alert window"
  side_command "$side" set-option -gu display-time ||
    fixture_failure "$side could not restore inherited display-time"
  side_command "$side" set-option -gu visual-bell ||
    fixture_failure "$side could not restore inherited visual-bell"
  side_command "$side" set-option -gu bell-action ||
    fixture_failure "$side could not restore inherited bell-action"
}

probe_command_prompt() {
  local side="$1"

  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" C-b ','
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" BSpace BSpace BSpace BSpace
  tmux_outer_command send-keys -l -t "$OUTER_SESSION:$side" prompted
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" Enter
  wait_for_side_output "$side" prompted "prompt rename" \
    list-windows -t "$INNER_SESSION" -F '#{window_name}'
}

probe_choose_tree() {
  local side="$1"

  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" C-b t
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" 1
  wait_for_side_output "$side" "$CHOOSER_SESSION" "tree row-key switch" \
    list-clients -F '#{client_session}'
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" C-b t
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" 0
  wait_for_side_output "$side" "$INNER_SESSION" "tree row-key return" \
    list-clients -F '#{client_session}'
}

probe_chooser_filter_fallback() {
  local side="$1"

  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" F5
  wait_for_current_marker "$side" "filter: no matches"
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" q
  wait_for_current_marker_absent "$side" "filter: no matches"
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" F4
  wait_for_current_marker "$side" "filter: no matches"
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" q
  wait_for_terminal_ready "$side"
}

probe_choose_buffer_row() {
  local side="$1"

  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" C-b '='
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" 0
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" Enter
  wait_for_pane_marker "$side" ATTACHED_BUFFER_ROW_OK
}

probe_choose_buffer_delete() {
  local side="$1"

  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" C-b '='
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" d
  wait_for_side_output "$side" keep "buffer deletion" \
    list-buffers -F '#{buffer_name}'
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" q
  wait_for_terminal_ready "$side"
}

probe_nested_attach() {
  local side="$1"
  local nested_command
  local refusal_count
  local refusal_text="sessions should be nested with care, unset \$TMUX to force"

  if [ "$side" = "zz" ]; then
    printf -v nested_command '%q --socket %q attach-session -t %q' \
      "$ZZ_BIN" "$ZZ_SOCKET" "$INNER_SESSION"
  else
    printf -v nested_command '%q -L %q attach-session -t %q' \
      "$TMUX_BIN" "$INNER_SOCKET_NAME" "$INNER_SESSION"
  fi
  refusal_count="$(pane_flattened_substring_count "$side" "$refusal_text")"
  tmux_outer_command send-keys -l -t "$OUTER_SESSION:$side" "$nested_command"
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" Enter
  wait_for_new_pane_flattened_substring "$side" "$refusal_text" "$refusal_count"
  assert_attached_client_count_stays "$side" 1
  wait_for_client_state "$side" root
  wait_for_terminal_ready "$side"

  if [ "$side" = "zz" ]; then
    printf -v nested_command '%q --socket %q new-session -A -s %q' \
      "$ZZ_BIN" "$ZZ_SOCKET" "$INNER_SESSION"
  else
    printf -v nested_command '%q -L %q new-session -A -s %q' \
      "$TMUX_BIN" "$INNER_SOCKET_NAME" "$INNER_SESSION"
  fi
  refusal_count="$(pane_flattened_substring_count "$side" "$refusal_text")"
  tmux_outer_command send-keys -l -t "$OUTER_SESSION:$side" "$nested_command"
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" Enter
  wait_for_new_pane_flattened_substring "$side" "$refusal_text" "$refusal_count"
  assert_attached_client_count_stays "$side" 1
  wait_for_client_state "$side" root
  wait_for_terminal_ready "$side"
}

probe_control_nested_terminal_facts() {
  local side="$1"
  local mode
  local nested_command
  local refusal_count
  local refusal_text="sessions should be nested with care, unset \$TMUX to force"
  local exit_count
  local fresh_session="control-fresh-$side"
  local attempt
  local fresh_clients

  for mode in attach new; do
    if [ "$side" = "zz" ]; then
      if [ "$mode" = "attach" ]; then
        printf -v nested_command '%q --socket %q -C attach-session -t %q' \
          "$ZZ_BIN" "$ZZ_SOCKET" "$INNER_SESSION"
      else
        printf -v nested_command '%q --socket %q -C new-session -A -s %q' \
          "$ZZ_BIN" "$ZZ_SOCKET" "$INNER_SESSION"
      fi
    elif [ "$mode" = "attach" ]; then
      printf -v nested_command '%q -L %q -C attach-session -t %q' \
        "$TMUX_BIN" "$INNER_SOCKET_NAME" "$INNER_SESSION"
    else
      printf -v nested_command '%q -L %q -C new-session -A -s %q' \
        "$TMUX_BIN" "$INNER_SOCKET_NAME" "$INNER_SESSION"
    fi
    refusal_count="$(pane_flattened_substring_count "$side" "$refusal_text")"
    exit_count="$(pane_flattened_substring_count "$side" "%exit")"
    tmux_outer_command send-keys -l -t "$OUTER_SESSION:$side" "$nested_command"
    tmux_outer_command send-keys -t "$OUTER_SESSION:$side" Enter
    wait_for_new_pane_flattened_substring "$side" "$refusal_text" "$refusal_count"
    wait_for_new_pane_flattened_substring "$side" "%exit" "$exit_count"
    assert_attached_client_count_stays "$side" 1
    wait_for_client_state "$side" root
    wait_for_terminal_ready "$side"
  done

  if [ "$side" = "zz" ]; then
    printf -v nested_command '%q --socket %q -C new-session -A -s %q' \
      "$ZZ_BIN" "$ZZ_SOCKET" "$fresh_session"
  else
    printf -v nested_command '%q -L %q -C new-session -A -s %q' \
      "$TMUX_BIN" "$INNER_SOCKET_NAME" "$fresh_session"
  fi
  exit_count="$(pane_flattened_substring_count "$side" "%exit")"
  tmux_outer_command send-keys -l -t "$OUTER_SESSION:$side" "$nested_command"
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" Enter
  for ((attempt = 0; attempt < 200; attempt++)); do
    fresh_clients="$(side_command "$side" list-clients -F '#{client_session}' 2>/dev/null |
      awk -v session="$fresh_session" '$0 == session { count++ } END { print count + 0 }')"
    if [ "$fresh_clients" = 1 ]; then
      break
    fi
    sleep 0.05
  done
  if [ "$fresh_clients" != 1 ]; then
    fixture_failure "$side Control new-session -A miss did not create and attach $fresh_session"
  fi
  tmux_outer_command send-keys -l -t "$OUTER_SESSION:$side" detach-client
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" Enter
  wait_for_new_pane_flattened_substring "$side" "%exit" "$exit_count"
  side_command "$side" kill-session -t "=$fresh_session" ||
    fixture_failure "$side could not clean up $fresh_session"
  wait_for_client_state "$side" root
  wait_for_terminal_ready "$side"

  if [ "$side" = "zz" ]; then
    printf -v nested_command "printf 'detach-client\\n' | %q --socket %q -C attach-session -t %q" \
      "$ZZ_BIN" "$ZZ_SOCKET" "$INNER_SESSION"
  else
    printf -v nested_command "printf 'detach-client\\n' | %q -L %q -C attach-session -t %q" \
      "$TMUX_BIN" "$INNER_SOCKET_NAME" "$INNER_SESSION"
  fi
  refusal_count="$(pane_flattened_substring_count "$side" "$refusal_text")"
  exit_count="$(pane_flattened_substring_count "$side" "%exit")"
  tmux_outer_command send-keys -l -t "$OUTER_SESSION:$side" "$nested_command"
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" Enter
  wait_for_new_pane_flattened_substring "$side" "%exit" "$exit_count"
  if [ "$(pane_flattened_substring_count "$side" "$refusal_text")" != "$refusal_count" ]; then
    fixture_failure "$side treated piped Control stdin as a nested tty"
  fi
  assert_attached_client_count_stays "$side" 1
  wait_for_client_state "$side" root
}

probe_forced_nested_attaches() {
  local side="$1"
  local mode
  local nested_command
  local root_tty

  for mode in attach new; do
    if [ "$side" = "zz" ]; then
      if [ "$mode" = "attach" ]; then
        printf -v nested_command 'env -u TMUX %q --socket %q attach-session -t %q' \
          "$ZZ_BIN" "$ZZ_SOCKET" "$INNER_SESSION"
      else
        printf -v nested_command 'env -u TMUX %q --socket %q new-session -A -s %q' \
          "$ZZ_BIN" "$ZZ_SOCKET" "$INNER_SESSION"
      fi
    elif [ "$mode" = "attach" ]; then
      printf -v nested_command 'env -u TMUX %q -L %q attach-session -t %q' \
        "$TMUX_BIN" "$INNER_SOCKET_NAME" "$INNER_SESSION"
    else
      printf -v nested_command 'env -u TMUX %q -L %q new-session -A -s %q' \
        "$TMUX_BIN" "$INNER_SOCKET_NAME" "$INNER_SESSION"
    fi
    tmux_outer_command send-keys -l -t "$OUTER_SESSION:$side" "$nested_command"
    tmux_outer_command send-keys -t "$OUTER_SESSION:$side" Enter
    wait_for_attached_client_count "$side" 2
    root_tty="$(tmux_outer_command display-message -p -t "$OUTER_SESSION:$side" '#{pane_tty}')"
    [ -n "$root_tty" ] || fixture_failure "$side root attach did not publish a tty"
    side_command "$side" detach-client -a -t "$root_tty"
    wait_for_attached_client_count "$side" 1
    wait_for_client_state "$side" root
  done
  wait_for_terminal_ready "$side"
}

probe_display_panes_target_no_select() {
  local side="$1"
  local client_name
  local client_tty
  local error
  local linux_basename
  local stripped_tty
  local target

  client_name="$(side_command "$side" list-clients -F '#{client_name}')"
  if [ -z "$client_name" ] || [[ "$client_name" == *$'\n'* ]]; then
    fixture_failure "$side did not report exactly one target client name"
  fi
  client_tty="$(tmux_outer_command display-message -p -t "$OUTER_SESSION:$side" '#{pane_tty}')"
  if [[ "$client_tty" != /dev/* ]]; then
    fixture_failure "$side outer pane did not expose an attached client tty"
  fi
  stripped_tty="${client_tty#/dev/}"
  if error="$(side_command "$side" display-panes -t missing -d not-a-delay 2>&1)"; then
    fixture_failure "$side accepted a missing display-panes target"
  fi
  error="${error//$'\r'/}"
  if [[ "$error" != *"can't find client: missing"* ]] || [[ "$error" == *"delay"* ]]; then
    fixture_failure "$side resolved display-panes delay before target; got: ${error:-<empty>}"
  fi
  for target in "$client_tty" "$client_tty:" "$stripped_tty" "$stripped_tty:"; do
    if error="$(side_command "$side" display-panes -t "$target" -d not-a-delay 2>&1)"; then
      fixture_failure "$side accepted an invalid display-panes delay for tty target $target"
    fi
    error="${error//$'\r'/}"
    if [[ "$error" == *"can't find client"* ]] || [[ "$error" != *"delay"* ]]; then
      fixture_failure "$side did not resolve tty target $target before delay validation; got: ${error:-<empty>}"
    fi
  done
  linux_basename="${stripped_tty##*/}"
  if [ "$linux_basename" = "$stripped_tty" ]; then
    linux_basename=3
  fi
  if [ "$linux_basename" = "$client_name" ]; then
    if error="$(side_command "$side" display-panes -t "$linux_basename" -d not-a-delay 2>&1)"; then
      fixture_failure "$side accepted an invalid display-panes delay for exact client name $linux_basename"
    fi
    error="${error//$'\r'/}"
    if [[ "$error" == *"can't find client"* ]] || [[ "$error" != *"delay"* ]]; then
      fixture_failure "$side did not preserve exact client-name precedence for $linux_basename; got: ${error:-<empty>}"
    fi
  else
    if error="$(side_command "$side" display-panes -t "$linux_basename" -d not-a-delay 2>&1)"; then
      fixture_failure "$side accepted Linux tty basename target $linux_basename"
    fi
    error="${error//$'\r'/}"
    if [[ "$error" != *"can't find client: $linux_basename"* ]] || [[ "$error" == *"delay"* ]]; then
      fixture_failure "$side did not reject Linux tty basename before delay validation; got: ${error:-<empty>}"
    fi
  fi
  side_command "$side" display-panes -bN -t "$client_name:" -d 0 || \
    fixture_failure "$side could not target a non-selectable pane overlay"
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" F11
  wait_for_marker "$side" 5a5a5a5a
}

probe_display_message_unattached_target() {
  local side="$1"
  local client_name
  local target_client
  local output
  local -a command

  client_name="$(side_command "$side" list-clients -F '#{client_name}')"
  if [ -z "$client_name" ] || [[ "$client_name" == *$'\n'* ]]; then
    fixture_failure "$side did not report exactly one display-message fallback client"
  fi
  for target_client in '' "$client_name" missing-client; do
    command=(display-message -p -t "=$CHOOSER_SESSION:" '#{client_name}|#{client_session}|#{session_name}')
    if [ -n "$target_client" ]; then
      command=(display-message -p -c "$target_client" -t "=$CHOOSER_SESSION:" '#{client_name}|#{client_session}|#{session_name}')
    fi
    output="$(side_command "$side" "${command[@]}")" ||
      fixture_failure "$side could not expand unattached-target client facts"
    output="${output//$'\r'/}"
    if [ "$output" != "$client_name|$INNER_SESSION|$CHOOSER_SESSION" ]; then
      fixture_failure "$side selected the wrong unattached-target client facts for ${target_client:-omitted -c}; got: ${output:-<empty>}"
    fi
  done
}

probe_source_file_cwd() {
  local side="$1"
  local missing_path="config-$INNER_SESSION/missing.conf"
  local missing_warning="No such file or directory: $missing_path"
  local quiet_screen

  side_command "$side" set-option -gu @attached_source_order || \
    fixture_failure "$side could not reset the attached source-file option"
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" F10
  wait_for_side_output "$side" glob-second "source-file formatted glob order" \
    show-options -gv @attached_source_order

  side_command "$side" set-option -gu @attached_source_order || \
    fixture_failure "$side could not reset the attached source-file option"
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" F9
  wait_for_side_output "$side" glob-first "source-file declared path order" \
    show-options -gv @attached_source_order

  side_command "$side" set-option -gu @attached_source_order || \
    fixture_failure "$side could not reset the attached source-file option"
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" F8
  wait_for_side_output "$side" glob-second "source-file quiet miss continuation" \
    show-options -gv @attached_source_order
  quiet_screen="$(capture_current_screen "$side")"
  if grep -Fq -- "No such file" <<<"$quiet_screen" || \
    grep -Fq -- "$missing_path" <<<"$quiet_screen"; then
    fixture_failure "$side displayed a diagnostic for a quiet source-file miss"
  fi

  tmux_outer_command resize-window -t "$OUTER_SESSION:$side" -x 160
  if [ "$side" = "zz" ]; then
    tmux_outer_command send-keys -t "$OUTER_SESSION:$side" M-s q
  fi
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" F7
  wait_for_marker "$side" "$missing_warning"
}

probe_source_file_depth() {
  local side="$1"
  local leaf

  side_command "$side" set-option -g display-time 20000 ||
    fixture_failure "$side could not hold its status message"
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" F6
  wait_for_side_output "$side" 50 "source-file depth limit" \
    show-options -gv @attached_depth
  wait_for_side_output "$side" yes "source-file depth continuation" \
    show-options -gv @attached_after50
  wait_for_marker "$side" "Too many nested files"
  leaf="$(side_command "$side" show-options -gqv @attached_depth_leaf)"
  if [ -n "$leaf" ]; then
    fixture_failure "$side loaded the file that source invocation 51 refused"
  fi
}

probe_source_file_output() {
  local side="$1"

  tmux_outer_command resize-window -t "$OUTER_SESSION:$side" -x 200
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" F3
  wait_for_mode_state "$side" copy-mode
  wait_for_ordered_current_lines "$side" \
    "$SOURCE_OUTPUT_ROOT:1: display-message -p ATTACHED_ROOT_ONE" \
    "$SOURCE_OUTPUT_ROOT:2: source-file -v $SOURCE_OUTPUT_CHILD" \
    "$SOURCE_OUTPUT_ROOT:3: display-message -p ATTACHED_ROOT_TWO" \
    ATTACHED_ROOT_ONE \
    "$SOURCE_OUTPUT_CHILD:1: display-message -p ATTACHED_CHILD_ONE" \
    "$SOURCE_OUTPUT_CHILD:2: list-sessions -F \"ATTACHED_CHILD_LIST_#{session_name}\"" \
    ATTACHED_CHILD_ONE \
    ATTACHED_CHILD_LIST_attached \
    ATTACHED_CHILD_LIST_chooser-target \
    ATTACHED_ROOT_TWO
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" q
  wait_for_mode_state "$side" root
}

probe_command_output_navigation() {
  local side="$1"
  local original_mode_keys
  local copied_buffer
  local copied_text
  local navigation_keys=()
  local navigation_step

  original_mode_keys="$(side_command "$side" show-window-options -gv mode-keys)"
  side_command "$side" set-window-option -t "$INNER_WINDOW_TARGET" mode-keys vi ||
    fixture_failure "$side could not select vi mode keys"
  wait_for_side_output "$side" vi "vi mode-keys selection" \
    show-window-options -v -t "$INNER_WINDOW_TARGET" mode-keys

  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" F2
  wait_for_output_mode "$side" copy-mode-vi
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" g
  wait_for_current_marker "$side" ATTACHED_NAV_00
  for ((navigation_step = 0; navigation_step < 30; navigation_step++)); do
    navigation_keys+=(j)
  done
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" "${navigation_keys[@]}"
  wait_for_current_marker_absent "$side" ATTACHED_NAV_00

  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" g
  wait_for_current_marker "$side" ATTACHED_NAV_00
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" C-f
  wait_for_current_marker_absent "$side" ATTACHED_NAV_00

  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" Escape
  assert_output_mode_stays "$side" copy-mode-vi
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" Space j j Escape
  assert_output_mode_stays "$side" copy-mode-vi

  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" g /
  tmux_outer_command send-keys -l -t "$OUTER_SESSION:$side" ATTACHED_NAV_CANCEL
  wait_for_current_marker "$side" ATTACHED_NAV_CANCEL
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" Escape
  wait_for_current_marker_absent "$side" ATTACHED_NAV_CANCEL
  assert_output_mode_stays "$side" copy-mode-vi

  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" /
  tmux_outer_command send-keys -l -t "$OUTER_SESSION:$side" ATTACHED_NAV_MATX
  wait_for_current_marker "$side" ATTACHED_NAV_MATX
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" BSpace
  tmux_outer_command send-keys -l -t "$OUTER_SESSION:$side" CH
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" Enter
  wait_for_current_marker "$side" "ATTACHED_NAV_35 ATTACHED_NAV_MATCH"
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" n
  wait_for_current_marker "$side" "ATTACHED_NAV_65 ATTACHED_NAV_MATCH"
  wait_for_current_marker_absent "$side" ATTACHED_NAV_35
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" N
  wait_for_current_marker "$side" "ATTACHED_NAV_35 ATTACHED_NAV_MATCH"
  wait_for_current_marker_absent "$side" ATTACHED_NAV_65

  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" Space j j Enter
  wait_for_mode_state "$side" root
  copied_buffer="$(side_command "$side" list-buffers -F '#{buffer_name}' | \
    awk '$0 != "keep" { print; exit }')"
  if [ -z "$copied_buffer" ]; then
    fixture_failure "$side selection did not create a paste buffer"
  fi
  copied_text="$(side_command "$side" show-buffer -b "$copied_buffer")"
  if ! grep -Fq -- ATTACHED_NAV_MATCH <<<"$copied_text"; then
    fixture_failure "$side copied buffer did not contain the selected search match"
  fi
  side_command "$side" delete-buffer -b "$copied_buffer" ||
    fixture_failure "$side could not remove its navigation paste buffer"
  wait_for_side_output "$side" keep "navigation buffer cleanup" \
    list-buffers -F '#{buffer_name}'

  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" F2
  wait_for_output_mode "$side" copy-mode-vi
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" G
  wait_for_current_marker "$side" ATTACHED_NAV_95
  side_command "$side" bind-key -T copy-mode-vi q send-keys -X page-up ||
    fixture_failure "$side could not install the live copy-mode-vi q binding"
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" q
  wait_for_current_marker_absent "$side" ATTACHED_NAV_95
  assert_output_mode_stays "$side" copy-mode-vi
  side_command "$side" bind-key -T copy-mode-vi q send-keys -X cancel ||
    fixture_failure "$side could not restore the copy-mode-vi q binding"
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" q
  wait_for_mode_state "$side" root

  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" F2
  wait_for_output_mode "$side" copy-mode-vi
  side_command "$side" set-window-option -t "$INNER_WINDOW_TARGET" mode-keys emacs ||
    fixture_failure "$side could not select emacs mode keys"
  wait_for_side_output "$side" emacs "emacs mode-keys selection" \
    show-window-options -v -t "$INNER_WINDOW_TARGET" mode-keys
  wait_for_output_mode "$side" copy-mode
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" Escape
  wait_for_mode_state "$side" root

  side_command "$side" set-window-option -u -t "$INNER_WINDOW_TARGET" mode-keys ||
    fixture_failure "$side could not restore inherited mode keys"
  wait_for_side_output "$side" "" "mode-keys inheritance restoration" \
    show-window-options -v -t "$INNER_WINDOW_TARGET" mode-keys
  wait_for_side_output "$side" "$original_mode_keys" "effective mode-keys restoration" \
    show-window-options -gv mode-keys
}

probe_list_keys_single() {
  local side="$1"
  local screen

  side_command "$side" set-option -g display-time 20000 ||
    fixture_failure "$side could not hold the list-keys status message"
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" C-b B Enter
  wait_for_pane_marker "$side" ATTACHED_LIST_KEYS_BASE
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" C-b Q
  wait_for_marker "$side" ATTACHED_LIST_KEYS_PROMPT
  tmux_outer_command send-keys -l -t "$OUTER_SESSION:$side" \
    "list-keys -1 -T attached-list -F '#{key_note}'"
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" Enter
  wait_for_marker "$side" ZZLK1
  screen="$(capture_current_screen "$side")"
  if ! grep -Fq -- ATTACHED_LIST_KEYS_BASE <<<"$screen"; then
    fixture_failure "$side list-keys -1 replaced the terminal with command output"
  fi
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" C-b R Enter
  wait_for_pane_marker "$side" ATTACHED_LIST_KEYS_RESUMED
}

run_control_environment_attach() {
  local side="$1"
  local environment="$2"
  local output="$SCRATCH_DIR/control-environment-$side.out"
  local error="$SCRATCH_DIR/control-environment-$side.err"
  shift 2

  if ! printf 'detach-client\n' | environment_side_command \
    "$side" "$environment" -C "$@" >"$output" 2>"$error"; then
    fixture_failure "$side Control environment attachment failed: $(tr '\n' ' ' <"$error")"
  fi
}

validate_context_format_row() {
  local side="$1"
  local kind="$2"
  local row="$3"
  local expected_tty="${4:-}"
  local activity cell_height cell_width colours control_mode created discarded flags
  local height key_table last_session name pid prefix readonly session termfeatures
  local termname termtype theme tty uid user utf8 width written last_attached

  IFS='|' read -r activity cell_height cell_width colours control_mode created \
    discarded flags height key_table last_session name pid prefix readonly session \
    termfeatures termname termtype theme tty uid user utf8 width written last_attached <<<"$row"
  for value in "$activity" "$created" "$discarded" "$pid" "$uid" "$written" \
    "$last_attached"; do
    if ! [[ "$value" =~ ^[0-9]+$ ]]; then
      fixture_failure "$side $kind client context row has a nonnumeric retained fact: ${value:-<empty>}"
    fi
  done
  if [ "$session" != "$INNER_SESSION" ] || [ "$key_table" != root ] || \
    [ "$prefix" != 0 ] || [ "$readonly" != 0 ] || [ -z "$user" ] || \
    [ -z "$termname" ]; then
    fixture_failure "$side $kind client context row has incorrect common facts: $row"
  fi
  if [[ ",$flags," != *,attached,* ]] || [[ ",$flags," != *,focused,* ]]; then
    fixture_failure "$side $kind client context row is missing attached focus flags: $flags"
  fi
  case "$kind" in
  interactive)
    if ! [[ "$cell_height" =~ ^[0-9]+$ ]] || ! [[ "$cell_width" =~ ^[0-9]+$ ]] || \
      ! [[ "$colours" =~ ^[0-9]+$ ]] || [ "$control_mode" != 0 ] || \
      [ "$height" != 24 ] || [ "$name" != "$expected_tty" ] || \
      [ "$tty" != "$expected_tty" ] || [ "$width" != 80 ] || [ "$utf8" != 1 ]; then
      fixture_failure "$side interactive client context row has incorrect terminal facts: $row"
    fi
    ;;
  control)
    if [ -n "$cell_height" ] || [ -n "$cell_width" ] || [ -n "$colours" ] || \
      [ "$control_mode" != 1 ] || [ -n "$height" ] || [ -n "$termfeatures" ] || \
      [ -n "$termtype" ] || [ -n "$theme" ] || [ -n "$tty" ] || \
      [ "$width" != 91 ] || [[ "$name" != client-* ]] || \
      [[ ",$flags," != *,control-mode,* ]]; then
      fixture_failure "$side Control client context row has incorrect terminal facts: $row"
    fi
    ;;
  esac
}

probe_client_context_formats() {
  local side="$1"
  local client_tty
  local row
  local stable
  local selected
  local no_client
  local control_output="$SCRATCH_DIR/control-context-$side.out"
  local control_error="$SCRATCH_DIR/control-context-$side.err"
  local full_format='#{client_activity}|#{client_cell_height}|#{client_cell_width}|#{client_colours}|#{client_control_mode}|#{client_created}|#{client_discarded}|#{client_flags}|#{client_height}|#{client_key_table}|#{client_last_session}|#{client_name}|#{client_pid}|#{client_prefix}|#{client_readonly}|#{client_session}|#{client_termfeatures}|#{client_termname}|#{client_termtype}|#{client_theme}|#{client_tty}|#{client_uid}|#{client_user}|#{client_utf8}|#{client_width}|#{client_written}|#{session_last_attached}'
  local stable_format='#{client_name}|#{client_pid}|#{client_tty}|#{client_session}|#{client_width}|#{client_height}|#{client_key_table}|#{client_prefix}|#{client_readonly}|#{client_control_mode}|#{client_termname}|#{client_utf8}'

  client_tty="$(tmux_outer_command display-message -p -t "$OUTER_SESSION:$side" '#{pane_tty}')"
  row="$(side_command "$side" list-clients -t "=$INNER_SESSION" -F "$full_format")" ||
    fixture_failure "$side could not list retained client context facts"
  row="${row//$'\r'/}"
  if [ -z "$row" ] || [[ "$row" == *$'\n'* ]]; then
    fixture_failure "$side did not produce exactly one Interactive client context row"
  fi
  validate_context_format_row "$side" interactive "$row" "$client_tty"

  stable="$(side_command "$side" list-clients -t "=$INNER_SESSION" -F "$stable_format")" ||
    fixture_failure "$side could not list stable client context facts"
  selected="$(side_command "$side" display-message -p -c "$client_tty" \
    -t "$INNER_PANE_TARGET" "$stable_format")" ||
    fixture_failure "$side could not select explicit display-message client facts"
  stable="${stable//$'\r'/}"
  selected="${selected//$'\r'/}"
  if [ "$selected" != "$stable" ]; then
    fixture_failure "$side list and explicit display-message client facts differ: $stable != $selected"
  fi

  no_client="$(side_command "$side" display-message -p -t "$INNER_PANE_TARGET" \
    '#{client_name}|#{session_last_attached}')" ||
    fixture_failure "$side could not print a clientless attached target context"
  no_client="${no_client//$'\r'/}"
  if [ "${no_client%%|*}" != "$client_tty" ] || \
    ! [[ "${no_client#*|}" =~ ^[0-9]+$ ]]; then
    fixture_failure "$side did not select the attached client and retain session_last_attached: ${no_client:-<empty>}"
  fi

  side_command "$side" bind-key -n F12 run-shell -C \
    "set-option -g @attached_context '$stable_format'" ||
    fixture_failure "$side could not bind its ordinary client context probe"
  tmux_outer_command send-keys -t "$OUTER_SESSION:$side" F12
  wait_for_side_output "$side" "$stable" "ordinary attached client context" \
    show-options -gqv @attached_context
  side_command "$side" unbind-key -n F12 ||
    fixture_failure "$side could not remove its ordinary client context probe"
  side_command "$side" set-option -gu @attached_context ||
    fixture_failure "$side could not clear its ordinary client context result"

  side_command "$side" set-option -t "=$INNER_SESSION:" status-left \
    "CTXSTATUS:#{client_pid}:#{client_tty}" ||
    fixture_failure "$side could not set its status client context probe"
  side_command "$side" set-option -t "=$INNER_SESSION:" status-left-length 80 ||
    fixture_failure "$side could not widen its status client context probe"
  wait_for_marker "$side" "CTXSTATUS:"
  wait_for_marker "$side" "$client_tty"
  side_command "$side" set-option -u -t "=$INNER_SESSION:" status-left ||
    fixture_failure "$side could not restore status-left"
  side_command "$side" set-option -u -t "=$INNER_SESSION:" status-left-length ||
    fixture_failure "$side could not restore status-left-length"

  if ! printf 'refresh-client -C 91x31 ; list-clients -f "#{client_control_mode}" -F "CTX|%s" ; detach-client\n' \
    "$full_format" | environment_side_command "$side" control -C attach-session \
      -t "=$INNER_SESSION" >"$control_output" 2>"$control_error"; then
    fixture_failure "$side Control context probe failed: $(tr '\n' ' ' <"$control_error")"
  fi
  row="$(grep '^CTX|' "$control_output" | tail -1 || true)"
  row="${row#CTX|}"
  row="${row//$'\r'/}"
  if [ -z "$row" ]; then
    fixture_failure "$side Control context probe produced no row"
  fi
  validate_context_format_row "$side" control "$row"
  wait_for_attached_client_count "$side" 1
  wait_for_client_state "$side" root
}

probe_client_event_hooks() {
  local side="$1"
  local event_window="events-$side"
  local event_attach="$SCRATCH_DIR/attach-$event_window"
  local primary_target="=$OUTER_SESSION:$side"
  local event_target="=$OUTER_SESSION:$event_window"
  local primary_tty
  local event_tty
  local event_pid
  local event_context
  local primary_event
  local event_active
  local event_focus_out
  local event_focus_in
  local event_resized
  local hook

  write_attach "$side" "$event_attach"
  tmux_outer_command new-window -d -t "$OUTER_SESSION:" -n "$event_window" 'sleep 600' ||
    fixture_failure "$side could not create its client event window"
  tmux_outer_command respawn-pane -k -t "$event_target" "$event_attach" ||
    fixture_failure "$side could not attach its client event client"
  wait_for_attached_client_count "$side" 2

  primary_tty="$(tmux_outer_command display-message -p -t "$primary_target" '#{pane_tty}')"
  event_tty="$(tmux_outer_command display-message -p -t "$event_target" '#{pane_tty}')"
  event_pid="$(tmux_outer_command display-message -p -t "$event_target" '#{pane_pid}')"
  if [[ "$primary_tty" != /dev/* || "$event_tty" != /dev/* ]] ||
    ! [[ "$event_pid" =~ ^[0-9]+$ ]]; then
    fixture_failure "$side outer panes did not expose client event identities"
  fi
  event_context="$(side_command "$side" display-message -p -c "$event_tty" \
    '#{session_id}|#{session_name}|#{window_id}|#{window_name}|#{pane_id}')" ||
    fixture_failure "$side could not select its client event context"
  event_context="${event_context//$'\r'/}"

  primary_event="A|$primary_tty|$event_context||,"
  event_active="A|$event_tty|$event_context||,"
  event_focus_out="O|$event_tty|$event_context||,"
  event_focus_in="I|$event_tty|$event_context||,"
  event_resized="R|$event_tty|$event_context||,"

  side_command "$side" set-option -g focus-events on ||
    fixture_failure "$side could not enable client focus events"
  side_command "$side" set-option -g @attached_event_hooks '' ||
    fixture_failure "$side could not initialize its client event log"
  side_command "$side" set-option -g @attached_event_keys '' ||
    fixture_failure "$side could not initialize its client event key log"
  side_command "$side" bind-key -n F12 set-option -agF @attached_event_keys \
    'K|#{client_tty},' ||
    fixture_failure "$side could not bind its client event input"
  side_command "$side" set-hook -g client-active \
    "set-option -agF @attached_event_hooks 'A|#{hook_client}|#{session_id}|#{session_name}|#{window_id}|#{window_name}|#{pane_id}|#{hook_session}|#{hook_session_name},'" ||
    fixture_failure "$side could not install client-active"
  side_command "$side" set-hook -g client-focus-out \
    "set-option -agF @attached_event_hooks 'O|#{hook_client}|#{session_id}|#{session_name}|#{window_id}|#{window_name}|#{pane_id}|#{hook_session}|#{hook_session_name},'" ||
    fixture_failure "$side could not install client-focus-out"
  side_command "$side" set-hook -g client-focus-in \
    "set-option -agF @attached_event_hooks 'I|#{hook_client}|#{session_id}|#{session_name}|#{window_id}|#{window_name}|#{pane_id}|#{hook_session}|#{hook_session_name},'" ||
    fixture_failure "$side could not install client-focus-in"
  side_command "$side" set-hook -g client-resized \
    "set-option -agF @attached_event_hooks 'R|#{hook_client}|#{session_id}|#{session_name}|#{window_id}|#{window_name}|#{pane_id}|#{hook_session}|#{hook_session_name},'" ||
    fixture_failure "$side could not install client-resized"

  tmux_outer_command send-keys -t "$primary_target" F12
  wait_for_side_output "$side" "K|$primary_tty," "primary client event input" \
    show-options -gqv @attached_event_keys
  side_command "$side" set-option -g @attached_event_hooks '' ||
    fixture_failure "$side could not clear its client event baseline"
  side_command "$side" set-option -g @attached_event_keys '' ||
    fixture_failure "$side could not clear its client event key baseline"

  tmux_outer_command send-keys -t "$event_target" F12
  wait_for_side_output "$side" "$event_active" "client-active event" \
    show-options -gqv @attached_event_hooks
  wait_for_side_output "$side" "K|$event_tty," "active client input" \
    show-options -gqv @attached_event_keys
  tmux_outer_command send-keys -t "$event_target" F12
  wait_for_side_output "$side" "K|$event_tty,K|$event_tty," \
    "repeated active client input" show-options -gqv @attached_event_keys
  tmux_outer_command send-keys -t "$primary_target" F12
  wait_for_side_output "$side" "$event_active$primary_event" \
    "deduplicated client-active events" show-options -gqv @attached_event_hooks

  side_command "$side" set-option -g @attached_event_hooks '' ||
    fixture_failure "$side could not clear its client focus event log"
  tmux_outer_command send-keys -l -t "$event_target" $'\033[O'
  wait_for_side_output "$side" "$event_focus_out" "client-focus-out event" \
    show-options -gqv @attached_event_hooks
  tmux_outer_command send-keys -l -t "$event_target" $'\033[I'
  wait_for_side_output "$side" "$event_focus_out$event_focus_in$event_active" \
    "client-focus-in event ordering" show-options -gqv @attached_event_hooks

  tmux_outer_command send-keys -t "$primary_target" F12
  wait_for_side_output "$side" "$event_focus_out$event_focus_in$event_active$primary_event" \
    "client resize latest-client baseline" show-options -gqv @attached_event_hooks
  side_command "$side" set-option -g @attached_event_hooks '' ||
    fixture_failure "$side could not clear its client resize event log"
  kill -WINCH "$event_pid"
  wait_for_side_output "$side" "$event_active$event_resized" \
    "client-resized event ordering" show-options -gqv @attached_event_hooks
  kill -WINCH "$event_pid"
  wait_for_side_output "$side" "$event_active$event_resized$event_resized" \
    "repeated unchanged client-resized event" show-options -gqv @attached_event_hooks

  for hook in client-active client-focus-out client-focus-in client-resized; do
    side_command "$side" set-hook -gu "$hook" ||
      fixture_failure "$side could not remove $hook"
  done
  side_command "$side" unbind-key -n F12 ||
    fixture_failure "$side could not remove its client event input"
  side_command "$side" set-option -gu @attached_event_hooks ||
    fixture_failure "$side could not clear its client event log"
  side_command "$side" set-option -gu @attached_event_keys ||
    fixture_failure "$side could not clear its client event key log"
  side_command "$side" set-option -gu focus-events ||
    fixture_failure "$side could not restore client focus events"
  tmux_outer_command kill-window -t "$event_target" ||
    fixture_failure "$side could not remove its client event window"
  wait_for_attached_client_count "$side" 1
  wait_for_client_state "$side" root
}

probe_client_environment() {
  local side="$1"
  local session="environment-$side"
  local created_session="environment-created-$side"
  local switch_session="environment-switch-$side"
  local terminal_window="environment-terminal-$side"
  local terminal_attach="$SCRATCH_DIR/attach-$terminal_window"
  local terminal_target="=$OUTER_SESSION:$terminal_window"
  local terminal_tty
  local root_tty
  local update_names="$ENV_SELECTED $ENV_MISSING $ENV_EMPTY $ENV_GLOB_PATTERN $ENV_HIDDEN"

  side_command "$side" set-option -g update-environment "$update_names" ||
    fixture_failure "$side could not set client environment selection"
  environment_side_command "$side" seed new-session -d -s "$session" ||
    fixture_failure "$side could not create a client-seeded session"
  wait_for_side_output "$side" "$ENV_SELECTED=seed" "created client environment" \
    show-environment -t "=$session" "$ENV_SELECTED"
  wait_for_side_output "$side" "$ENV_EMPTY=" "created empty client environment" \
    show-environment -t "=$session" "$ENV_EMPTY"
  wait_for_side_output "$side" "-$ENV_MISSING" "created missing client marker" \
    show-environment -t "=$session" "$ENV_MISSING"
  wait_for_side_output "$side" "$ENV_GLOB_A=glob-a" "created wildcard client environment" \
    show-environment -t "=$session" "$ENV_GLOB_A"

  side_command "$side" set-option -t "=$session:" update-environment "$update_names" ||
    fixture_failure "$side could not set session client environment selection"
  side_command "$side" set-environment -h -t "=$session" "$ENV_HIDDEN" hidden-before ||
    fixture_failure "$side could not hide its selected environment value"
  side_command "$side" set-environment -h -t "=$session" "$ENV_HIDDEN_KEEP" hidden-keep ||
    fixture_failure "$side could not hide its unselected environment value"
  run_control_environment_attach "$side" control attach-session -t "=$session"
  wait_for_side_output "$side" "$ENV_SELECTED=control" "Control environment refresh" \
    show-environment -t "=$session" "$ENV_SELECTED"
  wait_for_side_output "$side" "$ENV_HIDDEN=revealed" "selected hidden environment refresh" \
    show-environment -t "=$session" "$ENV_HIDDEN"
  wait_for_side_output "$side" "$ENV_HIDDEN_KEEP=hidden-keep" \
    "unselected hidden environment retention" \
    show-environment -h -t "=$session" "$ENV_HIDDEN_KEEP"

  run_control_environment_attach "$side" missing attach-session -t "=$session"
  wait_for_side_output "$side" "-$ENV_SELECTED" "missing selected client marker" \
    show-environment -t "=$session" "$ENV_SELECTED"
  wait_for_side_output "$side" "-$ENV_EMPTY" "missing empty client marker" \
    show-environment -t "=$session" "$ENV_EMPTY"
  wait_for_side_output "$side" "-$ENV_GLOB_PATTERN" "missing wildcard literal marker" \
    show-environment -t "=$session" "$ENV_GLOB_PATTERN"
  wait_for_side_output "$side" "$ENV_GLOB_A=glob-a2" "retained wildcard expansion" \
    show-environment -t "=$session" "$ENV_GLOB_A"
  wait_for_side_output "$side" "$ENV_HIDDEN_KEEP=hidden-keep" \
    "hidden environment retention after a missing refresh" \
    show-environment -h -t "=$session" "$ENV_HIDDEN_KEEP"

  side_command "$side" set-environment -t "=$session" "$ENV_SELECTED" before-E ||
    fixture_failure "$side could not set its attach -E baseline"
  run_control_environment_attach "$side" changed attach-session -E -t "=$session"
  wait_for_side_output "$side" "$ENV_SELECTED=before-E" "attach -E environment preservation" \
    show-environment -t "=$session" "$ENV_SELECTED"

  run_control_environment_attach "$side" client-a new-session -A -d -s "$session" \
    -e "$ENV_SELECTED=explicit"
  wait_for_side_output "$side" "$ENV_SELECTED=client-A" \
    "existing new-session -A client refresh" \
    show-environment -t "=$session" "$ENV_SELECTED"
  side_command "$side" set-environment -t "=$session" "$ENV_SELECTED" before-AE ||
    fixture_failure "$side could not set its new-session -AE baseline"
  run_control_environment_attach "$side" changed new-session -A -d -E -s "$session" \
    -e "$ENV_SELECTED=ignored"
  wait_for_side_output "$side" "$ENV_SELECTED=before-AE" \
    "existing new-session -AE preservation" \
    show-environment -t "=$session" "$ENV_SELECTED"

  environment_side_command "$side" client-a new-session -A -d -s "$created_session" \
    -e "$ENV_SELECTED=explicit-new-A" ||
    fixture_failure "$side could not create a missing new-session -A target"
  wait_for_side_output "$side" "$ENV_SELECTED=explicit-new-A" \
    "missing new-session -A creation environment" \
    show-environment -t "=$created_session" "$ENV_SELECTED"

  write_attach "$side" "$terminal_attach" "$session" terminal-value
  tmux_outer_command new-window -d -t "$OUTER_SESSION:" -n "$terminal_window" \
    "$terminal_attach" ||
    fixture_failure "$side could not start its terminal environment client"
  wait_for_side_output "$side" "$ENV_SELECTED=terminal-value" \
    "terminal client environment refresh" \
    show-environment -t "=$session" "$ENV_SELECTED"
  terminal_tty="$(tmux_outer_command display-message -p -t "$terminal_target" '#{pane_tty}')"
  if [[ "$terminal_tty" != /dev/* ]]; then
    fixture_failure "$side terminal environment client did not expose a tty"
  fi
  side_command "$side" detach-client -t "$terminal_tty" ||
    fixture_failure "$side could not detach its terminal environment client"
  tmux_outer_command kill-window -t "$terminal_target" ||
    fixture_failure "$side could not remove its terminal environment window"
  wait_for_side_output "$side" "$ENV_SELECTED=terminal-value" \
    "session environment survives client disconnect" \
    show-environment -t "=$session" "$ENV_SELECTED"

  environment_side_command "$side" missing new-session -d -E -s "$switch_session" ||
    fixture_failure "$side could not create its switch environment destination"
  side_command "$side" set-option -t "=$switch_session:" update-environment "$ENV_SELECTED" ||
    fixture_failure "$side could not set its switch environment selection"
  side_command "$side" set-environment -t "=$switch_session" "$ENV_SELECTED" before-switch ||
    fixture_failure "$side could not set its switch environment baseline"
  root_tty="$(tmux_outer_command display-message -p -t "=$OUTER_SESSION:$side" '#{pane_tty}')"
  if [[ "$root_tty" != /dev/* ]]; then
    fixture_failure "$side root environment client did not expose a tty"
  fi
  environment_side_command "$side" external switch-client -c "$root_tty" -t "=$session" ||
    fixture_failure "$side targeted switch environment refresh failed"
  wait_for_side_output "$side" "$ENV_SELECTED=root-client" \
    "targeted switch client environment" \
    show-environment -t "=$session" "$ENV_SELECTED"
  environment_side_command "$side" external switch-client -E -c "$root_tty" \
    -t "=$switch_session" ||
    fixture_failure "$side targeted switch -E failed"
  wait_for_side_output "$side" "$ENV_SELECTED=before-switch" \
    "targeted switch -E environment preservation" \
    show-environment -t "=$switch_session" "$ENV_SELECTED"
  environment_side_command "$side" external switch-client -E -c "$root_tty" \
    -t "=$INNER_SESSION" ||
    fixture_failure "$side could not return its root client after environment switching"
  wait_for_client_state "$side" root

  side_command "$side" kill-session -t "=$session" ||
    fixture_failure "$side could not remove its client environment session"
  side_command "$side" kill-session -t "=$created_session" ||
    fixture_failure "$side could not remove its created environment session"
  side_command "$side" kill-session -t "=$switch_session" ||
    fixture_failure "$side could not remove its switch environment session"
  side_command "$side" set-option -gu update-environment ||
    fixture_failure "$side could not restore update-environment"
}

run_requested_client_command() {
  local side="$1"
  shift

  side_command "$side" bind-key -n F1 "$@" ||
    fixture_failure "$side could not bind its requested-client command"
  tmux_outer_command send-keys -t "=$OUTER_SESSION:$side" F1
}

respawn_attached_client() {
  local side="$1"
  local client_tty
  local attach_script

  client_tty="$(tmux_outer_command display-message -p -t "=$OUTER_SESSION:$side" '#{pane_tty}')"
  if [[ "$client_tty" != /dev/* ]]; then
    fixture_failure "$side outer pane did not expose a requested-flags client tty"
  fi
  side_command "$side" detach-client -t "$client_tty" ||
    fixture_failure "$side could not detach its requested-flags client"
  wait_for_attached_client_count "$side" 0

  if [ "$side" = "zz" ]; then
    attach_script="$ZZ_ATTACH"
  else
    attach_script="$TMUX_ATTACH"
  fi
  tmux_outer_command respawn-pane -k -t "=$OUTER_SESSION:$side" "$attach_script" ||
    fixture_failure "$side could not respawn its requested-flags client"
  wait_for_client_state "$side" root
}

probe_requested_client_flags() {
  local side="$1"
  local fresh_session="requested-flags-fresh"
  local detached_session="requested-flags-detached"
  local missing_session="z"
  local full_flags="ignore-size,no-detach-on-destroy,read-only,active-pane"
  local read_only_flags

  wait_for_terminal_ready "$side"
  wait_for_requested_client_state "$side" "$INNER_SESSION|"

  run_requested_client_command "$side" attach-session -f \
    active-pane,ignore-size,no-detach-on-destroy,read-only,unknown -t "=$INNER_SESSION"
  wait_for_requested_client_state "$side" "$INNER_SESSION|$full_flags"

  run_requested_client_command "$side" switch-client -t "=$CHOOSER_SESSION"
  wait_for_requested_client_state "$side" "$CHOOSER_SESSION|$full_flags"
  run_requested_client_command "$side" attach-session -t "=$INNER_SESSION"
  wait_for_requested_client_state "$side" "$INNER_SESSION|$full_flags"

  run_requested_client_command "$side" attach-session -f '!ignore-size,!active-pane' \
    -t "=$missing_session"
  wait_for_current_marker "$side" "find session: $missing_session"
  wait_for_requested_client_state "$side" "$INNER_SESSION|$full_flags"

  run_requested_client_command "$side" attach-session -f '!active-pane' -t "=$INNER_SESSION"
  wait_for_requested_client_state "$side" \
    "$INNER_SESSION|ignore-size,no-detach-on-destroy,read-only"

  respawn_attached_client "$side"
  wait_for_requested_client_state "$side" "$INNER_SESSION|"

  run_requested_client_command "$side" new-session -f ignore-size -s "$fresh_session"
  wait_for_requested_client_state "$side" "$fresh_session|ignore-size"
  run_requested_client_command "$side" attach-session -t "=$INNER_SESSION"
  wait_for_requested_client_state "$side" "$INNER_SESSION|ignore-size"

  run_requested_client_command "$side" new-session -d -f active-pane -s "$detached_session"
  wait_for_session_name "$side" "$detached_session"
  wait_for_requested_client_state "$side" "$INNER_SESSION|ignore-size"
  side_command "$side" kill-session -t "=$detached_session" ||
    fixture_failure "$side could not clean up its detached requested-flags session"
  side_command "$side" kill-session -t "=$fresh_session" ||
    fixture_failure "$side could not clean up its fresh requested-flags session"

  run_requested_client_command "$side" attach-session -f '!ignore-size' -t "=$INNER_SESSION"
  wait_for_requested_client_state "$side" "$INNER_SESSION|"
  run_requested_client_command "$side" attach-session -r -t "=$INNER_SESSION"
  if [ "$side" = "zz" ]; then
    read_only_flags="read-only"
  else
    read_only_flags="ignore-size,read-only"
  fi
  wait_for_requested_client_state "$side" "$INNER_SESSION|$read_only_flags"
}

probe_attached_client_sizing() {
  local side="$1"
  local decoy_session="sizing-decoy-$side"
  local wide_window="sizing-$side-wide"
  local decoy_window="sizing-$side-decoy"
  local wide_attach="$SCRATCH_DIR/attach-$wide_window"
  local decoy_attach="$SCRATCH_DIR/attach-$decoy_window"
  local primary_target="=$OUTER_SESSION:$side"
  local wide_target="=$OUTER_SESSION:$wide_window"
  local decoy_target="=$OUTER_SESSION:$decoy_window"
  local primary_tty
  local wide_tty
  local decoy_tty

  respawn_attached_client "$side"
  side_command "$side" set-option -t "=$INNER_SESSION:" status off ||
    fixture_failure "$side could not disable status for attached sizing"
  side_command "$side" set-option -t "=$INNER_SESSION:" default-size 73x19 ||
    fixture_failure "$side could not set the attached sizing default"
  side_command "$side" new-session -d -s "$decoy_session" ||
    fixture_failure "$side could not create its attached sizing decoy session"
  side_command "$side" set-option -t "=$decoy_session:" status off ||
    fixture_failure "$side could not disable its attached sizing decoy status"

  write_attach "$side" "$wide_attach"
  write_attach "$side" "$decoy_attach" "$decoy_session"

  tmux_outer_command resize-window -t "$primary_target" -x 101 -y 31 ||
    fixture_failure "$side could not size its primary attached client"
  wait_for_outer_pane_size "$primary_target" 101x31
  wait_for_session_client_sizes "$side" "$INNER_SESSION" 101x31

  tmux_outer_command new-window -d -t "$OUTER_SESSION:" -n "$wide_window" 'sleep 600' ||
    fixture_failure "$side could not create its wide attached sizing window"
  tmux_outer_command resize-window -t "$wide_target" -x 141 -y 21 ||
    fixture_failure "$side could not size its wide attached client"
  wait_for_outer_pane_size "$wide_target" 141x21
  tmux_outer_command respawn-pane -k -t "$wide_target" "$wide_attach" ||
    fixture_failure "$side could not attach its wide sizing client"
  wait_for_session_client_sizes "$side" "$INNER_SESSION" $'101x31\n141x21'

  tmux_outer_command new-window -d -t "$OUTER_SESSION:" -n "$decoy_window" 'sleep 600' ||
    fixture_failure "$side could not create its decoy attached sizing window"
  tmux_outer_command resize-window -t "$decoy_target" -x 181 -y 51 ||
    fixture_failure "$side could not size its decoy attached client"
  wait_for_outer_pane_size "$decoy_target" 181x51
  tmux_outer_command respawn-pane -k -t "$decoy_target" "$decoy_attach" ||
    fixture_failure "$side could not attach its decoy sizing client"
  wait_for_session_client_sizes "$side" "$decoy_session" 181x51

  primary_tty="$(tmux_outer_command display-message -p -t "$primary_target" '#{pane_tty}')"
  wide_tty="$(tmux_outer_command display-message -p -t "$wide_target" '#{pane_tty}')"
  decoy_tty="$(tmux_outer_command display-message -p -t "$decoy_target" '#{pane_tty}')"
  if [[ "$primary_tty" != /dev/* || "$wide_tty" != /dev/* || "$decoy_tty" != /dev/* ]]; then
    fixture_failure "$side outer panes did not expose all attached sizing client ttys"
  fi

  side_command "$side" resize-window -A -t "$INNER_WINDOW_TARGET" ||
    fixture_failure "$side resize-window -A failed"
  wait_for_side_output "$side" 141x31 "componentwise largest attached sizing" \
    display-message -p -t "$INNER_WINDOW_TARGET" '#{window_width}x#{window_height}'
  side_command "$side" resizew -a -t "$INNER_WINDOW_TARGET" ||
    fixture_failure "$side resizew -a failed"
  wait_for_side_output "$side" 101x21 "componentwise smallest attached sizing" \
    display-message -p -t "$INNER_WINDOW_TARGET" '#{window_width}x#{window_height}'
  side_command "$side" resize-window -Aa -R -x 77 -y 17 \
    -t "$INNER_WINDOW_TARGET" 4 ||
    fixture_failure "$side combined attached sizing command failed"
  wait_for_side_output "$side" 141x31 "combined largest attached sizing" \
    display-message -p -t "$INNER_WINDOW_TARGET" '#{window_width}x#{window_height}'
  wait_for_side_output "$side" manual "attached sizing manual mode" \
    show-window-options -v -t "$INNER_WINDOW_TARGET" window-size

  tmux_outer_command resize-window -t "$primary_target" -x 111 -y 35 ||
    fixture_failure "$side could not resize its primary client during manual sizing"
  tmux_outer_command resize-window -t "$wide_target" -x 151 -y 25 ||
    fixture_failure "$side could not resize its wide client during manual sizing"
  wait_for_session_client_sizes "$side" "$INNER_SESSION" $'111x35\n151x25'
  wait_for_side_output "$side" 141x31 "manual attached sizing freeze" \
    display-message -p -t "$INNER_WINDOW_TARGET" '#{window_width}x#{window_height}'

  tmux_outer_command resize-window -t "$primary_target" -x 101 -y 31 ||
    fixture_failure "$side could not restore its primary sizing client"
  tmux_outer_command resize-window -t "$wide_target" -x 141 -y 21 ||
    fixture_failure "$side could not restore its wide sizing client"
  wait_for_session_client_sizes "$side" "$INNER_SESSION" $'101x31\n141x21'

  side_command "$side" refresh-client -t "$wide_tty" -f ignore-size ||
    fixture_failure "$side could not ignore its wide sizing client"
  side_command "$side" resize-window -A -t "$INNER_WINDOW_TARGET" ||
    fixture_failure "$side mixed-ignore largest sizing failed"
  wait_for_side_output "$side" 101x31 "mixed-ignore largest attached sizing" \
    display-message -p -t "$INNER_WINDOW_TARGET" '#{window_width}x#{window_height}'
  side_command "$side" resizew -a -t "$INNER_WINDOW_TARGET" ||
    fixture_failure "$side mixed-ignore smallest sizing failed"
  wait_for_side_output "$side" 101x31 "mixed-ignore smallest attached sizing" \
    display-message -p -t "$INNER_WINDOW_TARGET" '#{window_width}x#{window_height}'

  side_command "$side" refresh-client -t "$primary_tty" -f ignore-size ||
    fixture_failure "$side could not ignore its primary sizing client"
  side_command "$side" resize-window -A -t "$INNER_WINDOW_TARGET" ||
    fixture_failure "$side no-candidate largest sizing failed"
  wait_for_side_output "$side" 73x19 "no-candidate largest attached sizing" \
    display-message -p -t "$INNER_WINDOW_TARGET" '#{window_width}x#{window_height}'
  side_command "$side" resizew -a -t "$INNER_WINDOW_TARGET" ||
    fixture_failure "$side no-candidate smallest sizing failed"
  wait_for_side_output "$side" 73x19 "no-candidate smallest attached sizing" \
    display-message -p -t "$INNER_WINDOW_TARGET" '#{window_width}x#{window_height}'

  side_command "$side" refresh-client -t "$decoy_tty" -f ignore-size ||
    fixture_failure "$side could not ignore its decoy sizing client"
  side_command "$side" resize-window -A -t "$INNER_WINDOW_TARGET" ||
    fixture_failure "$side all-ignore largest sizing failed"
  wait_for_side_output "$side" 141x31 "all-ignore largest attached sizing" \
    display-message -p -t "$INNER_WINDOW_TARGET" '#{window_width}x#{window_height}'
  side_command "$side" resizew -a -t "$INNER_WINDOW_TARGET" ||
    fixture_failure "$side all-ignore smallest sizing failed"
  wait_for_side_output "$side" 101x21 "all-ignore smallest attached sizing" \
    display-message -p -t "$INNER_WINDOW_TARGET" '#{window_width}x#{window_height}'

  side_command "$side" refresh-client -t "$primary_tty" -f '!ignore-size' ||
    fixture_failure "$side could not restore its primary sizing flags"
  side_command "$side" refresh-client -t "$wide_tty" -f '!ignore-size' ||
    fixture_failure "$side could not restore its wide sizing flags"
  side_command "$side" refresh-client -t "$decoy_tty" -f '!ignore-size' ||
    fixture_failure "$side could not restore its decoy sizing flags"

  tmux_outer_command kill-window -t "$wide_target" ||
    fixture_failure "$side could not remove its wide attached sizing window"
  wait_for_attached_client_count "$side" 1
  tmux_outer_command kill-window -t "$decoy_target" ||
    fixture_failure "$side could not remove its decoy attached sizing window"
  wait_for_session_client_sizes "$side" "$decoy_session" ""
  side_command "$side" kill-session -t "=$decoy_session" ||
    fixture_failure "$side could not remove its attached sizing decoy session"

  side_command "$side" set-option -u -t "=$INNER_SESSION:" status ||
    fixture_failure "$side could not restore inherited status after attached sizing"
  side_command "$side" set-option -u -t "=$INNER_SESSION:" default-size ||
    fixture_failure "$side could not restore inherited default-size after attached sizing"
  side_command "$side" set-window-option -u -t "$INNER_WINDOW_TARGET" window-size ||
    fixture_failure "$side could not restore inherited window-size after attached sizing"
  tmux_outer_command resize-window -t "$primary_target" -x 80 -y 24 ||
    fixture_failure "$side could not restore its primary outer window size"
  wait_for_outer_pane_size "$primary_target" 80x24
  wait_for_session_client_sizes "$side" "$INNER_SESSION" 80x24
  wait_for_client_state "$side" root
}

probe_detach_client_tty() {
  local side="$1"
  local client_tty
  local output

  client_tty="$(tmux_outer_command display-message -p -t "$OUTER_SESSION:$side" '#{pane_tty}')"
  if [[ "$client_tty" != /dev/* ]]; then
    fixture_failure "$side outer pane did not expose an attached client tty"
  fi
  side_command "$side" detach-client -t "$client_tty" ||
    fixture_failure "$side could not detach its tty-targeted client"
  wait_for_attached_client_count "$side" 0
  output="$(side_command "$side" display-message -p -c missing-client -t "=$CHOOSER_SESSION:" '#{client_name}|#{client_session}|#{session_name}')" ||
    fixture_failure "$side could not print display-message facts without attached clients"
  output="${output//$'\r'/}"
  if [ "$output" != "||$CHOOSER_SESSION" ]; then
    fixture_failure "$side did not leave client facts empty without attached clients; got: ${output:-<empty>}"
  fi
}

ATTACHED_ENV_SELECTED=global-old \
  ATTACHED_ENV_KEEP=global-old \
  ATTACHED_ENV_MISSING=global-old \
  zz_command daemon >"$DAEMON_STDOUT" 2>"$DAEMON_STDERR" &
ZZ_PID=$!
wait_for_socket

zz_command new-session -d -c "$COMMAND_CWD" -s "$INNER_SESSION" || fixture_failure "could not create zz session"
ATTACHED_ENV_SELECTED=global-old \
  ATTACHED_ENV_KEEP=global-old \
  ATTACHED_ENV_MISSING=global-old \
  tmux_inner_start new-session -d -c "$COMMAND_CWD" -s "$INNER_SESSION" || fixture_failure "could not create tmux session"
zz_command rename-window -t "$INNER_SESSION:0" main || fixture_failure "could not name zz window"
tmux_inner_command rename-window -t "$INNER_SESSION:0" main || fixture_failure "could not name tmux window"
zz_command new-session -d -s "$CHOOSER_SESSION" || fixture_failure "could not create zz chooser session"
tmux_inner_command new-session -d -s "$CHOOSER_SESSION" || fixture_failure "could not create tmux chooser session"
configure_side zz || fixture_failure "could not configure zz bindings"
configure_side tmux || fixture_failure "could not configure tmux bindings"
zz_command set-buffer -b keep ATTACHED_BUFFER_KEEP || fixture_failure "could not create zz keep buffer"
tmux_inner_command set-buffer -b keep ATTACHED_BUFFER_KEEP || fixture_failure "could not create tmux keep buffer"
zz_command set-buffer -b drop 'printf ATTACHED_BUFFER_ROW_OK' || fixture_failure "could not create zz row buffer"
tmux_inner_command set-buffer -b drop 'printf ATTACHED_BUFFER_ROW_OK' || fixture_failure "could not create tmux row buffer"
assert_buffer_parity $'drop\nkeep'

write_attach zz "$ZZ_ATTACH"
write_attach tmux "$TMUX_ATTACH"

tmux_outer_start new-session -d -x 80 -y 24 -s "$OUTER_SESSION" -n zz || fixture_failure "could not start outer tmux"
tmux_outer_command set-option -g remain-on-exit on || fixture_failure "could not retain outer panes"
tmux_outer_command respawn-pane -k -t "$OUTER_SESSION:zz" "$ZZ_ATTACH" || fixture_failure "could not start zz attach"
tmux_outer_command new-window -d -t "$OUTER_SESSION" -n tmux "$TMUX_ATTACH" || fixture_failure "could not start tmux attach"

wait_for_client_state zz root
wait_for_client_state tmux root
probe_side zz
probe_side tmux
probe_alert_message_lifecycle zz
probe_alert_message_lifecycle tmux
probe_command_prompt zz
probe_command_prompt tmux
probe_choose_tree zz
probe_choose_tree tmux
probe_chooser_filter_fallback zz
probe_chooser_filter_fallback tmux
probe_choose_buffer_row zz
probe_choose_buffer_row tmux
probe_choose_buffer_delete zz
probe_choose_buffer_delete tmux
assert_buffer_parity keep
probe_display_panes_target_no_select zz
probe_display_panes_target_no_select tmux
probe_display_message_unattached_target zz
probe_display_message_unattached_target tmux
probe_list_keys_single zz
probe_list_keys_single tmux
probe_nested_attach zz
probe_nested_attach tmux
probe_control_nested_terminal_facts zz
probe_control_nested_terminal_facts tmux
probe_forced_nested_attaches zz
probe_forced_nested_attaches tmux
probe_source_file_cwd zz
probe_source_file_cwd tmux
probe_source_file_output zz
probe_source_file_output tmux
probe_command_output_navigation zz
probe_command_output_navigation tmux
assert_buffer_parity keep
probe_source_file_depth zz
probe_source_file_depth tmux
probe_client_environment zz
probe_client_environment tmux
probe_requested_client_flags zz
probe_requested_client_flags tmux
probe_attached_client_sizing zz
probe_attached_client_sizing tmux
probe_client_context_formats zz
probe_client_context_formats tmux
probe_client_event_hooks zz
probe_client_event_hooks tmux
probe_detach_client_tty zz
probe_detach_client_tty tmux

printf 'attached-client compatibility: PASS\n'
