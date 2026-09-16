#!/usr/bin/env bash
set -eEuo pipefail
cd "$(dirname "$0")/../../../../.."
eval "$(sed -e 's|^COMPAT_DIR=.*|COMPAT_DIR="$PWD/compat"|' -e '/^if \[ "$SELF_CHECK" -eq 1 \]; then/,$d' compat/tui-client-commands.sh)"
attach_both_at 80 24
case_run probe-baseline same '' -- display-message -p -t PANE '#{pane_in_mode}/#{pane_mode}'
run_both clock-mode -t PANE
case_run probe-injected-x same '' -- send-keys -t PANE x
case_run probe-injected-mode same '' -- display-message -p -t PANE '#{pane_in_mode}/#{pane_mode}'
case_run probe-injected-shell same '' -- capture-pane -p -t PANE
for key in BTab KPEnter; do
  run_both clock-mode -t PANE
  case_run "probe-injected-$key" same '' -- send-keys -t PANE "$key"
  case_run "probe-injected-$key-shell" same '' -- capture-pane -p -t PANE
done
case_run probe-chooser-flag-status same '' -- choose-client -Q -t PANE
case_run probe-chooser-arity-status same '' -- choose-client -t PANE one two
case_run probe-refresh-argument-status same '' -- refresh-client -r
run_both clock-mode -t PANE
case_run probe-cancel-clock same '' -- copy-mode -q -t PANE
case_run probe-cancel-state same '' -- display-message -p -t PANE '#{pane_in_mode}/#{pane_mode}'
run_both clock-mode -t PANE
case_run probe-stack-switch same '' -- switch-mode -t PANE
case_run probe-stack-depth same '' -- display-message -p -t PANE '#{pane_in_mode}/#{pane_mode}'
for side in tmux zz; do
  [ "$(side_command "$side" display-message -p -t "$(active_pane "$side")" '#{pane_in_mode}/#{pane_mode}')" = '2/switch-mode' ] || die "wrong $side stack depth"
  tmux_outer_command send-keys -t "=$OUTER_SESSION:$side" Escape
done
CASE_CLOCK_FACE=1
case_run probe-restored-clock same '' -- display-message -p -t PANE '#{pane_in_mode}/#{pane_mode}'
for side in tmux zz; do
  [ "$(side_command "$side" display-message -p -t "$(active_pane "$side")" '#{pane_in_mode}/#{pane_mode}')" = '1/clock-mode' ] || die "wrong restored $side mode"
done
case_run probe-stack-exit same '' -- send-keys -t PANE x
case_run probe-stack-shell same '' -- capture-pane -p -t PANE
run_on_both new-session -d -s alpha -n "$WINDOW_NAME" -x 80 -y 24 "$INNER_SHELL"
run_on_both new-session -d -s zulu -n "$WINDOW_NAME" -x 80 -y 24 "$INNER_SHELL"
case_run probe-switch-format same '' -- switch-mode -F 'REVIEW-#{session_name}' -t PANE
restore_case probe-switch-format-closed
case_run probe-switch-window-ties same '' -- switch-mode -w -F '#{session_name}:#{window_name}' -t PANE
restore_case probe-switch-window-ties-closed
case_run probe-switch-template same '' -- switch-mode -t PANE 'set-option -g @review-command yes'
for side in tmux zz; do
  tmux_outer_command send-keys -t "=$OUTER_SESSION:$side" Enter
done
case_run probe-template-effect same '' -- show-options -gv @review-command
for side in tmux zz; do
  [ "$(side_command "$side" show-options -gv @review-command)" = yes ] || die "missing $side template effect"
done
run_on_both kill-session -t '=alpha'
run_on_both kill-session -t '=zulu'
case_run probe-formatted-identity same '' -- server-access '#{?#{==:1,1},nobody,root}'
for side in tmux zz; do
  [ "$(cat "$SCRATCH_DIR/$side.rc")" = 0 ] || die "formatted identity failed on $side"
  [ ! -s "$SCRATCH_DIR/$side.out" ] && [ ! -s "$SCRATCH_DIR/$side.err" ] || die "formatted identity printed on $side"
done
run_on_both split-window -d -t '=cli:win' "$INNER_SHELL"
case_run probe-switch-kill record 'TUI-014: -k remains explicitly rejected and measured' -- switch-mode -k -t PANE
for side in tmux zz; do
  tmux_outer_command send-keys -t "=$OUTER_SESSION:$side" Escape
done
case_run probe-switch-kill-effect record 'TUI-014: pin kills the source pane on Escape; zz retains it' -- list-panes -t '=cli:win' -F '#{pane_index}'
printf 'probe complete: %s asserted, %s failures, %s recorded\n' "$CHECKS" "$FAILURES" "$RECORDS"
[ "$FAILURES" -eq 0 ]
