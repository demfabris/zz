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

session=menu-resize
work="$HOME/display-menu-resize-lifecycle-work-$side"
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
    main_client set-environment -gu MENU_RESIZE_ROW >/dev/null 2>&1
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
    row="$(main_client show-environment -g MENU_RESIZE_ROW 2>/dev/null || true)"
    printf '%s' "${row#MENU_RESIZE_ROW=}"
}

probe() {
    label="$1"
    columns="$2"
    rows="$3"
    bytes="$4"
    expected="$5"
    main_client set-environment -g MENU_RESIZE_ROW pending
    rm -f "$work/exit-$label"
    (
        main_client display-menu -c "$client" -x 40 -y 22 -T '' \
            alpha a 'set-environment -g MENU_RESIZE_ROW alpha' \
            beta b 'set-environment -g MENU_RESIZE_ROW beta'
        echo "$?" >"$work/exit-$label"
    ) &
    menu_pid=$!
    sleep 0.8
    drive "size $columns $rows"
    sleep 0.5
    drive "keys $bytes"
    attempt=0
    while [ "$attempt" -lt 200 ]; do
        if [ "$(chosen_row)" != pending ]; then
            break
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    check_equal "$label" "$expected" "$(chosen_row)"
    attempt=0
    while [ "$attempt" -lt 200 ]; do
        if [ -f "$work/exit-$label" ]; then
            break
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    if [ ! -f "$work/exit-$label" ]; then
        main_client display-popup -c "$client" -C
    fi
    wait "$menu_pid" >/dev/null 2>&1 || true
    check_equal "$label-exit" 0 "$(cat "$work/exit-$label" 2>/dev/null)"
    drive "size 80 24"
    sleep 0.3
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
    echo "display-menu-resize-lifecycle-$side: attach-client"
    exit 0
fi

probe shrink-inside 60 20 61 alpha
probe shrink-past-sidebar 40 10 62 beta
probe grow-back 100 30 61 alpha

if [ "$check_count" -ne 6 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g DISPLAY_MENU_RESIZE_LIFECYCLE clean:6
else
    sed "s/^/display-menu-resize-lifecycle-$side: /" "$work/failures"
fi
