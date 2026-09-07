#!/usr/bin/env bash
# Row-level differential for the tmux status line.
#
# The scenario harness compares daemon facts and never a rendered row, and the
# attached-client fixture drives the raw TUI at a width where its sidebar is
# visible, so no campaign proof ever compared what a status format DRAWS.
# This fixture runs both binaries inside an outer pinned tmux at a width below
# the sidebar's auto-hide threshold, where the zz TUI paints the daemon's
# expanded status rows across the full width, applies the same status options
# to both, and diffs the bytes of the last row of each pane after every step,
# escapes included. It also walks the band the sidebar's auto-hide covers, 80
# and 100 columns, and diffs the row's TEXT there: status-left, the window
# list and status-right have to be drawn in the pin's order at a width where
# the raw TUI has no sidebar to hide status-left in. A divergence is a finding, not a failure of this script:
# it exits 1 so a caller can gate on it, and prints both rows so the next
# lane has the measurement.
set -eEuo pipefail

usage() {
  printf 'usage: compat/status-row.sh [ZZ_BIN [TMUX_BIN]]\n' >&2
  printf '       ZZ_BIN=path TMUX_BIN=path compat/status-row.sh\n' >&2
}

COMPAT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd -- "$COMPAT_DIR/.." && pwd)"
ZZ_INPUT="${1:-${ZZ_BIN:-$REPO_DIR/target/debug/zz}}"
TMUX_INPUT="${2:-${TMUX_BIN:-${ZZ_COMPAT_TMUX:-$COMPAT_DIR/.cache/tmux-src/tmux}}}"
[ "$#" -le 2 ] || { usage; exit 2; }

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

COLUMNS_UNDER_TEST=79
ROWS_UNDER_TEST=24
SCRATCH_DIR="$(mktemp -d /tmp/zzsr.XXXXXX)"
TOKEN="${SCRATCH_DIR##*.}"
OUTER_SOCKET_NAME="zzsro-$TOKEN"
INNER_SOCKET_NAME="zzsri-$TOKEN"
ZZ_SOCKET="/tmp/zzsr-$TOKEN.sock"
OUTER_SESSION="driver"
INNER_SESSION="rows"
PANE_TITLE="rowtitle"
ZZ_HOME="$SCRATCH_DIR/zz-home"
TMUX_HOME="$SCRATCH_DIR/tmux-home"
OUTER_HOME="$SCRATCH_DIR/outer-home"
ZZ_PID=""
FAILURES=0
mkdir -p "$ZZ_HOME" "$TMUX_HOME" "$OUTER_HOME"

scrubbed() {
  env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL \
    TMUX_TMPDIR=/tmp "$@"
}
tmux_outer_command() {
  scrubbed HOME="$OUTER_HOME" XDG_CONFIG_HOME="$OUTER_HOME/config" \
    "$TMUX_BIN" -L "$OUTER_SOCKET_NAME" "$@"
}
zz_command() {
  scrubbed HOME="$ZZ_HOME" XDG_CONFIG_HOME="$ZZ_HOME/config" \
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
  rm -f -- "$ZZ_SOCKET" "/tmp/tmux-$(id -u)/$OUTER_SOCKET_NAME" "/tmp/tmux-$(id -u)/$INNER_SOCKET_NAME"
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
  die "$label did not happen within 10 seconds"
}

outer_pane_is() {
  local target="$1"
  local expected="$2"
  [ "$(tmux_outer_command display-message -p -t "$target" '#{pane_width}x#{pane_height}' 2>/dev/null)" = "$expected" ]
}

last_row_bytes() {
  local side="$1"
  tmux_outer_command capture-pane -p -e -t "=$OUTER_SESSION:$side" | tail -n 1
}

row_settled() {
  local side="$1"
  local expected="$2"
  [ "$(last_row_bytes "$side")" = "$expected" ]
}

write_attach() {
  local side="$1"
  local destination="$2"
  printf '#!/usr/bin/env bash\n' >"$destination"
  if [ "$side" = zz ]; then
    printf 'exec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL HOME=%q XDG_CONFIG_HOME=%q TMUX_TMPDIR=/tmp %q --socket %q attach-session -t %q\n' \
      "$ZZ_HOME" "$ZZ_HOME/config" "$ZZ_BIN" "$ZZ_SOCKET" "=$INNER_SESSION" >>"$destination"
  else
    printf 'exec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL HOME=%q XDG_CONFIG_HOME=%q TMUX_TMPDIR=/tmp %q -L %q attach-session -t %q\n' \
      "$TMUX_HOME" "$TMUX_HOME/config" "$TMUX_BIN" "$INNER_SOCKET_NAME" "=$INNER_SESSION" >>"$destination"
  fi
  chmod +x "$destination"
}

zz_command -f /dev/null daemon >"$SCRATCH_DIR/zz-daemon.out" 2>"$SCRATCH_DIR/zz-daemon.err" &
ZZ_PID=$!
wait_for "zz daemon socket" test -S "$ZZ_SOCKET"
client_attached() {
  local side="$1"
  [ "$(side_command "$side" list-clients -F '#{client_session}' 2>/dev/null)" = "$INNER_SESSION" ]
}

# Both binaries attached to a session of the given width, inside one outer
# pinned tmux window each. Called once per width under test.
attach_both_at() {
  local columns="$1"
  tmux_outer_command kill-server >/dev/null 2>&1 || true
  zz_command kill-session -t "=$INNER_SESSION" >/dev/null 2>&1 || true
  tmux_inner_command kill-session -t "=$INNER_SESSION" >/dev/null 2>&1 || true
  zz_command new-session -d -s "$INNER_SESSION" -x "$columns" -y "$ROWS_UNDER_TEST" ||
    die "could not create the zz session"
  tmux_inner_command -f /dev/null new-session -d -s "$INNER_SESSION" -x "$columns" -y "$ROWS_UNDER_TEST" ||
    die "could not create the tmux session"
  tmux_outer_command -f /dev/null new-session -d -s "$OUTER_SESSION" -n zz \
    -x "$columns" -y "$ROWS_UNDER_TEST" "$SCRATCH_DIR/attach-zz.sh" ||
    die "could not create the outer session"
  tmux_outer_command set-option -g status off
  tmux_outer_command new-window -d -n tmux "$SCRATCH_DIR/attach-tmux.sh"
  wait_for "outer zz pane at ${columns}x${ROWS_UNDER_TEST}" outer_pane_is "=$OUTER_SESSION:zz" "${columns}x${ROWS_UNDER_TEST}"
  wait_for "outer tmux pane at ${columns}x${ROWS_UNDER_TEST}" outer_pane_is "=$OUTER_SESSION:tmux" "${columns}x${ROWS_UNDER_TEST}"
  wait_for "zz client attached" client_attached zz
  wait_for "tmux client attached" client_attached tmux
  # The default status-right ends in #{=21:pane_title}. window.c:1141 seeds a
  # pane's title from gethostname, while a zz pane with the default shell
  # reports its shell name through zz's shell integration - the recorded
  # pane.runtime-facts decision, not a divergence of this row. Pinning the
  # title on both sides takes that one token out of the comparison so the
  # WHOLE row can be asserted.
  side_command zz select-pane -t "=$INNER_SESSION:0.0" -T "$PANE_TITLE" ||
    die "zz refused select-pane -T"
  side_command tmux select-pane -t "=$INNER_SESSION:0.0" -T "$PANE_TITLE" ||
    die "tmux refused select-pane -T"
}

set_on_both() {
  side_command zz set-option -g "$1" "$2" || die "zz refused set-option -g $1"
  side_command tmux set-option -g "$1" "$2" || die "tmux refused set-option -g $1"
}

unset_on_both() {
  side_command zz set-option -gu "$1" || die "zz refused set-option -gu $1"
  side_command tmux set-option -gu "$1" || die "tmux refused set-option -gu $1"
}

# Server options. status-style defaults to bg=themegreen,fg=themeblack, and
# server_client_update_theme_colours expands the dark-theme-* or light-theme-*
# option for each client from the `theme` option and the theme the client
# reported (server-client.c:3085). Both binaries' clients run inside the same
# outer pinned tmux, so both are answered the same way and the resolved set has
# to be the same on both sides.
set_server_on_both() {
  side_command zz set-option -s "$1" "$2" || die "zz refused set-option -s $1"
  side_command tmux set-option -s "$1" "$2" || die "tmux refused set-option -s $1"
}

unset_server_on_both() {
  side_command zz set-option -su "$1" || die "zz refused set-option -su $1"
  side_command tmux set-option -su "$1" || die "tmux refused set-option -su $1"
}

write_attach zz "$SCRATCH_DIR/attach-zz.sh"
write_attach tmux "$SCRATCH_DIR/attach-tmux.sh"

# The band the sidebar's auto-hide covers. status.c draws status-left, the
# window list and status-right in one row at every width; the raw TUI has no
# sidebar below 109 columns, so all three have to be in that row there too.
# Text, not bytes: colour encoding is the corpus's claim below, not this one.
last_row_text() {
  tmux_outer_command capture-pane -p -t "=$OUTER_SESSION:$1" | tail -n 1
}
compare_band() {
  local columns="$1"
  local zz_row tmux_row token
  sleep 0.3
  zz_row="$(last_row_text zz)"
  tmux_row="$(last_row_text tmux)"
  if [ "$zz_row" != "$tmux_row" ]; then
    FAILURES=$((FAILURES + 1))
    printf 'DIFF  status-left band at %s columns\n' "$columns"
    printf '      tmux: %q\n' "$tmux_row"
    printf '      zz:   %q\n' "$zz_row"
    return 0
  fi
  for token in LEFT CUSTOM RIGHT; do
    case "$zz_row" in
    *"$token"*) ;;
    *)
      FAILURES=$((FAILURES + 1))
      printf 'DIFF  status-left band at %s columns: %s is not drawn\n' "$columns" "$token"
      printf '      zz:   %q\n' "$zz_row"
      return 0
      ;;
    esac
  done
  printf 'ok    status-left band at %s columns\n' "$columns"
}

printf 'status-left band below the sidebar auto-hide threshold (pin %s)\n' "$(basename -- "$TMUX_BIN")"
BAND_WIDTHS=(80 100)
BAND_CHECKS=0
for band_columns in "${BAND_WIDTHS[@]}"; do
  attach_both_at "$band_columns"
  set_on_both status-left LEFT
  set_on_both status-right RIGHT
  set_on_both window-status-current-format CUSTOM
  BAND_CHECKS=$((BAND_CHECKS + 1))
  compare_band "$band_columns"
  unset_on_both status-left
  unset_on_both status-right
  unset_on_both window-status-current-format
done

attach_both_at "$COLUMNS_UNDER_TEST"

# The corpus: one status option per step, applied identically to both servers.
# Each step names the option and the value; the row is captured after both
# sides have repainted at least once. Keep the values free of clocks and of
# anything host-specific so the bytes can be equal at all.
# Recorded, not asserted, while presentation:tui-status-row-theme-colours-per-
# client is open: the daemon does not publish the ten resolved theme colours,
# so the raw TUI resolves themeX from the pin's dark defaults and cannot follow
# a user-set dark-theme-* or a forced `theme`. The rows print both sides so the
# lane that closes the item has the bytes without re-deriving them.
SERVER_CORPUS=(
  "dark-theme-green|colour124"
  "dark-theme-black|colour231"
  "theme|light"
)

CORPUS=(
  "status-left|[#{session_name}]"
  "status-right|#{window_width}x#{window_height} #{client_width}"
  "window-status-format|#I:#W#F"
  "window-status-current-format|#I:#W#F*"
  "status-style|bg=blue,fg=white"
  "status-left-style|bold"
  "status-justify|centre"
  "status-position|bottom"
)

# The registered-divergence twin of compare_step: prints both rows and never
# raises FAILURES, so an open item stays visible without turning the tool red.
record_step() {
  local step="$1"
  local zz_row tmux_row
  sleep 0.3
  zz_row="$(last_row_bytes zz)"
  tmux_row="$(last_row_bytes tmux)"
  if [ "$zz_row" = "$tmux_row" ]; then
    printf 'note  %s: identical, presentation:tui-status-row-theme-colours-per-client may be closed\n' "$step"
    return 0
  fi
  printf 'note  %s: presentation:tui-status-row-theme-colours-per-client, open\n' "$step"
  printf '      tmux: %q\n' "$tmux_row"
  printf '      zz:   %q\n' "$zz_row"
}

# The default status-right carries a clock, and the two sides are captured one
# after the other, so a minute boundary between the captures is a difference
# that says nothing. Retry a bounded number of times before reporting one.
compare_step() {
  local step="$1"
  local zz_row tmux_row attempt
  for ((attempt = 0; attempt < 8; attempt++)); do
    sleep 0.3
    zz_row="$(last_row_bytes zz)"
    tmux_row="$(last_row_bytes tmux)"
    if [ "$zz_row" = "$tmux_row" ]; then
      printf 'ok    %s\n' "$step"
      return 0
    fi
  done
  FAILURES=$((FAILURES + 1))
  printf 'DIFF  %s\n' "$step"
  printf '      tmux: %s\n' "$(printf '%s' "$tmux_row" | od -An -c | tr -s ' \n' ' ')"
  printf '      zz:   %s\n' "$(printf '%s' "$zz_row" | od -An -c | tr -s ' \n' ' ')"
  printf '      tmux: %q\n' "$tmux_row"
  printf '      zz:   %q\n' "$zz_row"
}

printf 'status row differential at %sx%s (pin %s)\n' "$COLUMNS_UNDER_TEST" "$ROWS_UNDER_TEST" "$(basename -- "$TMUX_BIN")"
compare_step "defaults"
for entry in "${SERVER_CORPUS[@]}"; do
  option="${entry%%|*}"
  value="${entry#*|}"
  set_server_on_both "$option" "$value"
  record_step "-s $option = $value"
done
for entry in "${SERVER_CORPUS[@]}"; do
  unset_server_on_both "${entry%%|*}"
done
for entry in "${CORPUS[@]}"; do
  option="${entry%%|*}"
  value="${entry#*|}"
  set_on_both "$option" "$value"
  compare_step "$option = $value"
done

if [ "$FAILURES" -ne 0 ]; then
  printf '%s of %s comparisons differ\n' "$FAILURES" "$((${#CORPUS[@]} + 1 + BAND_CHECKS))"
  exit 1
fi
printf 'all %s comparisons identical, %s rows recorded not asserted\n' "$((${#CORPUS[@]} + 1 + BAND_CHECKS))" "${#SERVER_CORPUS[@]}"
