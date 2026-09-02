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
    env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
        "$binary" $prefix_args -C attach-session -t "=$session"
}

session=resizectx
work="$HOME/client-resized-context-work-$side"
rm -rf "$work"
mkdir -p "$work/one" "$work/two"
: >"$work/failures"
log="$work/hooks.log"
: >"$log"
failed=0
check_count=0
first_pid=""
second_pid=""
first_step=0
second_step=0

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
    first_step=$((first_step + 1))
    echo quit >"$work/one/step-$first_step" 2>/dev/null
    second_step=$((second_step + 1))
    echo quit >"$work/two/step-$second_step" 2>/dev/null
    for pid in $first_pid $second_pid; do
        kill "$pid" >/dev/null 2>&1
        wait "$pid" >/dev/null 2>&1
    done
    main_client set-hook -gu client-resized >/dev/null 2>&1
    main_client set-hook -gu client-active >/dev/null 2>&1
    main_client kill-session -t "=$session" >/dev/null 2>&1
    exit "$cleanup_status"
}
trap cleanup EXIT

drive_first() {
    first_step=$((first_step + 1))
    printf '%s\n' "$1" >"$work/one/step-$first_step"
    await_ack "$work/one" "$first_step"
}

drive_second() {
    second_step=$((second_step + 1))
    printf '%s\n' "$1" >"$work/two/step-$second_step"
    await_ack "$work/two" "$second_step"
}

await_ack() {
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if [ -f "$1/ack-$2" ]; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "drive-$1-$2"
    return 0
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

await_lines() {
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if [ "$(grep -c . "$log" 2>/dev/null || echo 0)" -ge "$1" ]; then
            sleep 0.3
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "await-lines-$1"
    return 1
}

attach() {
    env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
        TERM=xterm-256color \
        python3 "$HOME/pty-drive.py" "$work/$1" 80 24 \
        "$binary" $prefix_args attach-session -t "=$session" \
        >"$work/attach-$1.out" 2>&1 &
    echo $!
}

# The two client ttys are the only stable identity across the binaries: zz names
# a pty client by its tty exactly as the pin does, while session_attached_list
# and the client name a device reports are not comparable.
labelled() {
    sed "s|$first_tty|A|g; s|$second_tty|B|g" "$log" | tr '\n' ';'
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session" -x 80 -y 24
pane="$(main_client list-panes -t "=$session" -F '#{pane_id}' | sed -n '1p')"
main_client set-hook -g client-resized \
    "run-shell 'sh ~/format-hook-log.sh $log resized h:#{hook_client} t:#{client_tty} c:#{client_width}x#{client_height} w:#{window_width}x#{window_height}'"
main_client set-hook -g client-active \
    "run-shell 'sh ~/format-hook-log.sh $log active h:#{hook_client} t:#{client_tty}'"

# refresh-client -C on a control client resizes the window and raises neither
# hook, repeat invocations included.
printf 'refresh-client -C 120,40\n' | control_client >/dev/null 2>&1 || true
printf 'refresh-client -C 120,40\n' | control_client >/dev/null 2>&1 || true
await_clients 0 || { echo "client-resized-context-$side: control-detach"; exit 0; }
check_equal control-refresh-silent '' "$(cat "$log")"
check_equal control-refresh-resized 120x40 \
    "$(main_client display-message -p -t "$pane" '#{window_width}x#{window_height}')"

first_pid="$(attach one)"
await_clients 1 || { echo "client-resized-context-$side: attach-one"; exit 0; }
first_tty="$(main_client list-clients -t "=$session" -F '#{client_tty}' | sed -n '1p')"
: >"$log"

# One client, changed report: the hook fires once, the client size it reads is
# the new one and the window geometry is the one the resize replaced.
before="$(main_client display-message -p -t "$pane" '#{window_width}x#{window_height}')"
drive_first "size 100 30"
await_lines 1 || { echo "client-resized-context-$side: resize-one"; exit 0; }
check_equal single-changed-count 1 "$(grep -c resized "$log")"
check_equal single-changed-client c:100x30 "$(sed -n '1s/.* \(c:[^ ]*\).*/\1/p' "$log")"
check_equal single-changed-window "w:$before" "$(sed -n '1s/.* \(w:[^ ]*\).*/\1/p' "$log")"

# An unchanged report emits nothing at all.
: >"$log"
drive_first "size 100 30"
sleep 0.8
check_equal single-unchanged '' "$(cat "$log")"

# A second changed report is again one line, one resize behind.
before="$(main_client display-message -p -t "$pane" '#{window_width}x#{window_height}')"
drive_first "size 61 19"
await_lines 1 || { echo "client-resized-context-$side: resize-one-again"; exit 0; }
check_equal single-second-count 1 "$(grep -c resized "$log")"
check_equal single-second-client c:61x19 "$(sed -n '1s/.* \(c:[^ ]*\).*/\1/p' "$log")"
check_equal single-second-window "w:$before" "$(sed -n '1s/.* \(w:[^ ]*\).*/\1/p' "$log")"

drive_first "size 80 24"
sleep 0.5

second_pid="$(attach two)"
await_clients 2 || { echo "client-resized-context-$side: attach-two"; exit 0; }
second_tty="$(main_client list-clients -t "=$session" -F '#{client_tty}' \
    | grep -v "^$first_tty$" | sed -n '1p')"
: >"$log"

# cmd_find_best_client compares activity_time and the resize arm never touches
# it, so the hook body reads the other client while hook_client names the
# reporter, and the client-active the resize provokes precedes it.
drive_first "size 100 30"
await_lines 2 || { echo "client-resized-context-$side: resize-older"; exit 0; }
check_equal older-resize 'active h:A t:B;resized h:A t:B c:80x24 w:;' \
    "$(labelled | sed 's/w:[0-9]*x[0-9]*/w:/')"

# One key in the older client makes it the activity winner, and the next resize
# of the other client reads it instead.
: >"$log"
drive_first "keys 61"
sleep 0.5
: >"$log"
drive_second "size 90 28"
await_lines 2 || { echo "client-resized-context-$side: resize-newer"; exit 0; }
check_equal newer-resize 'active h:B t:A;resized h:B t:A c:100x30 w:;' \
    "$(labelled | sed 's/w:[0-9]*x[0-9]*/w:/')"

if [ "$check_count" -ne 11 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g CLIENT_RESIZED_CONTEXT clean:11
else
    sed "s/^/client-resized-context-$side: /" "$work/failures"
fi
