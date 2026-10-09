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

recorder="$ZZ_SMOKE_TMUX_BIN"
label="zzzorder-$side-$$"
record() {
    "$recorder" -L "$label" -f /dev/null "$@"
}

session=select-layout-z-order
work="$HOME/select-layout-z-order-work-$side"
snaps="$work/snaps"
rm -rf "$work"
mkdir -p "$snaps"
: >"$work/failures"
failed=0
check_count=0
recorder_started=0

record_failure() {
    failed=1
    echo "$1" >>"$work/failures"
}

check_equal() {
    check_count=$((check_count + 1))
    if [ "$2" != "$3" ]; then
        record_failure "$1 want=$2 got=$3"
    fi
}

cleanup() {
    cleanup_status=$?
    trap - EXIT
    set +e
    if [ "$recorder_started" -eq 1 ]; then
        record kill-server >/dev/null 2>&1
    fi
    main_client kill-session -t "=$session" >/dev/null 2>&1
    exit "$cleanup_status"
}
trap cleanup EXIT

snap() {
    sleep 1.0
    record capture-pane -p -t recorder >"$snaps/$1" 2>/dev/null || : >"$snaps/$1"
}

divider_drawn() {
    if grep -qE '^0+1+3+$' "$snaps/$1" 2>/dev/null; then
        printf yes
    else
        printf no
    fi
}

same_screen() {
    if cmp -s "$snaps/$1" "$snaps/$2"; then
        printf same
    else
        printf changed
    fi
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session"
main_client set-option -g status off
main_client split-window -v -t "=$session:0.0"
main_client split-window -h -t "=$session:0.1"
main_client split-window -h -b -l 20 -t "=$session:0.0"
main_client set-window-option -t "=$session:0" pane-border-lines number
main_client select-pane -t "=$session:0.3"

record new-session -d -x 100 -y 30 -s recorder \
    "env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
     LC_ALL=en_US.UTF-8 LANG=en_US.UTF-8 \
     $binary $prefix_args attach-session -t =$session"
recorder_started=1
record set-option -g status off
sleep 1.8

snap created
check_equal divider-row-drawn yes "$(divider_drawn created)"

layout="$(main_client display-message -p -t "=$session:0" '#{window_layout}')"
main_client select-layout -t "=$session:0" "$layout"
snap reparsed
check_equal reparse-keeps-the-z-order same "$(same_screen created reparsed)"

if [ "$check_count" -ne 2 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g SELECT_LAYOUT_Z_ORDER clean:2
else
    sed "s/^/select-layout-z-order-$side: /" "$work/failures"
    for snap_file in "$snaps"/*; do
        sed "s/^/select-layout-z-order-$side: $(basename "$snap_file"): /" "$snap_file"
    done
fi
