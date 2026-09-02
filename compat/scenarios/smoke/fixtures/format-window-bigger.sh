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

session=winbigger
work="$HOME/format-window-bigger-work-$side"
rm -rf "$work"
mkdir -p "$work/one"
: >"$work/failures"
failed=0
check_count=0
client_pid=""

record_failure() {
    failed=1
    echo "$1" >>"$work/failures"
}

check_equal() {
    check_count=$((check_count + 1))
    if [ "$2" != "$3" ]; then
        record_failure "$1 want=[$2] got=[$3]"
    fi
}

cleanup() {
    cleanup_status=$?
    trap - EXIT
    set +e
    if [ -n "$client_pid" ]; then
        kill "$client_pid" >/dev/null 2>&1
        wait "$client_pid" >/dev/null 2>&1
    fi
    main_client kill-session -t "=$session" >/dev/null 2>&1
    exit "$cleanup_status"
}
trap cleanup EXIT

# window_width is not comparable: the zz raw client reserves a sidebar and two
# rows, so its window is smaller than the pin's for the same pty. The boolean
# and the two offsets are derived from the client viewport and the pane cursor,
# so they answer the same on both.
probe='big=[#{window_bigger}] ox=[#{window_offset_x}] oy=[#{window_offset_y}]'

expand() {
    main_client display-message -p -t "$pane" "$probe"
}

attach() {
    env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
        TERM=xterm-256color \
        python3 "$HOME/pty-drive.py" "$work/one" 80 24 \
        "$binary" $prefix_args attach-session -t "=$session" \
        >"$work/attach.out" 2>&1 &
    echo $!
}

await_clients() {
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        count="$(main_client list-clients -t "=$session" -F x 2>/dev/null | grep -c x || true)"
        if [ "$count" = "$1" ]; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "await-clients-$1"
    return 1
}

await_window_width() {
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        width="$(main_client display-message -p -t "$pane" '#{window_width}' 2>/dev/null || true)"
        if [ "$width" = "$1" ]; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "await-window-width-$1"
    return 1
}

# Park the cursor at an exact cell by writing to the pane's own tty, leaving a
# marker there so the poll can tell the sequence was consumed before the offsets
# are read. tty_window_offset caches its answer until the client redraws, so the
# marker is the signal that the redraw has something to do.
park_cursor() {
    row="$1"
    column="$2"
    marker="$3"
    printf '\033[H\033[2J\033[%s;%sH%s\033[%s;%sH' \
        "$row" "$column" "$marker" "$row" "$column" >"$pane_tty"
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if main_client capture-pane -p -t "$pane" 2>/dev/null | grep -q "$marker"; then
            sleep 0.4
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "park-cursor-$row-$column"
    return 1
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session" -x 80 -y 24
pane="$(main_client list-panes -t "=$session" -F '#{pane_id}' | sed -n '1p')"

# format_cb_window_bigger and both offset callbacks return NULL without a
# client in the format tree, whatever the window measures.
check_equal no-client 'big=[] ox=[] oy=[]' "$(expand)"

client_pid="$(attach)"
await_clients 1 || { echo "format-window-bigger-$side: attach"; exit 0; }

# A window that fits the client viewport is not bigger, with the status line
# taking a row and with it off.
check_equal fits 'big=[0] ox=[] oy=[]' "$(expand)"
main_client set -g status off
sleep 0.4
check_equal fits-without-status 'big=[0] ox=[] oy=[]' "$(expand)"
main_client set -g status on
sleep 0.4

# resize-window past the viewport flips the boolean, and the offsets appear.
main_client resize-window -t "=$session:" -x 200 -y 60
await_window_width 200 || { echo "format-window-bigger-$side: resize"; exit 0; }
pane_tty="$(main_client display-message -p -t "$pane" '#{pane_tty}')"

# tty_window_offset1's three x branches and three y branches, driven by the
# cursor of the pane the client is showing. sx is 80 and sy is 23.
park_cursor 31 151 Q || { echo "format-window-bigger-$side: park-31"; exit 0; }
check_equal cursor-past-right 'big=[1] ox=[120] oy=[8]' "$(expand)"
park_cursor 3 61 R || { echo "format-window-bigger-$side: park-3"; exit 0; }
check_equal cursor-inside 'big=[1] ox=[0] oy=[0]' "$(expand)"
park_cursor 26 101 S || { echo "format-window-bigger-$side: park-26"; exit 0; }
check_equal cursor-centred 'big=[1] ox=[60] oy=[3]' "$(expand)"

# cmd_list_windows_exec builds its rows with a null client, so a row answers
# null for all three even while the client is attached and the window is bigger.
check_equal list-windows-row 'big=[] ox=[] oy=[]' \
    "$(main_client list-windows -t "=$session" -F "$probe")"
check_equal list-panes-row 'big=[] ox=[] oy=[]' \
    "$(main_client list-panes -t "=$session" -F "$probe")"

kill "$client_pid" >/dev/null 2>&1 || true
wait "$client_pid" >/dev/null 2>&1 || true
client_pid=""
await_clients 0 || { echo "format-window-bigger-$side: detach"; exit 0; }
check_equal detached 'big=[] ox=[] oy=[]' "$(expand)"

if [ "$check_count" -ne 9 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g FORMAT_WINDOW_BIGGER clean:9
else
    sed "s/^/format-window-bigger-$side: /" "$work/failures"
fi
