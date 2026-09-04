#!/bin/sh
set -eu

export LC_ALL=C

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    side=zz
    binary="$ZZ_SMOKE_ZZ_BIN"
    set -- --socket "$ZZ_SMOKE_ZZ_SOCKET"
else
    side=tmux
    binary="$ZZ_SMOKE_TMUX_BIN"
    set -- -L "$ZZ_SMOKE_TMUX_LABEL"
fi
prefix_args="$*"
main_client() {
    # shellcheck disable=SC2086
    "$binary" $prefix_args "$@"
}

session=ckvocab
work="$HOME/chooser-tree-vocabulary-work-$side"
steps="$work/steps"
snaps="$work/snaps"
rm -rf "$work"
mkdir -p "$steps" "$snaps"
: >"$work/failures"
failed=0
check_count=0
step=0
attach_pid=""
client=""

record_failure() {
    failed=1
    echo "$1 want=[$2] got=[$3]" >>"$work/failures"
}

check_equal() {
    check_count=$((check_count + 1))
    if [ "$2" != "$3" ]; then
        record_failure "$1" "$2" "$3"
    fi
}

cleanup() {
    cleanup_status=$?
    trap - EXIT
    set +e
    step=$((step + 1))
    echo quit >"$steps/step-$step" 2>/dev/null
    main_client kill-session -t "=$session" >/dev/null 2>&1
    if [ -n "$attach_pid" ]; then
        kill "$attach_pid" >/dev/null 2>&1
        wait "$attach_pid" >/dev/null 2>&1
    fi
    main_client set-environment -gu CHOOSER_ROW >/dev/null 2>&1
    main_client unbind-key -n C-o >/dev/null 2>&1
    exit "$cleanup_status"
}
trap cleanup EXIT

drive() {
    step=$((step + 1))
    printf '%s\n' "$1" >"$steps/step-$step"
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if [ -f "$steps/ack-$step" ]; then
            sleep 0.25
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "drive-$step" ack ""
    return 0
}

chosen_row() {
    row="$(main_client show-environment -g CHOOSER_ROW 2>/dev/null || true)"
    printf '%s' "${row#CHOOSER_ROW=}"
}

windows() {
    main_client list-windows -t "=$session" \
        -F '#{?window_active,*,}#{window_index}:#{window_name}' 2>/dev/null | tr '\n' ' '
}

marks() {
    main_client list-panes -s -t "=$session" \
        -F '#{window_index}#{?pane_marked,!,-}' 2>/dev/null | tr '\n' ' '
}

open_chooser() {
    main_client set-environment -g CHOOSER_ROW pending
    drive "keys 0f"
    sleep 0.5
}

press() {
    drive "keys $1"
}

type_text() {
    drive "keys $(printf '%s' "$1" | od -An -v -tx1 | tr -d ' \n')"
}

settle() {
    attempt=0
    while [ "$attempt" -lt 60 ]; do
        if [ "$(chosen_row)" != pending ]; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    return 0
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client set-option -g status-keys emacs >/dev/null 2>&1 || true
main_client new-session -d -s "$session" -n d
for name in c e a b; do
    main_client new-window -t "=$session" -n "$name" -d
done
main_client select-window -t "=$session:0"
mine="#{==:#{session_name},$session}"

env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    TERM=xterm-256color \
    python3 "$HOME/chooser-drive.py" "$steps" "$snaps" 80 24 \
    "$binary" $prefix_args attach-session -t "=$session" \
    >"$work/attach.out" 2>&1 &
attach_pid=$!

attempt=0
while [ "$attempt" -lt 400 ]; do
    client="$(main_client list-clients -t "=$session" -F '#{client_name}' | sed -n '1p')"
    if [ -n "$client" ]; then
        break
    fi
    attempt=$((attempt + 1))
    sleep 0.05
done
if [ -z "$client" ]; then
    echo "chooser-tree-vocabulary-$side: attach-client"
    exit 0
fi
sleep 0.5

check_equal setup-windows '*0:d 1:c 2:e 3:a 4:b ' "$(windows)"

main_client bind-key -n C-o choose-tree -w -f "$mine" \
    'set-environment -g CHOOSER_ROW %%'

# mode_tree_swap hands the current row and the next one at the same depth to
# window_tree_swap, which trades the two windows between their slots and moves
# curw so the same window stays current. mtd->current follows the row it moved
# to, so J puts them back and the Enter after it lands on the same window.
open_chooser
press 67
press 6a
press 6a
press 4b
sleep 0.5
check_equal K-swaps-the-window-above '0:c *1:d 2:e 3:a 4:b ' "$(windows)"
press 4a
sleep 0.5
check_equal J-swaps-it-back '*0:d 1:c 2:e 3:a 4:b ' "$(windows)"
press 0d
settle
check_equal swap-keeps-the-row-on-the-moved-window "=$session:1." "$(chosen_row)"

# The first window row has only the session row above it, one depth up, so
# mode_tree_swap gives up before it reaches window_tree_swap; the last row has
# nothing below it at all.
open_chooser
press 67
press 6a
press 4b
sleep 0.4
check_equal K-refuses-across-a-depth '*0:d 1:c 2:e 3:a 4:b ' "$(windows)"
press 47
press 4a
sleep 0.4
check_equal J-refuses-at-the-last-row '*0:d 1:c 2:e 3:a 4:b ' "$(windows)"
press 0d
settle
check_equal refused-swap-still-activates "=$session:4." "$(chosen_row)"

# O steps window_tree_order_seq, which starts at index; one step is name, and
# a name order makes the second window row the window named b at index 4.
# sort_would_window_tree_swap then refuses K, because the two rows do not
# compare equal under that order.
open_chooser
press 67
press 4f
press 6a
press 6a
press 4b
sleep 0.4
check_equal O-name-order-refuses-a-swap '*0:d 1:c 2:e 3:a 4:b ' "$(windows)"
press 0d
settle
check_equal O-steps-to-name-order "=$session:4." "$(chosen_row)"

# The sequence is index, name, activity, z, and sort_next_order wraps from the
# end, so four steps are back where it started.
open_chooser
press 67
press 4f
press 4f
press 4f
press 4f
press 6a
press 0d
settle
check_equal O-wraps-back-to-index "=$session:0." "$(chosen_row)"

# r flips sort_crit.reversed and rebuilds, so the second window row under a
# reversed index order is the window at index 3.
open_chooser
press 67
press 72
press 6a
press 6a
press 0d
settle
check_equal r-reverses-the-order "=$session:3." "$(chosen_row)"

# M-- collapses every top-level item rather than the current one, which leaves
# the session row alone in the tree, and M-+ expands them all again.
open_chooser
press 67
press 1b2d
press 6a
press 6a
press 0d
settle
check_equal meta-minus-collapses-every-top-level "=$session:" "$(chosen_row)"

open_chooser
press 67
press 1b2d
press 1b2b
press 6a
press 0d
settle
check_equal meta-plus-expands-every-top-level "=$session:0." "$(chosen_row)"

# mode_tree_display_help puts the help screen up, and the next key of any kind
# closes it and does nothing else: the q that follows F1 does not cancel the
# chooser, so the Enter after it still activates the row.
open_chooser
press 67
press 6a
press 1b4f50
press 71
press 0d
settle
check_equal F1-help-swallows-the-next-key "=$session:0." "$(chosen_row)"

open_chooser
press 67
press 6a
press 08
press 71
press 0d
settle
check_equal ctrl-h-raises-the-same-help "=$session:0." "$(chosen_row)"

# window_tree_key's m is server_set_marked on the pane the row pulls, which for
# a window row is that window's active pane, and M is server_clear_marked.
open_chooser
press 67
press 6a
press 6a
press 6d
sleep 0.4
check_equal m-marks-the-rows-pane '0- 1! 2- 3- 4- ' "$(marks)"
press 4d
sleep 0.4
check_equal M-clears-the-mark '0- 1- 2- 3- 4- ' "$(marks)"
press 71
sleep 0.3

# ':' raises the mode's own command prompt, which is an edited line rather than
# a single key, and window_tree_command_each runs it once for the current row
# when nothing is tagged.
open_chooser
press 67
press 6a
press 6a
press 3a
type_text 'set-environment -g CHOOSER_ROW %%'
press 0d
settle
check_equal colon-runs-on-the-current-row "=$session:1." "$(chosen_row)"
press 71
sleep 0.3

# With rows tagged it runs once per tagged row instead, so the last write is
# the second tagged row and never the row the cursor sits on.
open_chooser
press 67
press 6a
press 6a
press 74
press 74
press 3a
type_text 'set-environment -g CHOOSER_ROW %%'
press 0d
settle
check_equal colon-runs-on-every-tagged-row "=$session:2." "$(chosen_row)"
press 71
sleep 0.3

# mode_tree_set_prompt only queues the PROMPT_ACCEPT answer when PROMPT_SINGLE
# is set beside it, which ':' does not pass: -y leaves this prompt to be typed,
# and the x that follows is text in the line rather than a kill.
main_client bind-key -n C-o choose-tree -y -w -f "$mine" \
    'set-environment -g CHOOSER_ROW %%'
open_chooser
press 67
press 6a
press 6a
press 3a
press 78
type_text 'set-environment -g CHOOSER_ROW typed'
press 0d
settle
check_equal dash-y-does-not-answer-the-command-prompt pending "$(chosen_row)"
check_equal dash-y-command-prompt-killed-nothing '*0:d 1:c 2:e 3:a 4:b ' "$(windows)"
press 71
sleep 0.3

if [ "$check_count" -ne 21 ]; then
    record_failure "total-checks" 21 "$check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g CHOOSER_TREE_VOCABULARY "clean:$check_count"
else
    sed "s/^/chooser-tree-vocabulary-$side: /" "$work/failures"
fi
