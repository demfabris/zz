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

session=cmerrors
work="$HOME/copy-mode-command-errors-$side"
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

# cmd_send_keys_exec runs one guard before it reaches the mode command:
# `if (wme == NULL || wme->mode->command == NULL) { cmdq_error("not in a mode") }`
# at status 1. It covers every -X spelling, including a bare -X with no action
# and an -N count paired with -X, and it does not cover a bare -N count.
expect_case() {
    label="$1"
    want_status="$2"
    want_stderr="$3"
    shift 3
    set +e
    main_client send-keys "$@" >"$work/$label.out" 2>"$work/$label.err"
    status=$?
    set -e
    check_equal "$label-status" "$want_status" "$status"
    check_equal "$label-stdout" '' "$(cat "$work/$label.out")"
    check_equal "$label-stderr" "$want_stderr" "$(cat "$work/$label.err")"
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
await_clients 1 || { echo "copy-mode-command-errors-$side: attach"; exit 0; }

expect_case before-action 1 'not in a mode' -t "$pane" -X cursor-up
expect_case before-bare 1 'not in a mode' -t "$pane" -X
expect_case before-counted 1 'not in a mode' -t "$pane" -N 3 -X
expect_case before-unknown 1 'not in a mode' -t "$pane" -X bogus-action
expect_case before-selection 1 'not in a mode' -t "$pane" -X begin-selection

# The bare count has no -X, so it skips the guard even with no mode entry.
expect_case before-count 0 '' -t "$pane" -N 3

main_client copy-mode -t "$pane"

expect_case in-action 0 '' -t "$pane" -X cursor-up
expect_case in-bare 0 '' -t "$pane" -X
expect_case in-counted 0 '' -t "$pane" -N 3 -X
expect_case in-unknown 0 '' -t "$pane" -X bogus-action
expect_case in-selection 0 '' -t "$pane" -X begin-selection

main_client send-keys -t "$pane" -X cancel

expect_case after-cancel 1 'not in a mode' -t "$pane" -X cursor-up

if [ "$check_count" -ne 36 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g COPY_MODE_COMMAND_ERRORS clean:36
else
    sed "s/^/copy-mode-command-errors-$side: /" "$work/failures"
fi
