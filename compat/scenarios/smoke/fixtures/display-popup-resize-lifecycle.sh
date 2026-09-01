#!/bin/sh
set -eu

export LC_ALL=C

case "${1:-}" in
popup-loop)
    out=$2
    while :; do
        size="$(stty size 2>/dev/null || true)"
        if [ -n "$size" ]; then
            printf '%s\n' "$size" >"$out.tmp"
            mv "$out.tmp" "$out"
        fi
        sleep 0.2
    done
    ;;
esac

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

script="$HOME/display-popup-resize-lifecycle.sh"
session=popup-resize
work="$HOME/display-popup-resize-lifecycle-work-$side"
steps="$work/steps"
size_file="$work/size"
rm -rf "$work"
mkdir -p "$steps"
: >"$work/failures"
failed=0
check_count=0
step=0
attach_pid=""
popup_pid=""
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
    if [ -n "$popup_pid" ]; then
        wait "$popup_pid" >/dev/null 2>&1
    fi
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

await_size() {
    attempt=0
    while [ "$attempt" -lt 300 ]; do
        if [ "$(cat "$size_file" 2>/dev/null || true)" = "$1" ]; then
            break
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    printf '%s' "$(cat "$size_file" 2>/dev/null || true)"
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session"

env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    TERM=xterm-256color \
    python3 "$HOME/pty-drive.py" "$steps" 60 24 \
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
    echo "display-popup-resize-lifecycle-$side: attach-client"
    exit 0
fi

main_client display-popup -c "$client" -w 40 -h 8 -x 10 -y 20 \
    "sh '$script' popup-loop '$size_file'" >"$work/popup.out" 2>&1 &
popup_pid=$!

check_equal opened '6 38' "$(await_size '6 38')"
drive "size 30 24"
check_equal squeezed '6 28' "$(await_size '6 28')"
drive "size 60 24"
check_equal restored '6 38' "$(await_size '6 38')"

if [ "$check_count" -ne 3 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g DISPLAY_POPUP_RESIZE_LIFECYCLE clean:3
else
    sed "s/^/display-popup-resize-lifecycle-$side: /" "$work/failures"
fi
