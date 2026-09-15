#!/usr/bin/env bash
# Oracle for TUI-008 formats.mouse-context: read #{mouse_word}, #{mouse_line}
# and #{mouse_hyperlink} out of a real SGR MouseDown3Pane report on one side.
#
# usage: oracle-probe.sh <binary> <kind: tmux|zz> [word-separators]
#
# The binary under test runs its own server on a throwaway socket. An outer
# pinned tmux owns the terminal and injects the SGR bytes with send-keys -H,
# exactly the way compat/tui-mouse.sh does, so the report is real.
set -eEuo pipefail

BIN="$1"
KIND="$2"
WORD_SEPARATORS="${3-}"
PIN="${ZZ_COMPAT_TMUX:-/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux}"

SCRATCH="$(mktemp -d /tmp/zzorc.XXXXXX)"
TOKEN="${SCRATCH##*.}"
OUTER="zzorco-$TOKEN"
INNER="zzorci-$TOKEN"
SOCK="/tmp/zzorc-$TOKEN.sock"
HOME_DIR="$SCRATCH/home"
OUTER_HOME="$SCRATCH/outer"
mkdir -p "$HOME_DIR" "$OUTER_HOME"

cleanup() {
  local status=$? pid
  trap - EXIT
  set +e
  env -u TMUX TMUX_TMPDIR=/tmp HOME="$OUTER_HOME" XDG_CONFIG_HOME="$OUTER_HOME/config" \
    "$PIN" -L "$OUTER" kill-server >/dev/null 2>&1
  inner kill-server >/dev/null 2>&1
  for pid in $(pgrep -f -- "$SCRATCH" 2>/dev/null); do kill "$pid" >/dev/null 2>&1; done
  rm -rf -- "$SCRATCH" "$SOCK"
  exit "$status"
}
trap cleanup EXIT

scrubbed() {
  env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u XDG_RUNTIME_DIR \
    -u XDG_STATE_HOME TMUX_TMPDIR=/tmp ZZ_TRAY=0 "$@"
}
outer() {
  scrubbed HOME="$OUTER_HOME" XDG_CONFIG_HOME="$OUTER_HOME/config" "$PIN" -L "$OUTER" "$@"
}
inner() {
  if [ "$KIND" = zz ]; then
    scrubbed HOME="$HOME_DIR" XDG_CONFIG_HOME="$HOME_DIR/config" "$BIN" --socket "$SOCK" "$@"
  else
    scrubbed HOME="$HOME_DIR" XDG_CONFIG_HOME="$HOME_DIR/config" "$BIN" -L "$INNER" "$@"
  fi
}

# The sample the probe reads. Printed by a program that then parks, so the
# grid is exactly these rows with no prompt and no job control noise.
XS="$(printf 'x%.0s' $(seq 1 60))"
LONGWORD="WRAPPEDHEAD${XS}LONGTAIL"          # 11 + 60 + 8 = 79 -> no wrap yet
LONGWORD="WRAPPEDHEAD${XS}${XS}LONGTAIL"     # 11 + 120 + 8 = 139 -> wraps once
ENDWORD_PAD="$(printf ' %.0s' $(seq 1 $((80 - 7))))"
SAMPLE=$(cat <<SAMPLEEOF
printf '\033[H\033[2J'
printf 'alpha beta gamma\n'
printf '%s\n' '$LONGWORD'
printf '\033]8;;https://example.com/page\033\\\\LINKTEXT\033]8;;\033\\\\\n'
printf 'pre\there post\n'
printf 'wide CJK日本語 tail\n'
printf '\n'
printf '%s%s\n' '$ENDWORD_PAD' 'ENDWORD'
printf 'dash-joined_word end\n'
while :; do sleep 1; done
SAMPLEEOF
)
printf '%s\n' "$SAMPLE" >"$SCRATCH/sample.sh"

write_attach() {
  {
    printf '#!/usr/bin/env bash\n'
    if [ "$KIND" = zz ]; then
      printf 'exec env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE -u XDG_RUNTIME_DIR -u XDG_STATE_HOME TMUX_TMPDIR=/tmp ZZ_TRAY=0 HOME=%q XDG_CONFIG_HOME=%q %q --socket %q attach-session -t probe\n' \
        "$HOME_DIR" "$HOME_DIR/config" "$BIN" "$SOCK"
    else
      printf 'exec env -u TMUX -u TMUX_PANE -u XDG_RUNTIME_DIR -u XDG_STATE_HOME TMUX_TMPDIR=/tmp HOME=%q XDG_CONFIG_HOME=%q %q -L %q attach-session -t probe\n' \
        "$HOME_DIR" "$HOME_DIR/config" "$PIN" "$INNER"
    fi
  } >"$SCRATCH/attach.sh"
  chmod +x "$SCRATCH/attach.sh"
}
write_attach

if [ "$KIND" = zz ]; then
  inner new-session -d -s probe -n win -x 80 -y 24 "sh $SCRATCH/sample.sh" >/dev/null
else
  inner -f /dev/null new-session -d -s probe -n win -x 80 -y 24 "sh $SCRATCH/sample.sh" >/dev/null
fi
inner set-option -g status-right '' >/dev/null
inner set-option -g status-left L >/dev/null
inner set-option -g automatic-rename off >/dev/null
inner set-option -g mouse on >/dev/null
[ -n "$WORD_SEPARATORS" ] && inner set-option -g word-separators "$WORD_SEPARATORS" >/dev/null
inner bind-key -n MouseDown3Pane set-option -gF @mc \
  '<#{mouse_word}|#{mouse_line}|#{mouse_hyperlink}|#{mouse_x},#{mouse_y}|#{mouse_status_line}|#{mouse_status_range}|#{mouse_pane}>' >/dev/null
for name in MouseDown3Status MouseDown3StatusLeft MouseDown3StatusRight MouseDown3StatusDefault; do
  inner bind-key -n "$name" set-option -gF @mc \
    '<#{mouse_word}|#{mouse_line}|#{mouse_hyperlink}|#{mouse_x},#{mouse_y}|#{mouse_status_line}|#{mouse_status_range}|#{mouse_pane}>' >/dev/null 2>&1 || \
    printf 'note: %s refused bind-key -n %s\n' "$KIND" "$name"
done

outer -f /dev/null new-session -d -s drv -n side -x 80 -y 24 "$SCRATCH/attach.sh" >/dev/null
outer set-option -g status off >/dev/null

wait_for() {
  local label="$1" attempt
  shift
  for ((attempt = 0; attempt < 200; attempt++)); do
    if "$@" >/dev/null 2>&1; then return 0; fi
    sleep 0.05
  done
  printf 'TIMEOUT: %s\n' "$label" >&2
  outer capture-pane -p -t '=drv:side' | cat -v >&2
  exit 3
}
screen_has() { outer capture-pane -p -t '=drv:side' | grep -Fq -- "$1"; }
wait_for 'the sample on the screen' screen_has LINKTEXT
wait_for 'the end word on the screen' screen_has ENDWORD

send_mouse() {
  local button="$1" column="$2" row="$3" kind="$4" hex
  hex="$(printf '\033[<%s;%s;%s%s' "$button" "$column" "$row" "$kind" | od -An -tx1 | tr -s ' \n' '  ')"
  # shellcheck disable=SC2086
  outer send-keys -t '=drv:side' -H $hex
}
option_set() { [ -n "$(inner show-options -gqv @mc 2>/dev/null)" ]; }

# A right press inside KEYC_CLICK_TIMEOUT of the previous one is a
# SecondClick3Pane, which nothing binds: server_client_check_mouse resets the
# sequence on the BUTTON, not on the cell. A left click between probes is that
# reset, and it is inert here (one pane, no application mouse).
reset_click_sequence() {
  send_mouse 0 40 20 M
  send_mouse 0 40 20 m
}
probe() {
  local label="$1" column="$2" row="$3" value
  reset_click_sequence
  inner set-option -gu @mc >/dev/null 2>&1 || true
  send_mouse 2 "$column" "$row" M
  wait_for "the $label report" option_set
  send_mouse 2 "$column" "$row" m
  value="$(inner show-options -gqv @mc)"
  printf '%-28s col=%-3s row=%-3s %s\n' "$label" "$column" "$row" "$(printf '%s' "$value" | cat -v)"
}

printf '=== %s (%s) word-separators=%s ===\n' "$KIND" "$BIN" "${WORD_SEPARATORS:-<default>}"
printf -- '--- the sample grid (capture-pane -p on the inner side) ---\n'
inner capture-pane -p -t '=probe:win.0' | cat -A | sed -n '1,10p'
printf -- '--- probes (SGR MouseDown3Pane, 1-based screen cell) ---\n'
probe plain-word-beta 8 1
probe word-first-cell 1 1
probe word-at-row-end 80 1
probe separator-cell 6 1
probe wrapped-head 3 2
probe wrapped-head-last-col 80 2
probe wrapped-tail 5 3
probe hyperlink 4 4
probe hyperlink-past-end 20 4
probe tab-row 2 5
probe tab-cell 4 5
probe wide-char-first-half 11 6
probe wide-char-second-half 12 6
probe blank-row 10 7
probe end-of-screen-word 78 8
probe dash-joined 3 9
probe underscore-joined 12 9
probe below-content 10 20
probe status-row-left 1 24
probe status-row-window 5 24
probe status-row-right 79 24
