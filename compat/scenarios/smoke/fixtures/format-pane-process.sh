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

session=paneproc
work="$HOME/format-pane-process-work-$side"
rm -rf "$work"
mkdir -p "$work/one"
: >"$work/failures"
failed=0
check_count=0
client_pid=""

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
    if [ -n "$client_pid" ]; then
        kill "$client_pid" >/dev/null 2>&1
        wait "$client_pid" >/dev/null 2>&1
    fi
    main_client kill-session -t "=$session" >/dev/null 2>&1
    exit "$cleanup_status"
}
trap cleanup EXIT

probe() {
    main_client display-message -p -t "$pane" "$1"
}

await() {
    attempt=0
    while [ "$attempt" -lt 200 ]; do
        if [ "$(probe "$1")" = "$2" ]; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
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

numeric() {
    case "$1" in
    '' | *[!0-9]*) echo no ;;
    *) echo yes ;;
    esac
}

pipe_job() {
    if ps -o args= -p "$1" 2>/dev/null | grep -q pane-process-pipe; then
        echo yes
    else
        echo no
    fi
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session" -x 80 -y 24
pane="$(main_client list-panes -t "=$session" -F '#{pane_id}' | sed -n '1p')"
pane_tty="$(probe '#{pane_tty}')"

# format_cb_pane_pipe reads wp->pipe_fd and format_cb_pane_pipe_pid reads
# wp->pipe_pid, so both answer only while a pipe is attached; pane_pipe answers
# 0 rather than declining, while pane_pipe_pid declines.
check_equal fresh 'pipe=0 pid=[] unseen=0' \
    "$(probe 'pipe=#{pane_pipe} pid=[#{pane_pipe_pid}] unseen=#{pane_unseen_changes}')"

main_client pipe-pane -t "$pane" "cat >$work/pane-process-pipe.out"
if ! await 'pipe=#{pane_pipe}' 'pipe=1'; then
    echo "format-pane-process-$side: pipe-attach"
    exit 0
fi
pipe_pid="$(probe '#{pane_pipe_pid}')"
check_equal piped-pid-numeric yes "$(numeric "$pipe_pid")"
# wp->pipe_pid is the job's own process, which runs the pipe command through a
# shell, so its argument vector still names the destination.
check_equal piped-pid-is-the-job yes "$(pipe_job "$pipe_pid")"

main_client pipe-pane -t "$pane"
if ! await 'pipe=#{pane_pipe}' 'pipe=0'; then
    echo "format-pane-process-$side: pipe-detach"
    exit 0
fi
check_equal unpiped 'pipe=0 pid=[]' "$(probe 'pipe=#{pane_pipe} pid=[#{pane_pipe_pid}]')"

# PANE_UNSEENCHANGES is not "output nobody looked at": input.c raises it only
# while the pane holds a mode, and window.c drops it when the last mode goes.
# zz keeps copy mode per client, so the clause needs an attached one.
env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    TERM=xterm-256color \
    python3 "$HOME/pty-drive.py" "$work/one" 80 24 \
    "$binary" $prefix_args attach-session -t "=$session" \
    >"$work/attach.out" 2>&1 &
client_pid=$!
await_clients 1 || { echo "format-pane-process-$side: attach"; exit 0; }

printf 'before-mode\r\n' >"$pane_tty"
sleep 0.5
check_equal output-without-mode 'unseen=0' "$(probe 'unseen=#{pane_unseen_changes}')"

main_client copy-mode -t "$pane"
if ! await 'inmode=#{pane_in_mode}' 'inmode=1'; then
    echo "format-pane-process-$side: copy-mode"
    exit 0
fi
check_equal mode-without-output 'unseen=0' "$(probe 'unseen=#{pane_unseen_changes}')"

printf 'during-mode\r\n' >"$pane_tty"
if ! await 'unseen=#{pane_unseen_changes}' 'unseen=1'; then
    echo "format-pane-process-$side: unseen-raise"
    exit 0
fi
check_equal mode-with-output 'unseen=1' "$(probe 'unseen=#{pane_unseen_changes}')"

main_client send-keys -t "$pane" -X cancel
if ! await 'inmode=#{pane_in_mode}' 'inmode=0'; then
    echo "format-pane-process-$side: cancel"
    exit 0
fi
check_equal after-cancel 'unseen=0' "$(probe 'unseen=#{pane_unseen_changes}')"

# Re-entering the mode starts clean, which is window_pane_reset_mode dropping
# the bit before the next mode entry can see it.
main_client copy-mode -t "$pane"
if ! await 'inmode=#{pane_in_mode}' 'inmode=1'; then
    echo "format-pane-process-$side: copy-mode-again"
    exit 0
fi
check_equal reentered 'unseen=0' "$(probe 'unseen=#{pane_unseen_changes}')"

if [ "$check_count" -ne 9 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g FORMAT_PANE_PROCESS clean:9
else
    sed "s/^/format-pane-process-$side: /" "$work/failures"
fi
