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

session=cmtail
work="$HOME/copy-mode-mode-keys-tail-$side"
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

# The wrapped line's two rows are told apart by their first four cells: the
# filled row is all `a`, the continuation row starts `qqq `.
col_abs() {
    printf '%s|%s' \
        "$(main_client display-message -p -t "$pane" '#{copy_cursor_x}')" \
        "$(main_client display-message -p -t "$pane" '#{=4:copy_cursor_line}')"
}

col_rel() {
    x="$(main_client display-message -p -t "$pane" '#{copy_cursor_x}')"
    printf '%s|%s' \
        "$((x - width))" \
        "$(main_client display-message -p -t "$pane" '#{=4:copy_cursor_line}')"
}

scrolled() {
    printf '%s|%s' \
        "$(main_client display-message -p -t "$pane" '#{scroll_position}')" \
        "$(main_client display-message -p -t "$pane" '#{copy_cursor_line}')"
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
await_clients 1 || { echo "copy-mode-mode-keys-tail-$side: attach"; exit 0; }

printf 'abcz\n(one) [two]\nplain text here\n' >"$work/in.txt"
main_client load-buffer -b cmtail "$work/in.txt"
main_client paste-buffer -b cmtail -t "$pane"
await_output 'plain text here' || { echo "copy-mode-mode-keys-tail-$side: output"; exit 0; }

# window_copy_cmd_next_matching_bracket and its previous twin both read
# data->modekeys, the value the option had when the mode was entered. vi walks a
# closing bracket found first back to its opener; emacs never does that walk,
# looks at exactly one neighbouring cell, and falls back to a word motion whose
# separators are the bracket set itself.
for keys in vi emacs; do
    main_client set-window-option -t "=$session" mode-keys "$keys"
    if [ "$keys" = vi ]; then
        from_close='0,1'
        off_bracket='5,2'
    else
        from_close='5,1'
        off_bracket='0,2'
    fi

    main_client copy-mode -t "$pane"
    X history-top
    X cursor-down
    X start-of-line
    X cursor-right
    X cursor-right
    X cursor-right
    X cursor-right
    check_equal "$keys-on-the-closing-paren" '4,1' "$(xy)"
    X next-matching-bracket
    check_equal "$keys-next-matching-bracket-from-a-closing-one" "$from_close" "$(xy)"
    X cancel

    main_client copy-mode -t "$pane"
    X history-top
    X cursor-down
    X cursor-down
    X start-of-line
    X cursor-right
    X cursor-right
    X cursor-right
    X cursor-right
    X cursor-right
    check_equal "$keys-on-a-row-with-no-bracket" '5,2' "$(xy)"
    X previous-matching-bracket
    check_equal "$keys-previous-matching-bracket-off-a-bracket" "$off_bracket" "$(xy)"
    X cancel
done

# window_copy_cursor_jump_to_back passes onemore to the right step it takes
# after the jump, and grid_reader_cursor_jump_back follows a wrapped line back
# onto the row it came from. The line is built from the pane's own width so the
# jump target is the last cell of the filled row, which is where the vi and
# emacs limits differ; the pane is not the same width on the two engines, so the
# column is compared against that width.
width="$(main_client display-message -p -t "$pane" '#{pane_width}')"
filler="$(awk -v n="$((width - 1))" 'BEGIN { while (i++ < n) printf "a" }')"
printf '%szqqq tail\n' "$filler" >"$work/wrapped.txt"
main_client load-buffer -b cmtail2 "$work/wrapped.txt"
main_client paste-buffer -b cmtail2 -t "$pane"
await_output 'tail' || { echo "copy-mode-mode-keys-tail-$side: wrapped-output"; exit 0; }

# The three short lines are echoed and printed twice before the wrapped one, so
# the filled row is the seventh from the top of the history and its continuation
# the eighth. Both engines wrap that line in the same place because it is built
# from their own pane width.
park_on_the_continuation_row() {
    main_client copy-mode -t "$pane"
    X history-top
    main_client send-keys -t "$pane" -N 7 -X cursor-down
    X cursor-right
    X cursor-right
    X cursor-right
}

for keys in vi emacs; do
    main_client set-window-option -t "=$session" mode-keys "$keys"

    park_on_the_continuation_row
    check_equal "$keys-parked-on-the-continuation-row" '3|qqq ' "$(col_abs)"
    X jump-to-backward z
    if [ "$keys" = vi ]; then
        # The target is the filled row's last cell, which is the vi limit, so
        # the right step wraps onto the continuation row instead.
        check_equal "$keys-jump-to-backward-across-the-wrap" '0|qqq ' "$(col_abs)"
    else
        check_equal "$keys-jump-to-backward-across-the-wrap" '0|aaaa' "$(col_rel)"
    fi
    X cancel

    park_on_the_continuation_row
    X jump-backward z
    check_equal "$keys-jump-backward-across-the-wrap" '-1|aaaa' "$(col_rel)"
    X cancel
done

# window_copy_cursor_up and _down with scroll_only: the view moves one row
# either way, and under vi the cursor first steps one screen row against the
# scroll so it keeps the line it was on, while under emacs it keeps its screen
# row and the line under it changes.
seq 1 40 | sed 's/^/L/' >"$work/long.txt"
main_client load-buffer -b cmtail3 "$work/long.txt"
main_client paste-buffer -b cmtail3 -t "$pane"
await_output 'L40' || { echo "copy-mode-mode-keys-tail-$side: long-output"; exit 0; }

for keys in vi emacs; do
    main_client set-window-option -t "=$session" mode-keys "$keys"
    if [ "$keys" = vi ]; then
        up1='1|L38'
        up2='2|L38'
        down1='1|L38'
    else
        up1='1|L37'
        up2='2|L36'
        down1='1|L37'
    fi

    main_client copy-mode -t "$pane"
    X cursor-up
    X cursor-up
    X cursor-up
    check_equal "$keys-before-the-scroll" '0|L38' "$(scrolled)"
    X scroll-up
    check_equal "$keys-after-one-scroll-up" "$up1" "$(scrolled)"
    X scroll-up
    check_equal "$keys-after-two-scroll-ups" "$up2" "$(scrolled)"
    X scroll-down
    check_equal "$keys-after-a-scroll-down" "$down1" "$(scrolled)"
    X cancel
done


# window_copy_command's tail: after every command whose name does not start
# with `search-`, the marks a search laid down are dropped per the command
# table's clear class, read against the live mode-keys, and searchx/searchy go
# back to -1. The clear class is CLEAR_ALWAYS for 52 names, CLEAR_EMACS_ONLY
# for 36 and CLEAR_NEVER for 7. cursor-down is emacs-only, begin-selection and
# back-to-indentation are always, toggle-position is never. Losing the marks
# also re-latches the incremental origin, and the re-lay a following search of
# the same string does is visible-only, which leaves the count unset.
main_client new-window -t "=$session" -n search cat
spane="$(main_client list-panes -t "=$session:search" -F '#{pane_id}' | head -n 1)"

await_search_output() {
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if main_client capture-pane -p -t "$spane" 2>/dev/null | grep -q "$1"; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "await-search-output-$1"
    return 1
}

S() {
    main_client send-keys -t "$spane" -X "$@"
}

marks() {
    main_client display-message -p -t "$spane" \
        '#{copy_cursor_x},#{copy_cursor_y}|#{search_present}|#{search_match}|#{search_count}|#{search_count_partial}|#{search_timed_out}'
}

# save-buffer writes the bytes; show-buffer does not, because zz's CLI ends
# the buffer with a newline the pin does not add.
buffered() {
    rm -f "$work/copied"
    main_client save-buffer "$work/copied"
    tr '\n' '/' <"$work/copied"
}

printf 'alpha beta gamma\ndelta alpha epsilon\nzeta alpha\n' >"$work/search.txt"
main_client load-buffer -b cmtail4 "$work/search.txt"
main_client paste-buffer -b cmtail4 -t "$spane"
await_search_output 'zeta alpha' || { echo "copy-mode-mode-keys-tail-$side: search-output"; exit 0; }

for keys in vi emacs; do
    main_client set-window-option -t "=$session:search" mode-keys "$keys"
    if [ "$keys" = vi ]; then
        found='6,1|1|alpha|6|0|0'
        after_down='6,2|1|alpha|6|0|0'
        after_again='0,3|1|alpha|6|0|0'
        after_begin='0,3|0||||0'
        after_toggle='6,1|1|alpha|6|0|0'
        after_indent='0,1|0||||0'
        inc_first='6,1|1|alpha|6|0|0'
        inc_moved='6,4|1|alpha|6|0|0'
        inc_again='0,2|1|zeta|2|0|0'
        plain='alpha'
        rectangle='alpha/delta'
    else
        found='5,0|1|alpha|6|0|0'
        after_down='5,1|0||||0'
        after_again='11,1|1|alpha|||0'
        after_begin='11,1|0||||0'
        after_toggle='5,0|1|alpha|6|0|0'
        after_indent='0,0|0||||0'
        inc_first='5,0|1|alpha|6|0|0'
        inc_moved='5,3|0||||0'
        inc_again='4,5|1|zeta|2|0|0'
        plain='alph'
        rectangle='alph/delt'
    fi

    main_client copy-mode -t "$spane"
    S history-top
    S search-forward alpha
    check_equal "$keys-marks-after-the-first-search" "$found" "$(marks)"
    S cursor-down
    check_equal "$keys-marks-after-an-emacs-only-command" "$after_down" "$(marks)"
    S search-again
    check_equal "$keys-marks-relaid-without-a-count" "$after_again" "$(marks)"
    S begin-selection
    check_equal "$keys-marks-after-an-always-command" "$after_begin" "$(marks)"
    S cancel

    main_client copy-mode -t "$spane"
    S history-top
    S search-forward alpha
    S toggle-position
    check_equal "$keys-marks-after-a-never-command" "$after_toggle" "$(marks)"
    S back-to-indentation
    check_equal "$keys-marks-after-back-to-indentation" "$after_indent" "$(marks)"
    S cancel

    main_client copy-mode -t "$spane"
    S history-top
    S search-forward-incremental '=alpha'
    check_equal "$keys-incremental-origin-latched" "$inc_first" "$(marks)"
    S cursor-down
    S cursor-down
    S cursor-down
    check_equal "$keys-incremental-origin-after-the-moves" "$inc_moved" "$(marks)"
    S search-forward-incremental '=zeta'
    check_equal "$keys-incremental-origin-relatched" "$inc_again" "$(marks)"
    S cancel

    main_client copy-mode -t "$spane"
    S history-top
    S begin-selection
    S cursor-right
    S cursor-right
    S cursor-right
    S cursor-right
    S copy-selection
    check_equal "$keys-selection-keeps-or-drops-the-focus-cell" "$plain" "$(buffered)"
    S cancel

    main_client copy-mode -t "$spane"
    S history-top
    S rectangle-on
    S begin-selection
    S cursor-right
    S cursor-right
    S cursor-right
    S cursor-right
    S cursor-down
    S copy-selection
    check_equal "$keys-rectangle-keeps-or-drops-the-cursor-column" "$rectangle" "$(buffered)"
    S cancel
done

if [ "$check_count" -ne 44 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g COPY_MODE_MODE_KEYS_TAIL "clean:$check_count"
else
    sed "s/^/copy-mode-mode-keys-tail-$side: /" "$work/failures"
fi
