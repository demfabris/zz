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

recorder="$ZZ_SMOKE_TMUX_BIN"
label="zzindrec-$side-$$"
record() {
    "$recorder" -L "$label" -f /dev/null "$@"
}

session=pane-border-indicators
work="$HOME/pane-border-indicators-work-$side"
snaps="$work/snaps"
rm -rf "$work"
mkdir -p "$snaps"
: >"$work/failures"
failed=0
check_count=0
recorder_started=0

record_failure() {
    failed=1
    echo "$1" >>"$work/failures"
}

check_equal() {
    check_count=$((check_count + 1))
    if [ "$2" != "$3" ]; then
        record_failure "$1 want=$2 got=$3"
    fi
}

cleanup() {
    cleanup_status=$?
    trap - EXIT
    set +e
    if [ "$recorder_started" -eq 1 ]; then
        record kill-server >/dev/null 2>&1
    fi
    main_client kill-session -t "=$session" >/dev/null 2>&1
    exit "$cleanup_status"
}
trap cleanup EXIT

snap() {
    sleep 1.0
    record capture-pane -p -t recorder >"$snaps/$1" 2>/dev/null || : >"$snaps/$1"
}

arrows() {
    if grep -qE '(←|→|↑|↓)' "$snaps/$1" 2>/dev/null; then
        printf yes
    else
        printf no
    fi
}

# Under pane-border-lines number a border cell draws the index of the pane that
# owns it, so which pane owns which half of the divider is legible as a glyph
# and needs no colour encoding the two clients do not share.
owns() {
    if grep -qE "$2{8}" "$snaps/$1" 2>/dev/null; then
        printf yes
    else
        printf no
    fi
}

indicators() {
    main_client set-window-option -t "=$session:0" pane-border-indicators "$1"
    snap "$1"
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session"
main_client set-option -g status off
main_client split-window -v -t "=$session:0.0"
main_client set-window-option -t "=$session:0" pane-border-lines number
main_client select-pane -t "=$session:0.1"

record new-session -d -x 100 -y 30 -s recorder \
    "env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
     LC_ALL=en_US.UTF-8 LANG=en_US.UTF-8 \
     $binary $prefix_args attach-session -t =$session"
recorder_started=1
record set-option -g status off
sleep 1.8

# redraw_mark_border_arrows only marks when the option is arrows or both, and
# redraw_mark_two_pane_colours only splits the divider's style when it is colour
# or both, so the four values are the two independent switches crossed.
indicators off
check_equal off-draws-no-arrow no "$(arrows off)"
check_equal off-gives-the-whole-divider-to-the-active-pane yes "$(owns off 1)"
check_equal off-gives-the-inactive-pane-nothing no "$(owns off 0)"

indicators arrows
check_equal arrows-draws-an-arrow yes "$(arrows arrows)"
check_equal arrows-leaves-the-divider-undivided no "$(owns arrows 0)"

indicators colour
check_equal colour-draws-no-arrow no "$(arrows colour)"
check_equal colour-gives-the-top-half-to-the-first-pane yes "$(owns colour 0)"
check_equal colour-gives-the-bottom-half-to-the-second yes "$(owns colour 1)"

indicators both
check_equal both-draws-an-arrow yes "$(arrows both)"
check_equal both-splits-the-divider yes "$(owns both 0)"

if [ "$check_count" -ne 10 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g PANE_BORDER_INDICATORS clean:10
else
    sed "s/^/pane-border-indicators-$side: /" "$work/failures"
fi
