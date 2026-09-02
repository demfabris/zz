#!/bin/sh
set -eu

export LC_ALL=C

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    side=zz
    binary="$ZZ_SMOKE_ZZ_BIN"
    probe_socket="/tmp/zzcmpb-$$.sock"
    probe_args="--socket $probe_socket"
else
    side=tmux
    binary="$ZZ_SMOKE_TMUX_BIN"
    probe_args="-L zzcmpb-$$"
    probe_socket=""
fi

work="$HOME/copy-mode-prompt-bindings-$side"
rm -rf "$work"
mkdir -p "$work/steps"
: >"$work/failures"
failed=0
check_count=0
probe_daemon_pid=""
attach_pid=""
step=0

probe() {
    # shellcheck disable=SC2086
    "$binary" $probe_args "$@"
}

main_client() {
    if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
        "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" "$@"
    else
        "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" "$@"
    fi
}

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

stop() {
    [ -n "$1" ] || return 0
    kill "$1" >/dev/null 2>&1 || true
    wait "$1" >/dev/null 2>&1 || true
}

cleanup() {
    cleanup_status=$?
    trap - EXIT
    set +e
    probe kill-server >/dev/null 2>&1
    stop "$attach_pid"
    stop "$probe_daemon_pid"
    case "$probe_socket" in
    /tmp/zzcmpb-[0-9]*.sock) rm -f -- "$probe_socket" ;;
    esac
    exit "$cleanup_status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    "$binary" --socket "$probe_socket" -f /dev/null daemon \
        >"$work/daemon.out" 2>"$work/daemon.err" &
    probe_daemon_pid=$!
    attempt=0
    until [ -S "$probe_socket" ]; do
        attempt=$((attempt + 1))
        if [ "$attempt" -ge 400 ] || ! kill -0 "$probe_daemon_pid" 2>/dev/null; then
            echo "copy-mode-prompt-bindings-$side: probe-daemon"
            exit 0
        fi
        sleep 0.05
    done
    probe new-session -d -s cmpb -x 80 -y 24 cat
else
    probe -f /dev/null new-session -d -s cmpb -x 80 -y 24 cat
fi
pane="$(probe list-panes -t "=cmpb" -F '#{pane_id}' | head -n 1)"

env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    TERM=xterm-256color \
    python3 "$HOME/pty-drive.py" "$work/steps" 80 24 \
    $binary $probe_args attach-session -t "=cmpb" >"$work/attach.out" 2>&1 &
attach_pid=$!
attempt=0
client=""
while [ "$attempt" -lt 400 ]; do
    client="$(probe list-clients -t "=cmpb" -F '#{client_tty}' 2>/dev/null | sed -n '1p')"
    if [ -n "$client" ]; then
        break
    fi
    attempt=$((attempt + 1))
    sleep 0.05
done
if [ -z "$client" ]; then
    echo "copy-mode-prompt-bindings-$side: attach"
    exit 0
fi
sleep 0.6

drive() {
    step=$((step + 1))
    printf 'keys %s\n' "$1" >"$work/steps/step-$step"
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if [ -f "$work/steps/ack-$step" ]; then
            break
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    sleep 0.45
}

xy() {
    probe display-message -p -t "$pane" '#{copy_cursor_x},#{copy_cursor_y}'
}

scroll() {
    probe display-message -p -t "$pane" '#{scroll_position}'
}

match() {
    probe display-message -p -t "$pane" '#{search_present}|#{search_match}'
}

top() {
    probe send-keys -t "$pane" -X history-top
}

printf 'alpha beta gamma\ndelta alpha epsilon\nzeta alpha\n' >"$work/in.txt"
probe load-buffer -b cmpb "$work/in.txt"
probe paste-buffer -b cmpb -t "$pane"
attempt=0
while [ "$attempt" -lt 400 ]; do
    if probe capture-pane -p -t "$pane" | grep -q 'zeta alpha'; then
        break
    fi
    attempt=$((attempt + 1))
    sleep 0.05
done

probe set-window-option -t "=cmpb" mode-keys vi
probe copy-mode -t "$pane"
top
check_equal 'vi-entry' '0,0' "$(xy)"

# '/' and '?' raise the prompt whose answer runs `send-keys -X search-forward`
# or `-X search-backward` with the typed string. The prompt is the pin's
# `command-prompt -P`; zz raises it on the client surface it owns.
drive 2f
drive 7a657461
drive 0d
check_equal 'vi-slash-search-down' '0,2' "$(xy)"
check_equal 'vi-slash-match' '1|zeta' "$(match)"
top
drive 3f
drive 616c706861
drive 0d
check_equal 'vi-question-search-up' '5,5' "$(xy)"
check_equal 'vi-question-match' '1|alpha' "$(match)"

# The `#` and `*` bindings are `send-keys -FX search-backward` and
# `search-forward` with `#{copy_cursor_word}`, so the word under the cursor is
# expanded into the search string before the action runs.
top
drive 2a
check_equal 'vi-star-cursor-word' '6,1' "$(xy)"
check_equal 'vi-star-match' '1|alpha' "$(match)"
top
drive 23
check_equal 'vi-hash-cursor-word' '5,5' "$(xy)"
check_equal 'vi-hash-match' '1|alpha' "$(match)"

# 'f', 't', 'F' and 'T' are single-key prompts: the first key pressed is the
# whole answer, and it becomes the jump target.
top
drive 66
drive 67
check_equal 'vi-jump-forward' '11,0' "$(xy)"
top
drive 74
drive 67
check_equal 'vi-jump-to-forward' '10,0' "$(xy)"
drive 46
drive 61
check_equal 'vi-jump-backward' '9,0' "$(xy)"
drive 54
drive 61
check_equal 'vi-jump-to-backward' '5,0' "$(xy)"
probe send-keys -t "$pane" -X cancel

# 'C-s' and 'C-r' are the incremental prompts: the search runs on every
# keystroke, before Enter, and the prefix the prompt prepends picks the
# direction the -incremental action searches in.
probe set-window-option -t "=cmpb" mode-keys emacs
probe copy-mode -t "$pane"
top
drive 13
drive 7a657461
check_equal 'emacs-incremental-down-live' '4,2' "$(xy)"
check_equal 'emacs-incremental-down-match' '1|zeta' "$(match)"
drive 0d
check_equal 'emacs-incremental-down-answered' '4,2' "$(xy)"
drive 12
drive 616c706861
check_equal 'emacs-incremental-up-live' '0,0' "$(xy)"
check_equal 'emacs-incremental-up-match' '1|alpha' "$(match)"
drive 0d
check_equal 'emacs-incremental-up-answered' '0,0' "$(xy)"

# The emacs table binds the same four jump prompts, and this probe answers the
# same four columns the vi table does: window_copy_cursor_jump_to_back's
# `onemore` right step only shows up at the end of a row, which none of these
# jumps reach.
top
drive 66
drive 67
check_equal 'emacs-jump-forward' '11,0' "$(xy)"
top
drive 74
drive 67
check_equal 'emacs-jump-to-forward' '10,0' "$(xy)"
drive 46
drive 61
check_equal 'emacs-jump-backward' '9,0' "$(xy)"
drive 54
drive 61
check_equal 'emacs-jump-to-backward' '5,0' "$(xy)"
probe send-keys -t "$pane" -X cancel

# ':' and 'g' raise the goto-line prompt. Forty more lines put history behind
# the screen so the answer moves the view, which is what goto-line does with
# line numbers off. copy_cursor_y is not compared because the attached pane is
# not the same height on the two engines.
seq 1 40 | sed 's/^/L/' >"$work/long.txt"
probe load-buffer -b cmpb2 "$work/long.txt"
probe paste-buffer -b cmpb2 -t "$pane"
attempt=0
while [ "$attempt" -lt 400 ]; do
    if probe capture-pane -p -t "$pane" | grep -q 'L40'; then
        break
    fi
    attempt=$((attempt + 1))
    sleep 0.05
done

probe set-window-option -t "=cmpb" mode-keys vi
probe copy-mode -t "$pane"
check_equal 'scrolled-entry' '0' "$(scroll)"
drive 3a
drive 35
drive 0d
check_equal 'vi-colon-goto-line' '5' "$(scroll)"
probe send-keys -t "$pane" -X cancel
probe set-window-option -t "=cmpb" mode-keys emacs
probe copy-mode -t "$pane"
drive 67
drive 37
drive 0d
check_equal 'emacs-g-goto-line' '7' "$(scroll)"
probe send-keys -t "$pane" -X cancel

printf 'quit\n' >"$work/steps/step-$((step + 1))"

if [ "$check_count" -ne 26 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g COPY_MODE_PROMPT_BINDINGS "clean:$check_count"
else
    sed "s/^/copy-mode-prompt-bindings-$side: /" "$work/failures"
fi
