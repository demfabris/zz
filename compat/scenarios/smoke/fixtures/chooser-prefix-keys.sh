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
    main_client new-session -d -s "$session" -n w0 -x 80 -y 24 "$@"
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

hi_there() {
    main_client capture-pane -p -t "=$session:$1" | grep -cx hi-there || true
}

active_pane() {
    main_client display-message -p -t "=$session:" '#{pane_index}'
}

case_start() {
    name="$1"
    shift
    fresh_scene "$@"
    attach "$name"
}

detach_case choose-session 02 73
detach_case choose-window 02 77
detach_case choose-buffer 02 3d
detach_case tree-search-prompt 02 73 2f 61
detach_case tree-filter-prompt 02 73 66 61

typed=6563686f2068692d74686572650d

case_start tree-prefix-n
press 02
press 77
press 02
press 6e
press "$typed"
sleep 0.4
check_equal tree-prefix-n-selects-the-next-window '0 *1 ' "$(windows)"
check_equal tree-prefix-n-types-into-the-next-window 1 "$(hi_there 1)"
detach_all

case_start tree-prefix-c
press 02
press 77
press 02
press 63
press "$typed"
sleep 0.4
check_equal tree-prefix-c-creates-a-window '0 1 *2 ' "$(windows)"
check_equal tree-prefix-c-types-into-the-new-window 1 "$(hi_there 2)"
detach_all

case_start tree-copy-mode
press 02
press 77
press 02
press 5b
press 71
press 6a
press 0d
sleep 0.4
check_equal tree-copy-mode-q-returns-to-the-tree '0 *1 ' "$(windows)"
detach_all

case_start tree-view-mode
press 02
press 77
press 02
press 3f
press 71
press 6a
press 0d
sleep 0.4
check_equal tree-view-mode-q-returns-to-the-tree '0 *1 ' "$(windows)"
detach_all

case_start tree-other-pane
main_client split-window -d -t "=$session:0"
press 02
press 77
press 02
press 6f
press "$typed"
sleep 0.4
check_equal tree-other-pane-types-into-the-pane 1 "$(hi_there 0.1)"
check_equal tree-other-pane-keeps-the-windows '*0 1 ' "$(windows)"
press 02
press 1b5b41
press 6a
press 0d
sleep 0.4
check_equal tree-other-pane-up-returns-to-the-tree '0 *1 ' "$(windows)"
detach_all

main_client bind-key W choose-tree -w
case_start tree-display-panes
main_client split-window -d -t "=$session:0"
press 02
press 57
press 02
press 71
press 31
sleep 0.4
check_equal tree-display-panes-selects-the-pane 1 "$(active_pane)"
press 02
press 6f
press 6a
press 0d
sleep 0.4
check_equal tree-display-panes-keeps-the-tree '0 *1 ' "$(windows)"
detach_all
main_client unbind-key W

case_start copy-mode-tree-repeat
press 02
press 5b
press 02
press 77
press 02
press 1b5b41
press 6a
press 0d
sleep 0.4
check_equal copy-mode-tree-key-after-a-repeat '0 *1 ' "$(windows)"
detach_all

case_start copy-mode-tree-timeout
press 02
press 5b
press 02
press 77
main_client set-option -g prefix-timeout 300
press 02
sleep 0.8
press 6a
press 0d
sleep 0.4
check_equal copy-mode-tree-key-after-a-prefix-timeout '0 *1 ' "$(windows)"
detach_all
main_client set-option -gu prefix-timeout

case_start tree-send-prefix 'exec cat -v'
press 02
press 77
press 02
press 02
press 71
press 5a0d
sleep 0.4
check_equal tree-send-prefix-pages-the-tree 'Z|Z|' \
    "$(main_client capture-pane -p -t "=$session:0" | grep -F Z | tr '\n' '|')"
detach_all

if [ "$check_count" -ne 24 ]; then
    record_failure "total-checks" 24 "$check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g CHOOSER_PREFIX_KEYS "clean:$check_count"
else
    sed "s/^/chooser-prefix-keys-$side: /" "$work/failures"
fi
