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

session=chooser-confirm
work="$HOME/chooser-buffer-confirm-flag-work-$side"
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
    step=$((step + 1))
    echo quit >"$steps/step-$step" 2>/dev/null
    main_client kill-session -t "=$session" >/dev/null 2>&1
    if [ -n "$attach_pid" ]; then
        kill "$attach_pid" >/dev/null 2>&1
        wait "$attach_pid" >/dev/null 2>&1
    fi
    main_client set-environment -gu CHOOSER_BUFFER >/dev/null 2>&1
    main_client delete-buffer -b zzconfirm1 >/dev/null 2>&1
    main_client delete-buffer -b zzconfirm2 >/dev/null 2>&1
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
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "drive-$step"
    return 0
}

chosen_buffer() {
    row="$(main_client show-environment -g CHOOSER_BUFFER 2>/dev/null || true)"
    printf '%s' "${row#CHOOSER_BUFFER=}"
}

seed_buffers() {
    main_client delete-buffer -b zzconfirm1 >/dev/null 2>&1 || true
    main_client delete-buffer -b zzconfirm2 >/dev/null 2>&1 || true
    main_client set-buffer -b zzconfirm1 first
    main_client set-buffer -b zzconfirm2 second
}

remaining_buffers() {
    main_client list-buffers -f '#{m:zzconfirm*,#{buffer_name}}' -F '#{buffer_name}' \
        2>/dev/null | sort | tr '\n' ' '
}

# window_buffer_init never reads 'y', so every spelling of the flag has to leave
# the row the shortcut pastes and the row 'd' deletes exactly where they were.
variant() {
    label="$1"
    shift
    seed_buffers
    # shellcheck disable=SC2086
    main_client bind-key -n C-o choose-buffer "$@" \
        -f '#{m:zzconfirm*,#{buffer_name}}' 'set-environment -g CHOOSER_BUFFER %%'
    main_client set-environment -g CHOOSER_BUFFER pending
    drive "keys 0f"
    sleep 0.8
    drive "keys 30"
    attempt=0
    while [ "$attempt" -lt 200 ]; do
        if [ "$(chosen_buffer)" != pending ]; then
            break
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    check_equal "$label-paste" zzconfirm2 "$(chosen_buffer)"
    drive "keys 0f"
    sleep 0.8
    drive "keys 64"
    sleep 0.8
    check_equal "$label-delete" "zzconfirm1 " "$(remaining_buffers)"
    drive "keys 71"
    sleep 0.4
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
    echo "chooser-buffer-confirm-flag-$side: attach-client"
    exit 0
fi
sleep 0.5

variant control
variant alone -y
variant clustered -Zy
variant repeated -yy
variant separate -y -y

if [ "$check_count" -ne 10 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g CHOOSER_BUFFER_CONFIRM clean:10
else
    sed "s/^/chooser-buffer-confirm-flag-$side: /" "$work/failures"
fi
