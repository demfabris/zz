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

session=clipwrite
work="$HOME/buffer-clipboard-write-$side"
recording="$work/attach.raw"
rm -rf "$work"
mkdir -p "$work"
: >"$work/failures"
: >"$recording"
failed=0
check_count=0
attach_pid=""
control_pid=""
expected=""

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
    for pid in $attach_pid $control_pid; do
        kill "$pid" >/dev/null 2>&1
        wait "$pid" >/dev/null 2>&1
    done
    exit "$cleanup_status"
}
trap cleanup EXIT

scan() {
    python3 "$HOME/buffer-clipboard-write-pty.py" scan "$recording"
}

await_payloads() {
    attempt=0
    while [ "$attempt" -lt 200 ]; do
        got="$(scan)"
        if [ "$got" = "$1" ]; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "$2 want=[$1] got=[$got]"
    return 1
}

expect() {
    if [ -z "$expected" ]; then
        expected="$1"
    else
        expected="$expected
$1"
    fi
    check_count=$((check_count + 1))
    await_payloads "$expected" "$2" || true
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

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session"
main_client set-option -as terminal-features ",xterm-256color:clipboard" >/dev/null 2>&1 || true
main_client set-option -g set-clipboard off

env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    TERM=xterm-256color \
    python3 "$HOME/buffer-clipboard-write-pty.py" record "$recording" 80 24 \
    "$binary" $prefix_args attach-session -t "=$session" \
    >"$work/attach.out" 2>&1 &
attach_pid=$!
await_clients 1 || { echo "buffer-clipboard-write-$side: attach"; exit 0; }
viewer="$(main_client list-clients -t "=$session" -F '#{client_name}' | head -n 1)"

# The pin writes the selection from cmd-set-buffer.c straight to the target
# client's tty, so set-clipboard off does not suppress it.
main_client set-buffer -w -b one -t "$viewer" alpha-one
expect alpha-one set-buffer-w

# Without -w the buffer is stored and nothing reaches the client. The next
# write proves the silence rather than a sleep.
main_client set-buffer -b two -t "$viewer" plain-two

printf 'bravo-three' >"$work/bravo"
main_client load-buffer -w -b three -t "$viewer" "$work/bravo"
expect bravo-three load-buffer-w

# CMD_CLIENT_CANFAIL: an unresolvable -t is silent and still stores the buffer.
set +e
main_client set-buffer -w -b four -t no-such-client charlie-four \
    >"$work/canfail.out" 2>"$work/canfail.err"
canfail_status=$?
set -e
check_equal canfail-status 0 "$canfail_status"
check_equal canfail-stdout '' "$(cat "$work/canfail.out")"
check_equal canfail-stderr '' "$(cat "$work/canfail.err")"

# No -t at all resolves the current client.
main_client set-buffer -w -b five echo-five
expect echo-five set-buffer-w-current

# set-clipboard on changes nothing for the buffer write.
main_client set-option -g set-clipboard on
main_client set-buffer -w -b six -t "$viewer" foxtrot-six
expect foxtrot-six set-buffer-w-clipboard-on

# A control client has no started tty, so it is the clipboard-disabled target:
# the command still succeeds and stores the buffer, and nothing is written.
: >"$work/control.raw"
env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    TERM=xterm-256color \
    python3 "$HOME/buffer-clipboard-write-pty.py" record "$work/control.raw" 80 24 \
    "$binary" $prefix_args -C attach-session -t "=$session" \
    >"$work/control.out" 2>&1 &
control_pid=$!
await_clients 2 || { echo "buffer-clipboard-write-$side: control-attach"; exit 0; }
control="$(main_client list-clients -t "=$session" \
    -F '#{?client_control_mode,#{client_name},}' | grep . | head -n 1)"
if [ -z "$control" ]; then
    record_failure "control-client-name"
else
    main_client set-buffer -w -b seven -t "$control" golf-seven
fi
main_client set-buffer -w -b eight -t "$viewer" hotel-eight
expect hotel-eight set-buffer-w-after-control
check_equal control-recording '' \
    "$(python3 "$HOME/buffer-clipboard-write-pty.py" scan "$work/control.raw")"

check_equal buffers \
    'eight=hotel-eight
five=echo-five
four=charlie-four
one=alpha-one
seven=golf-seven
six=foxtrot-six
three=bravo-three
two=plain-two' \
    "$(main_client list-buffers -F '#{buffer_name}=#{buffer_sample}' | sort)"

if [ "$check_count" -ne 10 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g BUFFER_CLIPBOARD_WRITE clean:10
else
    sed "s/^/buffer-clipboard-write-$side: /" "$work/failures"
fi
