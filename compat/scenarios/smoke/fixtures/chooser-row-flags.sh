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

session=chooser-rows
work="$HOME/chooser-row-flags-work-$side"
steps="$work/steps"
snaps="$work/snaps"
rm -rf "$work"
mkdir -p "$steps" "$snaps"
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
    main_client set-environment -gu CHOOSER_ROW >/dev/null 2>&1
    main_client delete-buffer -b zzbufalpha >/dev/null 2>&1
    main_client delete-buffer -b zzbufbeta >/dev/null 2>&1
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

chosen_row() {
    row="$(main_client show-environment -g CHOOSER_ROW 2>/dev/null || true)"
    printf '%s' "${row#CHOOSER_ROW=}"
}

# Open the chooser from the attached client with C-o, press one key inside it,
# and report which row ran the template.
probe() {
    label="$1"
    key="$2"
    expected="$3"
    main_client set-environment -g CHOOSER_ROW pending
    drive "keys 0f"
    sleep 0.7
    drive "keys $key"
    attempt=0
    while [ "$attempt" -lt 200 ]; do
        if [ "$(chosen_row)" != pending ]; then
            break
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    check_equal "$label" "$expected" "$(chosen_row)"
}

marker_count() {
    grep -c -- "$2" "$snaps/$1" 2>/dev/null || true
}

marker_seen() {
    if [ "$(marker_count "$1" "$2")" -gt 0 ] 2>/dev/null; then
        printf yes
    else
        printf no
    fi
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session"
main_client split-window -t "=$session:0" -d
main_client new-window -t "=$session" -n second -d
main_client select-window -t "=$session:0"
main_client select-pane -t "=$session:0.0"
pane0="$(main_client list-panes -t "=$session:0" -F '#{pane_id}' | sed -n '1p')"
pane1="$(main_client list-panes -t "=$session:0" -F '#{pane_id}' | sed -n '2p')"
pane2="$(main_client list-panes -t "=$session:1" -F '#{pane_id}' | sed -n '1p')"
mine="#{==:#{session_name},$session}"
mine_source="#{&&:$mine,#{==:#{pane_id},$pane0}}"

env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    TERM=xterm-256color \
    python3 "$HOME/chooser-drive.py" "$steps" "$snaps" 80 24 \
    "$binary" $prefix_args attach-session -t "=$session" \
    >"$work/attach.out" 2>&1 &
attach_pid=$!

attempt=0
while [ "$attempt" -lt 400 ]; do
    client="$(main_client list-clients -t "=$session" -F '#{client_name}' | sed -n '1p')"
    if [ -n "$client" ]; then
        break
    fi
    attempt=$((attempt + 1))
    sleep 0.05
done
if [ -z "$client" ]; then
    record_failure attach-client
    echo "chooser-row-flags-$side: attach-client"
    exit 0
fi
sleep 0.5

# Without -h every pane of every window has a row, and the invoking pane is the
# one the chooser starts on (window_tree_init seeds the current pane).
main_client bind-key -n C-o choose-tree -f "$mine" \
    'set-environment -g CHOOSER_ROW %%'
probe plain-row0 30 "=$session:"
probe plain-row1 31 "=$session:0."
probe plain-row2 32 "=$session:0.$pane0"
probe plain-row3 33 "=$session:0.$pane1"
probe plain-row4 34 "=$session:1."
probe plain-row5 35 "=$session:1.$pane2"
probe plain-enter 0d "=$session:0.$pane0"

# -h drops the invoking pane's own row while its window and session rows stay,
# and the selection the hidden row would have taken falls back to the first row.
main_client bind-key -n C-o choose-tree -h -f "$mine" \
    'set-environment -g CHOOSER_ROW %%'
probe hide-row0 30 "=$session:"
probe hide-row1 31 "=$session:0."
probe hide-row2 32 "=$session:0.$pane1"
probe hide-row3 33 "=$session:1."
probe hide-row4 34 "=$session:1.$pane2"
probe hide-enter 0d "=$session:"

# window_tree_build_window counts the filtered panes before it drops the hidden
# one, so a window whose only match is the invoking pane keeps its own row.
main_client bind-key -n C-o choose-tree -h -f "$mine_source" \
    'set-environment -g CHOOSER_ROW %%'
probe hide-filter-row0 30 "=$session:"
probe hide-filter-row1 31 "=$session:0."
probe hide-filter-enter 0d "=$session:"

# A single-pane invoking window keeps the window row it started on, so -h moves
# nothing but its own pane row.
main_client select-window -t "=$session:1"
sleep 0.4
main_client bind-key -n C-o choose-tree -f "$mine" \
    'set-environment -g CHOOSER_ROW %%'
probe single-plain-row5 35 "=$session:1.$pane2"
probe single-plain-enter 0d "=$session:1."
main_client bind-key -n C-o choose-tree -h -f "$mine" \
    'set-environment -g CHOOSER_ROW %%'
probe single-hide-row4 34 "=$session:1."
probe single-hide-enter 0d "=$session:1."
main_client select-window -t "=$session:0"
main_client select-pane -t "=$session:0.0"
sleep 0.4

# -F is expanded once per visible row in that row's own context and becomes the
# text the row draws, while the shortcut key still runs the same row.
main_client bind-key -n C-o choose-tree -F 'ZZTREE<#{pane_id}>' -f "$mine" \
    'set-environment -g CHOOSER_ROW %%'
main_client set-environment -g CHOOSER_ROW pending
drive "keys 0f"
sleep 0.9
drive "snap tree-format"
drive "keys 33"
attempt=0
while [ "$attempt" -lt 200 ]; do
    if [ "$(chosen_row)" != pending ]; then
        break
    fi
    attempt=$((attempt + 1))
    sleep 0.05
done
check_equal format-tree-row "=$session:0.$pane1" "$(chosen_row)"
check_equal format-tree-pane0 yes "$(marker_seen tree-format "ZZTREE<$pane0>")"
check_equal format-tree-pane1 yes "$(marker_seen tree-format "ZZTREE<$pane1>")"
check_equal format-tree-pane2 yes "$(marker_seen tree-format "ZZTREE<$pane2>")"

# Without -F the same rows draw no such text.
main_client bind-key -n C-o choose-tree -f "$mine" \
    'set-environment -g CHOOSER_ROW %%'
drive "keys 0f"
sleep 0.9
drive "snap tree-plain"
drive "keys 71"
sleep 0.4
check_equal format-tree-absent no "$(marker_seen tree-plain 'ZZTREE<')"

# The buffer chooser reads its own -F in each row's paste-buffer context.
main_client set-buffer -b zzbufalpha alpha
main_client set-buffer -b zzbufbeta beta
main_client bind-key -n C-o choose-buffer -F 'ZZBUF<#{buffer_name}>' \
    -f '#{m:zzbuf*,#{buffer_name}}' 'set-environment -g CHOOSER_ROW %%'
main_client set-environment -g CHOOSER_ROW pending
drive "keys 0f"
sleep 0.9
drive "snap buffer-format"
drive "keys 30"
attempt=0
while [ "$attempt" -lt 200 ]; do
    if [ "$(chosen_row)" != pending ]; then
        break
    fi
    attempt=$((attempt + 1))
    sleep 0.05
done
check_equal format-buffer-row zzbufbeta "$(chosen_row)"
check_equal format-buffer-alpha yes "$(marker_seen buffer-format 'ZZBUF<zzbufalpha>')"
check_equal format-buffer-beta yes "$(marker_seen buffer-format 'ZZBUF<zzbufbeta>')"

if [ "$check_count" -ne 28 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g CHOOSER_ROW_FLAGS clean:28
else
    sed "s/^/chooser-row-flags-$side: /" "$work/failures"
fi
