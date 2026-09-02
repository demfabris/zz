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

session=cmkill
work="$HOME/copy-mode-kill-on-exit-$side"
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

# Pane ids depend on what the harness server already created, so the checks
# compare the session's pane list by index rather than by a fixed id.
panes() {
    main_client list-panes -t "=$session" -F '#{pane_index}' 2>/dev/null | tr '\n' ' '
}

# window_pane_reset_mode kills the pane after the mode is torn down, so the
# kill lands when copy mode ends, not when the command runs.
settle() {
    attempt=0
    while [ "$attempt" -lt 200 ]; do
        if [ "$(panes)" = "$1" ]; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    return 0
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session" -x 80 -y 24 cat

env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    TERM=xterm-256color \
    python3 "$HOME/send-keys-attach.py" record "$work/viewer.raw" 80 24 \
    "$binary" $prefix_args attach-session -t "=$session" \
    >"$work/viewer.out" 2>&1 &
viewer_pid=$!
await_clients 1 || { echo "copy-mode-kill-on-exit-$side: attach"; exit 0; }

main_client split-window -t "=$session:"
main_client split-window -t "=$session:"
main_client split-window -t "=$session:"
settle '0 1 2 3 '
check_equal panes-created '0 1 2 3 ' "$(panes)"
first="$(main_client list-panes -t "=$session" -F '#{pane_id}' | sed -n 1p)"
second="$(main_client list-panes -t "=$session" -F '#{pane_id}' | sed -n 2p)"
third="$(main_client list-panes -t "=$session" -F '#{pane_id}' | sed -n 3p)"
fourth="$(main_client list-panes -t "=$session" -F '#{pane_id}' | sed -n 4p)"

# cmd-copy-mode.c accepts -k and never reads it there; window.c stores it on
# the mode entry, so an entry without it leaves the pane alone at cancel.
main_client copy-mode -t "$first"
main_client send-keys -t "$first" -X cancel
settle '0 1 2 3 '
check_equal plain-cancel-keeps-the-pane '0 1 2 3 ' "$(panes)"

main_client copy-mode -k -t "$first"
main_client send-keys -t "$first" -X cancel
settle '0 1 2 '
check_equal kill-cancel-removes-the-pane '0 1 2 ' "$(panes)"

# window_pane_set_mode returns before the wme->kill assignment when the pane
# already holds the mode, so a re-entry neither arms nor clears the bit.
main_client copy-mode -k -t "$second"
main_client copy-mode -t "$second"
main_client send-keys -t "$second" -X cancel
settle '0 1 '
check_equal re-entry-keeps-an-armed-kill '0 1 ' "$(panes)"

main_client copy-mode -t "$third"
main_client copy-mode -k -t "$third"
main_client send-keys -t "$third" -X cancel
settle '0 1 '
check_equal re-entry-never-arms-the-kill '0 1 ' "$(panes)"

# copy-mode -q tears the mode down the same way, so -k takes the pane with it,
# and the last pane of the last window takes the session.
main_client copy-mode -k -t "$fourth"
main_client send-keys -t "$fourth" -X cancel
settle '0 '
check_equal kill-down-to-one-pane '0 ' "$(panes)"

main_client copy-mode -k -t "$third"
main_client copy-mode -q -t "$third"
attempt=0
while [ "$attempt" -lt 200 ]; do
    main_client has-session -t "=$session" >/dev/null 2>&1 || break
    attempt=$((attempt + 1))
    sleep 0.05
done
check_equal last-pane-takes-the-session 1 \
    "$(main_client has-session -t "=$session" >/dev/null 2>&1 && echo 0 || echo 1)"

# bind-key parses the flag where zz used to answer `unsupported command`.
main_client bind-key -T root F9 copy-mode -k
check_equal bind-key-renders-the-flag 'copy-mode -k' \
    "$(main_client list-keys -T root 2>/dev/null | sed -n 's/.*F9  *//p' | head -n 1)"
main_client unbind-key -T root F9

if [ "$check_count" -ne 8 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g COPY_MODE_KILL_ON_EXIT clean:8
else
    sed "s/^/copy-mode-kill-on-exit-$side: /" "$work/failures"
fi
