#!/usr/bin/env bash
# Regenerates crates/zz-mux/tests/fixtures/layout-pin.txt from the pinned tmux
# binary. Run after a pin bump, then review the diff like any golden update.
set -euo pipefail

COMPAT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
TMUX_BIN="$("$COMPAT_DIR/fetch-tmux.sh")"
SOCK="fixgen-$$"

t() { "$TMUX_BIN" -L "$SOCK" "$@"; }

run_fixture() {
  local name="$1"
  shift
  t -f /dev/null new-session -d -s w >/dev/null
  local step
  for step in "$@"; do
    eval "t $step" >/dev/null
  done
  local layout
  layout="$(t list-windows -t w -F '#{window_layout}')"
  local panes
  panes="$(t list-panes -t w -F '#{pane_index}:#{pane_width}x#{pane_height}' | tr '\n' ' ')"
  printf '%s\n  layout: %s\n  panes:  %s\n  steps:\n' "$name" "$layout" "$panes"
  for step in "$@"; do
    printf '    %s\n' "$step"
  done
  printf '\n'
  t kill-server >/dev/null 2>&1 || true
  sleep 0.1
}

trap '"$TMUX_BIN" -L "$SOCK" kill-server >/dev/null 2>&1 || true' EXIT

run_fixture "single"
run_fixture "split-h-default" "split-window -h -t w:0"
run_fixture "split-v-default" "split-window -v -t w:0"
run_fixture "split-h-then-v-right" "split-window -h -t w:0" "split-window -v -t w:0.1"
run_fixture "split-h-l30" "split-window -h -l 30 -t w:0"
run_fixture "split-h-p25" "split-window -h -p 25 -t w:0"
run_fixture "split-h-before" "split-window -h -b -t w:0"
run_fixture "split-v-l5" "split-window -v -l 5 -t w:0"
run_fixture "three-h-nested" "split-window -h -t w:0.0" "split-window -h -t w:0.1"
run_fixture "three-h-siblings-even" "split-window -h -t w:0.0" "split-window -h -t w:0.0"
run_fixture "kill-middle-of-three" "split-window -h -t w:0.0" "split-window -h -t w:0.1" "kill-pane -t w:0.1"
run_fixture "kill-gives-after-neighbor" "split-window -v -t w:0.0" "split-window -v -t w:0.1" "kill-pane -t w:0.0"
run_fixture "resize-r10" "split-window -h -t w:0" "resize-pane -t w:0.0 -R 10"
run_fixture "resize-x30" "split-window -h -t w:0" "resize-pane -t w:0.0 -x 30"
run_fixture "resize-x30-from-right" "split-window -h -t w:0" "resize-pane -t w:0.1 -x 30"
run_fixture "resize-d3-nested" "split-window -h -t w:0" "split-window -v -t w:0.1" "resize-pane -t w:0.1 -D 3"
run_fixture "resize-u2-percent-mix" "split-window -v -t w:0" "resize-pane -t w:0.0 -y 75%"
run_fixture "split-f-full-width" "split-window -h -t w:0" "split-window -v -f -t w:0.0"
run_fixture "split-f-before-full" "split-window -v -t w:0" "split-window -h -f -b -t w:0.1"
run_fixture "even-horizontal-3" "split-window -h -t w:0.0" "split-window -h -t w:0.1" "select-layout -t w even-horizontal"
run_fixture "even-vertical-4" "split-window -v -t w:0.0" "split-window -v -t w:0.1" "split-window -v -t w:0.2" "select-layout -t w even-vertical"
run_fixture "main-horizontal-3" "split-window -h -t w:0.0" "split-window -h -t w:0.1" "select-layout -t w main-horizontal"
run_fixture "main-vertical-3" "split-window -h -t w:0.0" "split-window -h -t w:0.1" "select-layout -t w main-vertical"
run_fixture "main-horizontal-mirrored-3" "split-window -h -t w:0.0" "split-window -h -t w:0.1" "select-layout -t w main-horizontal-mirrored"
run_fixture "main-vertical-mirrored-3" "split-window -h -t w:0.0" "split-window -h -t w:0.1" "select-layout -t w main-vertical-mirrored"
run_fixture "tiled-5" "split-window -h -t w:0.0" "split-window -h -t w:0.1" "split-window -v -t w:0.0" "split-window -v -t w:0.2" "select-layout -t w tiled"
run_fixture "tiled-3" "split-window -h -t w:0.0" "split-window -h -t w:0.1" "select-layout -t w tiled"
run_fixture "spread-after-uneven" "split-window -h -t w:0.0" "split-window -h -t w:0.1" "resize-pane -t w:0.0 -x 60" "select-layout -t w -E"
run_fixture "break-pane" "split-window -h -t w:0" "break-pane -s w:0.1 -t w:"
run_fixture "join-pane-v" "split-window -h -t w:0" "break-pane -s w:0.1 -t w:" "join-pane -v -s w:1.0 -t w:0.0"
run_fixture "resize-window-grow" "split-window -h -t w:0.0" "split-window -h -t w:0.1" "resize-window -t w -x 100 -y 30"
run_fixture "resize-window-shrink" "split-window -h -t w:0.0" "split-window -h -t w:0.1" "resize-window -t w -x 50 -y 20"
run_fixture "resize-pane-overclamp" "split-window -h -t w:0" "resize-pane -t w:0.0 -L 100"
run_fixture "resize-pane-grow-overclamp" "split-window -h -t w:0" "resize-pane -t w:0.0 -R 100"
run_fixture "split-v-l1-minimum" "split-window -v -l 1 -t w:0"
