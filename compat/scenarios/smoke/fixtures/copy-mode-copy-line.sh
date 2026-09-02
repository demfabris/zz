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

session=cmcopyline
work="$HOME/copy-mode-copy-line-$side"
rm -rf "$work"
mkdir -p "$work"
: >"$work/failures"
failed=0
check_count=0
viewer_pid=""

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
    main_client kill-session -t "=$session" >/dev/null 2>&1
    if [ -n "$viewer_pid" ]; then
        kill "$viewer_pid" >/dev/null 2>&1
        wait "$viewer_pid" >/dev/null 2>&1
    fi
    exit "$cleanup_status"
}
trap cleanup EXIT

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

await_output() {
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if main_client capture-pane -p -t "$pane" 2>/dev/null | grep -q "$1"; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "await-output-$1"
    return 1
}

await_file() {
    attempt=0
    while [ "$attempt" -lt 200 ]; do
        if [ -s "$1" ]; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    return 0
}

value() {
    main_client display-message -p -t "$pane" "#{$1}"
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session" -x 80 -y 24 cat
pane="$(main_client list-panes -t "=$session" -F '#{pane_id}' | head -n 1)"

env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    TERM=xterm-256color \
    python3 "$HOME/send-keys-attach.py" record "$work/viewer.raw" 80 24 \
    "$binary" $prefix_args attach-session -t "=$session" \
    >"$work/viewer.out" 2>&1 &
viewer_pid=$!
await_clients 1 || { echo "copy-mode-copy-line-$side: attach"; exit 0; }

printf 'alpha beta gamma\ndelta epsilon\nzeta\n' >"$work/lines.txt"
main_client load-buffer -b cmcopyline "$work/lines.txt"
main_client paste-buffer -b cmcopyline -t "$pane"
await_output 'zeta' || { echo "copy-mode-copy-line-$side: output"; exit 0; }

main_client copy-mode -t "$pane"
main_client send-keys -t "$pane" -X cursor-up
main_client send-keys -t "$pane" -X cursor-up
check_equal cursor-is-on-the-middle-line 'delta epsilon' "$(value copy_cursor_line)"

# window_copy_do_copy_line copies the whole logical line whatever column the
# cursor is in and needs no selection, then puts cx, cy and oy back and clears
# the selection it made.
# The pin's cursor-up keeps a desired column that zz's does not, so anchor the
# column before stepping into the line.
main_client send-keys -t "$pane" -X start-of-line
main_client send-keys -t "$pane" -X cursor-right
main_client send-keys -t "$pane" -X cursor-right
check_equal cursor-is-still-on-the-middle-line 'delta epsilon' "$(value copy_cursor_line)"
before_x="$(value copy_cursor_x)"
before_y="$(value copy_cursor_y)"
main_client send-keys -t "$pane" -X copy-line
check_equal copy-line-copies-the-line 'delta epsilon' "$(main_client show-buffer)"
check_equal copy-line-restores-the-column "$before_x" "$(value copy_cursor_x)"
check_equal copy-line-restores-the-row "$before_y" "$(value copy_cursor_y)"
check_equal copy-line-clears-the-selection 0 "$(value selection_present)"
check_equal copy-line-stays-in-the-mode 1 "$(value pane_in_mode)"

# `for (; np > 1; np--) window_copy_cursor_down(wme, 0)` before the end of line,
# so the count reaches down past the cursor's own line.
main_client send-keys -t "$pane" -N 2 -X copy-line
check_equal counted-copy-line-copies-two 'delta epsilon
zeta' "$(main_client show-buffer)"

main_client send-keys -t "$pane" -X copy-pipe-line "cat >$work/pipe.txt"
await_file "$work/pipe.txt"
check_equal copy-pipe-line-feeds-the-command 'delta epsilon' "$(cat "$work/pipe.txt" 2>/dev/null)"
check_equal copy-pipe-line-stays-in-the-mode 1 "$(value pane_in_mode)"

# window_copy_command sets wme->prefix back to 1 whether or not a command ran,
# so a count spent on a name the table does not carry is gone.
main_client send-keys -t "$pane" -N 5
main_client send-keys -t "$pane" -X bogus-action
unknown_y="$(value copy_cursor_y)"
main_client send-keys -t "$pane" -X cursor-up
check_equal unknown-action-resets-the-count 1 "$((unknown_y - $(value copy_cursor_y)))"

main_client send-keys -t "$pane" -X copy-pipe-line-and-cancel "cat >$work/pipe-cancel.txt"
await_file "$work/pipe-cancel.txt"
check_equal pipe-line-and-cancel-feeds-the-command 'alpha beta gamma' \
    "$(cat "$work/pipe-cancel.txt" 2>/dev/null)"
attempt=0
while [ "$attempt" -lt 200 ]; do
    [ "$(value pane_in_mode)" = 0 ] && break
    attempt=$((attempt + 1))
    sleep 0.05
done
check_equal pipe-line-and-cancel-leaves-the-mode 0 "$(value pane_in_mode)"

main_client copy-mode -t "$pane"
main_client send-keys -t "$pane" -X cursor-up
main_client send-keys -t "$pane" -X copy-line-and-cancel
attempt=0
while [ "$attempt" -lt 200 ]; do
    [ "$(value pane_in_mode)" = 0 ] && break
    attempt=$((attempt + 1))
    sleep 0.05
done
check_equal copy-line-and-cancel-leaves-the-mode 0 "$(value pane_in_mode)"
check_equal copy-line-and-cancel-copies-the-line 'zeta' "$(main_client show-buffer)"

if [ "$check_count" -ne 15 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g COPY_MODE_COPY_LINE clean:15
else
    sed "s/^/copy-mode-copy-line-$side: /" "$work/failures"
fi
