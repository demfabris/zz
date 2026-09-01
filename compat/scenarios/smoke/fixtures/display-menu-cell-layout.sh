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

session=menu-cell-layout
work="$HOME/display-menu-cell-layout-work-$side"
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
    main_client set-environment -gu MENU_CELL_ROW >/dev/null 2>&1
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
    row="$(main_client show-environment -g MENU_CELL_ROW 2>/dev/null || true)"
    printf '%s' "${row#MENU_CELL_ROW=}"
}

probe() {
    label="$1"
    columns="$2"
    name="$3"
    spelling="$4"
    bytes="$5"
    expected="$6"
    main_client set-environment -g MENU_CELL_ROW pending
    rm -f "$work/exit-$label"
    drive "size $columns 24"
    sleep 0.5
    (
        main_client display-menu -c "$client" -T '' \
            "$name" "$spelling" 'set-environment -g MENU_CELL_ROW alpha' \
            beta z 'set-environment -g MENU_CELL_ROW beta'
        echo "$?" >"$work/exit-$label"
    ) &
    menu_pid=$!
    sleep 0.8
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
    echo "display-menu-cell-layout-$side: attach-client"
    exit 0
fi

overlong=""
count=0
while [ "$count" -lt 200 ]; do
    overlong="${overlong}A"
    count=$((count + 1))
done
wide=""
count=0
while [ "$count" -lt 30 ]; do
    wide="${wide}B"
    count=$((count + 1))
done
narrow=""
count=0
while [ "$count" -lt 10 ]; do
    narrow="${narrow}C"
    count=$((count + 1))
done

# A name far past the room a 40-column client has still opens a menu, because
# the row is trimmed to fit instead of shedding the whole descriptor.
probe overlong 40 "$overlong" a 61 alpha
# The bracketed M-Enter outruns a quarter of the room and the name no longer
# fits beside it, so the row draws no annotation and still answers the press.
probe hidden-annotation 40 "$wide" M-Enter 1b0d alpha
# The same key annotates once the whole name fits beside it.
probe shown-annotation 40 "$narrow" M-Enter 1b0d alpha
# The trim follows the room down to a much narrower client.
probe narrow-client 24 "$overlong" a 61 alpha
# A short row is untouched either way.
probe short 40 SHORT a 61 alpha

if [ "$check_count" -ne 10 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g DISPLAY_MENU_CELL_LAYOUT clean:10
else
    sed "s/^/display-menu-cell-layout-$side: /" "$work/failures"
fi
