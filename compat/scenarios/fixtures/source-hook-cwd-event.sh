#!/bin/sh
set -eu

export LC_ALL=C

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    side=zz
    main_client() {
        "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" "$@"
    }
    control_client() {
        "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" -C "$@" </dev/null
    }
else
    side=tmux
    main_client() {
        "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" "$@"
    }
    control_client() {
        "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" -C "$@" </dev/null
    }
fi

relative=compat/scenarios/source-hook-cwd-event/leaf.conf
work="$HOME/source-hook-cwd-event-work-$side"
rm -rf "$work"
mkdir -p "$work/other-client/$(dirname "$relative")"
mkdir -p "$work/session-cwd/$(dirname "$relative")"
: >"$work/failures"
failed=0
check_count=0

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
    for hook in session-created window-linked pane-exited; do
        main_client set-hook -gu "$hook" >/dev/null 2>&1
    done
    main_client set-option -gu @source_hook_cwd_event >/dev/null 2>&1
    for session in event-plain event-control event-window event-pane; do
        main_client kill-session -t "=$session" >/dev/null 2>&1
    done
    exit "$cleanup_status"
}
trap cleanup EXIT

mkdir -p "$HOME/$(dirname "$relative")"
printf 'set-option -g @source_hook_cwd_event daemon-home\n' >"$HOME/$relative"
printf 'set-option -g @source_hook_cwd_event other-client-decoy\n' \
    >"$work/other-client/$relative"
printf 'set-option -g @source_hook_cwd_event session-cwd-decoy\n' \
    >"$work/session-cwd/$relative"

await_selection() {
    attempt=0
    while [ "$attempt" -lt 200 ]; do
        value="$(main_client show-options -gv @source_hook_cwd_event)"
        if [ "$value" != "pending" ]; then
            printf '%s' "$value"
            return
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    printf 'timeout'
}

arm() {
    main_client set-hook -g "$1" "source-file $relative"
    main_client set-option -g @source_hook_cwd_event pending
}

disarm() {
    main_client set-hook -gu "$1"
}

arm session-created
(cd "$work/other-client" && main_client new-session -d -s event-plain \
    -c "$work/session-cwd")
check_equal session-created daemon-home "$(await_selection)"
main_client kill-session -t =event-plain

main_client set-option -g @source_hook_cwd_event pending
(cd "$work/other-client" && control_client new-session -d -s event-control \
    -c "$work/session-cwd" >/dev/null 2>&1)
check_equal session-created-control daemon-home "$(await_selection)"
main_client kill-session -t =event-control
disarm session-created

main_client new-session -d -s event-window -c "$work/session-cwd"
arm window-linked
(cd "$work/other-client" && main_client new-window -d -t =event-window)
check_equal window-linked daemon-home "$(await_selection)"
disarm window-linked
main_client kill-session -t =event-window

main_client new-session -d -s event-pane -c "$work/session-cwd"
arm pane-exited
(cd "$work/other-client" && main_client split-window -d -t =event-pane:0 true)
check_equal pane-exited daemon-home "$(await_selection)"
disarm pane-exited
main_client kill-session -t =event-pane

if [ "$check_count" -ne 4 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g SOURCE_HOOK_CWD_EVENT clean:4
else
    sed "s/^/source-hook-cwd-event-$side: /" "$work/failures"
fi
