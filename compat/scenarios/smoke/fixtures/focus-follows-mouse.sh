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

session=focus-follows-mouse
work="$HOME/focus-follows-mouse-work-$side"
steps="$work/steps"
rm -rf "$work"
mkdir -p "$steps"
: >"$work/failures"
failed=0
check_count=0
step=0
attach_pid=""

record_failure() {
    failed=1
    echo "$1" >>"$work/failures"
}

check_equal() {
    check_count=$((check_count + 1))
    if [ "$2" != "$3" ]; then
        record_failure "$1"
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
    exit "$cleanup_status"
}
trap cleanup EXIT

drive() {
    step=$((step + 1))
    printf '%s\n' "$1" >"$steps/step-$step"
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if [ -f "$steps/ack-$step" ]; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "drive-$step"
    return 0
}

# The client is 79 columns so both engines lay the split out the same way: a
# one-column divider at column 39 with the right pane on columns 40..78.
pointer() {
    drive "keys $(printf '\033%s' "$1" | od -An -tx1 | tr -d ' \n')"
    sleep 0.7
}

active_pane() {
    main_client list-panes -s -t "=$session" \
        -F '#{pane_index}#{?pane_active,*,}' | tr '\n' ' '
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session" -x 79 -y 24
main_client set-option -g status off
main_client set-option -g mouse on
main_client set-option -g focus-follows-mouse off
main_client split-window -h -t "=$session:"

env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    -u TERM_PROGRAM -u TERM_PROGRAM_VERSION \
    TERM=xterm-256color \
    python3 "$HOME/pty-drive.py" "$steps" 79 24 \
    "$binary" $prefix_args attach-session -t "=$session" \
    >"$work/attach.out" 2>&1 &
attach_pid=$!

attempt=0
client=""
while [ "$attempt" -lt 400 ]; do
    client="$(main_client list-clients -t "=$session" -F '#{client_name}' | sed -n '1p')"
    if [ -n "$client" ]; then
        break
    fi
    attempt=$((attempt + 1))
    sleep 0.05
done
if [ -z "$client" ]; then
    record_failure attach-client
    echo "focus-follows-mouse-$side: attach-client"
    exit 0
fi
sleep 1.0

main_client select-pane -t "=$session:.0"
sleep 0.4
check_equal starts-on-the-first-pane '0* 1 ' "$(active_pane)"

# With the option off the pin's MOUSEMOVE is only turned into a key binding, so
# the pane under the pointer is left alone.
pointer '[<35;61;11M'
check_equal move-leaves-the-active-pane-alone '0* 1 ' "$(active_pane)"

# window_set_active_pane runs before the mouse key becomes a binding, so the
# pointer moves the active pane while the option is on.
main_client set-option -g focus-follows-mouse on
sleep 0.5
pointer '[<35;62;12M'
check_equal move-selects-the-pane-under-the-pointer '0 1* ' "$(active_pane)"

pointer '[<35;11;11M'
check_equal move-back-selects-the-first-pane '0* 1 ' "$(active_pane)"

# A held button reports as MOUSEDRAG, which never reaches the focus switch.
pointer '[<32;63;13M'
check_equal drag-never-selects-a-pane '0* 1 ' "$(active_pane)"
pointer '[<0;63;13m'
sleep 0.3
main_client select-pane -t "=$session:.0"
sleep 0.4

# The switch is read before the mouse option decides what the key becomes, so
# it still runs while `mouse` is off.
main_client set-option -g mouse off
sleep 0.5
pointer '[<35;64;14M'
check_equal move-selects-with-the-mouse-option-off '0 1* ' "$(active_pane)"

main_client set-option -g mouse on
main_client select-pane -t "=$session:.0"
sleep 0.5
main_client set-option -g focus-follows-mouse off
sleep 0.5
pointer '[<35;65;15M'
check_equal move-stops-selecting-once-the-option-is-off '0* 1 ' "$(active_pane)"

if [ "$check_count" -ne 7 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g FOCUS_FOLLOWS_MOUSE clean:7
else
    sed "s/^/focus-follows-mouse-$side: /" "$work/failures"
fi
