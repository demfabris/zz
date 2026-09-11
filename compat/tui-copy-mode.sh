#!/usr/bin/env bash
# Pane copy mode and search, driven through the attached client's stdin.
#
# tui-stock-keys.sh types the stock ROOT and PREFIX chords; nothing in the
# campaign ever typed a key into the COPY tables. The closures under
# copy-mode.action-fidelity and copy-mode.command-fidelity drove the same
# engine through `send-keys -X`, which is the command path: it names the action
# directly and never consults `mode-keys`, the copy key tables or the prompt
# that a stock binding opens. This fixture types the stock chords instead, into
# both binaries at once, and compares what the two clients then show.
#
# THE DECODED SCREEN IS THE CONTRACT (tui-screen-diff.sh's header carries the
# full rule). Both binaries attach inside one outer pinned tmux, one window
# each; every chord is `send-keys` against the OUTER pane, so the bytes reach
# the inner client's terminal the way a keyboard delivers them, and every
# comparison reads the outer pane's own decoded grid.
#
# SIX CHANNELS. A case compares
#   rows    every visible cell of the outer pane, escapes included
#   text    the same cells with the styles left out: every glyph and every
#           column, so a case whose colours belong to a recorded divergence
#           still asserts what the two screens SAY
#   cursor  the outer cursor tuple, which in copy mode is the COPY cursor
#   facts   the LOGICAL position each side's own server reports: pane_mode,
#           pane_in_mode, copy_cursor_x, selection_present and copy_cursor_line
#   view    where each side left the viewport: copy_cursor_y and
#           scroll_position, kept apart from the logical position because the
#           two answers come apart
#   buffer  `show-buffer` from each side, byte for byte through cmp(1)
# and NAMES the channels it asserts. A channel a case does not name is recorded
# with the measurement that says why, so nothing is waived by omission. A case
# that names `none` asserts NOTHING and is recorded in full; no corpus case has
# named it since the whole-page step was fixed on 2026-09-10, and the summary
# line still counts such cases separately so the headline never borrows their
# weight. A recorded reason that starts `SIBLING:<lane> ` is a divergence that
# lane's landing closes; nothing else may be recorded.
#
# THE COPY CHANNEL LOOKS AT A RECTANGLE. The selection group toggles the
# rectangle on and back OFF before it copies, so its bytes are an ordinary
# selection's; the two rectangle groups after it copy with the rectangle still
# ON, once with the right edge inside every selected line and once past the end
# of the last one, which is where the pin's trailing-newline rule turns
# (window-copy.c:5737: in the vi table the pin keeps the final newline there).
#
# THE MODE FORMATS. selection_active, rectangle_toggle and pane_search_string
# are asserted by assert_mode_formats at named checkpoints, both sides through
# display-message -p, inside copy mode and outside it. The pin keeps the last
# search on the pane (window-copy.c:4508, format.c:2418), so pane_search_string
# answers it after the mode is gone and on the next entry too, and that entry's
# n searches UP for it (window-copy.c:566). Both are asserted, in both tables,
# after a backward search and after a forward one.
#
# CONTROLLED PANE CONTENT. Both panes clear their screen and scrollback and
# then `cat` the SAME numbered file: 60 lines `line-NN filler-NN`, with
# `needle-12` and `needle-48` as the two search targets, and a final SEEDEND
# line that is the settle marker. Identical bytes, identical path, so every
# copy-mode geometry question has the same answer on both sides.
#
# CONTROLLED DYNAMIC VALUES, pinned on both sides before any chord:
#   status off           for the movement corpus. zz paints its mode badge
#                        (`COPY 12/61`) into the status row's right side
#                        (crates/zz-tui/src/render.rs status_indicators) where
#                        the pin paints no badge at all; that is the
#                        presentation.native-status stance, not a copy-mode
#                        fact, and the presentation group at the end MEASURES
#                        it with the status row back on rather than hiding it.
#   copy-mode-position-format ''
#                        the pin draws `[#{copy_position}/#{copy_position_limit}]`
#                        right-aligned into the pane's own top row
#                        (window-copy.c:5228); the raw TUI side of
#                        options.native-mode-styles was decided 2026-09-10
#                        and lands with the modes lane. Emptied for
#                        the corpus and MEASURED at its default in the
#                        presentation group.
#   mode-keys            set explicitly per table on both sides, never
#                        inherited: the pin seeds it from $EDITOR/$VISUAL and
#                        the harness scrubs both.
#   status-right '' / status-left L / automatic-rename off / default-command /
#   select-pane -T       the same pins tui-stock-keys.sh carries and for the
#                        same reasons.
#   mode-style, copy-mode-selection-style, copy-mode-match-style,
#   copy-mode-mark-style, status-style, window-status-current-style,
#   message-style, pane-border-style, pane-active-border-style
#                        every surface a case paints, pinned to the same
#                        explicit RGB value on both sides. RGB on purpose: zz
#                        resolves a named or indexed colour through its palette
#                        before it writes and the pin keeps the class it was
#                        given, which is the recorded colour-promotion
#                        divergence tui-screen-diff.sh owns.
# Nothing else is masked.
#
# ONE SIZE, 80x24. Copy-mode geometry is a pane-relative question and the pane
# size channel belongs to tui-pane-geometry.sh; 80 columns is below the retired
# 109-column sidebar threshold, so no asserted case can invoke zz's own chrome.
#
# SETTLE. A case names a bounded OBSERVABLE (a server format, or a substring on
# the screen) and waits for it per side, then waits for that side's screen AND
# its outer cursor tuple to stop changing between two polls. The cursor is part
# of the settle because a repaint that rewrites identical cells (the prefix
# armed inside copy mode does exactly that) can be read by the outer tmux in two
# chunks: at 63acab72 one loaded run caught zz's cursor at the end of the last
# row it had written (2,23) before the frame's final cursor placement (9,22).
# A side that never reaches its observable is a comparison RESULT, not a crash.
# No wait here is a sleep.
#
# --self-check drives one deliberate one-sided difference per asserted channel
# and requires the comparison to catch it in that channel, plus two
# equivalences it must NOT report. A fixture that only passes has proved
# nothing.
#
# ZZ_COPY_CAPTURE_DIR, when set, keeps every capture, cursor tuple, facts line
# and buffer as text. A bounded wait that runs out dumps diagnostics into
# ZZ_COPY_DIAGNOSTICS_DIR or a fresh /tmp directory it names.
set -eEuo pipefail

usage() {
  printf 'usage: compat/tui-copy-mode.sh [--self-check] [ZZ_BIN [TMUX_BIN]]\n' >&2
  printf '       ZZ_BIN=path TMUX_BIN=path compat/tui-copy-mode.sh\n' >&2
}

COMPAT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd -- "$COMPAT_DIR/.." && pwd)"
SELF_CHECK=0
POSITIONAL=()
for argument in "$@"; do
  case "$argument" in
  --self-check) SELF_CHECK=1 ;;
  -*)
    usage
    exit 2
    ;;
  *) POSITIONAL+=("$argument") ;;
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

COLUMNS_UNDER_TEST=80
ROWS_UNDER_TEST=24
PANE_TITLE="copymode"
WINDOW_NAME="win"
SESSION_NAME="copy"
SCRATCH_DIR="$(mktemp -d /tmp/zzcm.XXXXXX)"
TOKEN="${SCRATCH_DIR##*.}"
OUTER_SOCKET_NAME="zzcmo-$TOKEN"
INNER_SOCKET_NAME="zzcmi-$TOKEN"
ZZ_SOCKET="/tmp/zzcm-$TOKEN.sock"
OUTER_SESSION="driver"
ZZ_HOME="$SCRATCH_DIR/zz-home"
TMUX_HOME="$SCRATCH_DIR/tmux-home"
OUTER_HOME="$SCRATCH_DIR/outer-home"
ZZ_LOG_DIR="$SCRATCH_DIR/zz-logs"
ZZ_CLIENT_STDERR="$SCRATCH_DIR/zz-client.err"
TMUX_CLIENT_STDERR="$SCRATCH_DIR/tmux-client.err"
LINES_FILE="$SCRATCH_DIR/lines.txt"
CLAMP_LINES_FILE="$SCRATCH_DIR/clamp-lines.txt"
CAPTURE_DIR="${ZZ_COPY_CAPTURE_DIR:-}"
DIAGNOSTICS_DIR=""
CASE_LABEL=""
ZZ_PID=""
FAILURES=0
CHECKS=0
RECORDS=0
ASSERTING=0
LAST_ROWS_DIFFERED=0
LAST_TEXT_DIFFERED=0
LAST_CURSOR_DIFFERED=0
LAST_FACTS_DIFFERED=0
LAST_VIEW_DIFFERED=0
LAST_BUFFER_DIFFERED=0
LAST_FORMATS_DIFFERED=0
LAST_MISSING_OBSERVABLE=""
INNER_SHELL="ENV= PS1='\$ ' exec /bin/sh"
mkdir -p "$ZZ_HOME" "$TMUX_HOME" "$OUTER_HOME" "$ZZ_LOG_DIR"
[ -z "$CAPTURE_DIR" ] || mkdir -p "$CAPTURE_DIR"

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

diagnostics_dir() {
  if [ -z "$DIAGNOSTICS_DIR" ]; then
    DIAGNOSTICS_DIR="${ZZ_COPY_DIAGNOSTICS_DIR:-$(mktemp -d /tmp/zzcm-diag.XXXXXX)}"
    mkdir -p "$DIAGNOSTICS_DIR"
  fi
  printf '%s\n' "$DIAGNOSTICS_DIR"
}

dump_diagnostics() {
  local label="$1"
  local dir side log
  dir="$(diagnostics_dir)"
  {
    printf 'wait that ran out: %s\n' "$label"
    printf 'at: %s\n' "$(date -Is 2>/dev/null || date)"
    printf 'case under test: %s\n' "${CASE_LABEL:-none yet}"
    printf 'zz: %s\n' "$ZZ_BIN"
    printf 'tmux: %s\n' "$TMUX_BIN"
    printf 'zz socket: %s\n' "$ZZ_SOCKET"
    printf 'outer socket: %s\n' "$OUTER_SOCKET_NAME"
    printf 'inner tmux socket: %s\n' "$INNER_SOCKET_NAME"
  } >"$dir/what-fired.txt" 2>&1 || true
  for side in zz tmux; do
    tmux_outer_command capture-pane -p -e -S - -t "=$OUTER_SESSION:$side" \
      >"$dir/outer-$side.screen.txt" 2>&1 || true
    side_command "$side" list-panes -a \
      -F '#{session_name}:#{window_index}.#{pane_index} #{pane_width}x#{pane_height} mode=#{pane_mode}' \
      >"$dir/$side.list-panes.txt" 2>&1 || true
    side_command "$side" display-message -p -t "=$SESSION_NAME:0.0" "$FACTS_FORMAT" \
      >"$dir/$side.facts.txt" 2>&1 || true
    side_command "$side" list-keys -T copy-mode >"$dir/$side.copy-mode-keys.txt" 2>&1 || true
    side_command "$side" list-keys -T copy-mode-vi >"$dir/$side.copy-mode-vi-keys.txt" 2>&1 || true
  done
  cp -f -- "$SCRATCH_DIR/zz-daemon.out" "$dir/zz-daemon.stdout.txt" 2>/dev/null || true
  cp -f -- "$SCRATCH_DIR/zz-daemon.err" "$dir/zz-daemon.stderr.txt" 2>/dev/null || true
  cp -f -- "$ZZ_CLIENT_STDERR" "$dir/zz-client.stderr.txt" 2>/dev/null || true
  cp -f -- "$TMUX_CLIENT_STDERR" "$dir/tmux-client.stderr.txt" 2>/dev/null || true
  for log in "$ZZ_LOG_DIR"/*; do
    [ -f "$log" ] || continue
    cp -f -- "$log" "$dir/ring-$(basename -- "$log").txt" 2>/dev/null || true
  done
  printf 'diagnostics retained in %s:\n' "$dir" >&2
  ls -1 -- "$dir" >&2 || true
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
  dump_diagnostics "$label"
  die "$label did not happen within 10 seconds"
}

CURSOR_FORMAT='#{cursor_x},#{cursor_y} flag=#{cursor_flag} pane=#{pane_width}x#{pane_height}'
# THE LOGICAL POSITION, read from each side's own server. copy_cursor_line is
# last because it is the only field that can carry a space.
FACTS_FORMAT='mode=#{pane_mode} in=#{pane_in_mode} column=#{copy_cursor_x} selection=#{selection_present} line=[#{copy_cursor_line}]'
# THE VIEW, kept apart from the logical position because the two answers can
# come apart: an engine that moves the cursor row where the pin moves the view
# lands on the same logical line with a different view. Its own channel, so
# the self-check can show that exact split being caught.
VIEW_FORMAT='row=#{copy_cursor_y} scroll=#{scroll_position}'
MODE_FORMATS='selection_active=[#{selection_active}] rectangle_toggle=[#{rectangle_toggle}] pane_search_string=[#{pane_search_string}]'

outer_pane_is() {
  [ "$(tmux_outer_command display-message -p -t "$1" '#{pane_width}x#{pane_height}' 2>/dev/null)" = "$2" ]
}
client_attached() {
  [ "$(side_command "$1" list-clients -F '#{client_session}' 2>/dev/null)" = "$SESSION_NAME" ]
}
capture_screen() {
  tmux_outer_command capture-pane -p -e -S 0 -E "$((ROWS_UNDER_TEST - 1))" \
    -t "=$OUTER_SESSION:$1"
}
capture_plain() {
  tmux_outer_command capture-pane -p -S 0 -E "$((ROWS_UNDER_TEST - 1))" \
    -t "=$OUTER_SESSION:$1"
}
cursor_tuple() {
  tmux_outer_command display-message -p -t "=$OUTER_SESSION:$1" "$CURSOR_FORMAT"
}
side_facts() {
  side_command "$1" display-message -p -t "=$SESSION_NAME:0.0" "$FACTS_FORMAT" 2>/dev/null ||
    printf 'no answer\n'
}
side_view() {
  side_command "$1" display-message -p -t "=$SESSION_NAME:0.0" "$VIEW_FORMAT" 2>/dev/null ||
    printf 'no answer\n'
}
side_mode_formats() {
  side_command "$1" display-message -p -t "=$SESSION_NAME:0.0" "$MODE_FORMATS" 2>/dev/null ||
    printf 'no answer\n'
}
# Called straight after a copy_case, so it reads a settled checkpoint.
assert_mode_formats() {
  local label="$1"
  local zz_formats tmux_formats
  CHECKS=$((CHECKS + 1))
  ASSERTING=$((ASSERTING + 1))
  zz_formats="$(side_mode_formats zz)"
  tmux_formats="$(side_mode_formats tmux)"
  LAST_FORMATS_DIFFERED=0
  if [ "$zz_formats" = "$tmux_formats" ]; then
    printf 'ok    %s-formats %s\n' "$label" "$tmux_formats"
    return 0
  fi
  LAST_FORMATS_DIFFERED=1
  FAILURES=$((FAILURES + 1))
  printf 'DIFF  %s-formats\n' "$label"
  printf '      formats tmux: %s\n' "$tmux_formats"
  printf '      formats zz:   %s\n' "$zz_formats"
}
# Bytes, not a string: `$(…)` eats trailing newlines and a copied line's
# terminator is exactly the kind of difference this channel exists to catch.
side_buffer() {
  local side="$1"
  local destination="$2"
  side_command "$side" show-buffer >"$destination" 2>/dev/null || : >"$destination"
}

write_attach() {
  local side="$1"
  local destination="$2"
  printf '#!/usr/bin/env bash\n' >"$destination"
  if [ "$side" = zz ]; then
    printf 'exec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL -u XDG_STATE_HOME HOME=%q XDG_CONFIG_HOME=%q ZZ_LOG_DIR=%q TMUX_TMPDIR=/tmp %q --socket %q attach-session -t %q 2> >(tee -a %q >&2)\n' \
      "$ZZ_HOME" "$ZZ_HOME/config" "$ZZ_LOG_DIR" "$ZZ_BIN" "$ZZ_SOCKET" "=$SESSION_NAME" "$ZZ_CLIENT_STDERR" >>"$destination"
  else
    printf 'exec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL -u XDG_STATE_HOME HOME=%q XDG_CONFIG_HOME=%q TMUX_TMPDIR=/tmp %q -L %q attach-session -t %q 2> >(tee -a %q >&2)\n' \
      "$TMUX_HOME" "$TMUX_HOME/config" "$TMUX_BIN" "$INNER_SOCKET_NAME" "=$SESSION_NAME" "$TMUX_CLIENT_STDERR" >>"$destination"
  fi
  chmod +x "$destination"
}

OWNED_OPTIONS=(
  status status-left status-right automatic-rename default-command repeat-time
  mode-keys mode-style copy-mode-position-format copy-mode-selection-style
  copy-mode-match-style copy-mode-current-match-style copy-mode-mark-style
  copy-mode-line-numbers
  pane-border-style pane-active-border-style message-style status-style
  window-status-current-style
)
PINNED_STYLES=(
  'status-style bg=#00af00,fg=#101010'
  'window-status-current-style bg=#00af00,fg=#101010'
  'pane-border-style fg=#7f7f7f,bg=#101010'
  'pane-active-border-style fg=#00af00,bg=#101010'
  'message-style bg=#cdcd00,fg=#101010'
  'mode-style bg=#cdcd00,fg=#101010'
  'copy-mode-selection-style bg=#cdcd00,fg=#101010'
  'copy-mode-match-style bg=#00cdcd,fg=#101010'
  'copy-mode-current-match-style bg=#cd00cd,fg=#101010'
  'copy-mode-mark-style bg=#cd00cd,fg=#101010'
)
pin_dynamic_values() {
  local side="$1"
  local option entry
  for option in "${OWNED_OPTIONS[@]}"; do
    side_command "$side" set-option -gu "$option" >/dev/null 2>&1 || true
  done
  side_command "$side" set-option -g status-right '' || die "$side refused status-right"
  side_command "$side" set-option -g status-left L || die "$side refused status-left"
  side_command "$side" set-option -g automatic-rename off || die "$side refused automatic-rename"
  side_command "$side" set-option -g default-command "$INNER_SHELL" ||
    die "$side refused default-command"
  side_command "$side" set-option -g copy-mode-position-format '' ||
    die "$side refused copy-mode-position-format"
  side_command "$side" set-option -g status off || die "$side refused status off"
  for entry in "${PINNED_STYLES[@]}"; do
    side_command "$side" set-option -g "${entry%% *}" "${entry#* }" ||
      die "$side refused ${entry%% *}"
  done
}

set_on_both() {
  side_command zz set-option -g "$1" "$2" || die "zz refused set-option -g $1"
  side_command tmux set-option -g "$1" "$2" || die "tmux refused set-option -g $1"
}

kill_every_session() {
  local side="$1"
  local session
  while read -r session; do
    [ -n "$session" ] || continue
    side_command "$side" kill-session -t "=$session" >/dev/null 2>&1 || true
  done < <(side_command "$side" list-sessions -F '#{session_name}' 2>/dev/null || true)
}
delete_every_buffer() {
  local side="$1"
  local buffer
  while read -r buffer; do
    [ -n "$buffer" ] || continue
    side_command "$side" delete-buffer -b "$buffer" >/dev/null 2>&1 || true
  done < <(side_command "$side" list-buffers -F '#{buffer_name}' 2>/dev/null || true)
}

generate_lines() {
  local index
  : >"$LINES_FILE"
  for ((index = 1; index <= 60; index++)); do
    if [ "$index" -eq 12 ] || [ "$index" -eq 48 ]; then
      printf 'line-%02d needle-%02d\n' "$index" "$index" >>"$LINES_FILE"
    else
      printf 'line-%02d filler-%02d\n' "$index" "$index" >>"$LINES_FILE"
    fi
  done
  printf 'SEEDEND\n' >>"$LINES_FILE"
}

# The page-clamp seed, the cycle-5 review's probe2 content: 77 numbered lines,
# then four short ones and the marker, so the rows above the prompt are
# aaaaaaaaaaaa (18), bbbbbbbbbbbbbbbbbbbb (19), cc (20), empty (21) and
# SEEDEND (22), with the shell prompt `$` on row 23.
generate_clamp_lines() {
  local index
  : >"$CLAMP_LINES_FILE"
  for ((index = 1; index <= 77; index++)); do
    printf 'line-%02d filler-%02d\n' "$index" "$index" >>"$CLAMP_LINES_FILE"
  done
  printf 'aaaaaaaaaaaa\nbbbbbbbbbbbbbbbbbbbb\ncc\n\nSEEDEND\n' >>"$CLAMP_LINES_FILE"
}

send_pane_both() {
  local side
  for side in zz tmux; do
    side_command "$side" send-keys -t "=$SESSION_NAME:0.0" "$1" Enter ||
      die "$side refused send-keys"
  done
}

# The screen AND the scrollback are erased before the seed, so every copy-mode
# geometry question (scroll_position, copy_cursor_y, what page-up reaches) has
# one answer on both sides.
seed_pane() {
  local file="${1:-$LINES_FILE}"
  local side
  send_pane_both "printf '\\033[2J\\033[3J\\033[H'"
  for side in zz tmux; do
    wait_settled "$side" 'the cleared screen'
  done
  send_pane_both "cat $file"
  for side in zz tmux; do
    if ! await_observable "$side" screen SEEDEND; then
      die "$side never showed the seeded content"
    fi
    wait_settled "$side" 'the seeded screen'
  done
}

attach_both_at() {
  local columns="$COLUMNS_UNDER_TEST"
  local rows="$ROWS_UNDER_TEST"
  tmux_outer_command kill-server >/dev/null 2>&1 || true
  kill_every_session zz
  kill_every_session tmux
  delete_every_buffer zz
  delete_every_buffer tmux
  side_command zz unbind-key -T copy-mode-vi Z >/dev/null 2>&1 || true
  side_command tmux unbind-key -T copy-mode-vi Z >/dev/null 2>&1 || true
  zz_command new-session -d -s "$SESSION_NAME" -n "$WINDOW_NAME" -x "$columns" -y "$rows" \
    "$INNER_SHELL" || die "could not create the zz session"
  tmux_inner_command -f /dev/null new-session -d -s "$SESSION_NAME" -n "$WINDOW_NAME" \
    -x "$columns" -y "$rows" "$INNER_SHELL" || die "could not create the tmux session"
  pin_dynamic_values zz
  pin_dynamic_values tmux
  tmux_outer_command -f /dev/null new-session -d -s "$OUTER_SESSION" -n zz \
    -x "$columns" -y "$rows" "$SCRATCH_DIR/attach-zz.sh" ||
    die "could not create the outer session"
  tmux_outer_command set-option -g status off
  tmux_outer_command set-option -g remain-on-exit on
  tmux_outer_command new-window -d -n tmux "$SCRATCH_DIR/attach-tmux.sh"
  wait_for "outer zz pane at ${columns}x${rows}" outer_pane_is "=$OUTER_SESSION:zz" "${columns}x${rows}"
  wait_for "outer tmux pane at ${columns}x${rows}" outer_pane_is "=$OUTER_SESSION:tmux" "${columns}x${rows}"
  wait_for "zz client attached" client_attached zz
  wait_for "tmux client attached" client_attached tmux
  side_command zz rename-window -t "=$SESSION_NAME:0" "$WINDOW_NAME" || die 'zz refused rename-window'
  side_command tmux rename-window -t "=$SESSION_NAME:0" "$WINDOW_NAME" || die 'tmux refused rename-window'
  side_command zz select-pane -t "=$SESSION_NAME:0.0" -T "$PANE_TITLE" || die 'zz refused select-pane'
  side_command tmux select-pane -t "=$SESSION_NAME:0.0" -T "$PANE_TITLE" || die 'tmux refused select-pane'
  wait_settled zz 'the fresh zz screen'
  wait_settled tmux 'the fresh tmux screen'
}

type_side() {
  local side="$1"
  shift
  tmux_outer_command send-keys -t "=$OUTER_SESSION:$side" "$@" ||
    die "the outer tmux refused send-keys for $side"
}
type_both() {
  type_side zz "$@"
  type_side tmux "$@"
}
type_prefix_both() {
  type_both C-b "$@"
}

wait_settled() {
  local side="$1"
  local label="$2"
  local previous=""
  local current attempt
  for ((attempt = 0; attempt < 200; attempt++)); do
    current="$(capture_plain "$side" 2>/dev/null || true)
$(cursor_tuple "$side" 2>/dev/null || true)"
    if [ -n "$previous" ] && [ "$current" = "$previous" ]; then
      return 0
    fi
    previous="$current"
    sleep 0.05
  done
  dump_diagnostics "$label"
  die "$label did not settle within 10 seconds"
}

await_observable() {
  local side="$1"
  local kind="$2"
  local value="$3"
  local attempt format expected
  for ((attempt = 0; attempt < 200; attempt++)); do
    case "$kind" in
    format)
      format="${value%%=*}"
      expected="${value#*=}"
      if [ "$(side_command "$side" display-message -p -t "=$SESSION_NAME:0.0" "$format" 2>/dev/null)" = "$expected" ]; then
        return 0
      fi
      ;;
    screen)
      if capture_plain "$side" 2>/dev/null | grep -Fq "$value"; then return 0; fi
      ;;
    none) return 0 ;;
    esac
    sleep 0.05
  done
  return 1
}

compare_sides() {
  local name="$1"
  local zz_rows tmux_rows zz_cursor tmux_cursor zz_facts tmux_facts index differing total
  local zz_view tmux_view zz_text tmux_text text_differing
  mapfile -t zz_rows < <(capture_screen zz)
  mapfile -t tmux_rows < <(capture_screen tmux)
  mapfile -t zz_text < <(capture_plain zz)
  mapfile -t tmux_text < <(capture_plain tmux)
  zz_cursor="$(cursor_tuple zz)"
  tmux_cursor="$(cursor_tuple tmux)"
  zz_facts="$(side_facts zz)"
  tmux_facts="$(side_facts tmux)"
  zz_view="$(side_view zz)"
  tmux_view="$(side_view tmux)"
  side_buffer zz "$SCRATCH_DIR/zz.buffer"
  side_buffer tmux "$SCRATCH_DIR/tmux.buffer"
  if [ -n "$CAPTURE_DIR" ]; then
    printf '%s\n' "${zz_rows[@]-}" >"$CAPTURE_DIR/$name.zz.screen.txt"
    printf '%s\n' "${tmux_rows[@]-}" >"$CAPTURE_DIR/$name.tmux.screen.txt"
    {
      printf 'cursor zz:   %s\n' "$zz_cursor"
      printf 'cursor tmux: %s\n' "$tmux_cursor"
      printf 'facts zz:   %s\n' "$zz_facts"
      printf 'facts tmux: %s\n' "$tmux_facts"
      printf 'view zz:    %s\n' "$zz_view"
      printf 'view tmux:  %s\n' "$tmux_view"
      printf 'buffer zz:\n'
      od -c -- "$SCRATCH_DIR/zz.buffer"
      printf 'buffer tmux:\n'
      od -c -- "$SCRATCH_DIR/tmux.buffer"
    } >"$CAPTURE_DIR/$name.facts.txt"
  fi
  total="$ROWS_UNDER_TEST"
  differing=-1
  for ((index = 0; index < total; index++)); do
    if [ "${zz_rows[index]-}" != "${tmux_rows[index]-}" ]; then
      differing="$index"
      break
    fi
  done
  text_differing=-1
  for ((index = 0; index < total; index++)); do
    if [ "${zz_text[index]-}" != "${tmux_text[index]-}" ]; then
      text_differing="$index"
      break
    fi
  done
  LAST_ROWS_DIFFERED=0
  LAST_TEXT_DIFFERED=0
  LAST_CURSOR_DIFFERED=0
  LAST_FACTS_DIFFERED=0
  LAST_VIEW_DIFFERED=0
  LAST_BUFFER_DIFFERED=0
  [ "$differing" -lt 0 ] || LAST_ROWS_DIFFERED=1
  [ "$text_differing" -lt 0 ] || LAST_TEXT_DIFFERED=1
  [ "$zz_cursor" = "$tmux_cursor" ] || LAST_CURSOR_DIFFERED=1
  [ "$zz_facts" = "$tmux_facts" ] || LAST_FACTS_DIFFERED=1
  [ "$zz_view" = "$tmux_view" ] || LAST_VIEW_DIFFERED=1
  cmp -s -- "$SCRATCH_DIR/zz.buffer" "$SCRATCH_DIR/tmux.buffer" || LAST_BUFFER_DIFFERED=1
  if [ "$LAST_ROWS_DIFFERED" -eq 0 ] && [ "$LAST_TEXT_DIFFERED" -eq 0 ] &&
    [ "$LAST_CURSOR_DIFFERED" -eq 0 ] && [ "$LAST_FACTS_DIFFERED" -eq 0 ] &&
    [ "$LAST_VIEW_DIFFERED" -eq 0 ] && [ "$LAST_BUFFER_DIFFERED" -eq 0 ] &&
    [ -z "$LAST_MISSING_OBSERVABLE" ]; then
    return 0
  fi
  printf '      case %s\n' "$name"
  if [ -n "$LAST_MISSING_OBSERVABLE" ]; then
    printf '      never reached the observable: %s\n' "$LAST_MISSING_OBSERVABLE"
  fi
  if [ "$LAST_ROWS_DIFFERED" -eq 1 ]; then
    printf '      first differing row %s of %s\n' "$differing" "$total"
    printf '        tmux: %s\n' "$(printf '%s' "${tmux_rows[differing]-}" | cat -v)"
    printf '        zz:   %s\n' "$(printf '%s' "${zz_rows[differing]-}" | cat -v)"
  fi
  if [ "$LAST_TEXT_DIFFERED" -eq 1 ]; then
    printf '      first differing glyph row %s of %s\n' "$text_differing" "$total"
    printf '        tmux text: %s\n' "$(printf '%s' "${tmux_text[text_differing]-}" | cat -v)"
    printf '        zz text:   %s\n' "$(printf '%s' "${zz_text[text_differing]-}" | cat -v)"
  fi
  if [ "$LAST_CURSOR_DIFFERED" -eq 1 ]; then
    printf '      cursor tmux: %s\n' "$tmux_cursor"
    printf '      cursor zz:   %s\n' "$zz_cursor"
  fi
  if [ "$LAST_FACTS_DIFFERED" -eq 1 ]; then
    printf '      facts tmux: %s\n' "$tmux_facts"
    printf '      facts zz:   %s\n' "$zz_facts"
  fi
  if [ "$LAST_VIEW_DIFFERED" -eq 1 ]; then
    printf '      view tmux:  %s\n' "$tmux_view"
    printf '      view zz:    %s\n' "$zz_view"
  fi
  if [ "$LAST_BUFFER_DIFFERED" -eq 1 ]; then
    printf '      buffer tmux: %s\n' "$(od -c -- "$SCRATCH_DIR/tmux.buffer" | head -4 | tr '\n' '|')"
    printf '      buffer zz:   %s\n' "$(od -c -- "$SCRATCH_DIR/zz.buffer" | head -4 | tr '\n' '|')"
  fi
  return 1
}

# A case: wait for each side's observable, settle, compare, then judge only the
# channels the case names. `reason` is the measurement behind every channel it
# leaves out.
copy_case() {
  local name="$1"
  local kind="$2"
  local observable="$3"
  local mode="${4:-rows,text,cursor,facts,view,buffer}"
  local reason="${5:-}"
  local side
  CASE_LABEL="$name"
  LAST_MISSING_OBSERVABLE=""
  for side in zz tmux; do
    if ! await_observable "$side" "$kind" "$observable"; then
      LAST_MISSING_OBSERVABLE="${LAST_MISSING_OBSERVABLE}${LAST_MISSING_OBSERVABLE:+, }$side never showed $kind $observable"
    fi
  done
  wait_settled zz "$name on the zz screen"
  wait_settled tmux "$name on the tmux screen"
  CHECKS=$((CHECKS + 1))
  case "$mode" in
  *rows* | *text* | *cursor* | *facts* | *view* | *buffer*) ASSERTING=$((ASSERTING + 1)) ;;
  esac
  if compare_sides "$name"; then
    printf 'ok    %s\n' "$name"
    return 0
  fi
  local asserted=0
  case "$mode" in *rows*) asserted=$((asserted + LAST_ROWS_DIFFERED)) ;; esac
  case "$mode" in *text*) asserted=$((asserted + LAST_TEXT_DIFFERED)) ;; esac
  case "$mode" in *cursor*) asserted=$((asserted + LAST_CURSOR_DIFFERED)) ;; esac
  case "$mode" in *facts*) asserted=$((asserted + LAST_FACTS_DIFFERED)) ;; esac
  case "$mode" in *view*) asserted=$((asserted + LAST_VIEW_DIFFERED)) ;; esac
  case "$mode" in *buffer*) asserted=$((asserted + LAST_BUFFER_DIFFERED)) ;; esac
  [ -z "$LAST_MISSING_OBSERVABLE" ] || asserted=$((asserted + 1))
  if [ "$asserted" -gt 0 ]; then
    FAILURES=$((FAILURES + 1))
    printf 'DIFF  %s\n' "$name"
  else
    [ -n "$reason" ] || die "recorded case $name says nothing about why"
    RECORDS=$((RECORDS + 1))
    if [ "$mode" = none ]; then
      printf 'note  %s asserts no channel, recorded in full: %s\n' "$name" "$reason"
    else
      printf 'note  %s asserted %s, the rest recorded: %s\n' "$name" "$mode" "$reason"
    fi
  fi
  return 0
}

# --- the corpus ------------------------------------------------------------

# THE MEASURED DIVERGENCES the corpus records rather than asserts, each
# measured at this fixture's own checkpoints at 80x24 against tmux d77c9dc6.
# Every one left is the current-match style, which the modes landing did not
# paint; its reason keeps `SIBLING:modes ` with the cycle-5 gate's measurement
# appended until an owner lands the painting.
#
# CLOSED 2026-09-10 and asserted since: the half page and the search landing
# (window_copy_pageup1 moves the view by screen_size_y/2 and keeps the cursor
# row; window_copy_scroll_to lands a search a quarter screen above the bottom),
# the whole page (screen_size_y-2 = 22 lines, cursor row kept), the vi
# rectangle past the end of the last selected line (window-copy.c:5737 keeps
# the final newline, 36 bytes where zz used to copy 35) and the prefix inside a
# copy table (server-client.c:1417). The fixes are in crates/zz-terminal and
# crates/zz-protocol. Closed the same day: the search that outlives the mode.
# zz kept the string on the per-client copy session, so after q it answered
# #{pane_search_string} empty where the pin answered the last search, and a
# fresh entry's n did nothing where the pin searched up for it; the pane's
# terminal now keeps it (crates/zz-terminal) and the daemon answers the format
# from there (crates/zz-daemon status.rs). Closed at the cycle-5 gate after the
# modes landing and asserted on every channel since: the selection style, the
# search prompt's message-style row and the copy-mode position indicator (the
# 18 cases that recorded SELECTION_REASON, PROMPT_REASON and POSITION_REASON).
#
# THE SELECTION, before the modes landing. The pin paints the selected cells with
# copy-mode-selection-style, which defaults to #{E:mode-style}; the raw TUI
# paints an OverlaySpan of kind Selection in reverse video and never reads the
# option. Measured on a two-line selection with mode-style pinned to
# bg=#cdcd00,fg=#101010: the pin opened the run with
# \e[38;2;16;16;16m\e[48;2;205;205;0m and closed it with \e[39m\e[49m on the
# last glyph of the last selected line, and zz opened \e[7m and closed \e[0m
# one cell further along. The raw TUI side of options.native-mode-styles was
# decided 2026-09-10 and lands with the modes lane (SIBLING:modes). The
# BUFFER channel asserts straight through the divergence: the bytes the two
# engines copy out of that selection are identical.
# MATCH_REASON. The pin paints the current search match with
# copy-mode-current-match-style, pinned here to bg=#cd00cd,fg=#101010; the raw
# TUI paints it in reverse video. Measured 2026-09-10 on line-12 after `needle`:
# the pin wrote \e[38;2;16;16;16m\e[48;2;205;0;205mneedle\e[39m\e[49m and zz
# \e[7mneedle\e[0m, every glyph, the cursor, the view and the buffer identical.
# The same options.native-mode-styles family as the selection.
# The cycle-5 gate flipped these cases after the modes landing and they stayed
# red on the rows channel only (the current-match cells); text, cursor, facts,
# view and buffer identical. The modes landing paints no match style, so the
# owner is still to be named.
MATCH_REASON='SIBLING:modes the pin paints the current search match with copy-mode-current-match-style; zz paints it in reverse video (kept recorded at the cycle-5 gate: flipped after the modes landing, still red on rows, owner not named)'

table_keys() {
  case "$1" in
  emacs)
    KEY_DOWN=C-n KEY_UP=C-p KEY_HALFDOWN=M-Down KEY_HALFUP=M-Up
    KEY_PAGEDOWN=C-v KEY_PAGEUP=M-v KEY_SOL=C-a KEY_EOL=C-e
    KEY_RIGHT=C-f
    KEY_BEGINSEL=C-Space KEY_RECT=R KEY_COPY=M-w KEY_CANCEL=q
    KEY_SEARCHFWD=C-s KEY_SEARCHBACK=C-r KEY_COUNT=M-5
    SEARCH_OPEN_MODE=text,cursor,facts,view,buffer
    SEARCH_OPEN_REASON="$MATCH_REASON; the incremental search already paints the current match"
    ;;
  vi)
    KEY_DOWN=j KEY_UP=k KEY_HALFDOWN=C-d KEY_HALFUP=C-u
    KEY_PAGEDOWN=NPage KEY_PAGEUP=PPage KEY_SOL=0 KEY_EOL='$'
    KEY_RIGHT=l
    KEY_BEGINSEL=Space KEY_RECT=v KEY_COPY=Enter KEY_CANCEL=q
    KEY_SEARCHFWD=/ KEY_SEARCHBACK='?' KEY_COUNT=5
    SEARCH_OPEN_MODE=rows,text,cursor,facts,view,buffer
    SEARCH_OPEN_REASON=''
    ;;
  esac
}

# Copy mode opens at oy 0 with the cursor on the live cursor, so a fresh entry
# is a resynchronisation point: a group that ends with the two views apart
# starts the next one with them together again.
enter_copy_mode() {
  local label="$1"
  type_prefix_both '['
  if ! await_observable zz format '#{pane_in_mode}=1' ||
    ! await_observable tmux format '#{pane_in_mode}=1'; then
    die "$label could not enter copy mode"
  fi
}
reenter_copy_mode() {
  local label="$1"
  type_both q
  if ! await_observable zz format '#{pane_in_mode}=0' ||
    ! await_observable tmux format '#{pane_in_mode}=0'; then
    die "$label could not leave copy mode"
  fi
  enter_copy_mode "$label"
}

run_movement() {
  local table="$1"
  table_keys "$table"

  attach_both_at
  set_on_both mode-keys "$table"
  seed_pane

  type_prefix_both '['
  copy_case "$table-enter" format '#{pane_in_mode}=1'

  type_both "$KEY_UP"
  type_both "$KEY_UP"
  copy_case "$table-cursor-up" none ''

  type_both "$KEY_DOWN"
  copy_case "$table-cursor-down" none ''

  type_both "$KEY_COUNT"
  type_both "$KEY_UP"
  copy_case "$table-count-five-up" none ''

  type_both "$KEY_EOL"
  copy_case "$table-end-of-line" none ''

  type_both "$KEY_SOL"
  copy_case "$table-start-of-line" none ''

  # The scrolling family, on its own after the line movements so its recorded
  # view divergence cannot reach an assertion that came before it.
  reenter_copy_mode "$table scrolling"
  type_both "$KEY_UP"
  type_both "$KEY_UP"
  copy_case "$table-scroll-start" none ''
  type_both "$KEY_HALFUP"
  copy_case "$table-halfpage-up" none ''
  type_both "$KEY_HALFDOWN"
  copy_case "$table-halfpage-down" none ''
  reenter_copy_mode "$table paging"
  type_both "$KEY_UP"
  type_both "$KEY_UP"
  copy_case "$table-page-start" none ''
  type_both "$KEY_PAGEUP"
  copy_case "$table-page-up" none ''
  type_both "$KEY_PAGEDOWN"
  copy_case "$table-page-down" none ''

  # Selection, rectangle and the copy, from a fresh entry.
  reenter_copy_mode "$table selection"
  type_both "$KEY_UP"
  type_both "$KEY_UP"
  type_both "$KEY_SOL"
  copy_case "$table-selection-start" none ''
  type_both "$KEY_BEGINSEL"
  copy_case "$table-begin-selection" none ''
  type_both "$KEY_DOWN"
  type_both "$KEY_EOL"
  copy_case "$table-selection-extends" none ''
  assert_mode_formats "$table-selection-extends"
  type_both "$KEY_RECT"
  copy_case "$table-rectangle-on" none ''
  assert_mode_formats "$table-rectangle-on"
  type_both "$KEY_RECT"
  copy_case "$table-rectangle-off" none ''

  # The copy leaves the mode and fills the paste buffer on both sides.
  type_both "$KEY_COPY"
  copy_case "$table-copy-and-cancel" format '#{pane_in_mode}=0'

  # A RECTANGLE COPY, which the selection group above cannot reach: it toggles
  # the rectangle back OFF before it copies, so the bytes it compares are an
  # ordinary selection's. Twice, because the pin's rule turns on where the
  # right edge falls: once with the edge INSIDE every selected line and once
  # with it PAST the end of the last one.
  enter_copy_mode "$table rectangle inside the line"
  type_both "$KEY_UP"
  type_both "$KEY_UP"
  type_both "$KEY_UP"
  type_both "$KEY_UP"
  type_both "$KEY_SOL"
  type_both "$KEY_RECT"
  type_both "$KEY_BEGINSEL"
  type_both "$KEY_DOWN"
  type_both "$KEY_COUNT"
  type_both "$KEY_RIGHT"
  copy_case "$table-rectangle-inside-line" format '#{copy_cursor_x}=5'
  assert_mode_formats "$table-rectangle-inside-line"
  type_both "$KEY_COPY"
  copy_case "$table-rectangle-inside-line-copy" format '#{pane_in_mode}=0'

  # The same rectangle four counts wider, so its right edge (column 20) is past
  # the end of every seeded line (17 glyphs) and therefore past the end of the
  # LAST selected one, which is the only line window_copy_get_selection's
  # trailing-newline guard looks at.
  enter_copy_mode "$table rectangle past the end of the line"
  type_both "$KEY_UP"
  type_both "$KEY_UP"
  type_both "$KEY_UP"
  type_both "$KEY_UP"
  type_both "$KEY_SOL"
  type_both "$KEY_RECT"
  type_both "$KEY_BEGINSEL"
  type_both "$KEY_DOWN"
  local step
  for step in 1 2 3 4; do
    type_both "$KEY_COUNT"
    type_both "$KEY_RIGHT"
  done
  copy_case "$table-rectangle-past-end-of-line" format '#{copy_cursor_x}=20'
  assert_mode_formats "$table-rectangle-past-end-of-line"
  type_both "$KEY_COPY"
  copy_case "$table-rectangle-past-end-of-line-copy" format '#{pane_in_mode}=0'
  # The paste buffer is the one channel a case cannot resynchronise by moving:
  # it keeps the last copy's bytes until the next copy replaces them. Dropping
  # every buffer on both sides is this channel's equivalent of a fresh
  # copy-mode entry, so no later case compares this copy's bytes again.
  delete_every_buffer zz
  delete_every_buffer tmux

  type_prefix_both '['
  copy_case "$table-reenter" format '#{pane_in_mode}=1'
  type_both "$KEY_CANCEL"
  copy_case "$table-cancel" format '#{pane_in_mode}=0'

  # The terminal takes input again: the shell echoes what is typed and runs it.
  type_both -l 'echo BACKALIVE'
  type_both Enter
  copy_case "$table-terminal-input-returns" screen 'BACKALIVE'

  # THE PREFIX INSIDE A COPY TABLE. Both tables bind C-b (cursor-left in
  # copy-mode, page-up in copy-mode-vi) and C-b is also the prefix, which
  # server-client.c:1417 gives precedence. The checkpoint asserts the copy
  # position and the view, which the copy binding would move (one column left
  # in emacs, a page up in vi). The `[` after it spends the prefix on
  # copy-mode, which the pin ignores for a pane already in the mode, and the
  # next key has to reach the copy table again.
  type_prefix_both '['
  if ! await_observable zz format '#{pane_in_mode}=1' ||
    ! await_observable tmux format '#{pane_in_mode}=1'; then
    die "$table prefix-precedence could not enter copy mode"
  fi
  type_both "$KEY_UP"
  type_both C-b
  copy_case "$table-prefix-takes-precedence" none ''
  type_both '['
  type_both "$KEY_UP"
  copy_case "$table-prefix-returns-to-the-copy-table" none ''
}

# THE PAGE CLAMP. window_copy_pageup1 and window_copy_pagedown1
# (window-copy.c:877-990) put the cursor back on data->lastcx and call
# window_copy_cursor_end_of_line only when (cx >= lastsx && cx != px) ||
# cx > px, so a page move pinned at the bottom that lands on a line exactly
# lastcx long keeps that column even in vi, where every other path stops one
# short of the line length. From column 1 of aaaaaaaaaaaa the landing on the
# one-glyph prompt row is column 1 on the pin; zz used to clamp it to 0.
run_page_clamp() {
  local table="$1"
  table_keys "$table"

  attach_both_at
  set_on_both mode-keys "$table"
  seed_pane "$CLAMP_LINES_FILE"

  type_prefix_both '['
  copy_case "$table-clamp-enter" format '#{pane_in_mode}=1'
  type_both "$KEY_COUNT"
  type_both "$KEY_UP"
  copy_case "$table-clamp-count-five-up" format '#{copy_cursor_line}=aaaaaaaaaaaa'
  type_both "$KEY_HALFUP"
  type_both "$KEY_HALFUP"
  copy_case "$table-clamp-halfpage-up-twice" format '#{scroll_position}=24'
  type_both "$KEY_HALFDOWN"
  type_both "$KEY_HALFDOWN"
  copy_case "$table-clamp-halfpage-down-twice" format '#{scroll_position}=0'
  type_both "$KEY_HALFDOWN"
  copy_case "$table-clamp-halfpage-down-onto-the-prompt" format '#{copy_cursor_y}=23'

  reenter_copy_mode "$table page clamp"
  type_both "$KEY_COUNT"
  type_both "$KEY_UP"
  copy_case "$table-clamp-page-count-five-up" format '#{copy_cursor_line}=aaaaaaaaaaaa'
  type_both "$KEY_PAGEDOWN"
  copy_case "$table-clamp-page-down-onto-the-prompt" format '#{copy_cursor_y}=23'
}

run_search() {
  local table="$1"
  table_keys "$table"

  attach_both_at
  set_on_both mode-keys "$table"
  seed_pane

  # ORDINARY PANE, not command output: the pane is a shell that has printed the
  # seeded file. TUI-005 opened on `terminal search is unsupported here`, which
  # crates/zz-tui/src/app.rs answers a TerminalUiCommand::BeginSearch with when
  # the pane is not the client's own command-output overlay. The stock chord
  # does not travel that way on either binary: both tables bind it to
  # `command-prompt -T search`, so this group is the ordinary-pane proof and
  # the command-output surface keeps its own name.
  type_prefix_both '['
  copy_case "$table-ordinary-pane-enter" format '#{pane_in_mode}=1'

  type_both "$KEY_SEARCHFWD"
  copy_case "$table-ordinary-pane-search-prompt" none ''

  type_both -l 'needlex'
  copy_case "$table-ordinary-pane-search-typed" none ''

  type_both BSpace
  copy_case "$table-ordinary-pane-search-backspace" none '' "$SEARCH_OPEN_MODE" "$SEARCH_OPEN_REASON"

  type_both Enter
  copy_case "$table-ordinary-pane-search-submit" none '' text,cursor,facts,view,buffer "$MATCH_REASON"
  assert_mode_formats "$table-ordinary-pane-search-submit"

  type_both n
  copy_case "$table-ordinary-pane-search-again" none '' text,cursor,facts,view,buffer "$MATCH_REASON"

  type_both N
  copy_case "$table-ordinary-pane-search-reverse" none '' text,cursor,facts,view,buffer "$MATCH_REASON"

  type_both "$KEY_SEARCHBACK"
  type_both -l 'filler-30'
  type_both Enter
  copy_case "$table-ordinary-pane-search-backward" none '' text,cursor,facts,view,buffer "$MATCH_REASON"
  assert_mode_formats "$table-ordinary-pane-search-backward"

  type_both q
  copy_case "$table-ordinary-pane-search-cancel" format '#{pane_in_mode}=0'
  assert_mode_formats "$table-ordinary-pane-search-cancel"

  # THE SEARCH OUTLIVES THE MODE. window_copy_search leaves the string on the
  # pane (window-copy.c:4508) and window_copy_common_init hands it to the next
  # entry with the direction UP (window-copy.c:566), so n on a fresh entry
  # searches up from the bottom for the last string searched on the pane.
  type_prefix_both '['
  copy_case "$table-ordinary-pane-search-reentry" format '#{pane_in_mode}=1'
  assert_mode_formats "$table-ordinary-pane-search-reentry"
  type_both n
  copy_case "$table-ordinary-pane-search-reentry-again" format '#{copy_cursor_line}=line-30 filler-30' \
    text,cursor,facts,view,buffer "$MATCH_REASON"

  # The last search above ran backward, so that landing cannot tell the UP rule
  # from a kept direction. A FORWARD search next, sent through the command path
  # on both sides as a setup step, then a fresh entry: n has to search up and
  # land on needle-48, where a kept forward direction would wrap to needle-12.
  reenter_copy_mode "$table forward search"
  side_command zz send-keys -t "=$SESSION_NAME:0.0" -X search-forward needle ||
    die 'zz refused search-forward'
  side_command tmux send-keys -t "=$SESSION_NAME:0.0" -X search-forward needle ||
    die 'tmux refused search-forward'
  await_observable zz format '#{copy_cursor_line}=line-12 needle-12' || true
  await_observable tmux format '#{copy_cursor_line}=line-12 needle-12' || true
  reenter_copy_mode "$table forward search re-entry"
  copy_case "$table-forward-search-reentry" format '#{pane_in_mode}=1'
  assert_mode_formats "$table-forward-search-reentry"
  type_both n
  copy_case "$table-forward-search-reentry-searches-up" format '#{copy_cursor_line}=line-48 needle-48' \
    text,cursor,facts,view,buffer "$MATCH_REASON"
  type_both q
  copy_case "$table-forward-search-cancel" format '#{pane_in_mode}=0'
  assert_mode_formats "$table-forward-search-cancel"
}

# THE DEFECT TUI-005 OPENED ON, measured rather than argued. zz carries a
# native `copy-mode-search-prompt` command with no counterpart on the pin; it
# emits MuxEffect::TerminalUi, which reaches the client as
# EventPayload::TerminalUiCommand, and crates/zz-tui/src/app.rs answers it with
# `terminal search is unsupported here` for every pane that is not the client's
# own command-output overlay. This group runs the command on both sides against
# an ORDINARY pane and prints what each answered. It is one-sided on purpose:
# the pin has no such command, and the stock chords the search group drives take
# the `command-prompt -T search` path on BOTH binaries, which is why that group
# asserts and this one records.
run_native_search_command() {
  local side output status
  attach_both_at
  set_on_both mode-keys emacs
  set_on_both status on
  seed_pane
  type_prefix_both '['
  if ! await_observable zz format '#{pane_in_mode}=1' ||
    ! await_observable tmux format '#{pane_in_mode}=1'; then
    die 'native-search-command could not enter copy mode'
  fi
  for side in zz tmux; do
    status=0
    output="$(side_command "$side" copy-mode-search-prompt -t "=$SESSION_NAME:0.0" 2>&1)" ||
      status=$?
    printf 'note  native-search-command %s exit %s: %s\n' "$side" "$status" "${output:-<no output>}"
  done
  wait_settled zz 'the zz screen after copy-mode-search-prompt'
  wait_settled tmux 'the tmux screen after copy-mode-search-prompt'
  for side in zz tmux; do
    printf 'note  native-search-command %s last row: %s\n' "$side" \
      "$(capture_plain "$side" | tail -1)"
  done
  type_both q
  if ! await_observable zz format '#{pane_in_mode}=0' ||
    ! await_observable tmux format '#{pane_in_mode}=0'; then
    die 'native-search-command could not leave copy mode'
  fi
}

# mode-keys changed while the client is attached: the pin reads the option in
# window_copy_init (window-copy.c:1235), so the table a copy-mode entry uses is
# the one the option held at ENTRY, not at bind time.
run_live_mode_keys() {
  attach_both_at
  set_on_both mode-keys emacs
  seed_pane

  type_prefix_both '['
  copy_case 'live-mode-keys-emacs-entry' format '#{pane_in_mode}=1'
  # `j` is a movement in vi and nothing at all in the emacs table.
  type_both j
  copy_case 'live-mode-keys-emacs-ignores-j' none ''
  type_both q
  copy_case 'live-mode-keys-emacs-cancel' format '#{pane_in_mode}=0'

  set_on_both mode-keys vi
  type_prefix_both '['
  copy_case 'live-mode-keys-vi-entry' format '#{pane_in_mode}=1'
  type_both j
  type_both j
  copy_case 'live-mode-keys-vi-moves-on-j' none ''
  type_both q
  copy_case 'live-mode-keys-vi-cancel' format '#{pane_in_mode}=0'

  # A BINDING changed while the client is attached, in the copy table the mode
  # is about to use. Z is bound in neither stock table, so the key does nothing
  # at all until the bind lands.
  type_prefix_both '['
  if ! await_observable zz format '#{pane_in_mode}=1' ||
    ! await_observable tmux format '#{pane_in_mode}=1'; then
    die 'live-binding could not enter copy mode'
  fi
  type_both '$'
  copy_case 'live-binding-before-the-bind' none ''
  type_both Z
  copy_case 'live-binding-unbound-key-does-nothing' none ''
  side_command zz bind-key -T copy-mode-vi Z send-keys -X start-of-line ||
    die 'zz refused bind-key -T copy-mode-vi'
  side_command tmux bind-key -T copy-mode-vi Z send-keys -X start-of-line ||
    die 'tmux refused bind-key -T copy-mode-vi'
  type_both Z
  copy_case 'live-binding-runs-without-a-re-entry' format '#{copy_cursor_x}=0'
  type_both q
  copy_case 'live-binding-cancel' format '#{pane_in_mode}=0'
}

# The two surfaces the corpus pins away, measured at their defaults with the
# status row back on: the pane-cell position box (options.native-mode-styles)
# and the status row without a COPY badge (presentation.native-status). The raw
# TUI side of both was decided 2026-09-10; the modes landing draws the pin's
# cells and the cycle-5 gate asserts the case on every channel.
run_presentation() {
  attach_both_at
  set_on_both mode-keys emacs
  set_on_both status on
  side_command zz set-option -gu copy-mode-position-format >/dev/null 2>&1 || true
  side_command tmux set-option -gu copy-mode-position-format >/dev/null 2>&1 || true
  seed_pane

  copy_case 'presentation-live-status-row' none ''
  type_prefix_both '['
  copy_case 'presentation-copy-mode-position' format '#{pane_in_mode}=1'
  type_both q
  copy_case 'presentation-cancel' format '#{pane_in_mode}=0'
}

# --- self-check ------------------------------------------------------------

SELF_CHECK_FAILURES=0

self_check_case() {
  local name="$1"
  local expectation="$2"
  local verdict=ok
  case "$expectation" in
  rows) [ "$LAST_ROWS_DIFFERED" -eq 1 ] || verdict='no row difference reported' ;;
  text) [ "$LAST_TEXT_DIFFERED" -eq 1 ] || verdict='no glyph difference reported' ;;
  style-only)
    if [ "$LAST_ROWS_DIFFERED" -ne 1 ] || [ "$LAST_TEXT_DIFFERED" -ne 0 ]; then
      verdict='the style-only difference did not land in the rows channel alone'
    fi
    ;;
  cursor) [ "$LAST_CURSOR_DIFFERED" -eq 1 ] || verdict='no cursor difference reported' ;;
  facts) [ "$LAST_FACTS_DIFFERED" -eq 1 ] || verdict='no copy-mode facts difference reported' ;;
  view) [ "$LAST_VIEW_DIFFERED" -eq 1 ] || verdict='no view difference reported' ;;
  view-only)
    if [ "$LAST_VIEW_DIFFERED" -ne 1 ] || [ "$LAST_FACTS_DIFFERED" -ne 0 ]; then
      verdict='the same line under a different view did not land in the view channel alone'
    fi
    ;;
  buffer) [ "$LAST_BUFFER_DIFFERED" -eq 1 ] || verdict='no paste-buffer difference reported' ;;
  formats) [ "$LAST_FORMATS_DIFFERED" -eq 1 ] || verdict='no mode-format difference reported' ;;
  none)
    if [ "$LAST_ROWS_DIFFERED" -ne 0 ] || [ "$LAST_TEXT_DIFFERED" -ne 0 ] ||
      [ "$LAST_CURSOR_DIFFERED" -ne 0 ] || [ "$LAST_FACTS_DIFFERED" -ne 0 ] ||
      [ "$LAST_VIEW_DIFFERED" -ne 0 ] || [ "$LAST_BUFFER_DIFFERED" -ne 0 ]; then
      verdict='reported a difference where both sides agree'
    fi
    ;;
  esac
  if [ "$verdict" = ok ]; then
    printf 'ok    self-check %s\n' "$name"
    return 0
  fi
  SELF_CHECK_FAILURES=$((SELF_CHECK_FAILURES + 1))
  printf 'FAIL  self-check %s: %s\n' "$name" "$verdict"
}

self_check_compare() {
  LAST_MISSING_OBSERVABLE=""
  CASE_LABEL="$1"
  wait_settled zz "$1 on the zz screen"
  wait_settled tmux "$1 on the tmux screen"
  compare_sides "$1" || true
}

run_self_check() {
  printf 'self-check: one deliberate one-sided difference per asserted channel, plus two the fixture must not report\n'

  # cursor, facts and view: one extra movement key on one side only. With the
  # copy cursor drawn as the terminal cursor it moves no cell at all, so this
  # is exactly the difference the three non-row channels exist for.
  attach_both_at
  set_on_both mode-keys vi
  seed_pane
  type_prefix_both '['
  await_observable zz format '#{pane_in_mode}=1' || true
  await_observable tmux format '#{pane_in_mode}=1' || true
  type_side zz k
  await_observable zz format '#{pane_in_mode}=1' || true
  self_check_compare one-sided-movement
  self_check_case 'cursor: one extra cursor-up on the zz side' cursor
  self_check_case 'facts: one extra cursor-up on the zz side' facts
  self_check_case 'view: one extra cursor-up on the zz side' view

  # rows: a one-sided page-up, which is the corpus's own asserted rows channel
  # carrying different cells.
  attach_both_at
  set_on_both mode-keys vi
  seed_pane
  type_prefix_both '['
  await_observable zz format '#{pane_in_mode}=1' || true
  await_observable tmux format '#{pane_in_mode}=1' || true
  type_side tmux PPage
  await_observable tmux format '#{scroll_position}=22' || true
  self_check_compare one-sided-page-up
  self_check_case 'rows: a page-up typed on the pin side only' rows
  self_check_case 'text: a page-up typed on the pin side only' text

  # rows again, this time a STYLE-only difference: copy-mode-selection-style
  # set on the pin side with a live selection changes the class of the
  # highlighted cells and nothing else. The corpus pins that option on both
  # sides, so mode-style alone would no longer reach the selection; since the
  # cycle-5 gate asserts the selection cases, this is the sabotage that shows
  # the rows channel sees a selection style that differs. The pin paints a
  # copy-mode line when the mode redraws, not on an option change, so the
  # style goes in before the selection is drawn.
  attach_both_at
  set_on_both mode-keys vi
  seed_pane
  type_prefix_both '['
  await_observable zz format '#{pane_in_mode}=1' || true
  await_observable tmux format '#{pane_in_mode}=1' || true
  side_command tmux set-option -g copy-mode-selection-style 'bg=#0000ff,fg=#ffffff' ||
    die 'tmux refused a one-sided copy-mode-selection-style'
  type_both Space
  type_both j
  type_both '$'
  self_check_compare one-sided-mode-style
  self_check_case 'rows alone: copy-mode-selection-style changed on the pin side only' style-only

  # buffer: the two sides copy different bytes.
  attach_both_at
  set_on_both mode-keys vi
  seed_pane
  side_command zz set-buffer 'zz-copied-bytes' || die 'zz refused set-buffer'
  side_command tmux set-buffer 'tmux-copied-bytes' || die 'tmux refused set-buffer'
  self_check_compare one-sided-buffer
  self_check_case 'buffer: a different byte string in the paste buffer' buffer

  # buffer again, this time through a real RECTANGLE COPY rather than
  # set-buffer: the same rectangle keys on both sides with ONE extra
  # cursor-down on the zz side, so the two engines copy a different number of
  # lines out of the same rectangle. This is the demonstration that the
  # corpus's own rectangle-copy cases can fail: they compare exactly this.
  attach_both_at
  set_on_both mode-keys emacs
  seed_pane
  type_prefix_both '['
  await_observable zz format '#{pane_in_mode}=1' || true
  await_observable tmux format '#{pane_in_mode}=1' || true
  type_both C-p
  type_both C-p
  type_both C-p
  type_both C-p
  type_both C-a
  type_both R
  type_both C-Space
  type_both C-n
  type_both M-5
  type_both C-f
  type_side zz C-n
  type_both M-w
  await_observable zz format '#{pane_in_mode}=0' || true
  await_observable tmux format '#{pane_in_mode}=0' || true
  self_check_compare one-sided-rectangle-copy
  self_check_case 'buffer: one extra line inside a rectangle copy on the zz side' buffer

  # none: the same five-line move as one counted key or five bare keys. The
  # fixture compares the RESULT of the keys, never the way they were typed.
  attach_both_at
  set_on_both mode-keys vi
  seed_pane
  type_prefix_both '['
  await_observable zz format '#{pane_in_mode}=1' || true
  await_observable tmux format '#{pane_in_mode}=1' || true
  type_side zz 5
  type_side zz k
  type_side tmux k
  type_side tmux k
  type_side tmux k
  type_side tmux k
  type_side tmux k
  self_check_compare counted-or-repeated
  self_check_case 'equivalence: 5k and five k land on the same cell' none

  # none: copy mode entered by the stock chord on one side and by the command
  # on the other. What the fixture reads is the mode, never how it was opened.
  attach_both_at
  set_on_both mode-keys vi
  seed_pane
  type_prefix_both '['
  await_observable zz format '#{pane_in_mode}=1' || true
  await_observable tmux format '#{pane_in_mode}=1' || true
  type_both q
  await_observable zz format '#{pane_in_mode}=0' || true
  await_observable tmux format '#{pane_in_mode}=0' || true
  type_side zz C-b '['
  side_command tmux copy-mode -t "=$SESSION_NAME:0.0" || die 'tmux refused copy-mode'
  await_observable zz format '#{pane_in_mode}=1' || true
  await_observable tmux format '#{pane_in_mode}=1' || true
  self_check_compare chord-or-command
  self_check_case 'equivalence: prefix [ and the copy-mode command open the same mode' none

  # view alone: a half page on one side and the same twelve lines by counted
  # cursor-up on the other, which is the split the half-page and search cases
  # used to record. Same logical line, different view.
  attach_both_at
  set_on_both mode-keys vi
  seed_pane
  type_prefix_both '['
  await_observable zz format '#{pane_in_mode}=1' || true
  await_observable tmux format '#{pane_in_mode}=1' || true
  type_both k
  type_both k
  type_side zz C-u
  type_side tmux 1
  type_side tmux 2
  type_side tmux k
  await_observable zz format '#{scroll_position}=12' || true
  await_observable tmux format '#{copy_cursor_y}=9' || true
  self_check_compare half-page-or-cursor-rows
  self_check_case 'view alone: a half page on the zz side, twelve cursor-ups on the pin side' view-only

  # facts: a whole page on one side and twenty-four counted cursor-ups on the
  # other, the step the page cases used to record.
  attach_both_at
  set_on_both mode-keys vi
  seed_pane
  type_prefix_both '['
  await_observable zz format '#{pane_in_mode}=1' || true
  await_observable tmux format '#{pane_in_mode}=1' || true
  type_both k
  type_both k
  type_side zz PPage
  type_side tmux 2
  type_side tmux 4
  type_side tmux k
  await_observable zz format '#{scroll_position}=22' || true
  await_observable tmux format '#{scroll_position}=3' || true
  self_check_compare page-or-twenty-four-rows
  self_check_case 'facts: a page on the zz side, twenty-four cursor-ups on the pin side' facts

  # buffer: the vi rectangle past the end of the last line, copied with the
  # pin's mode-keys switched to emacs just before the copy, so the pin strips
  # the final newline the vi table keeps: the rectangle byte difference the
  # corpus used to record.
  attach_both_at
  set_on_both mode-keys vi
  seed_pane
  type_prefix_both '['
  await_observable zz format '#{pane_in_mode}=1' || true
  await_observable tmux format '#{pane_in_mode}=1' || true
  type_both k
  type_both k
  type_both k
  type_both k
  type_both 0
  type_both v
  type_both Space
  type_both j
  local step
  for step in 1 2 3 4; do
    type_both 5
    type_both l
  done
  await_observable zz format '#{copy_cursor_x}=20' || true
  await_observable tmux format '#{copy_cursor_x}=20' || true
  side_command tmux set-option -g mode-keys emacs || die 'tmux refused a one-sided mode-keys'
  side_command zz send-keys -t "=$SESSION_NAME:0.0" -X copy-pipe-and-cancel ||
    die 'zz refused copy-pipe-and-cancel'
  side_command tmux send-keys -t "=$SESSION_NAME:0.0" -X copy-pipe-and-cancel ||
    die 'tmux refused copy-pipe-and-cancel'
  await_observable zz format '#{pane_in_mode}=0' || true
  await_observable tmux format '#{pane_in_mode}=0' || true
  self_check_compare rectangle-final-newline
  self_check_case 'buffer: the vi rectangle final newline kept on the zz side only' buffer

  # facts: the copy binding of C-b run on one side and the prefix on the other,
  # the precedence the prefix case used to record.
  attach_both_at
  set_on_both mode-keys vi
  seed_pane
  type_prefix_both '['
  await_observable zz format '#{pane_in_mode}=1' || true
  await_observable tmux format '#{pane_in_mode}=1' || true
  type_both k
  type_both k
  type_side zz PPage
  type_side tmux C-b
  await_observable zz format '#{scroll_position}=22' || true
  self_check_compare copy-binding-or-prefix
  self_check_case 'facts: C-b as page-up on the zz side, as the prefix on the pin side' facts

  # formats: a search, the rectangle toggle and a selection on one side only.
  attach_both_at
  set_on_both mode-keys vi
  seed_pane
  type_prefix_both '['
  await_observable zz format '#{pane_in_mode}=1' || true
  await_observable tmux format '#{pane_in_mode}=1' || true
  type_side zz /
  type_side zz -l needle
  type_side zz Enter
  type_side zz v
  type_side zz Space
  await_observable zz format '#{selection_active}=1' || true
  self_check_compare one-sided-mode-formats
  assert_mode_formats one-sided-mode-formats
  self_check_case 'formats: selection_active, rectangle_toggle and pane_search_string on the zz side only' formats

  # text: a search prompt left open on one side, the stale prompt row the emacs
  # search submit used to leave on zz.
  attach_both_at
  set_on_both mode-keys vi
  seed_pane
  type_prefix_both '['
  await_observable zz format '#{pane_in_mode}=1' || true
  await_observable tmux format '#{pane_in_mode}=1' || true
  type_side zz /
  await_observable zz screen '(search down)' || true
  self_check_compare one-sided-open-prompt
  self_check_case 'text: a search prompt open on the zz side only' text

  # formats after the mode and on a fresh entry: a search on one side only,
  # then a cancel and a re-entry on both, the answers zz used to give.
  attach_both_at
  set_on_both mode-keys vi
  seed_pane
  type_prefix_both '['
  await_observable zz format '#{pane_in_mode}=1' || true
  await_observable tmux format '#{pane_in_mode}=1' || true
  type_side zz /
  type_side zz -l needle
  type_side zz Enter
  await_observable zz format '#{copy_cursor_line}=line-12 needle-12' || true
  type_both q
  await_observable zz format '#{pane_in_mode}=0' || true
  await_observable tmux format '#{pane_in_mode}=0' || true
  assert_mode_formats one-sided-search-after-cancel
  self_check_case 'formats: pane_search_string after cancel, searched on the zz side only' formats
  type_prefix_both '['
  await_observable zz format '#{pane_in_mode}=1' || true
  await_observable tmux format '#{pane_in_mode}=1' || true
  assert_mode_formats one-sided-search-on-reentry
  self_check_case 'formats: pane_search_string on a fresh entry, searched on the zz side only' formats

  # cursor and facts: the same search and re-entry on both sides, then n on the
  # pin side only, which is what zz's fresh entry used to do with n: nothing.
  attach_both_at
  set_on_both mode-keys vi
  seed_pane
  type_prefix_both '['
  await_observable zz format '#{pane_in_mode}=1' || true
  await_observable tmux format '#{pane_in_mode}=1' || true
  type_both /
  type_both -l needle
  type_both Enter
  await_observable zz format '#{copy_cursor_line}=line-12 needle-12' || true
  await_observable tmux format '#{copy_cursor_line}=line-12 needle-12' || true
  type_both q
  await_observable zz format '#{pane_in_mode}=0' || true
  await_observable tmux format '#{pane_in_mode}=0' || true
  type_prefix_both '['
  await_observable zz format '#{pane_in_mode}=1' || true
  await_observable tmux format '#{pane_in_mode}=1' || true
  type_side tmux n
  await_observable tmux format '#{copy_cursor_line}=line-48 needle-48' || true
  self_check_compare one-sided-reentry-search-again
  self_check_case 'cursor: n on a fresh entry on the pin side only' cursor
  self_check_case 'facts: n on a fresh entry on the pin side only' facts

  # facts: the direction. On the same fresh entry after a forward search, n on
  # the pin side searches up to needle-48 and N on the zz side searches down
  # and wraps to needle-12, the landing a kept forward direction would give.
  type_both q
  await_observable zz format '#{pane_in_mode}=0' || true
  await_observable tmux format '#{pane_in_mode}=0' || true
  type_prefix_both '['
  await_observable zz format '#{pane_in_mode}=1' || true
  await_observable tmux format '#{pane_in_mode}=1' || true
  type_side tmux n
  type_side zz N
  await_observable tmux format '#{copy_cursor_line}=line-48 needle-48' || true
  await_observable zz format '#{copy_cursor_line}=line-12 needle-12' || true
  self_check_compare reentry-direction
  self_check_case 'facts: n (up) on the pin side, N (down) on the zz side on a fresh entry' facts

  # cursor and facts: the page-down onto the prompt row on both sides, then a
  # cursor-left on the zz side only, which leaves it on column 0 where the old
  # vi clamp put it, against the pin's remembered column 1.
  attach_both_at
  set_on_both mode-keys vi
  seed_pane "$CLAMP_LINES_FILE"
  type_prefix_both '['
  await_observable zz format '#{pane_in_mode}=1' || true
  await_observable tmux format '#{pane_in_mode}=1' || true
  type_both 5
  type_both k
  await_observable zz format '#{copy_cursor_line}=aaaaaaaaaaaa' || true
  await_observable tmux format '#{copy_cursor_line}=aaaaaaaaaaaa' || true
  type_both NPage
  await_observable zz format '#{copy_cursor_y}=23' || true
  await_observable tmux format '#{copy_cursor_y}=23' || true
  type_side zz h
  await_observable zz format '#{copy_cursor_x}=0' || true
  self_check_compare page-clamp
  self_check_case 'cursor: the page landing on column 0 on the zz side only' cursor
  self_check_case 'facts: the page landing on column 0 on the zz side only' facts

  if [ "$SELF_CHECK_FAILURES" -ne 0 ]; then
    printf '%s self-check expectations unmet\n' "$SELF_CHECK_FAILURES"
    return 1
  fi
  printf 'self-check complete: every sabotage was caught in its own channel and every equivalence passed\n'
  return 0
}

# --- run -------------------------------------------------------------------

generate_lines
generate_clamp_lines
write_attach zz "$SCRATCH_DIR/attach-zz.sh"
write_attach tmux "$SCRATCH_DIR/attach-tmux.sh"
zz_command -f /dev/null daemon >"$SCRATCH_DIR/zz-daemon.out" 2>"$SCRATCH_DIR/zz-daemon.err" &
ZZ_PID=$!
wait_for "zz daemon socket" test -S "$ZZ_SOCKET"

if [ "$SELF_CHECK" -eq 1 ]; then
  run_self_check
  exit $?
fi

printf 'copy mode and search through the client stdin at %sx%s (pin %s)\n' \
  "$COLUMNS_UNDER_TEST" "$ROWS_UNDER_TEST" "$(basename -- "$TMUX_BIN")"
run_movement emacs
run_movement vi
run_page_clamp emacs
run_page_clamp vi
run_search emacs
run_search vi
run_native_search_command
run_live_mode_keys
run_presentation

if [ "$FAILURES" -ne 0 ]; then
  printf '%s of %s cases differ in a channel they assert, %s recorded elsewhere\n' \
    "$FAILURES" "$CHECKS" "$RECORDS"
  exit 1
fi
printf 'all %s cases agree on every channel they assert, %s recorded a difference elsewhere\n' \
  "$CHECKS" "$RECORDS"
printf '%s of those cases assert at least one channel; %s assert none and are recorded in full\n' \
  "$ASSERTING" "$((CHECKS - ASSERTING))"
