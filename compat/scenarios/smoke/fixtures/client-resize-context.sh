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
control_client() {
    # shellcheck disable=SC2086
    env -u TMUX -u TMUX_PANE "$binary" $prefix_args -C attach-session -t "=$session"
}

session=resizectx
work="$HOME/client-resize-context-work-$side"
steps="$work/steps"
log="$work/log"
rm -rf "$work"
mkdir -p "$steps"
: >"$work/failures"
: >"$log"
failed=0
check_count=0
step=0
attach_pid=""

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
    if [ -n "$attach_pid" ]; then
        kill "$attach_pid" >/dev/null 2>&1
        wait "$attach_pid" >/dev/null 2>&1
    fi
    main_client set-hook -gu client-resized >/dev/null 2>&1
    main_client set-hook -gu client-active >/dev/null 2>&1
    main_client kill-session -t "=$session" >/dev/null 2>&1
    exit "$cleanup_status"
}
trap cleanup EXIT

drive() {
    step=$((step + 1))
    printf '%s\n' "$1" >"$steps/step-$step"
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if [ -f "$steps/ack-$step" ]; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "drive-$step"
    return 1
}

settle() {
    sleep 1
}

report() {
    tr '\n' ';' <"$log"
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session" -x 80 -y 24
main_client set-hook -g client-resized \
    "run-shell 'printf %s\\\\n \"resized #{client_width}x#{client_height} #{client_tty}\" >> $log'"
main_client set-hook -g client-active \
    "run-shell 'printf %s\\\\n \"active #{client_tty}\" >> $log'"

env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    TERM=xterm-256color \
    python3 "$HOME/pty-drive.py" "$steps" 80 24 \
    "$binary" $prefix_args attach-session -t "=$session" \
    >"$work/attach.out" 2>&1 &
attach_pid=$!

attempt=0
while [ "$attempt" -lt 400 ]; do
    tty="$(main_client list-clients -t "=$session" -F '#{client_tty}' 2>/dev/null | head -1)"
    [ -n "$tty" ] && break
    attempt=$((attempt + 1))
    sleep 0.05
done
if [ -z "$tty" ]; then
    echo "client-resize-context-$side: no attached client"
    exit 0
fi
settle
# The pin reports the attaching client's first size as a client-resized of its
# own; zz folds that into attach. Everything below is post-attach reports.
: >"$log"

drive "size 100 30" || exit 0
settle
check_equal changed-once "resized 100x30 $tty;" "$(report)"

drive "size 100 30" || exit 0
settle
check_equal unchanged-silent "resized 100x30 $tty;" "$(report)"

drive "size 61 19" || exit 0
settle
check_equal second-change "resized 100x30 $tty;resized 61x19 $tty;" "$(report)"

# Two Control size reports, one per connection because zz's Control client
# still truncates a piped queue to its first command
# (control-mode.disconnect-cancels-command-queue). Neither report raises
# client-resized, and neither raises a client-active for the Control client.
printf 'refresh-client -C 120,40\n' | control_client >"$work/control-1.out" 2>&1 || true
settle
printf 'refresh-client -C 120,40\n' | control_client >"$work/control-2.out" 2>&1 || true
settle
check_equal control-no-resize "resized 100x30 $tty;resized 61x19 $tty;" \
    "$(grep '^resized' "$log" | tr '\n' ';')"
check_equal control-no-foreign-active "" \
    "$(grep '^active' "$log" | grep -v " $tty\$" | tr '\n' ';')"

if [ "$check_count" -ne 5 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g CLIENT_RESIZE_CONTEXT clean:5
else
    sed "s/^/client-resize-context-$side: /" "$work/failures"
fi
