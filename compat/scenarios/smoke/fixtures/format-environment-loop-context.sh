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

session=envctx
work="$HOME/format-environment-loop-context-work-$side"
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

# Start from a store this fixture owns outright, so the row positions are exact.
main_client show-environment -t "$session" | sed -e 's/^-//' -e 's/=.*//' |
    while IFS= read -r name; do
        [ -n "$name" ] || continue
        main_client set-environment -t "$session" -u "$name"
    done

main_client set-environment -t "$session" BETA two
main_client set-environment -t "$session" ALPHA one
main_client set-environment -t "$session" GAMMA ''

row='#{V:<#{environ_name}|#{environ_value}|#{environ_hidden}|#{environ_removed}|#{loop_index}|#{loop_last_flag}>}'

# Every row carries its own entry and its own zero-based position, in the store
# order, and an empty value is not a removed one.
check_equal rows \
    '<ALPHA|one|0|0|0|0><BETA|two|0|0|1|0><GAMMA||0|0|2|1>' \
    "$(expand "$row")"

main_client set-environment -t "$session" -u ALPHA
main_client set-environment -t "$session" -u BETA
main_client set-environment -t "$session" -u GAMMA
main_client set-environment -t "$session" -h HIDDEN seen
main_client set-environment -t "$session" -r REMOVED
main_client set-environment -t "$session" EMPTY ''

# A removed entry keeps its name and loses its value; a hidden one keeps both.
check_equal flags \
    '<EMPTY||0|0|0|0><HIDDEN|seen|1|0|1|0><REMOVED||0|1|2|1>' \
    "$(expand "$row")"

# The global store carries the same six formats for its own entries.
main_client set-environment -g ENVCTX_GLOBAL g
check_equal global-row '<ENVCTX_GLOBAL=g:00>' \
    "$(expand '#{Vg:#{?#{==:#{environ_name},ENVCTX_GLOBAL},<#{environ_name}=#{environ_value}:#{environ_hidden}#{environ_removed}>,}}')"

# So does the client store of the connection running the command.
check_equal client-row '<ENVCTX_CLIENT=c:00>' \
    "$(ENVCTX_CLIENT=c expand '#{Vc:#{?#{==:#{environ_name},ENVCTX_CLIENT},<#{environ_name}=#{environ_value}:#{environ_hidden}#{environ_removed}>,}}')"

main_client set-environment -t "$session" -u HIDDEN
main_client set-environment -t "$session" -u REMOVED
main_client set-environment -t "$session" -u EMPTY
main_client set-environment -t "$session" OUTER o
main_client set-environment -t "$session" PLAIN p

# A nested loop owns all six names only for the length of its own rows.
check_equal nested-environment \
    '[OUTER0(OUTER0)(PLAIN1)OUTER0][PLAIN1(OUTER0)(PLAIN1)PLAIN1]' \
    "$(expand '#{V:[#{environ_name}#{loop_index}#{V:(#{environ_name}#{loop_index})}#{environ_name}#{loop_index}]}')"
check_equal nested-window \
    '[OUTER<01>OUTER][PLAIN<01>PLAIN]' \
    "$(expand '#{V:[#{environ_name}#{W:<#{loop_index}#{loop_last_flag}>}#{environ_name}]}')"

# None of the six answer anywhere else.
check_equal no-leak '[|||||]' \
    "$(expand '[#{environ_name}|#{environ_value}|#{environ_hidden}|#{environ_removed}|#{loop_index}|#{loop_last_flag}]')"
check_equal no-leak-after 'xx[]' \
    "$(expand '#{V:x}[#{environ_name}#{environ_removed}#{loop_index}]')"
check_equal conditional 'unsetsetset' \
    "$(expand '#{?environ_name,set,unset}#{V:#{?environ_name,set,unset}}')"

env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    TERM=xterm-256color \
    python3 "$HOME/pty-drive.py" "$work/one" 80 24 \
    "$binary" $prefix_args attach-session -t "=$session" \
    >"$work/attach.out" 2>&1 &
attach_pid=$!
await_clients 1 || { echo "format-environment-loop-context-$side: attach"; exit 0; }

# Inside a client row the session store still answers with its own six formats.
session_names="$(expand '#{V:#{environ_name}}')"
check_equal client-nested "[$session_names]" "$(expand '#{L:[#{V:#{environ_name}}]}')"

if [ "$check_count" -ne 10 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g FORMAT_ENVIRONMENT_LOOP_CONTEXT clean:10
else
    sed "s/^/format-environment-loop-context-$side: /" "$work/failures"
fi
