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

session=cmlinenumbers
work="$HOME/copy-mode-line-numbers-$side"
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

await_value() {
    attempt=0
    while [ "$attempt" -lt 200 ]; do
        [ "$(value "$1")" = "$2" ] && return 0
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
await_clients 1 || { echo "copy-mode-line-numbers-$side: attach"; exit 0; }

printf 'one\ntwo\n' >"$work/lines.txt"
main_client load-buffer -b cmlinenumbers "$work/lines.txt"
main_client paste-buffer -b cmlinenumbers -t "$pane"
await_output 'two' || { echo "copy-mode-line-numbers-$side: output"; exit 0; }

main_client copy-mode -t "$pane"
check_equal option-off-starts-hidden 0 "$(value copy_line_numbers)"
main_client send-keys -t "$pane" -X line-numbers-toggle
check_equal toggle-forces-default 1 "$(value copy_line_numbers)"
main_client send-keys -t "$pane" -X line-numbers-toggle
check_equal toggle-hides-again 0 "$(value copy_line_numbers)"
main_client send-keys -t "$pane" -X line-numbers-on
check_equal on-shows 1 "$(value copy_line_numbers)"
main_client send-keys -t "$pane" -X line-numbers-on
check_equal on-again-stays 1 "$(value copy_line_numbers)"
main_client send-keys -t "$pane" -X line-numbers-off
check_equal off-hides 0 "$(value copy_line_numbers)"
main_client send-keys -t "$pane" -X cancel

main_client set-option -w -t "$pane" copy-mode-line-numbers absolute
main_client copy-mode -t "$pane"
check_equal option-absolute-shows 1 "$(value copy_line_numbers)"
main_client send-keys -t "$pane" -X line-numbers-toggle
check_equal toggle-under-option-hides 0 "$(value copy_line_numbers)"
main_client send-keys -t "$pane" -X line-numbers-toggle
check_equal toggle-under-option-shows 1 "$(value copy_line_numbers)"
main_client set-option -w -t "$pane" copy-mode-line-numbers off
check_equal option-off-live-hides 0 "$(value copy_line_numbers)"
main_client send-keys -t "$pane" -X cancel
main_client set-option -wu -t "$pane" copy-mode-line-numbers

main_client copy-mode -t "$pane"
check_equal refresh-starts-off 0 "$(value refresh_active)"
main_client send-keys -t "$pane" -X refresh-toggle
check_equal refresh-toggle-on 1 "$(value refresh_active)"
main_client send-keys -t "$pane" -X refresh-toggle
check_equal refresh-toggle-off 0 "$(value refresh_active)"
check_equal cursor-on-the-empty-row '' "$(value copy_cursor_line)"
printf 'three\n' >"$(value pane_tty)"
await_output 'three' || { echo "copy-mode-line-numbers-$side: three"; exit 0; }
await_value pane_unseen_changes 1
check_equal output-is-unseen 1 "$(value pane_unseen_changes)"
check_equal frozen-view-keeps-the-old-row '' "$(value copy_cursor_line)"
main_client send-keys -t "$pane" -X refresh-now
await_value copy_cursor_line three
check_equal refresh-now-reclones 'three' "$(value copy_cursor_line)"
check_equal refresh-now-clears-unseen 0 "$(value pane_unseen_changes)"
check_equal refresh-now-leaves-the-timer-off 0 "$(value refresh_active)"
check_equal refresh-now-stays-in-the-mode 1 "$(value pane_in_mode)"

if [ "$check_count" -ne 20 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g COPY_MODE_LINE_NUMBERS clean:20
else
    sed "s/^/copy-mode-line-numbers-$side: /" "$work/failures"
fi
