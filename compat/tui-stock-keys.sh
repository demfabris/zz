#!/usr/bin/env bash
# Stock bindings and key ownership, driven through the attached client's stdin.
#
# tui-screen-diff.sh drives both sides with `send-keys` against the inner
# SERVER, so it proves what a command does and never what a KEY does: the key
# it types is pane input, and the binding that would have run it is never
# consulted. This fixture types the stock chords into the attached CLIENT's
# stdin instead. Both binaries are attached inside one outer pinned tmux, one
# window each, and every chord is sent with `send-keys` against the OUTER pane,
# so the bytes arrive on the inner client's terminal exactly as a user's
# keyboard would deliver them: prefix, then the key.
#
# THE DECODED SCREEN IS THE CONTRACT (see tui-screen-diff.sh's header for the
# full rule). The outer pinned tmux is the decoder; every visible cell is
# compared with its escapes, plus the cursor tuple, plus a SERVER FACTS line
# read from each side's own server. Facts are the second channel because two
# screens can agree while the servers do not: after a split both panes run the
# same shell and paint the same prompt, and only `#{pane_active}` says which
# one the next key reaches.
#
# CONTROLLED DYNAMIC VALUES, set on both sides before any chord is typed:
#   status-right ''      the default ends in a clock and carries %d-%b-%y,
#                        which status-row.sh owns.
#   status-left L        fixed literal, so the left of the row is asserted.
#   automatic-rename off the default name follows the running command and would
#                        race a rename case.
#   default-command      "ENV= PS1='$ ' exec /bin/sh": a stock split runs the
#                        session's default command, not a command this fixture
#                        can pass, so the default itself is pinned. No rc file,
#                        and a prompt with no host, user, path or clock.
#   select-pane -T       fixed title: the pin seeds one from gethostname and zz
#                        reports the shell through its shell integration. That
#                        is the recorded pane.runtime-facts decision.
#   pane-border-style, pane-active-border-style, message-style, mode-style,
#   status-style, window-status-current-style
#                        every surface a case paints, pinned to the same
#                        explicit value on both sides. zz ships its own theme
#                        palette in these defaults (`fg=themelightgrey`,
#                        `bg=themeyellow`, …) where the pin ships tmux's, which
#                        is the recorded status-row theme divergence and none of
#                        this fixture's business: a key case must fail on the
#                        key, never on a palette. Both a foreground and a
#                        background are always named, so the recorded
#                        explicit-default-foreground divergence cannot reach a
#                        case either.
# Nothing else is masked.
#
# SETTLE. A case names a bounded OBSERVABLE (a server fact, or a substring on
# the screen) and waits for it per side, then waits for that side's screen to
# stop changing between two polls. A side that never reaches its observable is
# a comparison RESULT, not a crash: the case still captures and compares both
# screens, and reports the side that never got there. No wait here is a sleep.
#
# CASES. Every stock chord this obligation names is typed, never executed as a
# command: split (prefix % and "), chooser (prefix s and w), rename (prefix ,
# and $), selection (prefix o and the arrows), zoom (prefix z) and detach
# (prefix d). Then key ownership: C-\, M-s and M-S bound in the ROOT table to
# an observable command, and the same three chords typed at an application that
# has turned its signal keys off, where both sides must hand the bytes to the
# program.
#
# THREE CHANNELS. Every case compares the rows, the cursor and the server
# facts, and NAMES the channels it asserts; a channel it does not name is
# recorded with the reason it cannot be asserted yet. The default is all three.
# A case that names fewer says so on every run, so a channel is never waived by
# omission, and the reasons are measurements: see BORDER_REASON, CHOOSER_REASON
# and PROMPT_REASON below.
#
# --self-check drives one deliberate one-sided difference per channel and
# requires the comparison to catch it in that channel, plus two differences it
# must NOT report. A fixture that only passes has proved nothing.
#
# ZZ_KEYS_CAPTURE_DIR, when set, keeps every capture, cursor tuple and facts
# line as text. A bounded wait that runs out dumps diagnostics into
# ZZ_KEYS_DIAGNOSTICS_DIR or a fresh /tmp directory it names.
set -eEuo pipefail

usage() {
  printf 'usage: compat/tui-stock-keys.sh [--self-check] [ZZ_BIN [TMUX_BIN]]\n' >&2
  printf '       ZZ_BIN=path TMUX_BIN=path compat/tui-stock-keys.sh\n' >&2
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

# Both asserted sizes stay below the 109 column sidebar threshold
# (crates/zz-tui/src/sidebar.rs AUTO_HIDE_COLUMNS), so an asserted case can
# never invoke zz's own chrome by accident.
SIZES=(80x24 100x24)
PANE_TITLE="stockkeys"
WINDOW_NAME="win"
SESSION_NAME="keys"
SCRATCH_DIR="$(mktemp -d /tmp/zzsk.XXXXXX)"
TOKEN="${SCRATCH_DIR##*.}"
OUTER_SOCKET_NAME="zzsko-$TOKEN"
INNER_SOCKET_NAME="zzski-$TOKEN"
ZZ_SOCKET="/tmp/zzsk-$TOKEN.sock"
OUTER_SESSION="driver"
ZZ_HOME="$SCRATCH_DIR/zz-home"
TMUX_HOME="$SCRATCH_DIR/tmux-home"
OUTER_HOME="$SCRATCH_DIR/outer-home"
ZZ_LOG_DIR="$SCRATCH_DIR/zz-logs"
ZZ_CLIENT_STDERR="$SCRATCH_DIR/zz-client.err"
TMUX_CLIENT_STDERR="$SCRATCH_DIR/tmux-client.err"
CAPTURE_DIR="${ZZ_KEYS_CAPTURE_DIR:-}"
DIAGNOSTICS_DIR=""
ROWS_UNDER_TEST=0
SIZE_LABEL=""
ZZ_PID=""
FAILURES=0
CHECKS=0
RECORDS=0
LAST_ROWS_DIFFERED=0
LAST_CURSOR_DIFFERED=0
LAST_FACTS_DIFFERED=0
LAST_MISSING_OBSERVABLE=""
INNER_SHELL="ENV= PS1='\$ ' exec /bin/sh"
CHOOSER_REASON='zz renders choose-tree as its own client overlay, the pin renders a mode tree in the pane'
PROMPT_REASON='zz paints the command prompt from its own palette and ignores message-style'
# Two measurements, both 2026-09-09, both outside a key's reach:
#  * the border cell carries no background on the wire. PaneSnapshot has
#    border_colour, a foreground, and nothing else, so zz paints its own theme
#    background under every border glyph where the pin paints the style's.
#  * crates/zz-tui/src/layout.rs scaled_extent floors an f32 product of the
#    split ratio, so at some client widths the drawn border sits one column
#    left of the geometry the daemon reports and the right pane's cursor with
#    it. Measured against the daemon's own #{pane_left}/#{pane_right}: wrong at
#    98 and 100 columns, right at 80, 88, 90, 96, 99, 101, 102, 104 and 108.
# The split geometry itself is asserted through each pane's width and height in
# the facts line, which is where a key's effect actually lives.
BORDER_REASON='the border cell has no background on the wire and layout.rs floors the split ratio'
# The moment a pane's foreground child starts, the two servers disagree about
# #{pane_current_command} for a beat: with default-command pinned to
# `exec /bin/sh`, the pin already answers `cat` while zz still answers the
# shell or an empty string, and zz has caught up by the next chord (every
# later application case asserts the field and agrees). Measured 2026-09-10 at
# the gate tip; the field is asserted everywhere else, where the pane is a
# settled shell and the two agree exactly.
COMMAND_SETTLE_REASON='zz reports #{pane_current_command} a beat later than the pin when a pane'"'"'s child starts'
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
    DIAGNOSTICS_DIR="${ZZ_KEYS_DIAGNOSTICS_DIR:-$(mktemp -d /tmp/zzsk-diag.XXXXXX)}"
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
    printf 'size under test: %s\n' "${SIZE_LABEL:-none yet}"
    printf 'zz: %s\n' "$ZZ_BIN"
    printf 'tmux: %s\n' "$TMUX_BIN"
    printf 'zz socket: %s\n' "$ZZ_SOCKET"
    printf 'outer socket: %s\n' "$OUTER_SOCKET_NAME"
    printf 'inner tmux socket: %s\n' "$INNER_SOCKET_NAME"
  } >"$dir/what-fired.txt" 2>&1 || true
  for side in zz tmux; do
    tmux_outer_command capture-pane -p -e -S - -t "=$OUTER_SESSION:$side" \
      >"$dir/outer-$side.screen.txt" 2>&1 || true
    side_command "$side" list-clients \
      -F '#{client_name} session=#{client_session} #{client_width}x#{client_height}' \
      >"$dir/$side.list-clients.txt" 2>&1 || true
    side_command "$side" list-panes -a \
      -F '#{session_name}:#{window_index}.#{pane_index} #{pane_width}x#{pane_height} active=#{pane_active}' \
      >"$dir/$side.list-panes.txt" 2>&1 || true
    side_command "$side" list-keys -T root >"$dir/$side.root-keys.txt" 2>&1 || true
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
# The facts a screen cannot show. Pane ids are left out on purpose: the two
# servers number their panes from different origins and the campaign has never
# claimed they match.
FACTS_FORMAT='#{session_name}:#{window_index}.#{pane_index} #{pane_width}x#{pane_height} active=#{pane_active} zoomed=#{window_zoomed_flag} window=#{window_name} panes=#{window_panes} cmd=#{pane_current_command}'

outer_pane_is() {
  [ "$(tmux_outer_command display-message -p -t "$1" '#{pane_width}x#{pane_height}' 2>/dev/null)" = "$2" ]
}
client_attached() {
  [ "$(side_command "$1" list-clients -F '#{client_session}' 2>/dev/null)" = "$SESSION_NAME" ]
}
client_gone() {
  [ -z "$(side_command "$1" list-clients -F '#{client_session}' 2>/dev/null)" ]
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
  side_command "$1" list-panes -a -F "$FACTS_FORMAT" 2>/dev/null || printf 'no server\n'
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
  pane-border-style pane-active-border-style message-style mode-style status-style
  window-status-current-style
)
# RGB on purpose. Measured 2026-09-09: with these styles named
# (`fg=colour8,bg=colour0`) the pin writes `\e[90m\e[40m` and zz writes
# `\e[38;2;127;127;127m\e[48;2;16;19;24m`, because zz resolves a named or
# indexed colour through its palette before it writes and the pin keeps the
# class it was given. That is the recorded colour-promotion divergence
# tui-screen-diff.sh's colour-classes case owns, and an RGB value is the one
# class both binaries pass through unchanged, so a key case is compared on the
# key rather than on somebody else's record.
PINNED_STYLES=(
  'status-style bg=#00af00,fg=#101010'
  'window-status-current-style bg=#00af00,fg=#101010'
  'pane-border-style fg=#7f7f7f,bg=#101010'
  'pane-active-border-style fg=#00af00,bg=#101010'
  'message-style bg=#cdcd00,fg=#101010'
  'mode-style bg=#cdcd00,fg=#101010'
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
  for entry in "${PINNED_STYLES[@]}"; do
    side_command "$side" set-option -g "${entry%% *}" "${entry#* }" ||
      die "$side refused ${entry%% *}"
  done
}

# Root bindings a case installed outlive the session it installed them in, so
# every attach starts by clearing the three chords this fixture ever binds.
reset_root_bindings() {
  local side="$1"
  local chord
  for chord in 'C-\' M-s M-S; do
    side_command "$side" unbind-key -n "$chord" >/dev/null 2>&1 || true
  done
}

# Every session, not just this fixture's: a rename case leaves one behind under
# its new name and the facts channel reads the whole server.
kill_every_session() {
  local side="$1"
  local session
  while read -r session; do
    [ -n "$session" ] || continue
    side_command "$side" kill-session -t "=$session" >/dev/null 2>&1 || true
  done < <(side_command "$side" list-sessions -F '#{session_name}' 2>/dev/null || true)
}

attach_both_at() {
  local columns="$1"
  local rows="$2"
  ROWS_UNDER_TEST="$rows"
  tmux_outer_command kill-server >/dev/null 2>&1 || true
  kill_every_session zz
  kill_every_session tmux
  zz_command new-session -d -s "$SESSION_NAME" -n "$WINDOW_NAME" -x "$columns" -y "$rows" \
    "$INNER_SHELL" || die "could not create the zz session"
  tmux_inner_command -f /dev/null new-session -d -s "$SESSION_NAME" -n "$WINDOW_NAME" \
    -x "$columns" -y "$rows" "$INNER_SHELL" || die "could not create the tmux session"
  pin_dynamic_values zz
  pin_dynamic_values tmux
  reset_root_bindings zz
  reset_root_bindings tmux
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

# THE KEYSTROKE. `send-keys` against the OUTER pane writes the bytes on the
# inner client's terminal, which is the only way a stock binding is exercised
# at all: the same keys sent to the inner server would be pane input.
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
# The prefix and the key in one call: they reach the client's terminal in one
# write, which is what a user's keyboard does too.
type_prefix_both() {
  type_both C-b "$@"
}

# Stability only: the screen has to stop changing between two polls.
wait_settled() {
  local side="$1"
  local label="$2"
  local previous=""
  local current attempt
  for ((attempt = 0; attempt < 200; attempt++)); do
    current="$(capture_plain "$side" 2>/dev/null || true)"
    if [ -n "$previous" ] && [ "$current" = "$previous" ]; then
      return 0
    fi
    previous="$current"
    sleep 0.05
  done
  dump_diagnostics "$label"
  die "$label did not settle within 10 seconds"
}

# A bounded wait for one side's observable that REPORTS instead of dying: a
# side that never gets there is the measurement, and both screens are still
# compared and printed.
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
      if [ "$(side_command "$side" display-message -p -t "=$SESSION_NAME:" "$format" 2>/dev/null)" = "$expected" ]; then
        return 0
      fi
      ;;
    screen)
      if capture_plain "$side" 2>/dev/null | grep -Fq "$value"; then return 0; fi
      ;;
    sessions)
      if [ "$(side_command "$side" list-sessions -F '#{session_name}' 2>/dev/null)" = "$value" ]; then
        return 0
      fi
      ;;
    gone)
      if client_gone "$side"; then return 0; fi
      ;;
    esac
    sleep 0.05
  done
  return 1
}

compare_sides() {
  local name="$1"
  local zz_rows tmux_rows zz_cursor tmux_cursor zz_facts tmux_facts index differing total
  mapfile -t zz_rows < <(capture_screen zz)
  mapfile -t tmux_rows < <(capture_screen tmux)
  zz_cursor="$(cursor_tuple zz)"
  tmux_cursor="$(cursor_tuple tmux)"
  zz_facts="$(side_facts zz)"
  tmux_facts="$(side_facts tmux)"
  if [ -n "$CAPTURE_DIR" ]; then
    printf '%s\n' "${zz_rows[@]-}" >"$CAPTURE_DIR/$SIZE_LABEL.$name.zz.screen.txt"
    printf '%s\n' "${tmux_rows[@]-}" >"$CAPTURE_DIR/$SIZE_LABEL.$name.tmux.screen.txt"
    {
      printf 'cursor zz:   %s\n' "$zz_cursor"
      printf 'cursor tmux: %s\n' "$tmux_cursor"
      printf 'facts zz:\n%s\n' "$zz_facts"
      printf 'facts tmux:\n%s\n' "$tmux_facts"
    } >"$CAPTURE_DIR/$SIZE_LABEL.$name.facts.txt"
  fi
  total="$ROWS_UNDER_TEST"
  differing=-1
  for ((index = 0; index < total; index++)); do
    if [ "${zz_rows[index]-}" != "${tmux_rows[index]-}" ]; then
      differing="$index"
      break
    fi
  done
  LAST_ROWS_DIFFERED=0
  LAST_CURSOR_DIFFERED=0
  LAST_FACTS_DIFFERED=0
  [ "$differing" -lt 0 ] || LAST_ROWS_DIFFERED=1
  [ "$zz_cursor" = "$tmux_cursor" ] || LAST_CURSOR_DIFFERED=1
  [ "$zz_facts" = "$tmux_facts" ] || LAST_FACTS_DIFFERED=1
  if [ "$LAST_ROWS_DIFFERED" -eq 0 ] && [ "$LAST_CURSOR_DIFFERED" -eq 0 ] &&
    [ "$LAST_FACTS_DIFFERED" -eq 0 ] && [ -z "$LAST_MISSING_OBSERVABLE" ]; then
    return 0
  fi
  printf '      case %s at %s\n' "$name" "$SIZE_LABEL"
  if [ -n "$LAST_MISSING_OBSERVABLE" ]; then
    printf '      never reached the observable: %s\n' "$LAST_MISSING_OBSERVABLE"
  fi
  if [ "$LAST_ROWS_DIFFERED" -eq 1 ]; then
    printf '      first differing row %s of %s\n' "$differing" "$total"
    printf '        tmux: %s\n' "$(printf '%s' "${tmux_rows[differing]-}" | cat -v)"
    printf '        zz:   %s\n' "$(printf '%s' "${zz_rows[differing]-}" | cat -v)"
  else
    printf '      all %s rows identical\n' "$total"
  fi
  if [ "$LAST_CURSOR_DIFFERED" -eq 1 ]; then
    printf '      cursor tmux: %s\n' "$tmux_cursor"
    printf '      cursor zz:   %s\n' "$zz_cursor"
  fi
  if [ "$LAST_FACTS_DIFFERED" -eq 1 ]; then
    printf '      facts tmux: %s\n' "$(printf '%s' "$tmux_facts" | tr '\n' '|')"
    printf '      facts zz:   %s\n' "$(printf '%s' "$zz_facts" | tr '\n' '|')"
  fi
  return 1
}

# A case: type the chord, wait for each side's observable, settle, compare.
# MODE `same` asserts; `record` prints the same report, keeps going, and says
# WHY it is recorded rather than asserted.
key_case() {
  local name="$1"
  local kind="$2"
  local observable="$3"
  local mode="${4:-rows,cursor,facts}"
  local reason="${5:-}"
  local side
  LAST_MISSING_OBSERVABLE=""
  for side in zz tmux; do
    if ! await_observable "$side" "$kind" "$observable"; then
      LAST_MISSING_OBSERVABLE="${LAST_MISSING_OBSERVABLE}${LAST_MISSING_OBSERVABLE:+, }$side never showed $kind $observable"
    fi
  done
  wait_settled zz "$name on the zz screen"
  wait_settled tmux "$name on the tmux screen"
  CHECKS=$((CHECKS + 1))
  if compare_sides "$name"; then
    printf 'ok    %s %s\n' "$SIZE_LABEL" "$name"
    return 0
  fi
  local asserted=0
  case "$mode" in
  *rows*) asserted=$((asserted + LAST_ROWS_DIFFERED)) ;;
  esac
  case "$mode" in
  *cursor*) asserted=$((asserted + LAST_CURSOR_DIFFERED)) ;;
  esac
  case "$mode" in
  *facts*) asserted=$((asserted + LAST_FACTS_DIFFERED)) ;;
  esac
  [ -z "$LAST_MISSING_OBSERVABLE" ] || asserted=$((asserted + 1))
  if [ "$asserted" -gt 0 ]; then
    FAILURES=$((FAILURES + 1))
    printf 'DIFF  %s %s\n' "$SIZE_LABEL" "$name"
  else
    RECORDS=$((RECORDS + 1))
    printf 'note  %s %s asserted %s, the rest recorded: %s\n' "$SIZE_LABEL" "$name" "$mode" "$reason"
  fi
  return 0
}

# --- the corpus ------------------------------------------------------------

run_stock_bindings() {
  local size="$1"
  local columns="${size%x*}"
  local rows="${size#*x}"
  SIZE_LABEL="$size"

  # split: prefix % and prefix ". The pin binds `split-window -h` and
  # `split-window`; a stock split has to produce a shell pane on both sides,
  # which is why default-command is pinned above.
  attach_both_at "$columns" "$rows"
  type_prefix_both '%'
  key_case split-horizontal format '#{window_panes}=2' facts "$BORDER_REASON"
  type_prefix_both '"'
  key_case split-vertical format '#{window_panes}=3' facts "$BORDER_REASON"

  # selection: prefix o walks the panes, the arrows pick a direction. Three
  # panes exist by now, so both have somewhere to go, and the active pane index
  # is the observable rather than the fact that some pane is active.
  type_prefix_both o
  key_case select-next-pane format '#{pane_index}=0' facts "$BORDER_REASON"
  type_prefix_both Right
  key_case select-pane-right format '#{pane_index}=2' facts "$BORDER_REASON"
  type_prefix_both Down
  key_case select-pane-down format '#{pane_index}=1' facts "$BORDER_REASON"
  type_prefix_both Up
  key_case select-pane-up format '#{pane_index}=2' facts "$BORDER_REASON"

  # zoom: prefix z, then prefix z again.
  type_prefix_both z
  key_case zoom format '#{window_zoomed_flag}=1'
  type_prefix_both z
  key_case unzoom format '#{window_zoomed_flag}=0' facts "$BORDER_REASON"

  # chooser: prefix s and prefix w open the pin's session and window trees.
  # `q` cancels on both sides; a bare Escape would race the pin client's
  # escape-time window instead of ending the mode.
  attach_both_at "$columns" "$rows"
  type_prefix_both s
  key_case chooser-sessions screen "$SESSION_NAME" facts "$CHOOSER_REASON"
  type_both q
  key_case chooser-sessions-cancel screen '$'
  type_prefix_both w
  key_case chooser-windows screen "$WINDOW_NAME" facts "$CHOOSER_REASON"
  type_both q
  key_case chooser-windows-cancel screen '$'

  # rename: prefix , and prefix $ open a command prompt seeded with the current
  # name; the typed answer is the observable on the server.
  attach_both_at "$columns" "$rows"
  type_prefix_both ','
  key_case rename-window-prompt screen "$WINDOW_NAME" facts "$PROMPT_REASON"
  type_both C-u
  type_both -l renamedwin
  type_both Enter
  key_case rename-window format '#{window_name}=renamedwin'
  type_prefix_both '$'
  key_case rename-session-prompt screen "$SESSION_NAME" facts "$PROMPT_REASON"
  type_both C-u
  type_both -l renamedses
  type_both Enter
  key_case rename-session sessions renamedses

  # detach: prefix d. The client exits, the outer pane keeps its last screen
  # (remain-on-exit is on), and both sides' farewell is compared.
  attach_both_at "$columns" "$rows"
  type_prefix_both d
  key_case detach gone ''
}

run_key_ownership() {
  local size="$1"
  local columns="${size%x*}"
  local rows="${size#*x}"
  SIZE_LABEL="$size"

  # Root bindings on the three chords zz's raw client used to own. A root
  # binding is consulted before the pane on both sides, so the bound command is
  # what a user sees.
  attach_both_at "$columns" "$rows"
  side_command zz bind-key -n 'C-\' rename-window boundbackslash || die 'zz refused bind-key -n C-\'
  side_command tmux bind-key -n 'C-\' rename-window boundbackslash || die 'tmux refused bind-key -n C-\'
  type_both 'C-\'
  key_case root-binding-backslash format '#{window_name}=boundbackslash'

  side_command zz bind-key -n M-s rename-window boundalts || die 'zz refused bind-key -n M-s'
  side_command tmux bind-key -n M-s rename-window boundalts || die 'tmux refused bind-key -n M-s'
  type_both M-s
  key_case root-binding-alt-s format '#{window_name}=boundalts'

  side_command zz bind-key -n M-S rename-window boundaltshifts || die 'zz refused bind-key -n M-S'
  side_command tmux bind-key -n M-S rename-window boundaltshifts || die 'tmux refused bind-key -n M-S'
  type_both M-S
  key_case root-binding-alt-shift-s format '#{window_name}=boundaltshifts'

  # The chord the raw client used to own, restored the way the pin restores
  # anything: as a binding. `bind -n C-\ detach-client` is what a user who wants
  # the old behaviour writes, and both sides answer it the same way.
  attach_both_at "$columns" "$rows"
  side_command zz bind-key -n 'C-\' detach-client || die 'zz refused bind-key -n C-\'
  side_command tmux bind-key -n 'C-\' detach-client || die 'tmux refused bind-key -n C-\'
  type_both 'C-\'
  key_case root-binding-detaches gone ''

  # Application keys. With the root bindings gone and the program's signal keys
  # turned off, C-\, M-s and M-S are ordinary bytes the program must receive.
  # `cat -v` renders them, so the screen is the receipt.
  attach_both_at "$columns" "$rows"
  side_command zz send-keys -t "=$SESSION_NAME:0.0" 'stty -isig; cat -v' Enter ||
    die 'zz refused send-keys'
  side_command tmux send-keys -t "=$SESSION_NAME:0.0" 'stty -isig; cat -v' Enter ||
    die 'tmux refused send-keys'
  key_case application-reader screen 'cat -v' rows,cursor "$COMMAND_SETTLE_REASON"
  type_both 'C-\'
  key_case application-backslash screen '^\'
  type_both M-s
  key_case application-alt-s screen '^[s'
  type_both M-S
  key_case application-alt-shift-s screen '^[S'
}

# --- self-check ------------------------------------------------------------

SELF_CHECK_FAILURES=0

self_check_case() {
  local name="$1"
  local expectation="$2"
  local verdict=ok
  case "$expectation" in
  rows) [ "$LAST_ROWS_DIFFERED" -eq 1 ] || verdict='no row difference reported' ;;
  cursor) [ "$LAST_CURSOR_DIFFERED" -eq 1 ] || verdict='no cursor difference reported' ;;
  facts) [ "$LAST_FACTS_DIFFERED" -eq 1 ] || verdict='no server-facts difference reported' ;;
  none)
    if [ "$LAST_ROWS_DIFFERED" -ne 0 ] || [ "$LAST_CURSOR_DIFFERED" -ne 0 ] ||
      [ "$LAST_FACTS_DIFFERED" -ne 0 ]; then
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
  wait_settled zz "$1 on the zz screen"
  wait_settled tmux "$1 on the tmux screen"
  compare_sides "$1" || true
}

run_self_check() {
  printf 'self-check: one deliberate one-sided difference per channel, plus two the fixture must not report\n'

  # rows: the stock split chord typed on one side only.
  SIZE_LABEL='80x24-one-sided-chord'
  attach_both_at 80 24
  type_side zz C-b '%'
  await_observable zz format '#{window_panes}=2' || true
  self_check_compare one-sided-chord
  self_check_case 'rows: prefix % typed on one side only' rows

  # facts: the same three panes on both sides with a different active pane. The
  # panes run the same shell and paint the same prompt, so this is the
  # difference only the server facts can see.
  SIZE_LABEL='80x24-active-pane'
  attach_both_at 80 24
  type_prefix_both '%'
  await_observable zz format '#{window_panes}=2' || true
  await_observable tmux format '#{window_panes}=2' || true
  side_command zz select-pane -t "=$SESSION_NAME:0.0" || die 'zz refused select-pane'
  self_check_compare active-pane
  self_check_case 'facts: the active pane moved on one side' facts

  # cursor: one side's program leaves the cursor further along the row.
  SIZE_LABEL='80x24-cursor'
  attach_both_at 80 24
  side_command zz send-keys -t "=$SESSION_NAME:0.0" 'printf CURSORMARK-WITH-A-LONGER-TAIL' Enter ||
    die 'zz refused send-keys'
  side_command tmux send-keys -t "=$SESSION_NAME:0.0" 'printf CURSORMARK' Enter ||
    die 'tmux refused send-keys'
  await_observable zz screen CURSORMARK || true
  await_observable tmux screen CURSORMARK || true
  self_check_compare cursor
  self_check_case 'cursor: a different-length unterminated line' cursor

  # none: the same chord delivered as two writes on one side and one write on
  # the other. The fixture compares the RESULT of a keystroke, never the way it
  # was typed. The chord is the zoom, whose screen carries no pane border: an
  # equivalence must not be built on top of the recorded border divergence.
  SIZE_LABEL='80x24-write-splitting'
  attach_both_at 80 24
  type_prefix_both '%'
  await_observable zz format '#{window_panes}=2' || true
  await_observable tmux format '#{window_panes}=2' || true
  type_side zz C-b
  type_side zz z
  type_side tmux C-b z
  await_observable zz format '#{window_zoomed_flag}=1' || true
  await_observable tmux format '#{window_zoomed_flag}=1' || true
  self_check_compare write-splitting
  self_check_case 'equivalence: the prefix and the key in one write or two' none

  # none: the same root binding installed through the two spellings of the root
  # table, `-n` on one side and `-T root` on the other. What the fixture reads
  # is the key's effect, never the way the binding was written.
  SIZE_LABEL='80x24-binding-spelling'
  attach_both_at 80 24
  side_command zz bind-key -T root M-s rename-window spellcheck || die 'zz refused bind-key'
  side_command tmux bind-key -n M-s rename-window spellcheck || die 'tmux refused bind-key'
  type_both M-s
  await_observable zz format '#{window_name}=spellcheck' || true
  await_observable tmux format '#{window_name}=spellcheck' || true
  self_check_compare binding-spelling
  self_check_case 'equivalence: -n and -T root name the same table' none

  if [ "$SELF_CHECK_FAILURES" -ne 0 ]; then
    printf '%s self-check expectations unmet\n' "$SELF_CHECK_FAILURES"
    return 1
  fi
  printf 'self-check complete: every sabotage was caught in its own channel and every equivalence passed\n'
  return 0
}

# --- run -------------------------------------------------------------------

write_attach zz "$SCRATCH_DIR/attach-zz.sh"
write_attach tmux "$SCRATCH_DIR/attach-tmux.sh"
zz_command -f /dev/null daemon >"$SCRATCH_DIR/zz-daemon.out" 2>"$SCRATCH_DIR/zz-daemon.err" &
ZZ_PID=$!
wait_for "zz daemon socket" test -S "$ZZ_SOCKET"

if [ "$SELF_CHECK" -eq 1 ]; then
  run_self_check
  exit $?
fi

printf 'stock bindings and key ownership through the client stdin (pin %s)\n' "$(basename -- "$TMUX_BIN")"
for size in "${SIZES[@]}"; do
  run_stock_bindings "$size"
  run_key_ownership "$size"
done

if [ "$FAILURES" -ne 0 ]; then
  printf '%s of %s cases differ in a channel they assert, %s recorded elsewhere\n' \
    "$FAILURES" "$CHECKS" "$RECORDS"
  exit 1
fi
printf 'all %s cases agree on every channel they assert, %s recorded a difference elsewhere\n' \
  "$CHECKS" "$RECORDS"
