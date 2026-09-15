#!/usr/bin/env bash
# TWO CLIENTS, ONE PANE, ONE OF THEM IN COPY MODE.
#
# attempt-05's review found that the copy mode whose revision the three
# grid-reading mouse formats answer off was picked by COUNTING the views that
# held one, so two clients in copy mode on a pane fell through to the live
# grid and a second client's click was answered off the first client's frozen
# revision. This probe drives what that review could not: two real clients
# attached to one session, one of them in copy mode and scrolled a page back,
# the other not, each sent the SAME real SGR report at the SAME pane cell.
#
# The pin holds ONE mode per pane and zz one per view, so the two binaries are
# expected to differ for the client that is NOT in copy mode. That difference
# is the measurement this probe exists to take.
#
# Usage: two-client-probe.sh <zz binary> <tmux pin>
set -uo pipefail

ZZ_BIN="${1:?usage: two-client-probe.sh <zz> <tmux>}"
TMUX_BIN="${2:?usage: two-client-probe.sh <zz> <tmux>}"
COLUMNS_UNDER_TEST=80
ROWS_UNDER_TEST=24
SCRATCH_DIR="$(mktemp -d /tmp/zz2c.XXXXXX)"
TOKEN="${SCRATCH_DIR##*.}"
OUTER_SOCKET_NAME="zz2co-$TOKEN"
INNER_SOCKET_NAME="zz2ci-$TOKEN"
ZZ_SOCKET="/tmp/zz2c-$TOKEN.sock"
OUTER_SESSION="driver"
INNER_SESSION="two"
ZZ_HOME="$SCRATCH_DIR/zz-home"
TMUX_HOME="$SCRATCH_DIR/tmux-home"
OUTER_HOME="$SCRATCH_DIR/outer-home"
mkdir -p "$ZZ_HOME" "$TMUX_HOME" "$OUTER_HOME"

scrubbed() {
  env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL \
    -u XDG_STATE_HOME -u ZZ_LOG_DIR -u XDG_RUNTIME_DIR \
    TMUX_TMPDIR=/tmp ZZ_TRAY=0 "$@"
}
outer() {
  scrubbed HOME="$OUTER_HOME" XDG_CONFIG_HOME="$OUTER_HOME/config" \
    "$TMUX_BIN" -L "$OUTER_SOCKET_NAME" "$@"
}
inner() {
  case "$SIDE" in
  zz) scrubbed HOME="$ZZ_HOME" XDG_CONFIG_HOME="$ZZ_HOME/config" \
    "$ZZ_BIN" --socket "$ZZ_SOCKET" "$@" ;;
  tmux) scrubbed HOME="$TMUX_HOME" XDG_CONFIG_HOME="$TMUX_HOME/config" \
    "$TMUX_BIN" -L "$INNER_SOCKET_NAME" "$@" ;;
  esac
}
cleanup() {
  local status=$? pid
  trap - EXIT INT TERM
  set +e
  outer kill-server >/dev/null 2>&1
  SIDE=zz inner kill-server >/dev/null 2>&1
  SIDE=tmux inner kill-server >/dev/null 2>&1
  for pid in $(pgrep -f -- "$SCRATCH_DIR" 2>/dev/null); do kill "$pid" >/dev/null 2>&1; done
  rm -f -- "$ZZ_SOCKET" "/tmp/tmux-$(id -u)/$OUTER_SOCKET_NAME" "/tmp/tmux-$(id -u)/$INNER_SOCKET_NAME"
  rm -rf -- "$SCRATCH_DIR"
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
die() { printf 'error: %s\n' "$*" >&2; exit 2; }
wait_for() {
  local label="$1" attempt
  shift
  for ((attempt = 0; attempt < 300; attempt++)); do
    "$@" >/dev/null 2>&1 && return 0
    sleep 0.05
  done
  die "$label did not happen in time"
}
send_bytes() {
  local window="$1" text="$2" hex
  hex="$(printf '%s' "$text" | od -An -tx1 | tr -s ' \n' '  ')"
  # shellcheck disable=SC2086
  outer send-keys -t "=$OUTER_SESSION:$window" -H $hex || die "send-keys -H refused"
}
send_mouse() {
  send_bytes "$1" "$(printf '\033[<%s;%s;%s%s' "$2" "$3" "$4" "$5")"
  send_bytes "$1" "$(printf '\033[<%s;%s;%sm' "$2" "$3" "$4")"
}
option_value() { inner show-options -gqv "$1" 2>/dev/null; }
option_set() { [ -n "$(option_value "$1")" ]; }
clients_attached() { [ "$(inner list-clients -F x 2>/dev/null | wc -l)" = 2 ]; }
in_copy_mode() { [ "$(inner display-message -p -t "=$INNER_SESSION:0.0" '#{pane_in_mode}')" = 1 ]; }
screen_has() {
  outer capture-pane -p -S 0 -E "$((ROWS_UNDER_TEST - 1))" -t "=$OUTER_SESSION:$1" |
    grep -Fq -- "$2"
}

# One client per outer window, both attached to the same inner session, so both
# look at the same pane.
write_attach() {
  local destination="$1"
  printf '#!/usr/bin/env bash\n' >"$destination"
  if [ "$SIDE" = zz ]; then
    printf 'exec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL -u XDG_STATE_HOME -u XDG_RUNTIME_DIR HOME=%q XDG_CONFIG_HOME=%q TMUX_TMPDIR=/tmp ZZ_TRAY=0 %q --socket %q attach-session -t %q\n' \
      "$ZZ_HOME" "$ZZ_HOME/config" "$ZZ_BIN" "$ZZ_SOCKET" "=$INNER_SESSION" >>"$destination"
  else
    printf 'exec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL -u XDG_STATE_HOME -u XDG_RUNTIME_DIR HOME=%q XDG_CONFIG_HOME=%q TMUX_TMPDIR=/tmp %q -L %q attach-session -t %q\n' \
      "$TMUX_HOME" "$TMUX_HOME/config" "$TMUX_BIN" "$INNER_SOCKET_NAME" "=$INNER_SESSION" >>"$destination"
  fi
  chmod +x "$destination"
}

probe_side() {
  SIDE="$1"
  local attach="$SCRATCH_DIR/attach-$SIDE.sh" left top column row
  write_attach "$attach"
  if [ "$SIDE" = zz ]; then
    inner new-session -d -s "$INNER_SESSION" -x "$COLUMNS_UNDER_TEST" -y "$ROWS_UNDER_TEST" \
      "ENV= PS1='\$ ' exec /bin/sh" || die "could not create the zz session"
  else
    inner -f /dev/null new-session -d -s "$INNER_SESSION" -x "$COLUMNS_UNDER_TEST" \
      -y "$ROWS_UNDER_TEST" "ENV= PS1='\$ ' exec /bin/sh" || die "could not create the tmux session"
  fi
  inner set-option -g status-right '' >/dev/null
  inner set-option -g status-left L >/dev/null
  inner set-option -g automatic-rename off >/dev/null
  inner set-option -g default-shell /bin/sh >/dev/null
  inner set-option -g mouse on >/dev/null
  inner set-option -g history-limit 2000 >/dev/null

  outer -f /dev/null new-session -d -s "$OUTER_SESSION" -n first \
    -x "$COLUMNS_UNDER_TEST" -y "$ROWS_UNDER_TEST" "$attach" || die "could not create the outer session"
  outer set-option -g status off >/dev/null
  outer new-window -d -n second "$attach" >/dev/null
  wait_for 'both clients attached' clients_attached

  # Sixty labelled rows, so the pane has real scrollback and every row names
  # itself. A page back is then a different row under the same cell.
  inner send-keys -t "=$INNER_SESSION:0.0" \
    "i=0; while [ \$i -lt 60 ]; do printf 'ROW%03d word%03d tail\\n' \$i \$i; i=\$((i+1)); done; printf 'SAMPLEEND\\n'" Enter >/dev/null
  wait_for 'the sample on the first client' screen_has first SAMPLEEND
  wait_for 'the sample on the second client' screen_has second SAMPLEEND

  inner bind-key -n C-MouseDown3Pane set-option -gF @mousectx \
    '[#{mouse_word}][#{mouse_line}]' >/dev/null || die "$SIDE refused the binding"

  # The FIRST client alone enters copy mode and pages back.
  send_bytes first "$(printf '\002[')"
  wait_for 'the first client in copy mode' in_copy_mode
  send_bytes first "$(printf '\033[5~')"
  sleep 0.5

  left="$(inner display-message -p -t "=$INNER_SESSION:0.0" '#{pane_left}')"
  top="$(inner display-message -p -t "=$INNER_SESSION:0.0" '#{pane_top}')"
  column="$((left + 2))"
  row="$((top + 2))"

  local window answer
  for window in first second; do
    inner set-option -gu @mousectx >/dev/null
    send_mouse "$window" 18 "$column" "$row" M
    wait_for "the $window client's binding published" option_set @mousectx
    answer="$(option_value @mousectx)"
    printf '%-4s ONE IN COPY MODE  %-6s client: %s\n' "$SIDE" "$window" "$answer"
  done

  # Now BOTH clients are in copy mode, at different offsets: the first still a
  # page back, the second entering at the bottom. This is the case the old
  # arm inverted - it counted the views holding a mode, found two, and fell
  # through to the LIVE grid for both, where the pin answers off the pane's
  # one mode for both.
  send_bytes second "$(printf '\002[')"
  sleep 0.5
  for window in first second; do
    inner set-option -gu @mousectx >/dev/null
    send_mouse "$window" 18 "$column" "$row" M
    wait_for "the $window client's binding published" option_set @mousectx
    answer="$(option_value @mousectx)"
    printf '%-4s BOTH IN COPY MODE %-6s client: %s\n' "$SIDE" "$window" "$answer"
  done
  printf '%-4s pane_in_mode after the probe: %s\n' "$SIDE" \
    "$(inner display-message -p -t "=$INNER_SESSION:0.0" '#{pane_in_mode}')"
  outer kill-server >/dev/null 2>&1
  inner kill-server >/dev/null 2>&1
  sleep 0.3
}

printf 'two clients on one pane, the FIRST in copy mode one page back, the SECOND not\n'
printf 'cell: pane_left+2, pane_top+2, a real SGR report with the control bit (button 18)\n'
printf 'binding: bind -n C-MouseDown3Pane set-option -gF @mousectx "[#{mouse_word}][#{mouse_line}]"\n\n'
probe_side zz
printf '\n'
probe_side tmux
