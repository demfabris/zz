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
SOURCE_CWD="$SCRATCH_DIR/client [literal]*? cwd"
SOURCE_CONFIG_DIR="$SOURCE_CWD/config-$INNER_SESSION"
SOURCE_DEPTH_DIR="$SCRATCH_DIR/source-depth"
SOURCE_OUTPUT_DIR="$SCRATCH_DIR/o"
SOURCE_OUTPUT_CHILD="$SOURCE_OUTPUT_DIR/c"
SOURCE_OUTPUT_ROOT="$SOURCE_OUTPUT_DIR/r"
DAEMON_STDOUT="$SCRATCH_DIR/zz-daemon.stdout"
DAEMON_STDERR="$SCRATCH_DIR/zz-daemon.stderr"
ZZ_ATTACH="$SCRATCH_DIR/attach-zz"
TMUX_ATTACH="$SCRATCH_DIR/attach-tmux"
ZZ_PID=""

mkdir -p "$ZZ_HOME" "$ZZ_CONFIG_HOME" "$TMUX_HOME" "$TMUX_CONFIG_HOME" \
  "$OUTER_HOME" "$OUTER_CONFIG_HOME" "$SOURCE_CONFIG_DIR" "$SOURCE_DEPTH_DIR" \
  "$SOURCE_OUTPUT_DIR"
printf 'set-option -g @attached_source_order glob-first\n' >"$SOURCE_CONFIG_DIR/10.conf"
printf 'set-option -g @attached_source_order glob-second\n' >"$SOURCE_CONFIG_DIR/20.conf"

printf 'set-option -g @attached_depth_leaf yes\n' >"$SOURCE_DEPTH_DIR/leaf.conf"
printf 'display-message -p ATTACHED_CHILD_ONE\nlist-sessions -F "ATTACHED_CHILD_LIST_#{session_name}"\n' \
  >"$SOURCE_OUTPUT_CHILD"
printf 'display-message -p ATTACHED_ROOT_ONE\nsource-file -v %s\ndisplay-message -p ATTACHED_ROOT_TWO\n' \
  "$SOURCE_OUTPUT_CHILD" >"$SOURCE_OUTPUT_ROOT"
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

capture_screen() {
  local side="$1"
  tmux_outer_command capture-pane -p -S - -t "$OUTER_SESSION:$side"
}

capture_current_screen() {
  local side="$1"
  tmux_outer_command capture-pane -p -t "$OUTER_SESSION:$side"
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

  printf '#!/usr/bin/env bash\n' >"$destination"
  printf 'cd -- %q\n' "$SOURCE_CWD" >>"$destination"
  if [ "$side" = "zz" ]; then
    printf 'exec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL HOME=%q XDG_CONFIG_HOME=%q TMUX_TMPDIR=/tmp %q --socket %q attach-session -t %q\n' \
      "$ZZ_HOME" "$ZZ_CONFIG_HOME" "$ZZ_BIN" "$ZZ_SOCKET" "$INNER_SESSION" >>"$destination"
  else
    printf 'exec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL HOME=%q XDG_CONFIG_HOME=%q TMUX_TMPDIR=/tmp %q -L %q attach-session -t %q\n' \
      "$TMUX_HOME" "$TMUX_CONFIG_HOME" "$TMUX_BIN" "$INNER_SOCKET_NAME" "$INNER_SESSION" >>"$destination"
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
    captured="$(side_command "$side" capture-pane -p -J -S - -t "$INNER_SESSION:0.0" 2>/dev/null || true)"
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

  captured="$(side_command "$side" capture-pane -p -J -S - -t "$INNER_SESSION:0.0" 2>/dev/null || true)"
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

zz_command daemon >"$DAEMON_STDOUT" 2>"$DAEMON_STDERR" &
ZZ_PID=$!
wait_for_socket

zz_command new-session -d -c "$SOURCE_CWD" -s "$INNER_SESSION" || fixture_failure "could not create zz session"
tmux_inner_start new-session -d -c "$SOURCE_CWD" -s "$INNER_SESSION" || fixture_failure "could not create tmux session"
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
probe_source_file_depth zz
probe_source_file_depth tmux
probe_detach_client_tty zz
probe_detach_client_tty tmux

printf 'attached-client compatibility: PASS\n'
