#!/bin/sh
set -eu

export LC_ALL=en_US.UTF-8

if [ "$1" = producer ]; then
    exec 3<"$2"
    number=1
    while IFS= read -r command <&3; do
        if [ "$command" = replace ]; then
            printf '\033[H\033[2J\033[3JLIVE-REPLACEMENT\r\n'
            continue
        fi
        if [ "$command" = clear ]; then
            printf '\033[3JCLEARED-HISTORY\r\n'
            continue
        fi
        while [ "$number" -le "$command" ]; do
            printf 'F%04d payload-%04d abcdefghijklmnopqrstuvwxyz ABCDEFGHIJKLMNOPQRSTUVWXYZ\r\n' \
                "$number" "$number"
            number=$((number + 1))
        done
        printf 'READY%04d\r\n' "$command"
    done
    exit 0
fi

case_name="$1"
if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    side=zz
    binary="$ZZ_SMOKE_ZZ_BIN"
    socket_flag=--socket
    socket_value="$ZZ_SMOKE_ZZ_SOCKET"
else
    side=tmux
    binary="$ZZ_SMOKE_TMUX_BIN"
    socket_flag=-L
    socket_value="$ZZ_SMOKE_TMUX_LABEL"
fi

main_client() {
    "$binary" "$socket_flag" "$socket_value" "$@"
}

session="cmrevision-$case_name"
work="$HOME/copy-mode-revision-$case_name-$side"
mkdir -p "$work"
: >"$work/failures"
: >"$work/transcript"
viewer_pid=""
check_count=0
failed=0

cleanup() {
    cleanup_status=$?
    trap - EXIT
    set +e
    main_client kill-session -t "=$session" >/dev/null 2>&1
    if [ -n "$viewer_pid" ]; then
        kill "$viewer_pid" >/dev/null 2>&1
        wait "$viewer_pid" >/dev/null 2>&1
    fi
    exit "$cleanup_status"
}
trap cleanup EXIT

record_failure() {
    failed=1
    printf '%s\n' "$1" >>"$work/failures"
}

check_equal() {
    check_count=$((check_count + 1))
    if [ "$2" != "$3" ]; then
        record_failure "$1 want=[$2] got=[$3]"
    fi
}

value() {
    main_client display-message -p -t "$pane" "#{$1}"
}

facts() {
    main_client display-message -p -t "$pane" \
        '#{pane_in_mode}|#{copy_cursor_line}|#{copy_cursor_word}|#{copy_cursor_x}|#{copy_cursor_y}|#{scroll_position}|#{selection_present}|#{selection_start_x}|#{selection_start_y}|#{selection_end_x}|#{selection_end_y}|#{search_present}|#{search_count}|#{search_count_partial}|#{search_match}'
}

await_clients() {
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        count="$(main_client list-clients -t "=$session" -F x 2>/dev/null | grep -c x || true)"
        if [ "$count" = 1 ]; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure attach-timeout
    return 1
}

await_output() {
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if main_client capture-pane -p -t "$pane" 2>/dev/null | grep -q "^$1$"; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "output-timeout-$1"
    return 1
}

await_mode_output() {
    attempt=0
    while [ "$attempt" -lt 100 ]; do
        if main_client capture-pane -M -p -t "$pane" -S - -E 32767 2>/dev/null | grep -q "^$1$"; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "mode-output-timeout-$1"
    return 1
}

await_size() {
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        current="$(main_client display-message -p -t "$pane" '#{pane_width}x#{pane_height}')"
        if [ "$current" = "$1" ]; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "size-timeout-$1"
    return 1
}

finish() {
    if [ "$failed" = 0 ]; then
        main_client set-environment -g "$result_name" "clean:$check_count|$(cat "$work/transcript")"
    else
        sed "s/^/copy-mode-revision-$case_name-$side: /" "$work/failures"
    fi
}

main_client set-option -g status off
if [ "$case_name" = history ]; then
    main_client set-option -g history-limit 32
else
    main_client set-option -g history-limit 10000
fi
main_client set-option -gw mode-keys emacs
main_client set-option -gw copy-mode-position-format ''
mkfifo "$work/commands"
main_client new-session -d -s "$session" -x 80 -y 24 \
    "sh '$HOME/copy-mode-revision.sh' producer '$work/commands'"
pane="$(main_client list-panes -t "=$session" -F '#{pane_id}' | head -n 1)"
exec 3>"$work/commands"

env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    TERM=xterm-256color \
    python3 "$HOME/send-keys-attach.py" record "$work/viewer.raw" 80 24 \
    "$binary" "$socket_flag" "$socket_value" attach-session -t "=$session" \
    >"$work/viewer.out" 2>&1 &
viewer_pid=$!

case "$case_name" in
history) result_name=COPY_MODE_HISTORY_FREEZE ;;
resize) result_name=COPY_MODE_RESIZE_FREEZE ;;
*) exit 2 ;;
esac

await_clients || { finish; exit 0; }
printf '128\n' >&3
await_output READY0128 || { finish; exit 0; }
main_client copy-mode -t "$pane"
if [ "$case_name" = resize ]; then
    main_client send-keys -t "$pane" -X search-backward 'payload-009[0] abcdefghijklmnopqrstuvwxyz'
    main_client send-keys -t "$pane" -N 3 -X cursor-down
    check_equal entry-cursor-is-inside-the-viewport 1 \
        "$(if [ "$(value copy_cursor_y)" -gt 0 ]; then echo 1; else echo 0; fi)"
    frozen_prefix=F0093
else
    main_client send-keys -t "$pane" -X search-backward F0100
    frozen_prefix=F0100
fi
main_client send-keys -t "$pane" -X start-of-line
check_equal entry-word "$frozen_prefix" "$(value copy_cursor_word)"
main_client send-keys -t "$pane" -X begin-selection
main_client send-keys -t "$pane" -N 5 -X cursor-right
check_equal selection-is-live 1 "$(value selection_present)"
entry_facts="$(facts)"

case "$case_name" in
history)
    main_client send-keys -t "$pane" -X refresh-on
    printf '4096\n' >&3
    await_output READY4096 || { finish; exit 0; }
    sleep 0.15
    check_equal output-preserves-revision "$entry_facts" "$(facts)"
    check_equal old-line-was-pruned 0 \
        "$(main_client capture-pane -p -t "$pane" -S - | grep -c '^F0100 ' || true)"
    printf 'clear\n' >&3
    await_output CLEARED-HISTORY || { finish; exit 0; }
    sleep 0.15
    check_equal ed3-preserves-revision "$entry_facts" "$(facts)"
    check_equal ed3-clears-live-history 1 \
        "$(if [ "$(value history_size)" -le 1 ]; then echo 1; else echo 0; fi)"
    main_client send-keys -t "$pane" -X copy-selection-no-clear
    check_equal frozen-copy-bytes F0100 "$(main_client show-buffer)"
    check_equal copy-keeps-frozen-selection "$entry_facts" "$(facts)"
    main_client send-keys -t "$pane" -X refresh-off
    main_client send-keys -t "$pane" -X clear-selection
    frozen_line="$(value copy_cursor_line)"
    main_client send-keys -t "$pane" -X refresh-on
    printf '4097\n' >&3
    await_output READY4097 || { finish; exit 0; }
    await_mode_output READY4097 || { finish; exit 0; }
    main_client send-keys -t "$pane" -X refresh-off
    check_equal refresh-replaces-frozen-line 0 \
        "$(if [ "$(value copy_cursor_line)" = "$frozen_line" ]; then echo 1; else echo 0; fi)"
    main_client send-keys -t "$pane" -X history-bottom
    main_client send-keys -t "$pane" -N 2 -X cursor-up
    main_client send-keys -t "$pane" -X start-of-line
    check_equal refreshed-cursor-reads-new-output F4097 "$(value copy_cursor_word)"
    printf 'copied=F0100|refreshed=%s' "$(value copy_cursor_line)" >>"$work/transcript"
    ;;
resize)
    printf 'replace\n' >&3
    await_output LIVE-REPLACEMENT || { finish; exit 0; }
    check_equal replace-preserves-revision "$entry_facts" "$(facts)"
    for size in 40x16 100x28 80x24; do
        columns="${size%x*}"
        rows="${size#*x}"
        main_client resize-window -t "=$session:0" -x "$columns" -y "$rows"
        await_size "$size" || { finish; exit 0; }
        check_equal "$size-keeps-frozen-word" payload "$(value copy_cursor_word)"
        check_equal "$size-clears-selection" 0 "$(value selection_present)"
        check_equal "$size-anchors-history-cursor-at-the-top" 0 "$(value copy_cursor_y)"
        main_client send-keys -t "$pane" -X start-of-line
        check_equal "$size-keeps-frozen-prefix" "$frozen_prefix" "$(value copy_cursor_word)"
        printf '%s=%s\n' "$size" "$(facts)" >>"$work/transcript"
        main_client send-keys -t "$pane" -N 5 -X cursor-right
    done
    main_client send-keys -t "$pane" -X start-of-line
    main_client send-keys -t "$pane" -X begin-selection
    main_client send-keys -t "$pane" -N 5 -X cursor-right
    main_client send-keys -t "$pane" -X copy-selection-no-clear
    check_equal resized-copy-bytes "$frozen_prefix" "$(main_client show-buffer)"
    main_client send-keys -t "$pane" -X clear-selection
    main_client set-window-option -t "=$session:0" mode-keys vi
    main_client send-keys -t "$pane" -X history-bottom
    main_client send-keys -t "$pane" -X search-backward 'payload-0090 abcdefghijklmnopqrstuvwxyz'
    main_client send-keys -t "$pane" -N 3 -X cursor-down
    check_equal marked-cursor-is-inside-the-viewport 1 \
        "$(if [ "$(value copy_cursor_y)" -gt 0 ]; then echo 1; else echo 0; fi)"
    check_equal marked-search-count 1 "$(value search_count)"
    for size in 40x16 100x28 80x24; do
        columns="${size%x*}"
        rows="${size#*x}"
        main_client resize-window -t "=$session:0" -x "$columns" -y "$rows"
        await_size "$size" || { finish; exit 0; }
        check_equal "$size-marked-history-cursor-at-the-top" 0 "$(value copy_cursor_y)"
        check_equal "$size-recomputes-search-count" 1 "$(value search_count)"
        printf 'marks:%s=%s\n' "$size" "$(facts)" >>"$work/transcript"
    done
    ;;
esac

main_client send-keys -t "$pane" -X cancel
check_equal cancel-leaves-mode 0 "$(value pane_in_mode)"
main_client copy-mode -t "$pane"
main_client clear-history -t "$pane"
check_equal clear-history-command-leaves-mode 0 "$(value pane_in_mode)"
finish
