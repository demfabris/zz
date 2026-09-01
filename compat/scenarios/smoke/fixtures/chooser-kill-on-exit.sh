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

session=chooser-kill
work="$HOME/chooser-kill-on-exit-work-$side"
steps="$work/steps"
rm -rf "$work"
mkdir -p "$steps"
: >"$work/failures"
failed=0
check_count=0
step=0
attach_pid=""
client=""

record_failure() {
    failed=1
    echo "$1" >>"$work/failures"
}

check_equal() {
    check_count=$((check_count + 1))
    if [ "$2" != "$3" ]; then
        record_failure "$1"
    fi
}

cleanup() {
    cleanup_status=$?
    trap - EXIT
    set +e
    step=$((step + 1))
    echo quit >"$steps/step-$step" 2>/dev/null
    main_client kill-session -t "=$session" >/dev/null 2>&1
    if [ -n "$attach_pid" ]; then
        kill "$attach_pid" >/dev/null 2>&1
        wait "$attach_pid" >/dev/null 2>&1
    fi
    main_client set-environment -gu CHOOSER_KILL_RAN >/dev/null 2>&1
    main_client unbind-key -n C-o >/dev/null 2>&1
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
    return 0
}

panes_here() {
    main_client list-panes -t "=$session:0" -F '#{pane_id}' 2>/dev/null | tr '\n' ' '
}

attach_client() {
    env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
        TERM=xterm-256color \
        python3 "$HOME/pty-drive.py" "$steps" 80 24 \
        "$binary" $prefix_args attach-session -t "=$session" \
        >>"$work/attach.out" 2>&1 &
    attach_pid=$!
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        client="$(main_client list-clients -t "=$session" -F '#{client_name}' | sed -n '1p')"
        if [ -n "$client" ]; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure attach-client
    return 1
}

# Restore the two-pane source window so every probe starts from the same shape
# without disturbing the client attached to the session.
reset_window() {
    count="$(main_client list-panes -t "=$session:0" -F '#{pane_id}' | wc -l)"
    while [ "$count" -lt 2 ]; do
        main_client split-window -t "=$session:0" -d
        count=$((count + 1))
    done
    while [ "$count" -gt 2 ]; do
        last="$(main_client list-panes -t "=$session:0" -F '#{pane_id}' | sed -n "${count}p")"
        main_client kill-pane -t "$last"
        count=$((count - 1))
    done
    main_client select-pane -t "=$session:0.0"
    sleep 0.3
    source_pane="$(main_client list-panes -t "=$session:0" -F '#{pane_id}' | sed -n '1p')"
    other_pane="$(main_client list-panes -t "=$session:0" -F '#{pane_id}' | sed -n '2p')"
}

mine="#{==:#{session_name},$session}"

# Open the chooser with C-o, press one key inside it, and report which panes the
# invoking window still has once the chooser is gone.
probe() {
    label="$1"
    key="$2"
    expected="$3"
    drive "keys 0f"
    sleep 0.8
    drive "keys $key"
    sleep 1.0
    check_equal "$label" "$expected" "$(panes_here)"
}


main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session"
reset_window
attach_client || exit 0
sleep 0.5
reset_window

# window_pane_reset_mode kills the pane the mode was entered on when the mode
# carried -k, whichever way the chooser is left.
main_client bind-key -n C-o choose-tree -k -f "$mine"
probe kill-on-cancel 71 "$other_pane "
reset_window
main_client bind-key -n C-o choose-tree -k -f "$mine" \
    'set-environment -g CHOOSER_KILL_RAN yes'
main_client set-environment -g CHOOSER_KILL_RAN no
probe kill-on-enter 0d "$other_pane "
check_equal kill-on-enter-template "CHOOSER_KILL_RAN=yes" \
    "$(main_client show-environment -g CHOOSER_KILL_RAN 2>/dev/null || true)"
reset_window
main_client bind-key -n C-o choose-tree -k -f "$mine"
probe kill-on-shortcut 30 "$other_pane "

# Without -k the same exits leave the pane alone.
reset_window
main_client bind-key -n C-o choose-tree -f "$mine"
probe keep-on-cancel 71 "$source_pane $other_pane "
reset_window
main_client bind-key -n C-o choose-tree -f "$mine"
probe keep-on-enter 0d "$source_pane $other_pane "

# The buffer chooser closes once its last buffer is deleted, and -k kills the
# pane it was entered on when it does.
reset_window
for name in $(main_client list-buffers -F '#{buffer_name}' 2>/dev/null); do
    main_client delete-buffer -b "$name" >/dev/null 2>&1 || true
done
main_client set-buffer -b zzkillbuf only
main_client bind-key -n C-o choose-buffer -k
probe kill-on-empty-after-delete 64 "$other_pane "
check_equal empty-after-delete-buffers "" "$(main_client list-buffers 2>/dev/null || true)"

# Detaching the client is not leaving the chooser, so the pane survives.
reset_window
main_client bind-key -n C-o choose-tree -k -f "$mine"
drive "keys 0f"
sleep 0.8
main_client detach-client -s "=$session" >/dev/null 2>&1 || true
sleep 1.0
check_equal detach-keeps-pane "$source_pane $other_pane " "$(panes_here)"

if [ "$check_count" -ne 9 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g CHOOSER_KILL_ON_EXIT clean:9
else
    sed "s/^/chooser-kill-on-exit-$side: /" "$work/failures"
fi
