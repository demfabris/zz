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

session=clientloop
work="$HOME/format-modifier-client-loop-work-$side"
rm -rf "$work"
mkdir -p "$work/one" "$work/two"
: >"$work/failures"
failed=0
check_count=0
first_pid=""
second_pid=""

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
    for pid in $first_pid $second_pid; do
        kill "$pid" >/dev/null 2>&1
        wait "$pid" >/dev/null 2>&1
    done
    main_client set-environment -gu CLIENT_LOOP_ROW >/dev/null 2>&1
    exit "$cleanup_status"
}
trap cleanup EXIT

expand() {
    main_client display-message -p -t "$session" "$1"
}

attach() {
    env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
        TERM=xterm-256color \
        python3 "$HOME/pty-drive.py" "$work/$1" 80 24 \
        "$binary" $prefix_args attach-session -t "=$session" \
        >"$work/attach-$1.out" 2>&1 &
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

names_sorted() {
    main_client list-clients -t "=$session" -F '#{client_name}' | sort
}

wrapped() {
    sed 's/.*/<&>/' | tr -d '\n'
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session"

# A server nobody has attached to has no rows at all.
check_equal empty-rows '[]' "$(expand '[#{L:<#{client_name}>}]')"
check_equal empty-count 0 "$(expand '#{n:#{L:x}}')"
expand '#{L:}' >"$work/empty-body"
check_equal empty-body-byte 0a "$(od -An -tx1 -v "$work/empty-body" | tr -d ' \n')"

first_pid="$(attach one)"
await_clients 1 || { echo "format-modifier-client-loop-$side: attach-one"; exit 0; }
check_equal one-count 1 "$(expand '#{n:#{L:x}}')"

second_pid="$(attach two)"
await_clients 2 || { echo "format-modifier-client-loop-$side: attach-two"; exit 0; }

ascending="$(names_sorted | wrapped)"
descending="$(names_sorted | sed '1!G;h;$!d' | wrapped)"
window="$(expand '#{window_index}')"

# Default, i, and n all reach the pin's name comparison; r negates the whole
# comparison, tie-break included.
check_equal order-default "$ascending" "$(expand '#{L:<#{client_name}>}')"
check_equal order-index "$ascending" "$(expand '#{Li:<#{client_name}>}')"
check_equal order-name "$ascending" "$(expand '#{Ln:<#{client_name}>}')"
check_equal order-reversed "$descending" "$(expand '#{Lr:<#{client_name}>}')"
check_equal order-index-reversed "$descending" "$(expand '#{Lir:<#{client_name}>}')"
check_equal order-name-reversed "$descending" "$(expand '#{Lnr:<#{client_name}>}')"

# An order letter the pin does not know falls back to the default order.
check_equal order-unknown "$ascending" "$(expand '#{Lz:<#{client_name}>}')"
check_equal order-unknown-reversed "$descending" "$(expand '#{Lzr:<#{client_name}>}')"

# The row replaces the client and keeps the outer session, window, and pane.
check_equal row-context \
    "<$session:$window:0><$session:$window:0>" \
    "$(expand '#{L:<#{session_name}:#{window_index}:#{pane_index}>}')"
check_equal row-tty "$ascending" "$(expand '#{L:<#{client_tty}>}')"

# Loops nest in both directions and the inner one keeps the outer row's client.
check_equal nested-window "[$window][$window]" "$(expand '#{L:[#{window_index}]}')"
check_equal nested-window-client "$ascending" "$(expand '#{L:#{W:<#{client_name}>}}')"
check_equal nested-client '[xx][xx]' "$(expand '#{L:[#{L:x}]}')"
check_equal nested-count 2 "$(expand '#{n:#{L:x}}')"

# Detaching takes the row away; the survivor keeps its own.
kill "$second_pid" >/dev/null 2>&1 || true
wait "$second_pid" >/dev/null 2>&1 || true
second_pid=""
await_clients 1 || { echo "format-modifier-client-loop-$side: detach-two"; exit 0; }
check_equal detached-count 1 "$(expand '#{n:#{L:x}}')"
check_equal detached-row "$(names_sorted | wrapped)" "$(expand '#{L:<#{client_name}>}')"

kill "$first_pid" >/dev/null 2>&1 || true
wait "$first_pid" >/dev/null 2>&1 || true
first_pid=""
await_clients 0 || { echo "format-modifier-client-loop-$side: detach-one"; exit 0; }
check_equal detached-all '[]' "$(expand '[#{L:<#{client_name}>}]')"

if [ "$check_count" -ne 21 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g FORMAT_MODIFIER_CLIENT_LOOP clean:21
else
    sed "s/^/format-modifier-client-loop-$side: /" "$work/failures"
fi
