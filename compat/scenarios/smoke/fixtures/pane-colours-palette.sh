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

session=pane-colours
work="$HOME/pane-colours-palette-work-$side"
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
    main_client set-option -gwu pane-colours >/dev/null 2>&1
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

paint() {
    printf '\033[31mZZA\033[0m\n' >"$tty0" 2>/dev/null || true
    printf '\033[32mZZB\033[0m\n' >"$tty1" 2>/dev/null || true
    sleep 0.6
}

seen() {
    if grep -q -- "$2" "$snaps/$1" 2>/dev/null; then
        printf yes
    else
        printf no
    fi
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session" "sh -c 'sleep 300'"
main_client split-window -t "=$session:0" -d "sh -c 'sleep 300'"
tty0="$(main_client display-message -p -t "=$session:0.0" '#{pane_tty}')"
tty1="$(main_client display-message -p -t "=$session:0.1" '#{pane_tty}')"

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
    echo "pane-colours-palette-$side: attach-client"
    exit 0
fi
sleep 0.6

# A clean base draws neither override, so every later marker is the option's
# doing and not the theme's.
paint
drive "snap base"
check_equal base-first no "$(seen base '38;2;18;52;86')"
check_equal base-second no "$(seen base '38;2;101;67;33')"

# colour_palette_from_option copies the window array into every pane's default
# palette, and the cells already on screen take it without the pane restarting.
main_client set-option -w -t "=$session:0" pane-colours[1] '#123456'
main_client set-option -w -t "=$session:0" pane-colours[2] '#654321'
sleep 1.2
drive "snap live"
check_equal live-first yes "$(seen live '38;2;18;52;86')"
check_equal live-second yes "$(seen live '38;2;101;67;33')"

paint
drive "snap window"
check_equal window-first yes "$(seen window '38;2;18;52;86')"
check_equal window-second yes "$(seen window '38;2;101;67;33')"

# A pane-level array is the whole array options_get finds first, so the second
# pane loses the window entry it was inheriting while the first keeps it.
main_client set-option -p -t "=$session:0.1" pane-colours[3] '#0f0f0f'
sleep 0.8
paint
drive "snap shadow"
check_equal shadow-first yes "$(seen shadow '38;2;18;52;86')"
check_equal shadow-second no "$(seen shadow '38;2;101;67;33')"

main_client set-option -pu -t "=$session:0.1" pane-colours
sleep 0.8
paint
drive "snap unshadow"
check_equal unshadow-second yes "$(seen unshadow '38;2;101;67;33')"

# colour_palette_get reads the OSC 4 palette before the configured default.
printf '\033]4;1;rgb:00/ff/00\007' >"$tty0" 2>/dev/null || true
sleep 0.8
paint
drive "snap osc"
check_equal osc-wins yes "$(seen osc '38;2;0;255;0')"
check_equal osc-hides-default no "$(seen osc '38;2;18;52;86')"
printf '\033]104;1\007' >"$tty0" 2>/dev/null || true
sleep 0.8

# options_array_getv reads "%u" for 0..255 only, so a named key and an index
# past the palette are stored and never consumed.
main_client set-option -w -t "=$session:0" 'pane-colours[named]' '#ff00ff'
main_client set-option -w -t "=$session:0" pane-colours[300] '#00ffff'
sleep 0.8
paint
drive "snap nonconsumers"
check_equal nonconsumer-named no "$(seen nonconsumers '38;2;255;0;255')"
check_equal nonconsumer-oversized no "$(seen nonconsumers '38;2;0;255;255')"
check_equal nonconsumer-stored \
    "pane-colours[1] #123456 pane-colours[2] #654321 pane-colours[300] #00ffff pane-colours[named] #ff00ff " \
    "$(main_client show-options -w -t "=$session:0" pane-colours | tr '\n' ' ')"

# Unsetting one index drops just that entry, and unsetting the array drops all
# of them, both live.
main_client set-option -wu -t "=$session:0" pane-colours[1]
sleep 0.8
paint
drive "snap unset-index"
check_equal unset-index-first no "$(seen unset-index '38;2;18;52;86')"
check_equal unset-index-second yes "$(seen unset-index '38;2;101;67;33')"

main_client set-option -wu -t "=$session:0" pane-colours
sleep 0.8
paint
drive "snap unset-all"
check_equal unset-all-second no "$(seen unset-all '38;2;101;67;33')"
check_equal unset-all-stored "" \
    "$(main_client show-options -w -t "=$session:0" pane-colours | tr '\n' ' ')"

if [ "$check_count" -ne 18 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g PANE_COLOURS_PALETTE clean:18
else
    sed "s/^/pane-colours-palette-$side: /" "$work/failures"
fi
