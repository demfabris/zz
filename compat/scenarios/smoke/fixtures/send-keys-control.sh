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

session=skcontrol
work="$HOME/send-keys-control-$side"
bytes="$work/bytes"
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
    exit "$cleanup_status"
}
trap cleanup EXIT

# cmd_send_keys_inject_string parses -H with strtol base 16 and injects
# KEYC_LITERAL|n, which input_key writes to the pane as one raw byte. A string
# that is empty, not entirely hexadecimal, or outside 0..0xff is dropped
# without an error, so every bad code below is a silent skip at status 0.
send_case() {
    label="$1"
    shift
    set +e
    main_client send-keys "$@" >"$work/$label.out" 2>"$work/$label.err"
    status=$?
    set -e
    check_equal "$label-status" 0 "$status"
    check_equal "$label-stdout" '' "$(cat "$work/$label.out")"
    check_equal "$label-stderr" '' "$(cat "$work/$label.err")"
}

await_bytes() {
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if [ -f "$bytes" ]; then
            got="$(od -An -tx1 <"$bytes" | tr -s ' \n' ' ' | sed 's/^ //; s/ $//')"
            if [ "$got" = "$1" ]; then
                return 0
            fi
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "$2 want=[$1] got=[${got:-}]"
    return 1
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session" -x 80 -y 24 "cat >$bytes"
pane="$(main_client list-panes -t "=$session" -F '#{pane_id}' | head -n 1)"

send_case ascii -t "$pane" -H 41
send_case high -t "$pane" -H 80
send_case top -t "$pane" -H ff
send_case prefixed -t "$pane" -H 0x42

# strtol failures and out-of-range codes are dropped, one argument at a time.
send_case not-hex -t "$pane" -H zz
send_case out-of-range -t "$pane" -H 100
send_case empty -t "$pane" -H ''
send_case mixed -t "$pane" -H 43 zz 44

# -N repeats the whole argument list.
send_case repeated -N 3 -t "$pane" -H 45

# 0x04 is the tty's EOF: the first one hands cat the pending bytes, the second
# ends its input so the redirect is complete.
send_case flush -t "$pane" -H 04
check_count=$((check_count + 1))
if ! await_bytes '41 80 ff 42 43 44 45 45 45' delivered; then
    :
fi
main_client send-keys -t "$pane" -H 04

if [ "$check_count" -ne 31 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g SEND_KEYS_CONTROL clean:31
else
    sed "s/^/send-keys-control-$side: /" "$work/failures"
fi
