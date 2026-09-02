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

session=cmmodekeys
work="$HOME/copy-mode-mode-keys-$side"
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

xy() {
    main_client display-message -p -t "$pane" '#{copy_cursor_x},#{copy_cursor_y}'
}

X() {
    main_client send-keys -t "$pane" -X "$@"
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
await_clients 1 || { echo "copy-mode-mode-keys-$side: attach"; exit 0; }

printf 'abcd efg\nhijk\nlmnopq rst\n' >"$work/short.txt"
main_client load-buffer -b cmmodekeys "$work/short.txt"
main_client paste-buffer -b cmmodekeys -t "$pane"
await_output 'lmnopq rst' || { echo "copy-mode-mode-keys-$side: short-output"; exit 0; }

# `cat` echoes the paste and then prints it, so the three lines land twice and
# the second copy sits three rows above the cursor. Row indices are the
# attached client's screen rows, which both engines agree on for lines that fit.
# Rows: `abcd efg` (length 8), `hijk` (4), `lmnopq rst` (10).
for keys in vi emacs; do
    main_client set-window-option -t "=$session" mode-keys "$keys"
    if [ "$keys" = vi ]; then
        eol=7      # window_copy_cursor_limit: vi stops on the last cell
        prev=7     # the row above, reached by wrapping cursor-left
        end1=3     # next-word-end on `hijk`
        end2=3     # and again, because vi cannot leave the last cell
        end3=3
    else
        eol=8      # emacs parks one past it
        prev=8
        end1=4
        end2=6     # one past `lmnopq` on the row below
        end3=10    # one past `rst`
    fi

    # window_copy_cursor_right reads the row's own limit, so four presses from
    # the end of `abcd efg` wrap onto `hijk` and stop where that row ends.
    main_client copy-mode -t "$pane"
    X cursor-up
    X cursor-up
    X cursor-up
    X start-of-line
    check_equal "$keys-parked-on-the-first-row" '0,3' "$(xy)"
    X end-of-line
    check_equal "$keys-end-of-line" "$eol,3" "$(xy)"
    X cursor-right
    check_equal "$keys-right-wraps" '0,4' "$(xy)"
    X cursor-right
    X cursor-right
    X cursor-right
    check_equal "$keys-right-stops-at-the-line-end" '3,4' "$(xy)"
    X cursor-left
    check_equal "$keys-left-steps-back" '2,4' "$(xy)"
    main_client send-keys -t "$pane" -X cancel

    # window_copy_cursor_next_word_end: emacs takes the reader's landing, vi
    # steps off the cell first and is pulled back onto the word's last cell.
    main_client copy-mode -t "$pane"
    X cursor-up
    X cursor-up
    X start-of-line
    check_equal "$keys-parked-on-the-second-row" '0,4' "$(xy)"
    X next-word-end
    check_equal "$keys-next-word-end" "$end1,4" "$(xy)"
    X next-word-end
    if [ "$keys" = vi ]; then
        check_equal "$keys-next-word-end-again" "$end2,4" "$(xy)"
    else
        check_equal "$keys-next-word-end-again" "$end2,5" "$(xy)"
    fi
    X next-word-end
    if [ "$keys" = vi ]; then
        check_equal "$keys-next-word-end-thrice" "$end3,4" "$(xy)"
    else
        check_equal "$keys-next-word-end-thrice" "$end3,5" "$(xy)"
    fi
    main_client send-keys -t "$pane" -X cancel

    # window_copy_cursor_up and _down keep one desired column, clamped to each
    # row's own limit and pushed to the line end once it reached the remembered
    # one, so the walk down and back up is not a straight line.
    main_client copy-mode -t "$pane"
    X cursor-up
    X cursor-up
    X cursor-up
    X end-of-line
    check_equal "$keys-walk-starts-at-the-line-end" "$eol,3" "$(xy)"
    X cursor-down
    check_equal "$keys-down-onto-the-short-row" "$end1,4" "$(xy)"
    X cursor-down
    if [ "$keys" = vi ]; then
        check_equal "$keys-down-onto-the-long-row" '3,5' "$(xy)"
    else
        check_equal "$keys-down-onto-the-long-row" '10,5' "$(xy)"
    fi
    X cursor-up
    check_equal "$keys-up-onto-the-short-row" "$end1,4" "$(xy)"
    X cursor-up
    if [ "$keys" = vi ]; then
        check_equal "$keys-up-onto-the-long-row" '3,3' "$(xy)"
    else
        check_equal "$keys-up-onto-the-long-row" "$eol,3" "$(xy)"
    fi
    main_client send-keys -t "$pane" -X cancel

    # grid_reader_cursor_left wraps at column zero onto the end of the row
    # above, which window_copy_update_cursor then clamps to that row's limit.
    main_client copy-mode -t "$pane"
    X cursor-up
    X cursor-up
    X start-of-line
    X cursor-left
    check_equal "$keys-left-wraps-onto-the-row-above" "$prev,3" "$(xy)"
    main_client send-keys -t "$pane" -X cancel
done

# A wrapped line is one logical line to the reader, and the pane is not the
# same width on the two engines while a client is attached, so the line is
# built from the pane's own width: the first row fills it and the second holds
# twenty more letters, a space and `tail`.
width="$(main_client display-message -p -t "$pane" '#{pane_width}')"
long="$(awk -v n="$((width + 20))" 'BEGIN { while (i++ < n) printf "a" }')"
printf '%s tail\n' "$long" >"$work/wrapped.txt"
main_client load-buffer -b cmmodekeys "$work/wrapped.txt"
main_client paste-buffer -b cmmodekeys -t "$pane"
await_output 'tail' || { echo "copy-mode-mode-keys-$side: wrapped-output"; exit 0; }

for keys in vi emacs; do
    main_client set-window-option -t "=$session" mode-keys "$keys"
    if [ "$keys" = vi ]; then
        weol=24                     # the continuation row holds 25 cells
        wend=19                     # the last cell of the wrapped word
        wfull="$((width - 1))"      # the filled row's own last cell
    else
        weol=25
        wend=20
        wfull="$width"
    fi

    # The six short rows are still on the screen, so the wrapped line's echo
    # sits on rows 6 and 7 and cat's copy of it on rows 8 and 9.
    # grid_reader_cursor_end_of_line walks down through the rows a wrap
    # continued onto, and start-of-line walks back up through them.
    main_client copy-mode -t "$pane"
    X cursor-up
    X cursor-up
    X start-of-line
    check_equal "$keys-wrapped-start" '0,8' "$(xy)"
    X end-of-line
    check_equal "$keys-wrapped-end-of-line" "$weol,9" "$(xy)"
    X start-of-line
    check_equal "$keys-wrapped-start-of-line" '0,8' "$(xy)"
    # grid_reader_cursor_next_word_end bounds a wrapped row at its last column
    # rather than its used width, so the word is not broken by the wrap.
    X next-word-end
    check_equal "$keys-wrapped-next-word-end" "$wend,9" "$(xy)"
    X next-word-end
    check_equal "$keys-wrapped-next-word-end-again" "$weol,9" "$(xy)"
    main_client send-keys -t "$pane" -X cancel

    # Wrapping left off the continuation row lands on the filled row, whose
    # length is the pane's own width, so this is the one check that reads it.
    main_client copy-mode -t "$pane"
    X cursor-up
    X cursor-up
    X start-of-line
    X cursor-down
    check_equal "$keys-wrapped-continuation-start" '0,9' "$(xy)"
    X cursor-left
    check_equal "$keys-wrapped-left-wraps-onto-the-filled-row" "$wfull,8" "$(xy)"
    main_client send-keys -t "$pane" -X cancel
done

if [ "$check_count" -ne 44 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g COPY_MODE_MODE_KEYS "clean:$check_count"
else
    sed "s/^/copy-mode-mode-keys-$side: /" "$work/failures"
fi
