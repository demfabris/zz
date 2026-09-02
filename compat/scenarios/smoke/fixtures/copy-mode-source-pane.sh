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

session=cmsource
work="$HOME/copy-mode-source-pane-$side"
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

value() {
    main_client display-message -p -t "$2" -c "$client" "#{$1}"
}

raw="sh -c 'stty -echo -icanon min 1 time 0; exec cat'"

main_client new-session -d -s "$session" -n one -x 80 -y 24 "$raw"
main_client new-window -t "=$session" -n two "$raw"
source_pane="$(main_client display-message -p -t "=$session:one" '#{pane_id}')"
target_pane="$(main_client display-message -p -t "=$session:two" '#{pane_id}')"

for index in 1 2 3 4 5; do
    main_client send-keys -t "$source_pane" -l "SRC$index"
    main_client send-keys -t "$source_pane" -H 0a
    main_client send-keys -t "$target_pane" -l "DST$index"
    main_client send-keys -t "$target_pane" -H 0a
done
await_line "$source_pane" '^SRC5$' || { echo "copy-mode-source-pane-$side: source"; exit 0; }
await_line "$target_pane" '^DST5$' || { echo "copy-mode-source-pane-$side: target"; exit 0; }

env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    TERM=xterm-256color \
    python3 "$HOME/send-keys-attach.py" record "$work/viewer.raw" 80 24 \
    "$binary" $prefix_args attach-session -t "=$session" \
    >"$work/viewer.out" 2>&1 &
viewer_pid=$!
await_clients 1 || { echo "copy-mode-source-pane-$side: attach"; exit 0; }
client="$(main_client list-clients -t "=$session" -F '#{client_name}' | head -n 1)"

# window_pane_set_mode(wp, swp) puts the mode on the target pane only; the
# source pane keeps no mode entry of its own.
main_client copy-mode -s "$source_pane" -t "$target_pane"
sleep 0.5
check_equal target-is-in-copy-mode 1 "$(value pane_in_mode "$target_pane")"
check_equal target-mode-name copy-mode "$(value pane_mode "$target_pane")"
check_equal source-stays-out-of-the-mode 0 "$(value pane_in_mode "$source_pane")"
check_equal source-mode-name-is-empty '' "$(value pane_mode "$source_pane")"

# window_copy_clone_screen trims the source's trailing blank rows and drops
# the cursor onto the last used row at column zero, so the mode opens on the
# source pane's last line, not the target's.
check_equal cursor-line-is-the-source-last-line SRC5 "$(value copy_cursor_line "$target_pane")"
check_equal cursor-column-is-zero 0 "$(value copy_cursor_x "$target_pane")"
check_equal cursor-word-is-the-source-word SRC5 "$(value copy_cursor_word "$target_pane")"

# Keys still go to the target pane's own mode, and they walk the source's rows.
main_client send-keys -t "$target_pane" -N 3 -X cursor-up
sleep 0.4
check_equal cursor-walks-the-source-rows SRC2 "$(value copy_cursor_line "$target_pane")"
main_client send-keys -t "$target_pane" -X cursor-down
sleep 0.3
check_equal cursor-walks-back-down SRC3 "$(value copy_cursor_line "$target_pane")"

# The source pane has no mode command, so send-keys -X still fails on it.
if main_client send-keys -t "$source_pane" -X cursor-up >"$work/src.out" 2>"$work/src.err"; then
    check_equal source-refuses-mode-commands 1 0
else
    check_equal source-refuses-mode-commands 1 1
fi
check_equal source-refusal-message 'not in a mode' "$(cat "$work/src.err")"

# capture-pane reads the pane's own grid, which the mode never replaced.
check_equal target-grid-keeps-its-own-lines 5 \
    "$(main_client capture-pane -p -t "$target_pane" | grep -cE '^DST[1-5]$' || true)"
check_equal target-grid-has-no-source-lines 0 \
    "$(main_client capture-pane -p -t "$target_pane" | grep -cE '^SRC[1-5]$' || true)"

# window_copy_refresh_start refuses a view of another pane, so refresh-on
# cannot arm on this mode and the backing stays where it was.
main_client send-keys -t "$target_pane" -X refresh-on
main_client send-keys -t "$source_pane" -l SRC6
main_client send-keys -t "$source_pane" -H 0a
sleep 0.8
check_equal refresh-cannot-follow-the-source SRC3 "$(value copy_cursor_line "$target_pane")"

main_client send-keys -t "$target_pane" -X cancel
sleep 0.4
check_equal cancel-leaves-the-mode 0 "$(value pane_in_mode "$target_pane")"
check_equal cancel-clears-the-mode-name '' "$(value pane_mode "$target_pane")"

# Naming the target pane as its own source leaves window_copy_clone_screen's
# trim off, so the entry parks on the live cursor row the way a plain
# copy-mode does rather than on the last used row.
main_client copy-mode -s "$target_pane" -t "$target_pane"
sleep 0.4
check_equal self-source-is-a-plain-entry 1 "$(value pane_in_mode "$target_pane")"
check_equal self-source-parks-on-the-live-cursor-row '' "$(value copy_cursor_line "$target_pane")"
main_client send-keys -t "$target_pane" -X cursor-up
sleep 0.3
check_equal self-source-reads-its-own-lines DST5 "$(value copy_cursor_line "$target_pane")"
main_client send-keys -t "$target_pane" -X cancel
sleep 0.3

if [ "$check_count" -ne 19 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g COPY_MODE_SOURCE_PANE "clean:$check_count"
else
    sed "s/^/copy-mode-source-pane-$side: /" "$work/failures"
fi
