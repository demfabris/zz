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

session=menu-grammar
work="$HOME/display-menu-shortcut-grammar-work-$side"
steps="$work/steps"
rm -rf "$work"
mkdir -p "$steps"
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
    if [ -n "$client" ]; then
        main_client display-popup -c "$client" -C >/dev/null 2>&1
    fi
    step=$((step + 1))
    echo quit >"$steps/step-$step" 2>/dev/null
    main_client kill-session -t "=$session" >/dev/null 2>&1
    if [ -n "$attach_pid" ]; then
        kill "$attach_pid" >/dev/null 2>&1
        wait "$attach_pid" >/dev/null 2>&1
    fi
    main_client set-environment -gu MENU_GRAMMAR_ROW >/dev/null 2>&1
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

chosen_row() {
    row="$(main_client show-environment -g MENU_GRAMMAR_ROW 2>/dev/null || true)"
    printf '%s' "${row#MENU_GRAMMAR_ROW=}"
}

probe() {
    label="$1"
    spelling="$2"
    bytes="$3"
    expected="$4"
    main_client set-environment -g MENU_GRAMMAR_ROW pending
    rm -f "$work/exit-$label"
    (
        main_client display-menu -c "$client" -T '' \
            alpha "$spelling" 'set-environment -g MENU_GRAMMAR_ROW alpha' \
            beta z 'set-environment -g MENU_GRAMMAR_ROW beta'
        echo "$?" >"$work/exit-$label"
    ) &
    menu_pid=$!
    sleep 0.8
    drive "keys $bytes"
    if [ "$expected" = pending ]; then
        sleep 0.5
        main_client display-popup -c "$client" -C
    else
        attempt=0
        while [ "$attempt" -lt 200 ]; do
            if [ "$(chosen_row)" != pending ]; then
                break
            fi
            attempt=$((attempt + 1))
            sleep 0.05
        done
    fi
    check_equal "$label" "$expected" "$(chosen_row)"
    attempt=0
    while [ "$attempt" -lt 200 ]; do
        if [ -f "$work/exit-$label" ]; then
            break
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    wait "$menu_pid" >/dev/null 2>&1 || true
    check_equal "$label-exit" 0 "$(cat "$work/exit-$label" 2>/dev/null)"
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session"

env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    TERM=xterm-256color \
    python3 "$HOME/pty-drive.py" "$steps" 80 24 \
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
    echo "display-menu-shortcut-grammar-$side: attach-client"
    exit 0
fi

probe caret '^A' 01 alpha
probe control 'C-a' 01 alpha
probe both-modifiers 'C-M-x' 1b18 alpha
probe space 'Space' 20 alpha
probe back-tab 'BTab' 1b5b5a alpha
probe function 'F12' 1b5b32347e alpha
probe long-modifier 'Ctrl-Alt-x' 1b18 pending
probe hex '0x41' 41 pending
probe shifted 'S-a' 41 pending
probe word 'Frobnicate' 41 pending

if [ "$check_count" -ne 20 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g DISPLAY_MENU_SHORTCUT_GRAMMAR clean:20
else
    sed "s/^/display-menu-shortcut-grammar-$side: /" "$work/failures"
fi
