#!/usr/bin/env bash
set -eEuo pipefail
eval "$(sed -e 's|^COMPAT_DIR=.*|COMPAT_DIR="$PWD/compat"|' -e '/^if \[ "$SELF_CHECK" -eq 1 \]; then/,$d' compat/tui-client-commands.sh)"
attach_both_at 80 24
pane="$(active_pane tmux)"
tmux_inner_command switch-mode -Z -t "$pane"
tmux_inner_command list-panes -t "$pane" -F '#{pane_id} #{pane_in_mode} #{pane_mode} #{window_zoomed_flag}'
tmux_inner_command split-window -d -t "$pane" "$INNER_SHELL"
tmux_inner_command resize-pane -Z -t "$pane"
tmux_inner_command list-panes -t "$pane" -F '#{pane_id} #{pane_in_mode} #{pane_mode} #{window_zoomed_flag}'
tmux_inner_command send-keys -t "$pane" Escape
tmux_inner_command list-panes -t "$pane" -F '#{pane_id} #{pane_in_mode} #{pane_mode} #{window_zoomed_flag}'
tmux_inner_command resize-pane -Z -t "$pane"
tmux_inner_command switch-mode -kZ -t "$pane"
tmux_inner_command clock-mode -t "$pane"
tmux_inner_command list-panes -t "$pane" -F '#{pane_id} #{pane_in_mode} #{pane_mode} #{window_zoomed_flag}'
tmux_inner_command send-keys -t "$pane" Escape
tmux_inner_command list-panes -t "$pane" -F '#{pane_id} #{pane_in_mode} #{pane_mode} #{window_zoomed_flag}'
tmux_inner_command send-keys -t "$pane" Escape
tmux_inner_command list-panes -t '=cli:0' -F '#{pane_id} #{pane_in_mode} #{pane_mode} #{window_zoomed_flag}'
