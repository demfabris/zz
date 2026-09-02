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

session=roefmt
work="$HOME/remain-on-exit-format-$side"
rm -rf "$work"
mkdir -p "$work"
: >"$work/failures"
failed=0
check_count=0

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
    main_client set-option -gu remain-on-exit >/dev/null 2>&1
    main_client set-option -gu remain-on-exit-format >/dev/null 2>&1
    exit "$cleanup_status"
}
trap cleanup EXIT

await_dead() {
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if [ "$(main_client display-message -p -t "=$session:$1" '#{pane_dead}' 2>/dev/null)" = 1 ]; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "await-dead-$1"
    return 1
}

await_gone() {
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if ! main_client list-windows -t "=$session" -F '#{window_name}' 2>/dev/null | grep -q "^$1\$"; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "await-gone-$1"
    return 1
}

# The visible rows are the observable: a detached pane's terminal has a
# different row and column count on the two engines, so nothing here counts
# rows or leans on the pin's clip at the pane width.
screen() {
    main_client capture-pane -p -t "=$session:$1" | tr -d ' \n'
}

# The pin draws the notice inside server_destroy_pane, so `#{pane_dead}` and
# the drawn row land together; zz marks the pane dead first and hands the
# expanded text to the pane's worker straight after, so the row settles a beat
# later. Both engines end on the same screen.
check_screen() {
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if [ "$(screen "$3")" = "$2" ]; then
            break
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    check_equal "$1" "$2" "$(screen "$3")"
}

main_client new-session -d -s "$session" -n keep -x 40 -y 6
main_client set-option -g remain-on-exit on
main_client set-option -g remain-on-exit-format 'DEADFMT[#{pane_dead_status}][#{pane_dead_signal}]'

# server_destroy_pane expands the format against the pane it has just marked
# dead, so the status and signal names are the ones the child left behind.
main_client new-window -t "=$session" -n wstatus 'sh -c "exit 7"'
await_dead wstatus || { echo "remain-on-exit-format-$side: wstatus"; exit 0; }
check_screen status-notice-is-drawn 'DEADFMT[7][]' wstatus
check_equal status-pane-is-dead 1 "$(main_client display-message -p -t "=$session:wstatus" '#{pane_dead}')"

main_client new-window -t "=$session" -n wsignal 'sh -c "kill -TERM $$"'
await_dead wsignal || { echo "remain-on-exit-format-$side: wsignal"; exit 0; }
check_screen signal-notice-is-drawn 'DEADFMT[][term]' wsignal

main_client new-window -t "=$session" -n wzero 'sh -c "exit 0"'
await_dead wzero || { echo "remain-on-exit-format-$side: wzero"; exit 0; }
check_screen zero-status-notice-is-drawn 'DEADFMT[0][]' wzero

# `if (*s != '\0')` guards the whole draw, so an empty template leaves the
# pane's own last output exactly where it was.
main_client set-option -g remain-on-exit-format ''
main_client new-window -t "=$session" -n wempty 'sh -c "printf MARKER; exit 4"'
await_dead wempty || { echo "remain-on-exit-format-$side: wempty"; exit 0; }
check_screen empty-template-draws-nothing MARKER wempty

# remain-on-exit failed retains only a child that failed, and the retained one
# still gets the notice.
main_client set-option -g remain-on-exit-format 'DEADFMT[#{pane_dead_status}][#{pane_dead_signal}]'
main_client set-option -g remain-on-exit failed
main_client new-window -t "=$session" -n wfailed 'sh -c "exit 5"'
await_dead wfailed || { echo "remain-on-exit-format-$side: wfailed"; exit 0; }
check_screen failed-retains-a-failure 'DEADFMT[5][]' wfailed
main_client new-window -t "=$session" -n wok 'sh -c "exit 0"'
await_gone wok || { echo "remain-on-exit-format-$side: wok"; exit 0; }
check_equal failed-closes-a-clean-exit 0 \
    "$(main_client list-windows -t "=$session" -F '#{window_name}' | grep -c '^wok$' || true)"

main_client set-option -g remain-on-exit off
main_client new-window -t "=$session" -n woff 'sh -c "exit 6"'
await_gone woff || { echo "remain-on-exit-format-$side: woff"; exit 0; }
check_equal off-closes-the-pane 0 \
    "$(main_client list-windows -t "=$session" -F '#{window_name}' | grep -c '^woff$' || true)"

if [ "$check_count" -ne 8 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g REMAIN_ON_EXIT_FORMAT "clean:$check_count"
else
    sed "s/^/remain-on-exit-format-$side: /" "$work/failures"
fi
