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

session=message-aliases
work="$HOME/display-message-client-aliases-work-$side"
rm -rf "$work"
mkdir -p "$work"
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
    main_client kill-session -t "=$session" >/dev/null 2>&1
    main_client set-environment -gu DM_CHAIN >/dev/null 2>&1
    exit "$cleanup_status"
}
trap cleanup EXIT

probe() {
    rc=0
    main_client display-message -p -t "$1" 'S=#{session_name}|W=#{window_index}|P=#{pane_id}' \
        >"$work/out" 2>"$work/err" || rc=$?
    printf '%s' "$rc"
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session"
window="$(main_client list-windows -t "=$session" -F '#{window_index}' | sed -n '1p')"
pane="$(main_client list-panes -t "=$session" -F '#{pane_id}' | sed -n '1p')"

# cmd_find_target reads @, {active} and {current} off cmdq_get_client(item),
# whose session is NULL for a command client, so all three are a loud miss that
# still runs the command under CMD_FIND_CANFAIL.
for alias in '@' '{active}' '{current}'; do
    rc="$(probe "$alias")"
    check_equal "loud-$alias-exit" 1 "$rc"
    check_equal "loud-$alias-stderr" "no current client" "$(cat "$work/err")"
    check_equal "loud-$alias-stdout" 'S=|W=|P=' "$(cat "$work/out")"
done

# The whole-target aliases that are not client-scoped stay quiet misses.
for alias in '{last}' '{marked}' '~'; do
    rc="$(probe "$alias")"
    check_equal "quiet-$alias-exit" 0 "$rc"
    check_equal "quiet-$alias-stderr" "" "$(cat "$work/err")"
    check_equal "quiet-$alias-stdout" 'S=|W=|P=' "$(cat "$work/out")"
done

# In a componentwise target the same spelling is a pane slot, not a client
# alias: cmd_find_get_pane_with_window misses, cmd_find_get_window resolves the
# window instead, and fs->wp falls back to that window's active pane.
rc="$(probe "$session:$window.@")"
check_equal componentwise-exit 0 "$rc"
check_equal componentwise-stderr "" "$(cat "$work/err")"
check_equal componentwise-stdout "S=$session|W=$window|P=$pane" "$(cat "$work/out")"

# The diagnostic does not stop the rest of the sequence.
main_client set-environment -g DM_CHAIN pending
chain_rc=0
main_client display-message -p -t '@' 'S=#{session_name}' \; \
    set-environment -g DM_CHAIN after >"$work/chain.out" 2>"$work/chain.err" || chain_rc=$?
check_equal chain-exit 1 "$chain_rc"
check_equal chain-stdout 'S=' "$(cat "$work/chain.out")"
check_equal chain-stderr "no current client" "$(cat "$work/chain.err")"
chain_value="$(main_client show-environment -g DM_CHAIN 2>/dev/null || true)"
check_equal chain-continued after "${chain_value#DM_CHAIN=}"

if [ "$check_count" -ne 25 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g DISPLAY_MESSAGE_CLIENT_ALIASES clean:25
else
    sed "s/^/display-message-client-aliases-$side: /" "$work/failures"
fi
