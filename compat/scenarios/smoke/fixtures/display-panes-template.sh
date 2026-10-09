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
    for name in DP_CHOSEN DP_AFTER DP_DEFAULT DP_BACKGROUND DP_TIMEOUT DP_SECOND; do
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

mode_of() {
    main_client display-message -p -t "$1" '#{pane_mode}:#{window_zoomed_flag}' 2>/dev/null || true
}

await_mode() {
    attempt=0
    while [ "$attempt" -lt 100 ]; do
        if [ "$(mode_of "$2")" = "$3" ]; then
            break
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    check_equal "$1" "$3" "$(mode_of "$2")"
}

# 3.8 (1a02c995) makes display-panes a mode of the target pane: the command
# returns at once, so the rest of the chain runs before any key, the pane is
# zoomed while the mode is up, and window_panes_key substitutes the chosen
# pane's %id for %%% when a digit selects it.
main_client select-pane -t "$pane0"
main_client set-environment -g DP_CHOSEN pending
main_client set-environment -g DP_AFTER pending
chosen_rc=0
main_client display-panes -d 0 \
    "set-environment -g DP_CHOSEN 'chose-%%%'" \; \
    set-environment -g DP_AFTER after || chosen_rc=$?
check_equal template-exit 0 "$chosen_rc"
check_equal template-chain-ran-at-once after "$(value DP_AFTER)"
check_equal template-waits-for-a-key pending "$(value DP_CHOSEN)"
await_mode template-zoomed-mode "$pane0" panes-mode:1
drive "keys 31"
await_mode template-mode-closed "$pane0" :0
check_equal template-substituted "chose-$pane1" "$(value DP_CHOSEN)"

# window_pane_set_mode returns at once when the pane already shows this mode,
# so a second display-panes neither replaces the first nor restarts it, and the
# key still runs the first template.
main_client select-pane -t "$pane0"
main_client set-environment -g DP_CHOSEN pending
main_client set-environment -g DP_SECOND pending
main_client display-panes -d 0 "set-environment -g DP_CHOSEN 'first-%%%'"
second_rc=0
main_client display-panes -d 0 \
    "set-environment -g DP_SECOND 'second-%%%'" || second_rc=$?
check_equal second-exit 0 "$second_rc"
await_mode second-left-the-first "$pane0" panes-mode:1
drive "keys 31"
await_mode first-mode-closed "$pane0" :0
check_equal first-template-ran "first-$pane1" "$(value DP_CHOSEN)"
check_equal second-template-unrun pending "$(value DP_SECOND)"

# The omitted template is `select-pane -t "%%%"`.
main_client select-pane -t "$pane0"
main_client display-panes -d 0
await_mode default-mode "$pane0" panes-mode:1
drive "keys 31"
await_mode default-mode-closed "$pane0" :0
check_equal default-template-selects "$pane1" \
    "$(main_client list-panes -t "=$session" -F '#{?pane_active,#{pane_id},}' | tr -d '\n')"

# -b went with the client overlay.
background_rc=0
main_client display-panes -b -d 0 >/dev/null 2>"$work/background.err" || background_rc=$?
check_equal background-refused 1 "$background_rc"
check_equal background-error "command display-panes: unknown flag -b" \
    "$(tr -d '\r' <"$work/background.err")"

# A template that fails at run time ends the mode and leaves the panes alone.
main_client select-pane -t "$pane0"
main_client display-panes -d 0 'kill-pane -t nosuchpanename'
await_mode failing-mode "$pane0" panes-mode:1
drive "keys 31"
await_mode failing-mode-closed "$pane0" :0
check_equal failing-left-the-panes-alone "$pane0 $pane1" \
    "$(main_client list-panes -t "=$session" -F '#{pane_id}' | tr '\n' ' ' | sed 's/ $//')"

# The mode's own timer ends it with the template unrun, and -Z leaves the
# window unzoomed.
main_client set-environment -g DP_TIMEOUT pending
main_client display-panes -Z -t "$pane0" -d 700 \
    "set-environment -g DP_TIMEOUT 'timed-%%%'"
await_mode timeout-unzoomed-mode "$pane0" panes-mode:0
sleep 1.2
check_equal timeout-closed ":0" "$(mode_of "$pane0")"
check_equal timeout-ran-nothing pending "$(value DP_TIMEOUT)"

# -N swallows a digit instead of choosing, so only q, Escape or the timer end
# the mode.
main_client select-pane -t "$pane0"
main_client display-panes -N -d 0 "set-environment -g DP_TIMEOUT 'silent-%%%'"
await_mode silent-mode "$pane0" panes-mode:1
drive "keys 31"
sleep 0.4
check_equal silent-kept-the-mode panes-mode:1 "$(mode_of "$pane0")"
drive "keys 71"
await_mode silent-closed-by-q "$pane0" :0
check_equal silent-ran-nothing pending "$(value DP_TIMEOUT)"

# A key that is not a pane index ends the mode, and -k kills the pane with it.
main_client select-pane -t "$pane1"
main_client display-panes -k -d 0
await_mode kill-mode "$pane1" panes-mode:1
drive "keys 5a"
sleep 0.6
check_equal kill-removed-the-pane "$pane0" \
    "$(main_client list-panes -t "=$session" -F '#{pane_id}' | tr '\n' ' ' | sed 's/ $//')"

# The mode needs no client: a detached session's pane takes it too, and a
# single pane cannot zoom.
main_client new-session -d -s "$session-detached"
main_client display-panes -t "=$session-detached:" -d 0
check_equal detached-mode panes-mode:0 "$(mode_of "=$session-detached:")"
main_client kill-session -t "=$session-detached"

if [ "$check_count" -ne 29 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g DISPLAY_PANES_TEMPLATE clean:29
else
    sed "s/^/display-panes-template-$side: /" "$work/failures"
fi
