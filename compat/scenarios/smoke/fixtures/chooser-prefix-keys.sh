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

session=cprefix
work="$HOME/chooser-prefix-keys-work-$side"
rm -rf "$work"
mkdir -p "$work"
: >"$work/failures"
failed=0
check_count=0
case_dir=""
step=0
attach_pid=""

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

drive() {
    step=$((step + 1))
    printf '%s\n' "$1" >"$case_dir/steps/step-$step"
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if [ -f "$case_dir/steps/ack-$step" ]; then
            sleep 0.25
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "drive-$step" ack ""
    return 0
}

press() {
    drive "keys $1"
}

clients() {
    main_client list-clients -t "=$session" -F '#{client_name}' 2>/dev/null | sed '/^$/d' | wc -l | tr -d ' '
}

windows() {
    main_client list-windows -t "=$session" \
        -F '#{?window_active,*,}#{window_index}' 2>/dev/null | tr '\n' ' '
}

detach_all() {
    main_client detach-client -s "=$session" >/dev/null 2>&1 || true
    wait_detached
    if [ -n "$attach_pid" ]; then
        drive quit
        wait "$attach_pid" >/dev/null 2>&1 || true
        attach_pid=""
    fi
}

cleanup() {
    cleanup_status=$?
    trap - EXIT
    set +e
    detach_all
    main_client kill-session -t "=$session" >/dev/null 2>&1
    main_client delete-buffer -b cprefix >/dev/null 2>&1
    exit "$cleanup_status"
}
trap cleanup EXIT

fresh_scene() {
    main_client kill-session -t "=$session" >/dev/null 2>&1 || true
    main_client new-session -d -s "$session" -n w0 -x 80 -y 24
    main_client new-window -d -t "=$session:1" -n w1
    main_client select-window -t "=$session:0"
    main_client set-buffer -b cprefix cprefix
}

attach() {
    case_dir="$work/$1"
    step=0
    mkdir -p "$case_dir/steps" "$case_dir/snaps"
    env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
        TERM=xterm-256color \
        python3 "$HOME/chooser-drive.py" "$case_dir/steps" "$case_dir/snaps" 80 24 \
        "$binary" $prefix_args attach-session -t "=$session" \
        >"$case_dir/attach.out" 2>&1 &
    attach_pid=$!
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if [ "$(clients)" = 1 ]; then
            sleep 0.5
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "$1-attach" 1 "$(clients)"
}

wait_detached() {
    attempt=0
    while [ "$attempt" -lt 100 ]; do
        if [ "$(clients)" = 0 ]; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
}

detach_case() {
    name="$1"
    shift
    fresh_scene
    attach "$name"
    for keys in "$@"; do
        press "$keys"
    done
    sleep 0.4
    check_equal "$name-mode-kept-the-client" 1 "$(clients)"
    press 02
    press 64
    wait_detached
    check_equal "$name-prefix-d-detaches" 0 "$(clients)"
    detach_all
}

detach_case choose-session 02 73
detach_case choose-window 02 77
detach_case choose-client 02 44
detach_case choose-buffer 02 3d
detach_case tree-search-prompt 02 73 2f 61
detach_case tree-filter-prompt 02 73 66 61
detach_case customize-mode 02 43
detach_case copy-mode 02 5b

fresh_scene
attach next-window
press 02
press 73
press 02
press 6e
sleep 0.4
check_equal tree-prefix-n-selects-the-next-window '0 *1 ' "$(windows)"
press 02
press 63
sleep 0.4
check_equal tree-prefix-c-creates-a-window '0 1 *2 ' "$(windows)"
detach_all

if [ "$check_count" -ne 18 ]; then
    record_failure "total-checks" 18 "$check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g CHOOSER_PREFIX_KEYS "clean:$check_count"
else
    sed "s/^/chooser-prefix-keys-$side: /" "$work/failures"
fi
