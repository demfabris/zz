#!/bin/sh
set -eu

export LC_ALL=C

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    side=zz
    binary="$ZZ_SMOKE_ZZ_BIN"
    set -- --socket "$ZZ_SMOKE_ZZ_SOCKET"
    bare_prefix="--bootstrap-launcher-client --socket $ZZ_SMOKE_ZZ_SOCKET"
else
    side=tmux
    binary="$ZZ_SMOKE_TMUX_BIN"
    set -- -L "$ZZ_SMOKE_TMUX_LABEL"
    bare_prefix="-L $ZZ_SMOKE_TMUX_LABEL"
fi
prefix_args="$*"
main_client() {
    # shellcheck disable=SC2086
    "$binary" $prefix_args "$@"
}

session=default-client-command
spawned=default-client-command-spawned
work="$HOME/default-client-command-work-$side"
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
        record_failure "$1"
    fi
}

cleanup() {
    cleanup_status=$?
    trap - EXIT
    set +e
    main_client kill-session -t "=$spawned" >/dev/null 2>&1
    main_client kill-session -t "=$session" >/dev/null 2>&1
    exit "$cleanup_status"
}
trap cleanup EXIT

# A client with an empty command vector reaches the server as a terminal
# client, so the probe hands it a pty instead of a pipe.
bare() {
    # shellcheck disable=SC2086
    env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
        TERM=xterm-256color \
        python3 "$HOME/bare-client.py" "$work/$1" 12 \
        "$binary" $bare_prefix 2>>"$work/probe.err"
}

drawn() {
    tr -d '\r' <"$work/$1" | sed -e 's/\x1b\[[0-9;?]*[ -\/]*[@-~]//g'
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session"
main_client set-option -g status off

# One command: server_client_default_command inserts the option's list after
# the callback, so the bare client runs it and exits with its status.
main_client set-option -s default-client-command 'display-message -p custom'
status="$(bare single)"
check_equal single-command-status exit=0 "$status"
check_equal single-command-output custom "$(drawn single)"

# A multi-command list is one cmd_list, so every member runs in order.
main_client set-option -s default-client-command \
    'display-message -p one ; display-message -p two'
status="$(bare multiple)"
check_equal multiple-command-status exit=0 "$status"
check_equal multiple-command-output 'one
two' "$(drawn multiple)"

# The list runs through the same path an explicit argument takes, so a session
# command in it creates the session and a detached one lets the client exit.
main_client set-option -s default-client-command "new-session -d -s $spawned"
status="$(bare session)"
check_equal session-command-status exit=0 "$status"
if main_client has-session -t "=$spawned" >/dev/null 2>&1; then
    check_equal session-command-created yes yes
else
    check_equal session-command-created yes no
fi
main_client kill-session -t "=$spawned" >/dev/null 2>&1 || true

# OPTIONS_TABLE_COMMAND parses at set time, so an unknown command is refused
# there and the stored list is left alone.
if refusal="$(main_client set-option -s default-client-command 'no-such-command' 2>&1)"; then
    check_equal refused-unknown-command failed succeeded
else
    check_equal refused-unknown-command failed failed
fi
check_equal refusal-message 'unknown command: no-such-command' "$refusal"
check_equal stored-list-survives-the-refusal "new-session -d -s $spawned" \
    "$(main_client show-options -sv default-client-command)"

if [ "$check_count" -ne 9 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g DEFAULT_CLIENT_COMMAND clean:9
else
    sed "s/^/default-client-command-$side: /" "$work/failures"
fi
