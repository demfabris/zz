#!/usr/bin/env bash
# Outer-terminal capability differential for the raw TUI.
#
# tui-screen-diff.sh compares the decoded screen an attached client paints.
# Nothing in the campaign ever compared the OTHER half of an attached client's
# contract with the terminal it runs in: the modes it arms, the capabilities it
# negotiates, the client facts its own daemon then records, and the colour
# class it puts on the wire to the outer terminal. This fixture attaches both
# binaries inside one outer pinned tmux and compares that half.
#
# THE OUTER PINNED TMUX IS THE DECODER, HERE TOO. It decodes each inner
# client's mode-setting sequences into its own pane state and publishes them as
# formats (format.c format_cb_pane_key_mode and the mode flag table), so
# `\e[?1049h`, `\e[?2004h`, `\e[?1002h`, `\e[?1003h`, `\e[?7l`, smkx and
# `\e[>4;1m` are compared as DECODED MODES rather than as bytes. Spelling,
# order and batching collapse; the mode a side did or did not arm does not.
# The colour case reads the same decoder's grid through `capture-pane -p -e`,
# where the colour CLASS survives: a named cell, an indexed cell and an RGB
# cell stay three different cells.
#
# THE DECLARED CAPABILITY MATRIX. Every channel of the client/terminal contract
# is named here. `driven` means this fixture drives it and compares both sides;
# `named` means it is not driven and is not silently absent.
#
#   channel                        how                                  status
#   ---------------------------------------------------------------------------
#   alternate screen               alternate_on, attached and detached   driven
#   alternate-screen restoration   the whole mode tuple after detach     driven
#   bracketed paste                bracket_paste_flag                    driven
#   cursor visibility              cursor_flag                           driven
#   cursor shape                   cursor_shape                          driven
#   autowrap (DECAWM)              wrap_flag                             driven
#   origin mode                    origin_flag                           driven
#   synchronized output            synchronized_output_flag              driven
#   application keypad             keypad_flag, keypad_cursor_flag       driven
#   mouse arming                   the five mouse flags, mouse on        driven
#   legacy keys                    pane_key_mode with extended-keys off  driven
#   extended keys                  pane_key_mode with extended-keys on   driven
#   client terminal name           client_termname                       driven
#   client feature negotiation     client_termfeatures, client_colours   driven
#   client UTF-8 from a locale     client_utf8, client_flags under a     driven
#                                  UTF-8 LANG and LC_ALL
#   client UTF-8 from the flag     client_utf8, client_flags under -u    driven
#   global flag -2                 accepted, and its own-side delta      driven
#   global flag -u                 accepted, and its own-side delta      driven
#   global flag -T                 accepted, and its own-side delta      driven
#   flag diagnostics               missing argument, unknown option      driven
#   colour class, stock palette    a named, an indexed and an RGB        driven
#                                  cell and a named background
#   colour class, palette change   the same four cells after OSC 4 on a  driven
#                                  named entry, after OSC 4 on an
#                                  indexed entry, and after OSC 104
#   colour class, an OSC 4 that    the pin substitutes any SET entry;     named
#     sets an entry to exactly     zz sees a palette entry only when it
#     its configured value         differs from the configured one,
#                                  because libghostty-vt exposes the
#                                  current and the default palette and no
#                                  override mask. Not driven.
#   colour class, pane-colours     zz applies pane-colours to the pane's  named
#                                  default palette (crates/zz-daemon
#                                  daemon.rs pane_palette), so a cell
#                                  keeps its index where the pin's
#                                  colour_palette_get substitutes the
#                                  option's colour. Not driven.
#   theme reply                    client_theme before any reply         driven
#   focus reporting                the pin publishes no pane format for      named
#                                  focus mode, so this decoder cannot see
#                                  either side arm it. Read from source
#                                  instead, and NOT equal: tty.c
#                                  tty_start_tty arms Enfcs only when
#                                  `focus-events` is on and its default is
#                                  0 (options-table.c), while tty.rs
#                                  TerminalGuard::enter writes `\e[?1004h`
#                                  on every attach. Driving it needs an
#                                  observable this decoder does not have.
#   user keys (User0..User9)       an option-store channel with no client named
#                                  terminal behind it on zz; recorded on
#                                  options.client-terminal-negotiation
#   Unicode widths                 the outer cursor column and the       driven
#                                  decoded line after a wide CJK pair, a
#                                  combining sequence and an emoji, with
#                                  UTF-8 clients on both sides
#   OSC 10/11 and DA replies       the reply path reaches the pin's       named
#                                  feature set, which the negotiation row
#                                  above measures as a whole; the
#                                  individual replies are not driven here.
#                                  The theme reply itself is measured by
#                                  compat/scenarios/smoke/fixtures/format-listing.sh
#
# RECORDED DIVERGENCES. These rows print their two measured values and do not
# fail the run. A row's disposition is per case and fixed in the driver below,
# never discovered at runtime: a row that this list explains is recorded in the
# case that measures the divergence, and asserts everywhere else. That is what
# lets --self-check sabotage the same row in a case where it asserts. Every one
# of them is written down in TUI-009's evidence with the source line that
# produces it.
#   wrap_flag              zz emits `\e[?7l` on entry (tty.rs TerminalGuard
#                          enter) and restores `\e[?7h` on exit; the pin leaves
#                          autowrap on and tracks the last cell itself.
#   mouse_all_flag         zz arms `\e[?1003h` (any-event) for the whole
#   mouse_button_flag      attach; the pin arms `\e[?1002h` (button-event) and
#                          raises MODE_MOUSE_ALL only while a menu is up. The
#                          two rows move together and are one divergence.
#   keypad_flag            the pin sends smkx; zz sends neither.
#   keypad_cursor_flag
#   pane_key_mode          under `extended-keys on` the pin arms Eneks
#     (extended case)      (`\e[>4;2m`, INPUT_CSI_MODSET) and the decoder
#                          records Ext 2; zz arms NOTHING and the decoder
#                          records VT10x. Under the TERM this case drives,
#                          xterm-256color, tty.rs writes `\e[>3u` only when
#                          guard.kitty_keyboard is set, and
#                          supports_kitty_keyboard reads TERM/TERM_PROGRAM for
#                          ghostty, kitty, wezterm, foot or zz - none of which
#                          xterm-256color matches. Measured at the gate
#                          2026-09-10 by capturing the attach stream: 0
#                          occurrences of `\e[>3u` under xterm-256color, 1
#                          under xterm-ghostty. So zz gets no extended keys
#                          here because it never asks, not because the pin
#                          ignored the request.
#   client_termfeatures    the pin's list is negotiated from the terminal's
#   client_colours         replies; zz derives its roster from TERM and
#                          COLORTERM.
#   client_utf8 under -u   the pin sets CLIENT_UTF8; zz's CLI accepts -u and
#   client_flags under -u  drops it.
#   client_termfeatures    the pin adds the named features; zz's CLI accepts
#     under -2 and -T      both and drops them.
#   client_theme           a zz client always carries a theme and the pin's is
#                          empty until its terminal answers. That is the
#                          recorded stance on
#                          options.client-terminal-negotiation, not a new
#                          finding; the row keeps it under this fixture's eye.
#
# CONTROLLED DYNAMIC VALUES, set on both sides and never left to chance:
#   the inner shell     ENV= PS1='$ ' exec /bin/sh, handed to new-session as
#                       the pane command: no rc file, and a prompt with no
#                       host, user, path or clock.
#   TERM, LANG, LC_ALL  named per case and identical on both sides. C is the
#                       default here and it matters: the pin's own locale
#                       fallback would set CLIENT_UTF8 on this box (LC_TIME is
#                       pt_BR.UTF-8) and hide what -u does. The two cases that
#                       need UTF-8 clients name C.UTF-8 explicitly.
#   mouse               left at its default, which is `on` on both binaries.
#   runtime and sockets XDG_RUNTIME_DIR is a directory inside the scratch
#                       directory for every zz process, the env -i attach
#                       scripts included, and the CLI diagnostics run with
#                       TMUX_TMPDIR there too. The CLI diagnostics name no
#                       socket, and a scrubbed environment without
#                       XDG_RUNTIME_DIR resolves zz's default socket to
#                       /tmp/zz-user/default.sock (with the caller's, to the
#                       user's real one), so a connecting command there would
#                       autostart a daemon outside the run. Inside the scratch
#                       directory it cannot, and the cleanup stops it and reaps
#                       every process whose environment names the scratch
#                       directory, on a normal exit and on SIGINT or SIGTERM.
#   the outer decoder   status off, and `extended-keys on` so the decoder
#                       records an inner client's extended-key request at all
#                       (input.c INPUT_CSI_MODSET returns early when its own
#                       option is off). The outer server has no client, so the
#                       option changes nothing it emits.
#
# SETTLED CHECKPOINTS. Every reading is taken after `printf 'MARK-%s\n' NAME`
# has reached the side's screen AND that screen has stopped changing between
# two polls. No wait here is a sleep.
#
# COLOUR CLASSES. The pane body keeps the class the program wrote: a named
# colour leaves the raw TUI as 3n/4n or 9n/10n, an indexed one as 38;5;n or
# 48;5;n, an RGB one as 38;2, and a default ground as 39/49, which is what
# tty_colours_fg and tty_colours_bg send through xterm-256color's setaf and
# setab. Once an OSC 4 entry exists for an index the pin resolves the cell
# through the pane palette and sends RGB, and after OSC 104 it sends the index
# again; zz does the same from the frame's per-style colour class (PROTOCOL
# 101). Every cell of every colour stage asserts.
#
# --self-check drives EIGHT deliberate one-sided differences and requires the
# comparison to report each in the channel that was sabotaged: a one-sided
# client flag (-u on the pin only), a named cell spelled as the RGB colour it
# resolves to on one side, a one-sided palette entry, an OSC 4 on an indexed
# entry sent to one side, an OSC 104 sent to one side, the legacy-terminal case
# driven with extended keys on one side, a one-sided unknown CLI option, and a
# one-sided wide codepoint. A ninth case is a CONTROL that sabotages nothing
# and must stay quiet. A fixture that only passes has proved nothing.
#
# ZZ_CAPS_DIAGNOSTICS_DIR names the directory a bounded wait that runs out
# copies its evidence into; without it a fresh /tmp directory is made and named
# on stderr.
set -eEuo pipefail

usage() {
  printf 'usage: compat/tui-caps.sh [--self-check] [ZZ_BIN [TMUX_BIN]]\n' >&2
  printf '       ZZ_BIN=path TMUX_BIN=path compat/tui-caps.sh\n' >&2
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

SCRATCH_DIR="$(mktemp -d /tmp/zzcaps.XXXXXX)"
TOKEN="${SCRATCH_DIR##*.}"
OUTER_SOCKET_NAME="zzcapso-$TOKEN"
INNER_SOCKET_NAME="zzcapsi-$TOKEN"
ZZ_SOCKET="/tmp/zzcaps-$TOKEN.sock"
OUTER_SESSION="driver"
INNER_SESSION="caps"
ZZ_HOME="$SCRATCH_DIR/zz-home"
TMUX_HOME="$SCRATCH_DIR/tmux-home"
OUTER_HOME="$SCRATCH_DIR/outer-home"
ZZ_LOG_DIR="$SCRATCH_DIR/zz-logs"
RUNTIME_DIR="$SCRATCH_DIR/run"
DIAGNOSTICS_DIR=""
CASE_UNDER_TEST=""
ZZ_PID=""
CHECKS=0
FAILURES=0
RECORDED=0
CASE_DIFFS=0
ASSERT_MODE=count
INNER_SHELL="ENV= PS1='\$ ' exec /bin/sh"
# The locale both sides attach under. C by default, so the pin's own locale
# fallback cannot set CLIENT_UTF8 behind the -u measurement's back; the two
# cases that want UTF-8 clients set it around themselves.
CASE_LOCALE=C
ZZ_PANE=""
TMUX_PANE_ID=""
mkdir -p "$ZZ_HOME/config" "$TMUX_HOME/config" "$OUTER_HOME/config" "$ZZ_LOG_DIR" "$RUNTIME_DIR"
chmod 700 "$RUNTIME_DIR"

# Every mode the outer decoder publishes for its own pane, plus the key mode.
# The pin's own format names are the row names; nothing is invented here.
MODE_NAMES=(
  alternate_on bracket_paste_flag cursor_flag cursor_shape wrap_flag
  mouse_all_flag mouse_any_flag mouse_button_flag mouse_sgr_flag
  mouse_standard_flag mouse_utf8_flag synchronized_output_flag origin_flag
  keypad_flag keypad_cursor_flag pane_key_mode
)
MODE_FORMAT='#{alternate_on}|#{bracket_paste_flag}|#{cursor_flag}|#{cursor_shape}|#{wrap_flag}|#{mouse_all_flag}|#{mouse_any_flag}|#{mouse_button_flag}|#{mouse_sgr_flag}|#{mouse_standard_flag}|#{mouse_utf8_flag}|#{synchronized_output_flag}|#{origin_flag}|#{keypad_flag}|#{keypad_cursor_flag}|#{pane_key_mode}'
# Rows the two sides are measured to disagree on. Each one is explained in the
# header block above and written into TUI-009's evidence.
MODE_RECORDED="wrap_flag mouse_all_flag mouse_button_flag keypad_flag keypad_cursor_flag"

FACT_NAMES=(client_termname client_utf8 client_flags client_colours client_termfeatures client_theme)
FACT_FORMAT='#{client_termname}|#{client_utf8}|#{client_flags}|#{client_colours}|#{client_termfeatures}|#{client_theme}'
FACT_RECORDED="client_colours client_termfeatures client_theme"
# Each global flag adds its own rows: what the flag CHANGED on its own side,
# against that side's own unflagged baseline. That delta is the flag's
# disposition, and it is what makes an ignored flag visible instead of hidden
# inside a roster the two sides never shared anyway. The delta is compared
# over the items both baselines agree on: an item only one baseline carries
# (the pin negotiates 256, RGB and progressbar from its terminal's replies,
# zz derives osc7 and sync from TERM) is the recorded client_termfeatures row,
# and a flag re-adding it on one side is that same difference, not a new one.
# A flag that names features adds a second row, whether each named feature is
# in the side's roster after the flag, which is what -2 does on a terminal
# whose pin baseline already carries 256.
BASELINE_ZZ=""
BASELINE_PIN=""

scrubbed() {
  env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u EDITOR -u VISUAL \
    -u XDG_STATE_HOME -u ZZ_LOG_DIR -u COLORTERM -u TERM_PROGRAM \
    LANG=C LC_ALL=C TMUX_TMPDIR=/tmp XDG_RUNTIME_DIR="$RUNTIME_DIR" "$@"
}
tmux_outer_command() {
  scrubbed HOME="$OUTER_HOME" XDG_CONFIG_HOME="$OUTER_HOME/config" TERM=xterm-256color \
    "$TMUX_BIN" -L "$OUTER_SOCKET_NAME" "$@"
}
zz_command() {
  scrubbed HOME="$ZZ_HOME" XDG_CONFIG_HOME="$ZZ_HOME/config" ZZ_LOG_DIR="$ZZ_LOG_DIR" \
    TERM=xterm-256color "$ZZ_BIN" --socket "$ZZ_SOCKET" "$@"
}
tmux_inner_command() {
  scrubbed HOME="$TMUX_HOME" XDG_CONFIG_HOME="$TMUX_HOME/config" TERM=xterm-256color \
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
outer_window() {
  printf '%s\n' "=$OUTER_SESSION:w-$1"
}
side_pane() {
  case "$1" in
  zz) printf '%s\n' "$ZZ_PANE" ;;
  tmux) printf '%s\n' "$TMUX_PANE_ID" ;;
  esac
}

# Every process this run starts carries the scratch directory in its
# environment: the daemon, the attach clients and their children through HOME,
# and a client without a socket selector through XDG_RUNTIME_DIR too. That is
# the reap list, so a daemon a stray client autostarted, an attach client the
# outer server left behind or anything a sabotage spawned goes with the run,
# and nothing else on the box is touched.
scratch_pids() {
  local entry
  for entry in /proc/[0-9]*; do
    if tr '\0' '\n' <"$entry/environ" 2>/dev/null | grep -qF -- "$SCRATCH_DIR"; then
      printf '%s\n' "${entry#/proc/}"
    fi
  done
}
reap_scratch() {
  local pids attempt
  pids="$(scratch_pids)"
  [ -n "$pids" ] || return 0
  printf 'cleanup: reaping %s leftover processes of this run\n' "$(printf '%s\n' "$pids" | wc -l)" >&2
  kill $pids >/dev/null 2>&1
  for ((attempt = 0; attempt < 60; attempt++)); do
    [ -n "$(scratch_pids)" ] || return 0
    sleep 0.05
  done
  pids="$(scratch_pids)"
  [ -z "$pids" ] || kill -KILL $pids >/dev/null 2>&1
}

cleanup() {
  local status=$?
  trap - EXIT ERR INT TERM
  set +e
  tmux_outer_command kill-server >/dev/null 2>&1
  zz_command kill-server >/dev/null 2>&1
  tmux_inner_command kill-server >/dev/null 2>&1
  if [ -S "$RUNTIME_DIR/zz/default.sock" ]; then
    scrubbed HOME="$ZZ_HOME" "$ZZ_BIN" --socket "$RUNTIME_DIR/zz/default.sock" kill-server >/dev/null 2>&1
  fi
  if [ -n "$ZZ_PID" ]; then
    kill "$ZZ_PID" >/dev/null 2>&1
    wait "$ZZ_PID" >/dev/null 2>&1
  fi
  reap_scratch
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
    DIAGNOSTICS_DIR="${ZZ_CAPS_DIAGNOSTICS_DIR:-$(mktemp -d /tmp/zzcaps-diag.XXXXXX)}"
    mkdir -p "$DIAGNOSTICS_DIR"
  fi
  printf '%s\n' "$DIAGNOSTICS_DIR"
}

# Every command below is allowed to fail: a wait can run out before a session
# exists, and a diagnostic that cannot be taken must not replace the timeout
# with its own error.
dump_diagnostics() {
  local label="$1"
  local dir side log
  dir="$(diagnostics_dir)"
  {
    printf 'wait that ran out: %s\n' "$label"
    printf 'at: %s\n' "$(date -Is 2>/dev/null || date)"
    printf 'case under test: %s\n' "${CASE_UNDER_TEST:-none yet}"
    printf 'zz: %s\n' "$ZZ_BIN"
    printf 'tmux: %s\n' "$TMUX_BIN"
    printf 'zz socket: %s\n' "$ZZ_SOCKET"
    printf 'outer socket: %s\n' "$OUTER_SOCKET_NAME"
    printf 'inner tmux socket: %s\n' "$INNER_SOCKET_NAME"
    printf 'ZZ_LOG_DIR (%s) holds:\n' "$ZZ_LOG_DIR"
    ls -1 -- "$ZZ_LOG_DIR" 2>&1 || true
  } >"$dir/what-fired.txt" 2>&1 || true
  for side in zz tmux; do
    tmux_outer_command capture-pane -p -e -S - -t "$(outer_window "$side")" \
      >"$dir/outer-$side.screen.txt" 2>&1 || true
    side_command "$side" list-clients \
      -F '#{client_name} session=#{client_session} #{client_width}x#{client_height} flags=#{client_flags}' \
      >"$dir/$side.list-clients.txt" 2>&1 || true
    side_command "$side" list-panes -a \
      -F '#{session_name}:#{window_index}.#{pane_index} #{pane_width}x#{pane_height} dead=#{pane_dead}' \
      >"$dir/$side.list-panes.txt" 2>&1 || true
  done
  tmux_outer_command list-panes -a \
    -F '#{window_name} #{pane_width}x#{pane_height} dead=#{pane_dead}' \
    >"$dir/outer.list-panes.txt" 2>&1 || true
  cp -f -- "$SCRATCH_DIR/zz-daemon.out" "$dir/zz-daemon.stdout.txt" 2>/dev/null || true
  cp -f -- "$SCRATCH_DIR/zz-daemon.err" "$dir/zz-daemon.stderr.txt" 2>/dev/null || true
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

outer_pane_is() {
  [ "$(tmux_outer_command display-message -p -t "$1" '#{pane_width}x#{pane_height}' 2>/dev/null)" = "$2" ]
}
client_attached() {
  [ "$(side_command "$1" list-clients -F '#{client_session}' 2>/dev/null | head -n 1)" = "$INNER_SESSION" ]
}
client_gone() {
  [ -z "$(side_command "$1" list-clients -F x 2>/dev/null)" ]
}
outer_screen() {
  tmux_outer_command capture-pane -p -e -t "$(outer_window "$1")" 2>/dev/null
}

# The marker reaches the screen only as the inner shell's own output: the typed
# command carries MARK-%s, so a copy of the command line cannot satisfy it. The
# stability half is what makes the reading safe to take: a repaint the case
# asked for travels to the outer grid through the same ordered stream, and two
# identical polls mean it has arrived.
wait_settled() {
  local side="$1" marker="$2"
  local attempt current previous=""
  for ((attempt = 0; attempt < 200; attempt++)); do
    current="$(outer_screen "$side")"
    if [ -n "$current" ] && [ "$current" = "$previous" ] && [[ "$current" == *"$marker"* ]]; then
      return 0
    fi
    previous="$current"
    sleep 0.05
  done
  dump_diagnostics "$side settled on $marker"
  die "$side did not settle on $marker within 10 seconds"
}

send_both() {
  local command="$1" side
  for side in zz tmux; do
    side_command "$side" send-keys -t "$(side_pane "$side")" "$command" Enter
  done
}
checkpoint() {
  local name="$1" side
  send_both "printf 'MARK-%s\\n' $name"
  for side in zz tmux; do
    wait_settled "$side" "MARK-$name"
  done
}

assert_row() {
  local name="$1" zz_value="$2" pin_value="$3"
  CHECKS=$((CHECKS + 1))
  if [ "$zz_value" = "$pin_value" ]; then
    [ "$ASSERT_MODE" = count ] && printf 'ok    %s: both %s\n' "$name" "$zz_value"
    return 0
  fi
  CASE_DIFFS=$((CASE_DIFFS + 1))
  if [ "$ASSERT_MODE" = count ]; then
    FAILURES=$((FAILURES + 1))
    printf 'DIFF  %s: tmux %s, zz %s\n' "$name" "$pin_value" "$zz_value"
  fi
}
record_row() {
  local name="$1" zz_value="$2" pin_value="$3"
  RECORDED=$((RECORDED + 1))
  [ "$ASSERT_MODE" = count ] || return 0
  if [ "$zz_value" = "$pin_value" ]; then
    printf 'note  %s: both %s\n' "$name" "$zz_value"
  else
    printf 'note  %s: tmux %s, zz %s\n' "$name" "$pin_value" "$zz_value"
  fi
}
# A row's disposition is fixed by the case that drives it, not discovered at
# runtime: `recorded` is a space-delimited list of ROW names the header block
# explains, and every row outside it asserts.
compare_row() {
  local recorded="$1" row="$2" label="$3" zz_value="$4" pin_value="$5"
  if [[ " $recorded " == *" $row "* ]]; then
    record_row "$label" "$zz_value" "$pin_value"
  else
    assert_row "$label" "$zz_value" "$pin_value"
  fi
}

read_modes() {
  tmux_outer_command display-message -p -t "$(outer_window "$1")" "$MODE_FORMAT"
}
read_facts() {
  side_command "$1" list-clients -F "$FACT_FORMAT" 2>/dev/null | head -n 1
}

compare_tuple() {
  local label="$1" recorded="$2" zz_tuple="$3" pin_tuple="$4"
  shift 4
  local names=("$@")
  local -a zz_fields pin_fields
  IFS='|' read -r -a zz_fields <<<"$zz_tuple"
  IFS='|' read -r -a pin_fields <<<"$pin_tuple"
  local index
  for index in "${!names[@]}"; do
    compare_row "$recorded" "${names[$index]}" "$label ${names[$index]}" \
      "${zz_fields[$index]-}" "${pin_fields[$index]-}"
  done
}

write_attach() {
  local side="$1" destination="$2" term="$3"
  shift 3
  printf '#!/usr/bin/env bash\n' >"$destination"
  if [ "$side" = zz ]; then
    printf 'exec env -i HOME=%q XDG_CONFIG_HOME=%q XDG_RUNTIME_DIR=%q ZZ_LOG_DIR=%q LANG=%q LC_ALL=%q TERM=%q PATH=%q TMUX_TMPDIR=/tmp %q --socket %q %s attach-session -t %q\n' \
      "$ZZ_HOME" "$ZZ_HOME/config" "$RUNTIME_DIR" "$ZZ_LOG_DIR" "$CASE_LOCALE" "$CASE_LOCALE" \
      "$term" "$PATH" "$ZZ_BIN" "$ZZ_SOCKET" "$*" "=$INNER_SESSION" >>"$destination"
  else
    printf 'exec env -i HOME=%q XDG_CONFIG_HOME=%q XDG_RUNTIME_DIR=%q LANG=%q LC_ALL=%q TERM=%q PATH=%q TMUX_TMPDIR=/tmp %q -L %q %s attach-session -t %q\n' \
      "$TMUX_HOME" "$TMUX_HOME/config" "$RUNTIME_DIR" "$CASE_LOCALE" "$CASE_LOCALE" "$term" \
      "$PATH" "$TMUX_BIN" "$INNER_SOCKET_NAME" "$*" "=$INNER_SESSION" >>"$destination"
  fi
  chmod +x "$destination"
}

# Both sides attach with the same TERM and the same extra flags unless a
# sabotage names different ones. The two windows are made fresh for every case
# so a mode a previous case armed cannot leak into the next reading.
open_case() {
  local name="$1" term="$2" zz_flags="$3" tmux_flags="$4"
  local side
  CASE_UNDER_TEST="$name"
  for side in zz tmux; do
    tmux_outer_command kill-window -t "$(outer_window "$side")" >/dev/null 2>&1 || true
  done
  for side in zz tmux; do
    wait_for "$side client gone before $name" client_gone "$side"
  done
  write_attach zz "$SCRATCH_DIR/attach-zz.sh" "$term" $zz_flags
  write_attach tmux "$SCRATCH_DIR/attach-tmux.sh" "$term" $tmux_flags
  for side in zz tmux; do
    tmux_outer_command new-window -d -n "w-$side" "$SCRATCH_DIR/attach-$side.sh"
  done
  for side in zz tmux; do
    wait_for "outer $side pane for $name" outer_pane_is "$(outer_window "$side")" 80x24
    wait_for "$side client attached for $name" client_attached "$side"
  done
}

detach_case() {
  local side
  for side in zz tmux; do
    side_command "$side" detach-client -s "=$INNER_SESSION" >/dev/null 2>&1 || true
  done
  for side in zz tmux; do
    wait_for "$side client detached" client_gone "$side"
  done
}

# --- the driven cases ------------------------------------------------------

case_modes() {
  local name="$1" term="$2" zz_flags="$3" tmux_flags="$4" recorded="${5:-$MODE_RECORDED}"
  open_case "$name" "$term" "$zz_flags" "$tmux_flags"
  checkpoint "${name//[^a-zA-Z0-9]/}"
  compare_tuple "$name" "$recorded" "$(read_modes zz)" "$(read_modes tmux)" "${MODE_NAMES[@]}"
}

# An extended key through the outer decoder. With an inner client's mode 2
# armed, the outer tmux sends C-Enter as \e[27;5;13~ and the client has to
# decode it; without mode 2 the key arrives as a plain Enter. A root binding on
# each inner server records what its client decoded, and F9, bound on both and
# sent after it through the same path, is the settle: once F9's binding has run
# on a side, C-Enter has been handled there too.
case_extended_key() {
  local name="$1" side
  for side in zz tmux; do
    side_command "$side" set-option -gu @extkey >/dev/null 2>&1 || true
    side_command "$side" set-option -gu @extdone >/dev/null 2>&1 || true
    side_command "$side" bind-key -n C-Enter set-option -g @extkey C-Enter >/dev/null
    side_command "$side" bind-key -n F9 set-option -g @extdone 1 >/dev/null
  done
  open_case "$name" xterm-256color '' ''
  checkpoint "${name//[^a-zA-Z0-9]/}"
  for side in zz tmux; do
    tmux_outer_command send-keys -t "$(outer_window "$side")" C-Enter F9
  done
  for side in zz tmux; do
    wait_for "$side decoded F9 for $name" extended_done "$side"
  done
  assert_row "$name @extkey" \
    "$(side_command zz show-options -gqv @extkey 2>/dev/null)" \
    "$(side_command tmux show-options -gqv @extkey 2>/dev/null)"
  for side in zz tmux; do
    side_command "$side" unbind-key -n C-Enter >/dev/null 2>&1 || true
    side_command "$side" unbind-key -n F9 >/dev/null 2>&1 || true
  done
}
extended_done() {
  [ "$(side_command "$1" show-options -gqv @extdone 2>/dev/null)" = 1 ]
}

# Items in the second comma list that the first does not carry.
list_delta() {
  local base="$1" now="$2" item out=""
  local IFS=,
  for item in $now; do
    case ",$base," in
    *",$item,"*) ;;
    *) out="${out:+$out,}$item" ;;
    esac
  done
  printf '%s\n' "${out:-none}"
}
# Items of the first comma list that the second does not carry, where `none`
# is the empty list.
list_without() {
  local list="$1" exclude="$2" item out=""
  [ "$list" = none ] && list=""
  local IFS=,
  for item in $list; do
    case ",$exclude," in
    *",$item,"*) ;;
    *) out="${out:+$out,}$item" ;;
    esac
  done
  printf '%s\n' "${out:-none}"
}
# Whether each named feature is in a roster, as name:yes or name:no.
features_present() {
  local roster="$1" names="$2" name out=""
  local IFS=,
  for name in $names; do
    case ",$roster," in
    *",$name,"*) out="${out:+$out,}$name:yes" ;;
    *) out="${out:+$out,}$name:no" ;;
    esac
  done
  printf '%s\n' "$out"
}
tuple_field() {
  local tuple="$1" index="$2"
  local -a fields
  IFS='|' read -r -a fields <<<"$tuple"
  printf '%s\n' "${fields[$index]-}"
}

case_facts() {
  local name="$1" term="$2" zz_flags="$3" tmux_flags="$4" recorded="${5:-$FACT_RECORDED}"
  local baseline="${6:-}" named="${7:-}"
  open_case "$name" "$term" "$zz_flags" "$tmux_flags"
  checkpoint "${name//[^a-zA-Z0-9]/}"
  local zz_tuple pin_tuple
  zz_tuple="$(read_facts zz)"
  pin_tuple="$(read_facts tmux)"
  compare_tuple "$name" "$recorded" "$zz_tuple" "$pin_tuple" "${FACT_NAMES[@]}"
  if [ "$baseline" = baseline ]; then
    BASELINE_ZZ="$zz_tuple"
    BASELINE_PIN="$pin_tuple"
    return 0
  fi
  [ -n "$BASELINE_ZZ" ] || return 0
  local index zz_base pin_base disagreed
  for index in 2 4; do
    zz_base="$(tuple_field "$BASELINE_ZZ" "$index")"
    pin_base="$(tuple_field "$BASELINE_PIN" "$index")"
    disagreed="$(list_delta "$zz_base" "$pin_base"),$(list_delta "$pin_base" "$zz_base")"
    compare_row "$recorded" "delta-${FACT_NAMES[$index]}" "$name delta-${FACT_NAMES[$index]}" \
      "$(list_without "$(list_delta "$zz_base" "$(tuple_field "$zz_tuple" "$index")")" "$disagreed")" \
      "$(list_without "$(list_delta "$pin_base" "$(tuple_field "$pin_tuple" "$index")")" "$disagreed")"
  done
  [ -n "$named" ] || return 0
  assert_row "$name flag-features" \
    "$(features_present "$(tuple_field "$zz_tuple" 4)" "$named")" \
    "$(features_present "$(tuple_field "$pin_tuple" 4)" "$named")"
}

# Restoration is the one place a detached client is the subject: the outer pane
# has to be back on its primary screen with autowrap, the cursor and every
# mouse mode where the client found them. Every row asserts here.
case_restore() {
  open_case restore xterm '' ''
  checkpoint restore
  detach_case
  local zz_tuple pin_tuple
  zz_tuple="$(read_modes zz)"
  pin_tuple="$(read_modes tmux)"
  compare_tuple "restore/detached" "" "$zz_tuple" "$pin_tuple" "${MODE_NAMES[@]}"
}

# The colour case reads the decoder's own grid, where the class survives. The
# sample line carries one named cell, one indexed cell, one RGB cell and one
# named background, and every stage asserts all four cells and the whole line.
COLOUR_SAMPLE="printf 'CLR \\033[31mR\\033[0m \\033[38;5;42mI\\033[0m \\033[38;2;10;20;30mX\\033[0m \\033[41mB\\033[0m END\\n'"
# The same line with the named cell spelled as the RGB colour it resolves to on
# a stock xterm palette: the same colour in a different class.
COLOUR_SAMPLE_RGB="printf 'CLR \\033[38;2;205;0;0mR\\033[0m \\033[38;5;42mI\\033[0m \\033[38;2;10;20;30mX\\033[0m \\033[41mB\\033[0m END\\n'"
PALETTE_NAMED='\033]4;1;rgb:00/ff/00\033\\'
PALETTE_INDEXED='\033]4;42;rgb:ff/00/ff\033\\'
PALETTE_RESET='\033]104\033\\'
colour_line() {
  outer_screen "$1" | grep -a 'CLR ' | grep -a 'END' | tail -n 1
}
strip_escapes() {
  printf '%s' "$1" | sed -e $'s/\033\\[[0-9;]*m//g'
}
# The SGR run the decoder left immediately in front of one glyph of the sample,
# which is that cell's colour class as the outer grid holds it. `CLR ` carries
# an R that no SGR precedes, so the pattern cannot pick it up.
cell_class() {
  local line="$1" glyph="$2" found
  found="$(printf '%s' "$line" | grep -ao $'\033\\[[0-9;]*m'"$glyph" | tail -n 1 | cat -v)"
  printf '%s\n' "${found:-none}"
}
# One stage: each side runs its own escape (an OSC 4, an OSC 104 or nothing),
# clears and prints its sample, and every cell of the line is compared. The
# clear keeps a one-sided command line off the compared screen.
colour_stage() {
  local stage="$1" zz_escape="$2" pin_escape="$3" zz_sample="$4" pin_sample="$5"
  local zz_line pin_line glyph
  side_command zz send-keys -t "$(side_pane zz)" "printf '$zz_escape'; clear; $zz_sample" Enter
  side_command tmux send-keys -t "$(side_pane tmux)" "printf '$pin_escape'; clear; $pin_sample" Enter
  checkpoint "colour$stage"
  zz_line="$(colour_line zz)"
  pin_line="$(colour_line tmux)"
  assert_row "colours/$stage glyphs" "$(strip_escapes "$zz_line")" "$(strip_escapes "$pin_line")"
  for glyph in R I X B; do
    assert_row "colours/$stage cell $glyph" \
      "$(cell_class "$zz_line" "$glyph")" "$(cell_class "$pin_line" "$glyph")"
  done
  assert_row "colours/$stage line" "$(printf '%s' "$zz_line" | cat -v)" \
    "$(printf '%s' "$pin_line" | cat -v)"
}

# THE STAGES CLAUSE 3 NAMES. stock: the pin keeps each class it was given.
# palette: once an OSC 4 entry exists for colour 1 the pin resolves the cell
# through the pane palette (tty.c tty_check_fg and tty_check_bg over
# colour_palette_get) and sends RGB, for the foreground AND the background the
# entry paints, while the indexed cell keeps its index. indexed: the same for
# an indexed entry. reset: OSC 104 clears every entry and both cells go back to
# their index. A sabotage names the one thing it hands the pin's side alone.
case_colours() {
  local sabotage="${1:-none}"
  local pin_sample="$COLOUR_SAMPLE" pin_named="$PALETTE_NAMED"
  local pin_indexed="$PALETTE_INDEXED" pin_reset="$PALETTE_RESET"
  case "$sabotage" in
  named-as-rgb) pin_sample="$COLOUR_SAMPLE_RGB" ;;
  named-entry) pin_named='\033]4;1;rgb:00/00/ff\033\\' ;;
  indexed-entry) pin_indexed='' ;;
  reset) pin_reset='' ;;
  esac
  open_case colours xterm-256color '' ''
  colour_stage stock '' '' "$COLOUR_SAMPLE" "$pin_sample"
  colour_stage palette "$PALETTE_NAMED" "$pin_named" "$COLOUR_SAMPLE" "$pin_sample"
  colour_stage indexed "$PALETTE_INDEXED" "$pin_indexed" "$COLOUR_SAMPLE" "$pin_sample"
  colour_stage reset "$PALETTE_RESET" "$pin_reset" "$COLOUR_SAMPLE" "$pin_sample"
}

# Unicode widths reach the outer terminal as the column the inner mux left its
# cursor in. The sample is a wide CJK pair, a base letter with a combining
# acute, and an astral emoji, so a side that gave any of the three the wrong
# width lands the cursor somewhere else. Both clients attach under a UTF-8
# locale, because a non-UTF-8 client changes what the pin puts on the wire for
# these bytes and that is the -u channel's subject, not this one. The typed
# command carries W%s so only the shell's own output can satisfy the settle.
WIDTH_SUFFIX=" \344\270\226\347\225\214 | e\314\201 | \360\237\230\200 |"
case_widths() {
  local zz_suffix="${1:-$WIDTH_SUFFIX}" pin_suffix="${2:-$WIDTH_SUFFIX}"
  local label="${3:-widths}" zz_flags="${4:-}" pin_flags="${5:-}"
  open_case "$label" xterm-256color "$zz_flags" "$pin_flags"
  side_command zz send-keys -t "$(side_pane zz)" \
    "clear; printf 'W%s$zz_suffix' 1" Enter
  side_command tmux send-keys -t "$(side_pane tmux)" \
    "clear; printf 'W%s$pin_suffix' 1" Enter
  local side
  for side in zz tmux; do
    wait_settled "$side" 'W1'
  done
  assert_row "$label/cursor" \
    "$(tmux_outer_command display-message -p -t "$(outer_window zz)" '#{cursor_x},#{cursor_y}')" \
    "$(tmux_outer_command display-message -p -t "$(outer_window tmux)" '#{cursor_x},#{cursor_y}')"
  compare_row "${6:-}" line "$label/line" \
    "$(outer_screen zz | grep -a 'W1' | head -n 1 | cat -v)" \
    "$(outer_screen tmux | grep -a 'W1' | head -n 1 | cat -v)"
}

# The flag diagnostics need no terminal at all: they are what each binary
# prints and returns before it ever opens one. Two things are normalised and
# nothing else is: the program name, because zz is not called tmux, and the
# version line, because zz answers `tmux 3.8-zz` where the pin answers
# `tmux next-3.8` - which is also what keeps a release bump from turning this
# row red.
normalise_diagnostic() {
  sed -e 's/^tmux: /PROG: /' -e 's/^zz: /PROG: /' \
    -e 's/^usage: tmux /usage: PROG /' -e 's/^usage: zz /usage: PROG /' \
    -e 's/^tmux [A-Za-z0-9.-]*$/tmux VERSION/'
}
run_cli() {
  local side="$1"
  shift
  local binary=("$ZZ_BIN")
  [ "$side" = zz ] || binary=("$TMUX_BIN")
  local output status
  output="$(scrubbed HOME="$SCRATCH_DIR/cli-home" XDG_CONFIG_HOME="$SCRATCH_DIR/cli-home/config" \
    TMUX_TMPDIR="$RUNTIME_DIR" TERM=xterm-256color "${binary[@]}" "$@" 2>&1)" && status=0 || status=$?
  printf 'exit=%s\n%s\n' "$status" "$(printf '%s' "$output" | normalise_diagnostic)"
}
case_cli() {
  local zz_extra="$1" tmux_extra="$2"
  mkdir -p "$SCRATCH_DIR/cli-home/config"
  local label
  for label in '-2 -V' '-u -V' '-T sixel -V' '-T' '-W -V'; do
    local -a arguments
    read -r -a arguments <<<"$label"
    assert_row "cli/$label" \
      "$(run_cli zz $zz_extra "${arguments[@]}")" \
      "$(run_cli tmux $tmux_extra "${arguments[@]}")"
  done
}

# --- driver ----------------------------------------------------------------

zz_command -f /dev/null daemon >"$SCRATCH_DIR/zz-daemon.out" 2>"$SCRATCH_DIR/zz-daemon.err" &
ZZ_PID=$!
wait_for "zz daemon socket" test -S "$ZZ_SOCKET"

zz_command new-session -d -s "$INNER_SESSION" -x 80 -y 24 "$INNER_SHELL" ||
  die "could not create the zz session"
tmux_inner_command -f /dev/null new-session -d -s "$INNER_SESSION" -x 80 -y 24 "$INNER_SHELL" ||
  die "could not create the tmux session"
ZZ_PANE="$(zz_command list-panes -t "=$INNER_SESSION" -F '#{pane_id}' | head -n 1)"
TMUX_PANE_ID="$(tmux_inner_command list-panes -t "=$INNER_SESSION" -F '#{pane_id}' | head -n 1)"
[ -n "$ZZ_PANE" ] && [ -n "$TMUX_PANE_ID" ] || die "an inner session has no pane"

tmux_outer_command -f /dev/null new-session -d -s "$OUTER_SESSION" -n hold -x 80 -y 24 \
  'sleep 86400' || die "could not create the outer session"
tmux_outer_command set-option -g status off
tmux_outer_command set-option -s extended-keys on

printf 'outer-terminal capability differential (pin %s)\n' "$(basename -- "$TMUX_BIN")"

if [ "$SELF_CHECK" -eq 0 ]; then
  case_modes 'legacy' xterm '' ''
  case_restore
  case_extended_key 'keys/off'
  side_command zz set-option -s extended-keys on >/dev/null
  side_command tmux set-option -s extended-keys on >/dev/null
  case_modes 'extended' xterm-256color '' ''
  case_extended_key 'keys/on'
  side_command zz set-option -s extended-keys always >/dev/null
  side_command tmux set-option -s extended-keys always >/dev/null
  case_modes 'extended-always' xterm-256color '' ''
  case_extended_key 'keys/always'
  side_command zz set-option -s extended-keys off >/dev/null
  side_command tmux set-option -s extended-keys off >/dev/null
  case_facts 'facts/bare' xterm '' '' "$FACT_RECORDED" baseline
  case_facts 'facts/-2' xterm -2 -2 "$FACT_RECORDED" '' 256
  case_facts 'facts/-u' xterm -u -u
  case_facts 'facts/-T' xterm '-T sixel' '-T sixel' "$FACT_RECORDED" '' sixel
  # A UTF-8 locale, where zz's fallback and the pin's are the same fallback:
  # both clients have to come up UTF-8 without any flag at all. It is the other
  # half of the -u measurement, and it says the gap is the flag and not the
  # rule behind it.
  CASE_LOCALE=C.UTF-8
  case_facts 'facts/utf8-locale' xterm '' ''
  case_widths
  CASE_LOCALE=C
  case_widths "$WIDTH_SUFFIX" "$WIDTH_SUFFIX" widths/non-utf8 '' '' line
  case_widths "$WIDTH_SUFFIX" "$WIDTH_SUFFIX" widths/-u -u -u
  case_colours
  case_cli '' ''

  printf '%s asserted rows, %s recorded rows\n' "$CHECKS" "$RECORDED"
  if [ "$FAILURES" -ne 0 ]; then
    printf '%s of %s asserted rows differ\n' "$FAILURES" "$CHECKS"
    exit 1
  fi
  printf 'all %s asserted rows identical\n' "$CHECKS"
  exit 0
fi

# --- self-check ------------------------------------------------------------
#
# Each sabotage drives the same driver with ONE deliberate one-sided difference
# and requires the comparison to report a difference in an asserted row. The
# recorded rows cannot satisfy a sabotage: they never fail.
SELF_CHECK_FAILURES=0
ASSERT_MODE=probe

self_check_case() {
  local name="$1" expectation="$2"
  shift 2
  CASE_DIFFS=0
  "$@"
  if [ "$expectation" = catches ] && [ "$CASE_DIFFS" -gt 0 ]; then
    printf 'ok    self-check %s: caught\n' "$name"
    return 0
  fi
  if [ "$expectation" = quiet ] && [ "$CASE_DIFFS" -eq 0 ]; then
    printf 'ok    self-check %s: no difference reported\n' "$name"
    return 0
  fi
  SELF_CHECK_FAILURES=$((SELF_CHECK_FAILURES + 1))
  printf 'FAIL  self-check %s: %s asserted rows differed, expected %s\n' \
    "$name" "$CASE_DIFFS" "$expectation"
}

printf 'self-check: one deliberate one-sided difference per channel\n'

# The control: the same case with nothing sabotaged has to be quiet, or a
# sabotage that "catches" would prove nothing.
self_check_case 'control, facts with no sabotage' quiet \
  case_facts 'sc/control' xterm '' '' "$FACT_RECORDED" baseline

# A one-sided flag. -u is the flag whose own-side effect the pin publishes as a
# client fact, so giving it to the pin alone has to show up in client_utf8,
# client_flags and delta-client_flags, all asserted rows.
self_check_case 'a one-sided flag, -u on the pin only' catches \
  case_facts 'sc/one-sided-u' xterm '' -u

# -T on one side: delta-client_termfeatures and flag-features have to catch it.
self_check_case 'a one-sided -T sixel on the pin only' catches \
  case_facts 'sc/one-sided-T' xterm '' '-T sixel' "$FACT_RECORDED" '' sixel

# -2 on one side: zz's TERM=xterm roster has no 256 of its own, so without the
# flag flag-features has to report 256:no against the pin's 256:yes.
self_check_case 'a one-sided -2 on the pin only' catches \
  case_facts 'sc/one-sided-2' xterm '' -2 "$FACT_RECORDED" '' 256

# The colour class itself: the pin's side writes the named cell as the RGB
# colour it resolves to, the same colour in another class, and the stock stage
# has to report it.
self_check_case 'a named cell spelled as its RGB colour on the pin only' catches \
  case_colours named-as-rgb

# A one-sided palette entry. Both sides resolve a set palette entry to the same
# RGB, so setting a different entry on one side has to break the palette line.
self_check_case 'a one-sided palette entry' catches case_colours named-entry

# An indexed entry set on one side only: zz's indexed cell has to leave its
# class for RGB while the pin's keeps 38;5;42, and the indexed stage catches it.
self_check_case 'an OSC 4 on an indexed entry sent to zz only' catches \
  case_colours indexed-entry

# A reset on one side only: zz's named and indexed cells go back to their
# index while the pin's stay RGB, and the reset stage catches it.
self_check_case 'an OSC 104 sent to zz only' catches case_colours reset

# The legacy-terminal case driven with extended keys on one side. With
# extended-keys off on both, pane_key_mode is VT10x on both and asserts; turning
# it on for the pin alone makes the pin arm Eneks and the decoder record Ext 1.
side_command tmux set-option -s extended-keys on >/dev/null
self_check_case 'the legacy case driven with extended keys on the pin' catches \
  case_modes 'sc/legacy-extended' xterm '' ''
# The decode channel: with extended keys on the pin alone, only the pin's client
# arms mode 2 and receives C-Enter as \e[27;5;13~, so only its binding fires.
self_check_case 'an extended key with extended keys on the pin only' catches \
  case_extended_key 'sc/extended-key'
side_command tmux set-option -s extended-keys off >/dev/null

# A one-sided CLI flag, on the channel that needs no terminal.
self_check_case 'a one-sided unknown CLI option' catches case_cli '' -W

# A one-sided width. One extra wide codepoint on the pin's sample has to move
# its cursor away from zz's and change its decoded line.
CASE_LOCALE=C.UTF-8
self_check_case 'a one-sided wide codepoint' catches \
  case_widths "$WIDTH_SUFFIX" "$WIDTH_SUFFIX\344\270\226"
CASE_LOCALE=C

ASSERT_MODE=count
if [ "$SELF_CHECK_FAILURES" -ne 0 ]; then
  printf '%s self-check cases did not behave as required\n' "$SELF_CHECK_FAILURES"
  exit 1
fi
printf 'self-check: every sabotage caught in its own channel\n'
exit 0
