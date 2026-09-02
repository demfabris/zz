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

session=skinject
work="$HOME/send-keys-inject-$side"
rm -rf "$work"
mkdir -p "$work"
: >"$work/failures"
: >"$work/log"
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

await_pane() {
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if main_client capture-pane -p -t "$1" 2>/dev/null | tr -d '\n' | grep -q "$2"; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "await-pane-$2"
    return 1
}

# Every assertion below reads a file a bound `run-shell` appends to, so the
# wait is for a line rather than for a fixed delay.
await_log() {
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if [ "$(wc -l <"$work/log" | tr -d ' ')" -ge "$1" ]; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "await-log-$1"
    return 1
}

log_body() {
    tr '\n' ' ' <"$work/log" | sed 's/ $//'
}

reset_log() {
    : >"$work/log"
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session" -x 80 -y 24 cat
pane="$(main_client list-panes -t "=$session" -F '#{pane_id}' | head -n 1)"
main_client new-window -d -n other
other="$(main_client list-panes -t "=$session:other" -F '#{pane_id}' | head -n 1)"

main_client bind-key -n Z run-shell "echo root-Z >> $work/log"
main_client bind-key -T probe Z run-shell "echo probe-Z >> $work/log"

for name in first second; do
    env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
        TERM=xterm-256color \
        python3 "$HOME/send-keys-attach.py" record "$work/$name.raw" 80 24 \
        "$binary" $prefix_args attach-session -t "=$session" \
        >"$work/$name.out" 2>&1 &
    eval "${name}_pid=\$!"
    await_clients "$([ "$name" = first ] && echo 1 || echo 2)" || {
        echo "send-keys-inject-$side: attach"
        exit 0
    }
done
alpha="$(main_client list-clients -t "=$session" -F '#{client_name}' | head -n 1)"
beta="$(main_client list-clients -t "=$session" -F '#{client_name}' | sed -n 2p)"
if [ -z "$alpha" ] || [ -z "$beta" ] || [ "$alpha" = "$beta" ]; then
    echo "send-keys-inject-$side: clients"
    exit 0
fi

main_client bind-key -T stage F5 \
    "switch-client -c $alpha -T replay ; send-keys -c $alpha -K"
main_client bind-key -T replay F5 run-shell "echo replay-F5 >> $work/log"
main_client bind-key -T count F6 \
    "run-shell 'echo saw-F6 >> $work/log' ; switch-client -c $alpha -T counted ; send-keys -c $alpha -N 2 -K"
main_client bind-key -T counted F6 run-shell "echo counted-F6 >> $work/log"

# A plain `send-keys` writes the key's bytes to the target pane and never
# consults a key table, so the root binding on Z stays quiet.
main_client send-keys -t "$pane" Z
await_pane "$pane" Z || true
check_equal plain-send-keys-writes-the-pane Z "$(main_client capture-pane -p -t "$pane" | tr -d '\n ')"
check_equal plain-send-keys-runs-no-binding '' "$(log_body)"

# `cmd_send_keys_inject_key` hands the key to `server_client_handle_key` on the
# target client instead, so the same key now fires the client's root binding
# and writes nothing to the pane.
main_client send-keys -c "$alpha" -K Z
await_log 1 || true
check_equal inject-fires-the-root-binding root-Z "$(log_body)"
check_equal inject-leaves-the-pane-alone Z "$(main_client capture-pane -p -t "$pane" | tr -d '\n ')"

# The `-N` repeat wraps the injection loop, so the binding runs once per count.
reset_log
main_client send-keys -c "$alpha" -N 3 -K Z
await_log 3 || true
check_equal inject-repeats-with-a-count 'root-Z root-Z root-Z' "$(log_body)"

# `cmd_send_keys_inject_string` resolves `-H` to `KEYC_LITERAL|n` before the
# injection, and the table lookup masks the literal flag off, so hex 5a is the
# same Z the name form injects.
reset_log
main_client send-keys -c "$alpha" -K -H 5a
await_log 1 || true
check_equal inject-hex-fires-the-binding root-Z "$(log_body)"

# An unbound key falls through the client's key table to the pane that client
# is looking at, and `-l` spends one key per character.
reset_log
main_client send-keys -c "$alpha" -K -l V
await_pane "$pane" ZV || true
check_equal unbound-key-reaches-the-client-pane ZV "$(main_client capture-pane -p -t "$pane" | tr -d '\n ')"

# `cmd_send_keys_inject_key` never reads the target pane under `-K`, so a `-t`
# naming a pane in another window changes nothing about where the key lands.
main_client send-keys -c "$alpha" -t "$other" -K -l W
await_pane "$pane" ZVW || true
check_equal target-pane-is-ignored ZVW "$(main_client capture-pane -p -t "$pane" | tr -d '\n ')"
check_equal other-pane-never-sees-the-key 0 \
    "$(main_client capture-pane -p -t "$other" | grep -c W || true)"
check_equal unbound-key-runs-no-binding '' "$(log_body)"

# `cmd_send_keys_inject_key` returns the queue item untouched when the target
# client is NULL, so an unresolvable `-c` injects nothing and still exits 0.
if main_client send-keys -c /dev/zz-no-such-client -K Z; then
    missing_status=0
else
    missing_status=$?
fi
check_equal no-target-client-exits-zero 0 "$missing_status"
main_client send-keys -c "$alpha" -K Z
await_log 1 || true
check_equal no-target-client-is-a-no-op root-Z "$(log_body)"

# With no positional key `send-keys -K` injects `event->key`, the key of the
# queue item that ran it, so a binding can replay the key that raised it into
# whichever table the same command list just selected.
main_client switch-client -c "$alpha" -T stage
reset_log
main_client send-keys -c "$alpha" -K F5
await_log 1 || true
check_equal no-key-replays-the-invoking-key replay-F5 "$(log_body)"

# `cmd_send_keys_exec` returns before the injection loop whenever `-N` is given
# with no positional key, so a count on the no-key form arms the pane's mode
# count and injects nothing at all.
main_client switch-client -c "$alpha" -T count
reset_log
main_client send-keys -c "$alpha" -K F6
await_log 1 || true
main_client send-keys -c "$beta" -K Z
await_log 2 || true
check_equal count-without-a-key-injects-nothing 'saw-F6 root-Z' "$(log_body)"

# `server_client_handle_key` reads the selected client's own key table, so the
# same key resolves differently per client.
main_client switch-client -c "$alpha" -T probe
reset_log
main_client send-keys -c "$alpha" -K Z
await_log 1 || true
check_equal selected-client-uses-its-own-table probe-Z "$(log_body)"
reset_log
main_client send-keys -c "$beta" -K Z
await_log 1 || true
check_equal other-client-keeps-the-root-table root-Z "$(log_body)"

if [ "$check_count" -ne 16 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g SEND_KEYS_INJECT clean:16
else
    sed "s/^/send-keys-inject-$side: /" "$work/failures"
fi
