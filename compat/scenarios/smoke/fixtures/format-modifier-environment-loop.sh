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

session=envloop
work="$HOME/format-modifier-environment-loop-work-$side"
rm -rf "$work"
mkdir -p "$work/one"
: >"$work/failures"
failed=0
check_count=0
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
    main_client kill-session -t "=$session" >/dev/null 2>&1
    if [ -n "$attach_pid" ]; then
        kill "$attach_pid" >/dev/null 2>&1
        wait "$attach_pid" >/dev/null 2>&1
    fi
    exit "$cleanup_status"
}
trap cleanup EXIT

expand() {
    main_client display-message -p -t "$session" "$1"
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

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session"

main_client set-environment -t "$session" ENVLOOP_PLAIN kept
main_client set-environment -t "$session" ENVLOOP_EMPTY ''
main_client set-environment -t "$session" -h ENVLOOP_HIDDEN secret
main_client set-environment -t "$session" -r ENVLOOP_REMOVED
main_client set-environment -g ENVLOOP_GLOBAL global
main_client set-environment -g ENVLOOP_COLLIDE from-global
main_client set-environment -t "$session" ENVLOOP_COLLIDE from-session

mine='#{V:#{?#{m:ENVLOOP_*,#{environ_name}},<#{environ_name}=#{environ_value}:#{environ_hidden}#{environ_removed}>,}}'
mine_global='#{Vg:#{?#{m:ENVLOOP_*,#{environ_name}},<#{environ_name}=#{environ_value}>,}}'
mine_session='#{Vs:#{?#{m:ENVLOOP_*,#{environ_name}},<#{environ_name}=#{environ_value}>,}}'

# The session store keeps every entry it holds, in name order, with hidden and
# removed rows intact and an empty value distinct from a removed one.
check_equal session-rows \
    '<ENVLOOP_COLLIDE=from-session:00><ENVLOOP_EMPTY=:00><ENVLOOP_HIDDEN=secret:10><ENVLOOP_PLAIN=kept:00><ENVLOOP_REMOVED=:01>' \
    "$(expand "$mine")"
check_equal session-flag \
    '<ENVLOOP_COLLIDE=from-session><ENVLOOP_EMPTY=><ENVLOOP_HIDDEN=secret><ENVLOOP_PLAIN=kept><ENVLOOP_REMOVED=>' \
    "$(expand "$mine_session")"

# show-environment hides the hidden entry, the loop does not.
check_equal hidden-not-listed '' \
    "$(main_client show-environment -t "$session" | grep '^ENVLOOP_HIDDEN' || true)"

# The global store is a different object, and a colliding name reads its own.
check_equal global-rows \
    '<ENVLOOP_COLLIDE=from-global><ENVLOOP_GLOBAL=global>' \
    "$(expand "$mine_global")"
check_equal collide-session from-session \
    "$(expand '#{V:#{?#{==:#{environ_name},ENVLOOP_COLLIDE},#{environ_value},}}')"
check_equal collide-global from-global \
    "$(expand '#{Vg:#{?#{==:#{environ_name},ENVLOOP_COLLIDE},#{environ_value},}}')"

# Row cardinality matches the store the flag word names.
session_rows="$(expand '#{n:#{V:x}}')"
check_equal session-count "$session_rows" "$(expand '#{n:#{Vs:x}}')"
check_equal global-differs-from-session skip skip
if [ "$session_rows" = "$(expand '#{n:#{Vg:x}}')" ]; then
    record_failure global-count-matches-session
fi

# The pin compares the whole flag word, so a combination selects nothing.
for flags in z gs sg sc gc S G C gg cs; do
    check_equal "flagword-$flags" '[]' "$(expand "[#{V$flags:x}]")"
done

# The client store is the store of the client that ran the command, so the
# variable exported into this invocation shows up and the session's own names do
# not. Attaching somebody else does not change that.
client_probe='#{Vc:#{?#{==:#{environ_name},ENVLOOP_CLIENT},<#{environ_value}>,}}'
check_equal client-invoking '<invoking>' \
    "$(ENVLOOP_CLIENT=invoking expand "$client_probe")"
check_equal client-not-session '' \
    "$(ENVLOOP_CLIENT=invoking expand '#{V:#{?#{==:#{environ_name},ENVLOOP_CLIENT},<#{environ_value}>,}}')"
check_equal client-not-global '' \
    "$(ENVLOOP_CLIENT=invoking expand '#{Vg:#{?#{==:#{environ_name},ENVLOOP_CLIENT},<#{environ_value}>,}}')"
check_equal client-lacks-session-name '' \
    "$(expand '#{Vc:#{?#{==:#{environ_name},ENVLOOP_PLAIN},<#{environ_value}>,}}')"
check_equal client-unset '' "$(expand "$client_probe")"

env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    TERM=xterm-256color ENVLOOP_CLIENT=attached \
    python3 "$HOME/pty-drive.py" "$work/one" 80 24 \
    "$binary" $prefix_args attach-session -t "=$session" \
    >"$work/attach.out" 2>&1 &
attach_pid=$!
await_clients 1 || { echo "format-modifier-environment-loop-$side: attach"; exit 0; }

# The attached client's own variable stays out of the invoking client's store.
check_equal client-attached-not-selected '' "$(expand "$client_probe")"
check_equal client-attached-in-loop '<attached>' \
    "$(expand '#{L:#{Vc:#{?#{==:#{environ_name},ENVLOOP_CLIENT},<#{environ_value}>,}}}')"

# Nesting: a window loop inside a row keeps the outer session, and environment
# loops nest without disturbing the outer row.
check_equal nested-window "$session_rows" "$(expand '#{n:#{V:#{W:x}}}')"
check_equal nested-inner \
    '[inENVLOOP_PLAIN]' \
    "$(expand '#{V:#{?#{==:#{environ_name},ENVLOOP_PLAIN},[#{V:#{?#{==:#{environ_name},ENVLOOP_PLAIN},in,}}#{environ_name}],}}')"
check_equal empty-body '' "$(expand '#{V:}')"

# The row formats answer nowhere else.
check_equal no-leak '[|||]' \
    "$(expand '[#{environ_name}|#{environ_value}|#{environ_hidden}|#{environ_removed}]')"

kill "$attach_pid" >/dev/null 2>&1 || true
wait "$attach_pid" >/dev/null 2>&1 || true
attach_pid=""
await_clients 0 || { echo "format-modifier-environment-loop-$side: detach"; exit 0; }
check_equal client-loop-detached '' \
    "$(expand '#{L:#{Vc:#{?#{==:#{environ_name},ENVLOOP_CLIENT},<#{environ_value}>,}}}')"

if [ "$check_count" -ne 30 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g FORMAT_MODIFIER_ENVIRONMENT_LOOP clean:30
else
    sed "s/^/format-modifier-environment-loop-$side: /" "$work/failures"
fi
