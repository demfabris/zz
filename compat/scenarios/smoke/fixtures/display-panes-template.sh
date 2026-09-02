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

session=panes-template
work="$HOME/display-panes-template-work-$side"
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
    for name in DP_CHOSEN DP_AFTER DP_DEFAULT DP_BACKGROUND DP_TIMEOUT; do
        main_client set-environment -gu "$name" >/dev/null 2>&1
    done
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

await_pid() {
    attempt=0
    while [ "$attempt" -lt 300 ]; do
        if ! kill -0 "$1" 2>/dev/null; then
            wait "$1" >/dev/null 2>&1 || true
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "$2"
    kill "$1" >/dev/null 2>&1
    wait "$1" >/dev/null 2>&1 || true
    return 0
}

value() {
    row="$(main_client show-environment -g "$1" 2>/dev/null || true)"
    printf '%s' "${row#"$1"=}"
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session"
main_client split-window -t "=$session:"
pane0="$(main_client list-panes -t "=$session" -F '#{pane_id}' | sed -n '1p')"
pane1="$(main_client list-panes -t "=$session" -F '#{pane_id}' | sed -n '2p')"

env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    TERM=xterm-256color \
    python3 "$HOME/pty-drive.py" "$steps" 80 24 \
    "$binary" $prefix_args attach-session -t "=$session" \
    >"$work/attach.out" 2>&1 &
attach_pid=$!

attempt=0
while [ "$attempt" -lt 400 ]; do
    client="$(main_client list-clients -t "=$session" -F '#{client_tty}' | sed -n '1p')"
    if [ -n "$client" ]; then
        break
    fi
    attempt=$((attempt + 1))
    sleep 0.05
done
if [ -z "$client" ]; then
    record_failure attach-client
    echo "display-panes-template-$side: attach-client"
    exit 0
fi
sleep 0.5

# cmd_display_panes_key substitutes the chosen pane's %id for %%% and runs the
# result, and CMD_RETURN_WAIT keeps the rest of the chain behind the overlay.
main_client set-environment -g DP_CHOSEN pending
main_client set-environment -g DP_AFTER pending
rm -f "$work/chosen.exit"
(
    main_client display-panes -t "$client" -d 0 \
        "set-environment -g DP_CHOSEN 'chose-%%%'" \; \
        set-environment -g DP_AFTER after
    echo "$?" >"$work/chosen.exit"
) &
chosen_pid=$!
sleep 1.0
check_equal blocks-the-template pending "$(value DP_CHOSEN)"
check_equal blocks-the-chain pending "$(value DP_AFTER)"
drive "keys 31"
sleep 1.0
await_pid "$chosen_pid" chosen-parked
check_equal template-exit 0 "$(cat "$work/chosen.exit" 2>/dev/null)"
check_equal template-substituted "chose-$pane1" "$(value DP_CHOSEN)"
check_equal template-chain-continued after "$(value DP_AFTER)"

# The omitted template is `select-pane -t "%%%"`.
main_client select-pane -t "$pane0"
main_client display-panes -b -t "$client" -d 0
sleep 0.4
drive "keys 31"
sleep 0.8
check_equal default-template-selects "$pane1" \
    "$(main_client list-panes -t "=$session" -F '#{?pane_active,#{pane_id},}' | tr -d '\n')"

# -b returns before the overlay closes.
main_client select-pane -t "$pane0"
main_client set-environment -g DP_BACKGROUND pending
bg_rc=0
main_client display-panes -b -t "$client" -d 0 \
    "set-environment -g DP_BACKGROUND 'bg-%%%'" || bg_rc=$?
check_equal background-exit 0 "$bg_rc"
check_equal background-returned-early pending "$(value DP_BACKGROUND)"
drive "keys 31"
sleep 0.8
check_equal background-template-ran "bg-$pane1" "$(value DP_BACKGROUND)"

# A template that fails at run time closes the overlay, leaves the chosen pane
# alone, and lets the rest of the chain follow. Its exit status is not asserted:
# the pin's failure rides the issuing queue, which zz has no equivalent for.
main_client select-pane -t "$pane0"
main_client set-environment -g DP_AFTER pending
(
    main_client display-panes -t "$client" -d 0 'kill-pane -t nosuchpanename' \; \
        set-environment -g DP_AFTER after
) >/dev/null 2>&1 &
failing_pid=$!
sleep 1.0
drive "keys 31"
sleep 0.8
await_pid "$failing_pid" failing-parked
check_equal failing-chain-continued after "$(value DP_AFTER)"
check_equal failing-left-the-panes-alone "$pane0 $pane1" \
    "$(main_client list-panes -t "=$session" -F '#{pane_id}' | tr '\n' ' ' | sed 's/ $//')"

# The overlay's own timer closes it and releases the parked chain with the
# template unrun.
main_client set-environment -g DP_TIMEOUT pending
main_client set-environment -g DP_AFTER pending
rm -f "$work/timeout.exit"
(
    main_client display-panes -t "$client" -d 700 \
        "set-environment -g DP_TIMEOUT 'timed-%%%'" \; \
        set-environment -g DP_AFTER after
    echo "$?" >"$work/timeout.exit"
) &
timeout_pid=$!
sleep 0.3
check_equal timeout-still-parked "" "$(cat "$work/timeout.exit" 2>/dev/null)"
sleep 1.4
await_pid "$timeout_pid" timeout-parked
check_equal timeout-exit 0 "$(cat "$work/timeout.exit" 2>/dev/null)"
check_equal timeout-ran-nothing pending "$(value DP_TIMEOUT)"
check_equal timeout-chain-continued after "$(value DP_AFTER)"

# -N drops the key handler but keeps the wait, so only the timer ends it.
main_client set-environment -g DP_TIMEOUT pending
rm -f "$work/silent.exit"
(
    main_client display-panes -N -t "$client" -d 700 \
        "set-environment -g DP_TIMEOUT 'silent-%%%'"
    echo "$?" >"$work/silent.exit"
) &
silent_pid=$!
sleep 0.3
drive "keys 31"
sleep 1.4
await_pid "$silent_pid" silent-parked
check_equal silent-exit 0 "$(cat "$work/silent.exit" 2>/dev/null)"
check_equal silent-ran-nothing pending "$(value DP_TIMEOUT)"

if [ "$check_count" -ne 17 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g DISPLAY_PANES_TEMPLATE clean:17
else
    sed "s/^/display-panes-template-$side: /" "$work/failures"
fi
