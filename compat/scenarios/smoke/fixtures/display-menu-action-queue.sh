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

session=menu-action-queue
work="$HOME/display-menu-action-queue-work-$side"
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
    for name in MENU_ITEM MENU_AFTER MENU_CTX MENU_NESTED; do
        main_client set-environment -gu "$name" >/dev/null 2>&1
    done
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

value() {
    row="$(main_client show-environment -g "$1" 2>/dev/null || true)"
    printf '%s' "${row#"$1"=}"
}

seen() {
    if grep -q -- "$2" "$snaps/$1" 2>/dev/null; then
        printf yes
    else
        printf no
    fi
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session"

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
    record_failure attach-client
    echo "display-menu-action-queue-$side: attach-client"
    exit 0
fi
sleep 0.5

# display-menu holds the command queue until the client leaves the menu, then
# the chosen action runs and the rest of the chain follows it.
main_client set-environment -g MENU_ITEM pending
main_client set-environment -g MENU_AFTER pending
main_client display-menu -c "$client" -T '' \
    item q 'set-environment -g MENU_ITEM chosen' \; \
    set-environment -g MENU_AFTER after &
menu_pid=$!
sleep 1.0
check_equal blocks-the-chain pending "$(value MENU_AFTER)"
check_equal blocks-the-item pending "$(value MENU_ITEM)"
drive "keys 71"
sleep 1.2
wait "$menu_pid" >/dev/null 2>&1 || true
check_equal item-ran chosen "$(value MENU_ITEM)"
check_equal chain-continued after "$(value MENU_AFTER)"

# Leaving the menu without choosing continues the chain and runs nothing.
main_client set-environment -g MENU_ITEM pending
main_client set-environment -g MENU_AFTER pending
main_client display-menu -c "$client" -T '' \
    item q 'set-environment -g MENU_ITEM chosen' \; \
    set-environment -g MENU_AFTER after &
menu_pid=$!
sleep 1.0
drive "keys 1b"
sleep 1.2
wait "$menu_pid" >/dev/null 2>&1 || true
check_equal cancel-runs-nothing pending "$(value MENU_ITEM)"
check_equal cancel-continues-chain after "$(value MENU_AFTER)"

# menu_add_item expands every row and every row command against the menu's own
# client, so a client format in an action names that client.
main_client set-environment -g MENU_CTX pending
main_client display-menu -c "$client" -T '' \
    ctx q 'set-environment -g MENU_CTX #{client_name}' &
menu_pid=$!
sleep 1.0
drive "keys 71"
sleep 1.2
wait "$menu_pid" >/dev/null 2>&1 || true
check_equal action-client-context "$client" "$(value MENU_CTX)"

# The menu is gone before its action runs, so an action that opens a second
# menu gets the client to itself.
main_client set-environment -g MENU_NESTED pending
main_client display-menu -c "$client" -T '' \
    outer q "display-menu -T '' second s 'set-environment -g MENU_NESTED nested'" &
menu_pid=$!
sleep 1.0
drive "keys 71"
sleep 1.0
drive "keys 73"
sleep 1.2
wait "$menu_pid" >/dev/null 2>&1 || true
check_equal nested-menu-ran nested "$(value MENU_NESTED)"

# A row command that fails neither fails display-menu nor stops the chain, and
# the client is told either way: cmdq_error puts the message on that client's
# status line with the first letter raised, for a run-time failure and a parse
# failure alike. The probe reads a prefix because each client clips the line to
# the room its own status chrome leaves.
main_client set-environment -g MENU_AFTER pending
rm -f "$work/exit"
(
    main_client display-menu -c "$client" -T '' \
        bad b 'kill-pane -t nosuchpanename' \; \
        set-environment -g MENU_AFTER after
    echo "$?" >"$work/exit"
) &
menu_pid=$!
sleep 1.0
drive "keys 62"
sleep 0.5
drive "snap runtime-error"
sleep 0.8
wait "$menu_pid" >/dev/null 2>&1 || true
check_equal runtime-error-exit 0 "$(cat "$work/exit" 2>/dev/null)"
check_equal runtime-error-continues after "$(value MENU_AFTER)"
check_equal runtime-error-is-shown yes "$(seen runtime-error "Can't find pan")"

main_client display-menu -c "$client" -T '' bad b 'list-panes -Q' &
menu_pid=$!
sleep 1.0
drive "keys 62"
sleep 0.5
drive "snap parse-error"
sleep 0.8
wait "$menu_pid" >/dev/null 2>&1 || true
check_equal parse-error-is-shown yes "$(seen parse-error 'Command list-p')"

if [ "$check_count" -ne 12 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g DISPLAY_MENU_ACTION_QUEUE clean:12
else
    sed "s/^/display-menu-action-queue-$side: /" "$work/failures"
fi
