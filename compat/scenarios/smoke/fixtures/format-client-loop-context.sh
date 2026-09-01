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

session=clientctx
work="$HOME/format-client-loop-context-work-$side"
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

position='#{L:<#{loop_index}:#{loop_last_flag}>}'

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session"

# No rows, no positions.
check_equal empty '' "$(expand "$position")"

first_pid="$(attach one)"
await_clients 1 || { echo "format-client-loop-context-$side: attach-one"; exit 0; }
check_equal one '<0:1>' "$(expand "$position")"

second_pid="$(attach two)"
await_clients 2 || { echo "format-client-loop-context-$side: attach-two"; exit 0; }

# Two rows count from zero and only the second one is flagged, whichever order
# the modifier walks.
for form in 'L' 'Li' 'Ln' 'Lr' 'Lnr' 'Lt' 'Ltr' 'Lz'; do
    check_equal "order-$form" '<0:0><1:1>' \
        "$(expand "#{$form:<#{loop_index}:#{loop_last_flag}>}")"
done

# Pairing the index with the client name shows the forward and reverse orders
# hand index 0 to different clients.
ascending="$(expand '#{L:<#{loop_index}#{client_name}>}')"
descending="$(expand '#{Lr:<#{loop_index}#{client_name}>}')"
check_equal reverse-repositions skip "$(if [ "$ascending" = "$descending" ]; then echo same; else echo skip; fi)"

# A nested loop owns the position only for the length of its own rows.
check_equal nested-client '[0(00)(11)0][1(00)(11)1]' \
    "$(expand '#{L:[#{loop_index}#{L:(#{loop_index}#{loop_last_flag})}#{loop_index}]}')"
check_equal nested-window '[0<01>][1<01>]' \
    "$(expand '#{L:[#{loop_index}#{W:<#{loop_index}#{loop_last_flag}>}]}')"
check_equal nested-outside '[0<0><1>]' \
    "$(expand '#{W:[#{loop_index}#{L:<#{loop_index}>}]}')"

# Neither name answers anywhere else.
check_equal no-leak '[|]' "$(expand '[#{loop_index}|#{loop_last_flag}]')"
check_equal no-leak-after 'xx[]' "$(expand '#{L:x}[#{loop_index}#{loop_last_flag}]')"
check_equal conditional 'unsetunsetset' \
    "$(expand '#{?loop_last_flag,set,unset}#{L:#{?loop_last_flag,set,unset}}')"

# Detaching renumbers what is left.
kill "$second_pid" >/dev/null 2>&1 || true
wait "$second_pid" >/dev/null 2>&1 || true
second_pid=""
await_clients 1 || { echo "format-client-loop-context-$side: detach"; exit 0; }
check_equal renumbered '<0:1>' "$(expand "$position")"

if [ "$check_count" -ne 18 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g FORMAT_CLIENT_LOOP_CONTEXT clean:18
else
    sed "s/^/format-client-loop-context-$side: /" "$work/failures"
fi
