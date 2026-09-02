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

session=cmformats
work="$HOME/copy-mode-formats-$side"
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
    while [ "$attempt" -lt 400 ]; do
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

# Outside a mode window_copy_formats never runs, so every name it would add is
# absent from the format tree and expands empty. pane_in_mode is a table entry
# instead, so it answers 0 for a pane with no mode, and pane_mode answers NULL.
absent_family() {
    for name in copy_cursor_line copy_cursor_word copy_cursor_x copy_cursor_y \
        scroll_position selection_end_x selection_end_y selection_present \
        selection_start_x selection_start_y; do
        check_equal "$1-$name" '' "$(value "$name")"
    done
    check_equal "$1-pane_in_mode" 0 "$(value pane_in_mode)"
    check_equal "$1-pane_mode" '' "$(value pane_mode)"
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
await_clients 1 || { echo "copy-mode-formats-$side: attach"; exit 0; }
client="$(main_client list-clients -t "=$session" -F '#{client_name}' | head -n 1)"

index=1
while [ "$index" -le 300 ]; do
    printf 'L%03d marker\n' "$index"
    index=$((index + 1))
done >"$work/lines.txt"
main_client load-buffer -b cmformats "$work/lines.txt"
main_client paste-buffer -b cmformats -t "$pane"
await_output 'L300 marker' || { echo "copy-mode-formats-$side: output"; exit 0; }

absent_family before

main_client copy-mode -t "$pane"

check_equal in-pane_in_mode 1 "$(value pane_in_mode)"
check_equal in-pane_mode copy-mode "$(value pane_mode)"
check_equal in-scroll_position 0 "$(value scroll_position)"
check_equal in-selection_present 0 "$(value selection_present)"
check_equal in-selection_start_x '' "$(value selection_start_x)"
check_equal in-selection_start_y '' "$(value selection_start_y)"
check_equal in-selection_end_x '' "$(value selection_end_x)"
check_equal in-selection_end_y '' "$(value selection_end_y)"

# The pane's own client and a named -c client answer the same mode, because
# tmux keeps one mode entry on the pane and zz falls back to the one client
# that holds a copy session on it.
entry="$(value copy_cursor_y)"
named="$(main_client display-message -p -t "$pane" -c "$client" '#{copy_cursor_y}')"
check_equal entry-is-a-row 1 "$(expr "$entry" : '[0-9][0-9]*$' >/dev/null && echo 1 || echo 0)"
check_equal entry-matches-named "$entry" "$named"

# window.c stores the -N count as wme->prefix on the mode entry and
# window_copy_command resets it to 1 only after a command actually ran, so a
# bare `send-keys -N 5` and a `send-keys -N 4 -X` with no action each leave the
# count armed for whichever mode command runs next. The pane geometry differs
# between the two clients, so the proof is the distance each cursor-up moved.
main_client send-keys -t "$pane" -N 5
main_client send-keys -t "$pane" -X cursor-up
after_counted="$(value copy_cursor_y)"
main_client send-keys -t "$pane" -X cursor-up
after_plain="$(value copy_cursor_y)"
main_client send-keys -t "$pane" -N 4 -X
main_client send-keys -t "$pane" -X cursor-up
after_empty="$(value copy_cursor_y)"

check_equal no-key-count-spends-five 5 "$((entry - after_counted))"
check_equal count-resets-after-one-command 1 "$((after_counted - after_plain))"
check_equal empty-copy-count-spends-four 4 "$((after_plain - after_empty))"
check_equal counts-leave-the-view 0 "$(value scroll_position)"

# A count larger than the cursor's row scrolls the view by the remainder, so
# the rows moved stay the count whatever the client's pane height is.
main_client send-keys -t "$pane" -N 100 -X cursor-up
check_equal counted-scroll-moves-one-hundred 100 \
    "$(($(value scroll_position) + after_empty - $(value copy_cursor_y)))"

main_client send-keys -t "$pane" -X history-bottom
check_equal history-bottom-returns 0 "$(value scroll_position)"

# window_copy_get_line trims the row's trailing blanks and window_copy_get_word
# returns the word the cursor sits in, so on a marker row the word is the
# line's first token.
main_client send-keys -t "$pane" -X cursor-up
main_client send-keys -t "$pane" -X start-of-line
line="$(value copy_cursor_line)"
word="$(value copy_cursor_word)"
check_equal cursor-line-is-a-marker 1 \
    "$(expr "$line" : 'L[0-9][0-9][0-9] marker$' >/dev/null && echo 1 || echo 0)"
check_equal cursor-word-is-the-first-token "${line%% *}" "$word"
check_equal cursor-x-at-start-of-line 0 "$(value copy_cursor_x)"

# format_grid_word walks back to the start of the word the cursor sits in, and
# a cursor parked on a single separator collects the word that follows it.
main_client send-keys -t "$pane" -N 4 -X cursor-right
check_equal cursor-x-after-four-rights 4 "$(value copy_cursor_x)"
check_equal cursor-word-on-the-separator "${line##* }" "$(value copy_cursor_word)"
main_client send-keys -t "$pane" -N 3 -X cursor-right
check_equal cursor-x-after-three-more 7 "$(value copy_cursor_x)"
check_equal cursor-word-inside-the-second-word "${line##* }" "$(value copy_cursor_word)"
check_equal cursor-line-unchanged-by-the-walk "$line" "$(value copy_cursor_line)"
main_client send-keys -t "$pane" -X start-of-line

# screen.sel is NULL until begin-selection, and selection_present stays 0 while
# the selection is still one cell wide.
main_client send-keys -t "$pane" -X begin-selection
check_equal selection-armed-is-empty 0 "$(value selection_present)"
start_x="$(value selection_start_x)"
start_y="$(value selection_start_y)"
check_equal selection-start-tracks-the-cursor "$(value copy_cursor_x)" "$start_x"
check_equal selection-start-equals-end "$start_x" "$(value selection_end_x)"
check_equal selection-start-row-equals-end "$start_y" "$(value selection_end_y)"

main_client send-keys -t "$pane" -N 3 -X cursor-right
check_equal selection-present-after-a-move 1 "$(value selection_present)"
check_equal selection-start-is-pinned "$start_x" "$(value selection_start_x)"
check_equal selection-start-row-is-pinned "$start_y" "$(value selection_start_y)"
check_equal selection-end-follows-the-cursor "$(value copy_cursor_x)" "$(value selection_end_x)"

main_client send-keys -t "$pane" -X cancel
absent_family after

if [ "$check_count" -ne 56 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g COPY_MODE_FORMATS clean:56
else
    sed "s/^/copy-mode-formats-$side: /" "$work/failures"
fi
