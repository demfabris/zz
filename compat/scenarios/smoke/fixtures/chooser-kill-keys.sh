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

session=ckill
work="$HOME/chooser-kill-keys-work-$side"
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
    main_client list-windows -t "=$session" -F '#{window_name}' 2>/dev/null | tr '\n' ' '
}

# Open the chooser with the root binding, send the keys one at a time, and let
# the caller read back whichever of the two answers the probe is after: the
# template's `%%` target when a row was activated, or the window list when a
# kill went through.
open_chooser() {
    main_client set-environment -g CHOOSER_ROW pending
    drive "keys 0f"
    sleep 0.5
}

press() {
    drive "keys $1"
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
main_client new-session -d -s "$session" -n w0
for name in w1 w2 w3 w4; do
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
    echo "chooser-kill-keys-$side: attach-client"
    exit 0
fi
sleep 0.5

check_equal setup-windows 'w0 w1 w2 w3 w4 ' "$(windows)"

main_client bind-key -n C-o choose-tree -w -f "$mine" \
    'set-environment -g CHOOSER_ROW %%'

# window_tree_key's `x` raises mtd->prompt, which mode_tree_key answers before
# the chooser's own keys: the Enter that follows is eaten by the prompt instead
# of activating the row, and the answer is not `y`, so nothing is killed.
open_chooser
press 67
press 6a
press 6a
press 78
press 0d
settle
check_equal x-prompt-eats-the-next-key pending "$(chosen_row)"
check_equal x-prompt-declined-kills-nothing 'w0 w1 w2 w3 w4 ' "$(windows)"
press 71
sleep 0.3

# With the prompt closed by a key that is not `y`, the chooser is still there
# and the next Enter activates the row the cursor is on.
open_chooser
press 67
press 6a
press 6a
press 78
press 6e
press 0d
settle
check_equal x-declined-leaves-the-chooser-open "=$session:1." "$(chosen_row)"
check_equal x-declined-still-kills-nothing 'w0 w1 w2 w3 w4 ' "$(windows)"

# `y` answers it, window_tree_kill_each runs, and the mode stays open.
open_chooser
press 67
press 6a
press 6a
press 78
press 79
sleep 0.6
check_equal x-confirmed-kills-the-row 'w0 w2 w3 w4 ' "$(windows)"
press 0d
settle
check_equal x-confirmed-leaves-the-chooser-open "=$session:2." "$(chosen_row)"

# `t` tags the current row, so `X` has something to count and raises its own
# prompt, which eats the Enter the same way.
open_chooser
press 67
press 6a
press 6a
press 74
press 58
press 0d
settle
check_equal tagged-X-prompts pending "$(chosen_row)"
check_equal tagged-X-declined-kills-nothing 'w0 w2 w3 w4 ' "$(windows)"
press 71
sleep 0.3

# X is inert when mode_tree_count_tagged is zero: no prompt is raised, so the
# Enter that follows activates the row.
open_chooser
press 67
press 6a
press 6a
press 58
press 0d
settle
check_equal untagged-X-is-inert "=$session:2." "$(chosen_row)"

# `T` untags every row, which puts X back to inert.
open_chooser
press 67
press 6a
press 6a
press 74
press 54
press 58
press 0d
settle
check_equal T-untags-every-row "=$session:3." "$(chosen_row)"

# C-t tags every row whose parent is absent, which in a tree filtered to one
# session is that session's own row, so X finds a tag without `t`.
open_chooser
press 67
press 14
press 58
press 0d
settle
check_equal ctrl-t-tags-the-top-level pending "$(chosen_row)"
check_equal ctrl-t-kills-nothing-on-a-declined-prompt 'w0 w2 w3 w4 ' "$(windows)"
press 71
sleep 0.3

# -y is PROMPT_ACCEPT, which mode_tree_set_prompt turns into a queued `y`, so
# `x` kills with no answer key at all.
main_client bind-key -n C-o choose-tree -y -w -f "$mine" \
    'set-environment -g CHOOSER_ROW %%'
open_chooser
press 67
press 6a
press 6a
press 78
sleep 0.6
check_equal y-flag-answers-the-current-kill 'w0 w3 w4 ' "$(windows)"
press 71
sleep 0.3

# PROMPT_SINGLE is set on X too, so -y answers the tagged kill as well.
open_chooser
press 67
press 6a
press 6a
press 74
press 58
sleep 0.6
check_equal y-flag-answers-the-tagged-kill 'w0 w4 ' "$(windows)"
press 71
sleep 0.3

if [ "$check_count" -ne 15 ]; then
    record_failure "total-checks" 15 "$check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g CHOOSER_KILL_KEYS "clean:$check_count"
else
    sed "s/^/chooser-kill-keys-$side: /" "$work/failures"
fi
