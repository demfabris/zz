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

session=cmrefresh
work="$HOME/copy-mode-refresh-$side"
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

await_output() {
    attempt=0
    while [ "$attempt" -lt 600 ]; do
        if main_client capture-pane -p -t "$pane" 2>/dev/null | grep -q "$1"; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "await-output-$1"
    return 1
}

value() {
    main_client display-message -p -t "$pane" "#{$1}"
}

climbed() {
    if [ "$2" -gt "$1" ]; then echo 1; else echo 0; fi
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session" -x 80 -y 24 \
    "sh -c 'n=0; while true; do n=\$((n + 1)); printf \"tick %04d\\n\" \$n; sleep 0.05; done'"
pane="$(main_client list-panes -t "=$session" -F '#{pane_id}' | head -n 1)"

env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    TERM=xterm-256color \
    python3 "$HOME/send-keys-attach.py" record "$work/viewer.raw" 80 24 \
    "$binary" $prefix_args attach-session -t "=$session" \
    >"$work/viewer.out" 2>&1 &
viewer_pid=$!
await_clients 1 || { echo "copy-mode-refresh-$side: attach"; exit 0; }

# The pane has to have scrolled before the frozen view can fall behind it.
await_output 'tick 0080' || { echo "copy-mode-refresh-$side: output"; exit 0; }

main_client copy-mode -t "$pane"
# window_copy_refresh_timer only follows new output when the view is at the
# bottom with the cursor on the last row, so step off that row first.
main_client send-keys -t "$pane" -X cursor-up

frozen="$(value scroll_position)"
line="$(value copy_cursor_line)"
check_equal cursor-line-is-a-tick 1 \
    "$(expr "$line" : 'tick [0-9][0-9][0-9][0-9]$' >/dev/null && echo 1 || echo 0)"
sleep 1
check_equal refresh-is-off-at-entry "$frozen" "$(value scroll_position)"
check_equal frozen-view-keeps-its-line "$line" "$(value copy_cursor_line)"

# window_copy_do_refresh re-clones the backing and keeps the view on the row it
# was already showing, so the distance to the bottom grows while the cursor
# line does not move.
main_client send-keys -t "$pane" -X refresh-on
sleep 1
running="$(value scroll_position)"
check_equal refresh-on-climbs 1 "$(climbed "$frozen" "$running")"
check_equal refresh-keeps-the-cursor-line "$line" "$(value copy_cursor_line)"

main_client send-keys -t "$pane" -X refresh-toggle
stopped="$(value scroll_position)"
sleep 1
check_equal refresh-toggle-freezes "$stopped" "$(value scroll_position)"

main_client send-keys -t "$pane" -X refresh-toggle
sleep 1
check_equal refresh-toggle-restarts 1 "$(climbed "$stopped" "$(value scroll_position)")"

# The tick is skipped while a selection is live, so the view holds even with
# the refresh running.
main_client send-keys -t "$pane" -X begin-selection
main_client send-keys -t "$pane" -X cursor-right
selecting="$(value scroll_position)"
sleep 1
check_equal selection-holds-the-refresh "$selecting" "$(value scroll_position)"
main_client send-keys -t "$pane" -X clear-selection
sleep 1
check_equal cleared-selection-resumes 1 "$(climbed "$selecting" "$(value scroll_position)")"

main_client send-keys -t "$pane" -X refresh-off
off="$(value scroll_position)"
sleep 1
check_equal refresh-off-freezes "$off" "$(value scroll_position)"

# oy == 0 with the cursor on the last row is the follow case, so the view rides
# the new output instead of falling behind it.
main_client send-keys -t "$pane" -X history-bottom
main_client send-keys -t "$pane" -X bottom-line
main_client send-keys -t "$pane" -X refresh-on
sleep 1
check_equal follow-stays-at-the-bottom 0 "$(value scroll_position)"
main_client send-keys -t "$pane" -X refresh-off
main_client send-keys -t "$pane" -X cancel

# Both stock tables bind r to the pin's stored send-keys shape.
check_equal emacs-r-binding 'send-keys -X refresh-toggle' \
    "$(main_client list-keys -T copy-mode 2>/dev/null | sed -n 's/^bind-key  *-T copy-mode  *r  *//p' | head -n 1)"
check_equal vi-r-binding 'send-keys -X refresh-toggle' \
    "$(main_client list-keys -T copy-mode-vi 2>/dev/null | sed -n 's/^bind-key  *-T copy-mode-vi  *r  *//p' | head -n 1)"

if [ "$check_count" -ne 13 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g COPY_MODE_REFRESH clean:13
else
    sed "s/^/copy-mode-refresh-$side: /" "$work/failures"
fi
