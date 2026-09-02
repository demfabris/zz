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

session=clearhl
work="$HOME/clear-history-hyperlinks-$side"
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

await_line() {
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if main_client capture-pane -p -t "$1" 2>/dev/null | grep -q "$2"; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "await-line-$2"
    return 1
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

raw="sh -c 'stty -echo -icanon min 1 time 0; exec cat'"
main_client new-session -d -s "$session" -n hl -x 40 -y 6 "$raw"
pane="$(main_client display-message -p -t "=$session:hl" '#{pane_id}')"

# clear-history reaches the pane through a client's terminal view on zz, so
# the differential needs a real attached client on both binaries.
env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    TERM=xterm-256color \
    python3 "$HOME/send-keys-attach.py" record "$work/viewer.raw" 80 24 \
    "$binary" $prefix_args attach-session -t "=$session" \
    >"$work/viewer.out" 2>&1 &
viewer_pid=$!
await_clients 1 || { echo "clear-history-hyperlinks-$side: attach"; exit 0; }

for line in H1 H2 H3 H4 H5 H6 H7 H8; do
    main_client send-keys -t "$pane" -l "$line"
    main_client send-keys -t "$pane" -H 0a
done
# OSC 8 open, the anchor text, OSC 8 close.
main_client send-keys -t "$pane" -H 1b 5d 38 3b 3b 68 74 74 70 3a 2f 2f 65 2e 63 6f 6d 1b 5c
main_client send-keys -t "$pane" -l LINK
main_client send-keys -t "$pane" -H 1b 5d 38 3b 3b 1b 5c
await_line "$pane" LINK

# cmd_capture_pane_exec runs the same grid_clear_history for both spellings and
# only adds screen_reset_hyperlinks under -H, so both answer 0 with no output
# and leave the visible rows alone.
if main_client clear-history -t "$pane" >"$work/plain.out" 2>"$work/plain.err"; then
    check_equal plain-clear-history-succeeds 1 1
else
    check_equal plain-clear-history-succeeds 1 0
fi
check_equal plain-clear-history-is-silent '' "$(cat "$work/plain.out" "$work/plain.err")"
check_equal plain-clear-history-empties-the-history \
    "$(main_client capture-pane -p -t "$pane" | md5)" \
    "$(main_client capture-pane -p -S - -t "$pane" | md5)"
check_equal plain-clear-history-keeps-the-screen 1 \
    "$(main_client capture-pane -p -t "$pane" | grep -c 'LINK' || true)"

for line in K1 K2 K3 K4 K5 K6 K7 K8; do
    main_client send-keys -t "$pane" -l "$line"
    main_client send-keys -t "$pane" -H 0a
done
await_line "$pane" K8

if main_client clear-history -H -t "$pane" >"$work/reset.out" 2>"$work/reset.err"; then
    check_equal hyperlink-clear-history-succeeds 1 1
else
    check_equal hyperlink-clear-history-succeeds 1 0
fi
check_equal hyperlink-clear-history-is-silent '' "$(cat "$work/reset.out" "$work/reset.err")"
check_equal hyperlink-clear-history-empties-the-history \
    "$(main_client capture-pane -p -t "$pane" | md5)" \
    "$(main_client capture-pane -p -S - -t "$pane" | md5)"
check_equal hyperlink-clear-history-keeps-the-screen 1 \
    "$(main_client capture-pane -p -t "$pane" | grep -c '^K8$' || true)"

# The reset takes the registry, not the cells: the anchor text stays wherever
# it still is on the screen, and plain capture never carried the URI.
check_equal hyperlink-text-survives-the-reset 0 \
    "$(main_client capture-pane -p -S - -t "$pane" | grep -c 'http' || true)"

main_client bind-key -T root F9 clear-history -H
check_equal hyperlink-flag-binds 'bind-key -T root F9 clear-history -H' \
    "$(main_client list-keys -T root F9 | tr -s ' ')"
main_client unbind-key -T root F9

if [ "$check_count" -ne 10 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g CLEAR_HISTORY_HYPERLINKS "clean:$check_count"
else
    sed "s/^/clear-history-hyperlinks-$side: /" "$work/failures"
fi
