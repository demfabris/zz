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

session=clipsel
work="$HOME/copy-selection-clipboard-bytes-$side"
rm -rf "$work"
mkdir -p "$work"
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
    for pid in $attach_pid; do
        kill "$pid" >/dev/null 2>&1
        wait "$pid" >/dev/null 2>&1
    done
    exit "$cleanup_status"
}
trap cleanup EXIT

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

await_line() {
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if main_client capture-pane -p -t "$pane" 2>/dev/null | grep -q "^$1$"; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "await-line-$1"
    return 1
}

await_mode() {
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        mode="$(main_client display-message -p -t "$pane" '#{pane_mode}' 2>/dev/null || true)"
        if [ "$mode" = "$1" ]; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "await-mode-[$1]"
    return 1
}

scan_from() {
    python3 "$HOME/copy-selection-clipboard-bytes-pty.py" scan "$work/attach.raw" "$1"
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session" -x 80 -y 24 \
    sh -c 'printf "alpha-bravo\n"; exec sleep 600'
main_client set-option -as terminal-features ",xterm-256color:clipboard" >/dev/null 2>&1 || true
pane="$(main_client list-panes -t "=$session" -F '#{pane_id}' | head -n 1)"
await_line alpha-bravo || { echo "copy-selection-clipboard-bytes-$side: pane-text"; exit 0; }

: >"$work/attach.raw"
env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    TERM=xterm-256color \
    python3 "$HOME/copy-selection-clipboard-bytes-pty.py" record "$work/attach.raw" 200 24 \
    "$binary" $prefix_args attach-session -t "=$session" \
    >"$work/attach.out" 2>&1 &
attach_pid=$!
await_clients 1 || { echo "copy-selection-clipboard-bytes-$side: attach"; exit 0; }

# Pinned tmux d77c9dc6: window_copy_copy_buffer writes the selection with
# screen_write_setselection(&ctx, "", ...) — an empty OSC 52 selection field,
# which asks the outer terminal for its default target — and only while
# set-clipboard is not off. Measured on the pin: external and on each emit one
# write with an empty field, off emits none.
#
# Both sides write the empty field: EventPayload::Clipboard carries the
# producer since PROTOCOL_VERSION 99, so the raw TUI writes the pin's empty
# field for every selection the server wrote itself and keeps the named field
# only for an application's own OSC 52, which the second phase below measures.
for spec in \
    'external|selection=<empty> payload=alpha-bravo' \
    'on|selection=<empty> payload=alpha-bravo' \
    'off|'; do
    value="${spec%%|*}"
    want="${spec#*|}"
    main_client set-option -g set-clipboard "$value"
    before="$(wc -c <"$work/attach.raw")"
    main_client copy-mode -t "$pane"
    await_mode copy-mode || true
    main_client send-keys -X -t "$pane" top-line
    main_client send-keys -X -t "$pane" begin-selection
    main_client send-keys -X -t "$pane" end-of-line
    main_client send-keys -X -t "$pane" copy-selection-and-cancel
    await_mode '' || true
    got=""
    attempt=0
    while [ "$attempt" -lt 60 ]; do
        got="$(scan_from "$before")"
        if [ -n "$got" ] && [ -n "$want" ]; then
            break
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    check_equal "selection-field-$value" "$want" "$got"
done

# Pinned tmux d77c9dc6: input_osc_52_parse returns 0 unless
# options_get_number(global_options, "set-clipboard") == 2, so an application
# writing its own ESC ] 52 ; c ; <base64> BEL reaches the outer terminal only
# under set-clipboard on, and under external and off the pin writes nothing and
# creates no buffer. Measured on the pin with this fixture: on emits
# selection=c payload=application and one buffer, external and off emit
# neither.
cat >"$work/emitter.sh" <<EOF
#!/bin/sh
printf 'emitter-ready\n'
while :; do
    if [ -f "$work/emit" ]; then
        rm -f "$work/emit"
        printf '\033]52;c;YXBwbGljYXRpb24=\007'
        printf 'emitted\n'
    fi
    sleep 0.05
done
EOF
chmod +x "$work/emitter.sh"
main_client new-window -n osc -t "=$session" "$work/emitter.sh"
pane="$(main_client list-panes -t "=$session:osc" -F '#{pane_id}' | head -n 1)"
await_line emitter-ready || record_failure "emitter-ready"
for spec in \
    'external|' \
    'on|selection=c payload=application' \
    'off|'; do
    value="${spec%%|*}"
    want="${spec#*|}"
    main_client set-option -g set-clipboard "$value"
    main_client delete-buffer -b oscapp >/dev/null 2>&1 || true
    before="$(wc -c <"$work/attach.raw")"
    buffers_before="$(main_client list-buffers -F x 2>/dev/null | grep -c x || true)"
    : >"$work/emit"
    attempt=0
    got=""
    while [ "$attempt" -lt 60 ]; do
        got="$(scan_from "$before")"
        if [ -n "$got" ] && [ -n "$want" ]; then
            break
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    check_equal "application-osc52-$value" "$want" "$got"
    buffers_after="$(main_client list-buffers -F x 2>/dev/null | grep -c x || true)"
    if [ -n "$want" ]; then
        check_equal "application-osc52-buffer-$value" "$((buffers_before + 1))" "$buffers_after"
    else
        check_equal "application-osc52-buffer-$value" "$buffers_before" "$buffers_after"
    fi
done

if [ "$check_count" -ne 9 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g COPY_SELECTION_CLIPBOARD_BYTES clean:9
else
    sed "s/^/copy-selection-clipboard-bytes-$side: /" "$work/failures"
fi
