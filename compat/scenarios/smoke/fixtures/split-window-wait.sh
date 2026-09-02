#!/bin/sh
# `split-window -W` parks the queue item that made the pane until the pane's
# command exits, then hands an unattached client that command's exit status, or
# 128 plus its signal, and only then lets the rest of the queue run.
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

session=splitwait
work="$HOME/split-window-wait-$side"
rm -rf "$work"
mkdir -p "$work"

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session" >/dev/null 2>&1

panes() {
    main_client list-panes -t "=$session:" -F '#{pane_id}' | tr '\n' ' ' | sed 's/ $//'
}

run_case() {
    label="$1"
    shift
    out="$work/$label.out"
    err="$work/$label.err"
    set +e
    main_client "$@" >"$out" 2>"$err"
    rc=$?
    set -e
    printf '%s rc=%s panes=[%s]\n' "$label" "$rc" "$(panes)"
    sed "s/^/$label | /" "$out"
    sed "s/^/$label ! /" "$err"
}

run_case exit-three split-window -d -W -t "=$session:" 'exit 3'
run_case success split-window -d -W -t "=$session:" true
run_case signal split-window -d -W -t "=$session:" 'kill -TERM $$'
run_case printed split-window -d -P -F '#{pane_index}' -W -t "=$session:" 'exit 5'

# A non-zero status keeps the item's after hook: the hook was inserted at exec
# and window_pane_wait_finish only sets c->retval, so the hook runs, the rest of
# the queue runs, and only the client's status carries the child's.
main_client set-hook -g after-split-window 'display-message -p HOOK'
main_client set-hook -g command-error 'display-message -p CMDERR'
run_case hooked split-window -d -P -W -t "=$session:" 'exit 2' \; display-message -p AFTER
main_client set-hook -gu after-split-window
main_client set-hook -gu command-error

# The rest of the queue runs after the pane's command is gone, so the marker the
# pane writes is already there when the next command in the same queue prints.
marker="$work/ordered"
rm -f "$marker"
set +e
main_client split-window -d -W -t "=$session:" "sleep 1; touch '$marker'" \; \
    display-message -p AFTER >"$work/ordered.out" 2>"$work/ordered.err"
rc=$?
set -e
printf 'ordered rc=%s marker=%s\n' "$rc" "$([ -e "$marker" ] && echo present || echo missing)"
sed 's/^/ordered | /' "$work/ordered.out"
sed 's/^/ordered ! /' "$work/ordered.err"

# While the item is parked the pane is alive and the client has not returned.
rm -f "$work/parked.exit"
(
    set +e
    main_client split-window -d -W -t "=$session:" 'sleep 1' >/dev/null 2>&1
    echo "$?" >"$work/parked.exit"
) &
parked_pid=$!
sleep 0.4
printf 'parked returned=[%s] panes=%s\n' \
    "$(cat "$work/parked.exit" 2>/dev/null)" \
    "$(panes | tr ' ' '\n' | grep -c .)"
wait "$parked_pid" 2>/dev/null || true
printf 'parked-after rc=%s panes=[%s]\n' "$(cat "$work/parked.exit" 2>/dev/null)" "$(panes)"

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
