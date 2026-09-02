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

session=cmsearch
work="$HOME/copy-mode-search-$side"
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

row() {
    main_client display-message -p -t "$pane" \
        '#{copy_cursor_x},#{copy_cursor_y}|#{search_present}|#{search_match}|#{search_count}|#{search_count_partial}|#{search_timed_out}'
}

X() {
    main_client send-keys -t "$pane" -X "$@"
}

enter() {
    main_client send-keys -t "$pane" -X cancel >/dev/null 2>&1 || true
    main_client copy-mode -t "$pane"
    X history-top
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
await_clients 1 || { echo "copy-mode-search-$side: attach"; exit 0; }

printf 'alpha beta gamma\ndelta alpha epsilon\nzeta alpha\n' >"$work/in.txt"
main_client load-buffer -b cmsearch "$work/in.txt"
main_client paste-buffer -b cmsearch -t "$pane"
await_output 'zeta alpha' || { echo "copy-mode-search-$side: output"; exit 0; }

# `cat` echoes the paste and prints it again, so the six rows at the top of the
# history are `alpha beta gamma`, `delta alpha epsilon`, `zeta alpha` twice, and
# history-top parks the cursor on the first of them.
main_client set-window-option -t "=$session" mode-keys vi
enter
check_equal 'vi-top' '0,0|0||0|0|0' "$(row)"

# window_copy_search under vi steps past the mark the cursor stands on before
# searching forward, so it lands on the start of the next match, and searching
# backward lands on the start of the previous one. searchcount is the whole
# backing's match count, not the visible screen's.
X search-forward alpha
check_equal 'vi-search-forward' '6,1|1|alpha|6|0|0' "$(row)"
X search-again
check_equal 'vi-search-again' '5,2|1|alpha|6|0|0' "$(row)"
X search-reverse
check_equal 'vi-search-reverse' '6,1|1|alpha|6|0|0' "$(row)"
X search-backward delta
check_equal 'vi-search-backward' '0,1|1|delta|2|0|0' "$(row)"

# The -text spellings clear data->searchregex, so the dot is a literal cell and
# nothing matches; the failed search leaves the cursor alone and clears the
# marks it had, which puts -1 back in searchcount and publishes neither count.
X search-forward-text 'al.ha'
check_equal 'vi-search-forward-text' '0,1|0||||0' "$(row)"
X search-forward 'al.ha'
check_equal 'vi-search-forward-regex' '6,1|1|alpha|6|0|0' "$(row)"

# wme->prefix repeats the whole search, so three of them walk three matches on.
enter
main_client send-keys -t "$pane" -N 3 -X search-forward alpha
check_equal 'vi-counted' '0,3|1|alpha|6|0|0' "$(row)"

# A miss on a mode that never laid marks down does not run clear_marks at all,
# so the zeroed searchcount the mode was entered with survives it, and a missing
# or empty string is window_copy_expand_search_string answering zero.
enter
X search-forward nosuchword
check_equal 'vi-miss' '0,0|0||0|0|0' "$(row)"
X search-forward ''
check_equal 'vi-empty' '0,0|0||0|0|0' "$(row)"

# send-keys -F reaches exactly one place in the pin: the search string goes
# through format_single in the target pane's context. Without it the braces are
# searched for literally.
X history-top
main_client send-keys -t "$pane" -FX search-forward '#{copy_cursor_word}'
check_equal 'vi-format-expanded' '6,1|1|alpha|6|0|0' "$(row)"
X search-forward '#{copy_cursor_word}'
check_equal 'vi-format-literal' '6,1|0||||0' "$(row)"

# Under emacs the forward search starts from the cursor itself, so it may find
# the mark the cursor is already on, and parks one cell past the match.
# window_copy_match_at_cursor steps one position back before giving up, so that
# cell still answers the match it just left.
main_client set-window-option -t "=$session" mode-keys emacs
enter
check_equal 'emacs-top' '0,0|0||0|0|0' "$(row)"
X search-forward alpha
check_equal 'emacs-search-forward' '5,0|1|alpha|6|0|0' "$(row)"
X search-again
check_equal 'emacs-search-again' '11,1|1|alpha|6|0|0' "$(row)"
X search-reverse
check_equal 'emacs-search-reverse' '6,1|1|alpha|6|0|0' "$(row)"
X search-backward delta
check_equal 'emacs-search-backward' '0,1|1|delta|2|0|0' "$(row)"
X search-backward-text alpha
check_equal 'emacs-search-backward-text' '0,0|1|alpha|6|0|0' "$(row)"

# The incremental spellings read the prefix command-prompt -i prepends: '=' is
# the spelling's own direction, '+' is down and '-' is up. The mode latches
# where it stood on the first step and every changed string goes back to the end
# of that row before searching again, and an empty string clears the marks.
main_client set-window-option -t "=$session" mode-keys vi
enter
X search-forward-incremental '=alpha'
check_equal 'incremental-forward-first' '6,1|1|alpha|6|0|0' "$(row)"
X search-forward-incremental '+zeta'
check_equal 'incremental-forward-changed' '0,2|1|zeta|2|0|0' "$(row)"
X search-forward-incremental '=alpha'
check_equal 'incremental-forward-back' '6,1|1|alpha|6|0|0' "$(row)"
X search-forward-incremental '='
check_equal 'incremental-forward-empty' '15,0|0||||0' "$(row)"

enter
X search-backward-incremental '=alpha'
check_equal 'incremental-backward-first' '5,5|1|alpha|6|0|0' "$(row)"
X search-backward-incremental '+alpha'
check_equal 'incremental-backward-down' '0,0|1|alpha|6|0|0' "$(row)"
X search-backward-incremental '=zeta'
check_equal 'incremental-backward-changed' '0,5|1|zeta|2|0|0' "$(row)"

# Outside a mode window_copy_formats never runs, so every name expands empty.
X cancel
check_equal 'outside-the-mode' ',|||||' "$(row)"

if [ "$check_count" -ne 26 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g COPY_MODE_SEARCH "clean:$check_count"
else
    sed "s/^/copy-mode-search-$side: /" "$work/failures"
fi
