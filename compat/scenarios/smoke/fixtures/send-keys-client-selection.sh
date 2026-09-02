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

session=skselect
work="$HOME/send-keys-client-selection-$side"
steps="$work/steps"
rm -rf "$work"
mkdir -p "$steps"
: >"$work/failures"
failed=0
check_count=0
step=0
writer_pid=""
reader_pid=""

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
    step=$((step + 1))
    echo quit >"$steps/step-$step" 2>/dev/null
    for key in x y z; do
        main_client unbind-key -n "$key" >/dev/null 2>&1
    done
    main_client kill-session -t "=$session" >/dev/null 2>&1
    for pid in $writer_pid $reader_pid; do
        kill "$pid" >/dev/null 2>&1
        wait "$pid" >/dev/null 2>&1
    done
    exit "$cleanup_status"
}
trap cleanup EXIT

attach_client() {
    label="$1"
    shift
    env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
        TERM=xterm-256color \
        python3 "$HOME/send-keys-attach.py" record "$work/$label.raw" 80 24 \
        "$binary" $prefix_args "$@" >"$work/$label.out" 2>&1 &
}

attach_driven_client() {
    label="$1"
    shift
    env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
        TERM=xterm-256color \
        python3 "$HOME/pty-drive.py" "$steps" 80 24 \
        "$binary" $prefix_args "$@" >"$work/$label.out" 2>&1 &
}

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

# Every case reports the exit status, stderr, and whether the keys landed. The
# pin's contract is cmd-send-keys.c CMD_CLIENT_CFLAG|CMD_CLIENT_CANFAIL: -c
# resolves through the target-client selector, a miss leaves tc NULL and is
# quiet, and the read-only guard tests the selected client, not the invoker.
send_case() {
    label="$1"
    shift
    set +e
    main_client send-keys "$@" >"$work/$label.out" 2>"$work/$label.err"
    status=$?
    set -e
    printf '%s' "$status" >"$work/$label.status"
}

expect_case() {
    label="$1"
    want_status="$2"
    want_stderr="$3"
    check_equal "$label-status" "$want_status" "$(cat "$work/$label.status")"
    check_equal "$label-stdout" '' "$(cat "$work/$label.out")"
    check_equal "$label-stderr" "$want_stderr" "$(cat "$work/$label.err")"
}

await_pane() {
    attempt=0
    while [ "$attempt" -lt 200 ]; do
        got="$(main_client capture-pane -p -t "$pane" | tr -d ' \n')"
        if [ "$got" = "$1" ]; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "$2 want=[$1] got=[$got]"
    return 1
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session" -x 80 -y 24 cat
pane="$(main_client list-panes -t "=$session" -F '#{pane_id}' | head -n 1)"

attach_client writer attach-session -t "=$session"
writer_pid=$!
await_clients 1 || { echo "send-keys-client-selection-$side: writer-attach"; exit 0; }
attach_driven_client reader attach-session -r -t "=$session"
reader_pid=$!
await_clients 2 || { echo "send-keys-client-selection-$side: reader-attach"; exit 0; }

writer="$(main_client list-clients -t "=$session" \
    -F '#{?client_readonly,,#{client_name}}' | grep . | head -n 1)"
reader="$(main_client list-clients -t "=$session" \
    -F '#{?client_readonly,#{client_name},}' | grep . | head -n 1)"
if [ -z "$writer" ] || [ -z "$reader" ]; then
    echo "send-keys-client-selection-$side: client-names writer=[$writer] reader=[$reader]"
    exit 0
fi

# A resolved read-write client delivers.
send_case writer -c "$writer" -t "$pane" -l A
expect_case writer 0 ''

# The guard follows the selected client: the invoker here is an unattached
# command client, so only the reader's own read-only flag can refuse this.
send_case reader -c "$reader" -t "$pane" -l B
expect_case reader 1 'client is read-only'

# CMD_CLIENT_CANFAIL: an unresolvable client is quiet and the keys still land.
send_case miss -c /dev/pts/nonexistent-client -t "$pane" -l C
expect_case miss 0 ''

# The empty target matches nothing either, and is equally quiet.
send_case empty -c '' -t "$pane" -l D
expect_case empty 0 ''

# cmd_find_client also matches a tty with the /dev/ prefix stripped, so this
# still selects the read-only client.
send_case short -c "${reader#/dev/}" -t "$pane" -l E
expect_case short 1 'client is read-only'

await_pane ACD delivered || true

# The guard is `tc != NULL && CLIENT_READONLY && !args_has(args, 'X')`, so -X
# skips it entirely and falls through to the mode command.
main_client copy-mode -t "$pane"
send_case readonly-x -c "$reader" -t "$pane" -X cursor-up
expect_case readonly-x 0 ''

# Without -X the same selected client is refused again, in a mode or not.
send_case readonly-again -c "$reader" -t "$pane" -l F
expect_case readonly-again 1 'client is read-only'

main_client send-keys -t "$pane" -X cancel

# The bound-key path tests the same client. key_bindings_dispatch admits a
# send-keys binding on a read-only client because cmd_send_keys_entry carries
# CMD_READONLY, and cmd_send_keys_exec then tests the -c client, so the
# reader's own bindings deliver through a read-write or missing target client
# and only the binding with no -c is refused.
main_client bind-key -n x send-keys -t "$pane" -l U
main_client bind-key -n z send-keys -c "$writer" -t "$pane" -l K
main_client bind-key -n y send-keys -c /dev/pts/nonexistent-client -t "$pane" -l N
drive 'keys 78'
drive 'keys 7a'
drive 'keys 79'
await_pane ACDKN bound-delivered || true
check_equal bound-pane ACDKN "$(main_client capture-pane -p -t "$pane" | tr -d ' \n')"

if [ "$check_count" -ne 22 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g SEND_KEYS_CLIENT_SELECTION clean:22
else
    sed "s/^/send-keys-client-selection-$side: /" "$work/failures"
fi
