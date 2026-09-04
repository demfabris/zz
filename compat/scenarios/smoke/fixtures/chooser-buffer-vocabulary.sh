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

session=cbvocab
work="$HOME/chooser-buffer-vocabulary-work-$side"
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
    main_client set-environment -gu CHOOSER_BUF >/dev/null 2>&1
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

chosen_buffer() {
    row="$(main_client show-environment -g CHOOSER_BUF 2>/dev/null || true)"
    printf '%s' "${row#CHOOSER_BUF=}"
}

buffers() {
    main_client list-buffers -F '#{buffer_name}' 2>/dev/null | sort | tr '\n' ' '
}

open_chooser() {
    main_client set-environment -g CHOOSER_BUF pending
    drive "keys 0f"
    sleep 0.5
}

press() {
    drive "keys $1"
}

settle() {
    attempt=0
    while [ "$attempt" -lt 60 ]; do
        if [ "$(chosen_buffer)" != pending ]; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    return 0
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client set-option -g status-keys emacs >/dev/null 2>&1 || true
main_client new-session -d -s "$session" -n w0
for stale in $(main_client list-buffers -F '#{buffer_name}' 2>/dev/null); do
    main_client delete-buffer -b "$stale" >/dev/null 2>&1 || true
done
main_client set-buffer -b b1 zzz
main_client set-buffer -b b2 y
main_client set-buffer -b b3 xx

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
    echo "chooser-buffer-vocabulary-$side: attach-client"
    exit 0
fi
sleep 0.5

check_equal setup-buffers 'b1 b2 b3 ' "$(buffers)"

main_client bind-key -n C-o choose-buffer 'set-environment -g CHOOSER_BUF %%'

# window_buffer_order_seq starts at creation, which is newest first, and O steps
# it to name and then to size before sort_next_order wraps it back. mode_tree_build
# puts the cursor back on the item it was on, not on the line, so the order is
# read from the first row after a g rather than from where the cursor stayed.
open_chooser
press 67
press 0d
settle
check_equal default-order-is-creation b3 "$(chosen_buffer)"

open_chooser
press 4f
press 67
press 0d
settle
check_equal O-steps-to-name b1 "$(chosen_buffer)"

open_chooser
press 4f
press 4f
press 67
press 0d
settle
check_equal O-steps-to-size b2 "$(chosen_buffer)"

open_chooser
press 4f
press 4f
press 4f
press 67
press 0d
settle
check_equal O-wraps-back-to-creation b3 "$(chosen_buffer)"

open_chooser
press 72
press 67
press 0d
settle
check_equal r-reverses-creation b1 "$(chosen_buffer)"

# mode_tree_display_help is the same screen here, and the next key of any kind
# closes it without reaching the chooser's own vocabulary.
open_chooser
press 67
press 1b4f50
press 71
press 0d
settle
check_equal F1-help-swallows-the-next-key b3 "$(chosen_buffer)"

open_chooser
press 67
press 08
press 71
press 0d
settle
check_equal ctrl-h-raises-the-same-help b3 "$(chosen_buffer)"

# P is mode_tree_each_tagged over window_buffer_do_paste with no fallback to the
# current row, so it runs once per tagged buffer in row order and the last one
# is the last write.
open_chooser
press 67
press 74
press 74
press 50
settle
check_equal P-pastes-every-tagged-buffer b2 "$(chosen_buffer)"

# It closes the mode even when nothing is tagged, and pastes nothing at all.
open_chooser
press 67
press 74
press 54
press 50
sleep 0.6
check_equal T-untags-and-P-pastes-nothing pending "$(chosen_buffer)"

# C-t tags every row, because no buffer row has a parent.
open_chooser
press 67
press 14
press 50
settle
check_equal ctrl-t-tags-every-row b1 "$(chosen_buffer)"

# D is the same each_tagged walk over window_buffer_do_delete, so it deletes
# nothing when nothing is tagged and leaves the mode standing.
open_chooser
press 67
press 44
sleep 0.5
check_equal D-with-no-tags-deletes-nothing 'b1 b2 b3 ' "$(buffers)"
press 0d
settle
check_equal D-leaves-the-chooser-open b3 "$(chosen_buffer)"

open_chooser
press 67
press 74
press 74
press 44
sleep 0.6
check_equal D-deletes-every-tagged-buffer 'b1 ' "$(buffers)"
press 71
sleep 0.3

if [ "$check_count" -ne 14 ]; then
    record_failure "total-checks" 14 "$check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g CHOOSER_BUFFER_VOCABULARY "clean:$check_count"
else
    sed "s/^/chooser-buffer-vocabulary-$side: /" "$work/failures"
fi
