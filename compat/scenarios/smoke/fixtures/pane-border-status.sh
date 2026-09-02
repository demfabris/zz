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
label="zzborderrec-$side-$$"
record() {
    "$recorder" -L "$label" -f /dev/null "$@"
}

session=pane-border-status
work="$HOME/pane-border-status-work-$side"
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
    sleep 0.9
    record capture-pane -p -t recorder >"$snaps/$1" 2>/dev/null || : >"$snaps/$1"
}

# How many rows carry the marker, and the first row that does. The two clients
# do not share a screen geometry, so the comparable facts are whether the row
# was drawn at all and whether it moved from the top of a pane to its bottom.
hits() {
    count=$(grep -c "$2" "$snaps/$1" 2>/dev/null) || count=0
    printf '%s' "$count"
}

first_row() {
    row=$(grep -n "$2" "$snaps/$1" 2>/dev/null | sed -n '1s/:.*//p') || row=0
    printf '%s' "${row:-0}"
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session"
main_client set-option -g status off
main_client split-window -t "=$session:0.0"
main_client set-window-option -t "=$session:0" pane-border-status off
main_client set-window-option -t "=$session:0" pane-border-format 'ZB#{pane_index}Q'

record new-session -d -x 100 -y 30 -s recorder \
    "env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
     LC_ALL=en_US.UTF-8 LANG=en_US.UTF-8 \
     $binary $prefix_args attach-session -t =$session"
recorder_started=1
record set-option -g status off
sleep 1.6

snap off
check_equal off-draws-no-row 0 "$(hits off ZB0Q)"

main_client set-window-option -t "=$session:0" pane-border-status top
snap top
check_equal top-draws-the-first-pane-row 1 "$(hits top ZB0Q)"
check_equal top-draws-the-second-pane-row 1 "$(hits top ZB1Q)"
top_row="$(first_row top ZB0Q)"

main_client set-window-option -t "=$session:0" pane-border-status bottom
snap bottom
check_equal bottom-draws-the-first-pane-row 1 "$(hits bottom ZB0Q)"
check_equal bottom-draws-the-second-pane-row 1 "$(hits bottom ZB1Q)"
bottom_row="$(first_row bottom ZB0Q)"
if [ "${top_row:-0}" -ge "${bottom_row:-0}" ]; then
    record_failure "row-moves-down top=$top_row bottom=$bottom_row"
fi
check_count=$((check_count + 1))

main_client set-window-option -t "=$session:0" pane-border-format 'ZC#{pane_index}Q'
snap reformat
check_equal reformat-retires-the-old-row 0 "$(hits reformat ZB0Q)"
check_equal reformat-draws-the-new-row 1 "$(hits reformat ZC0Q)"

main_client set-window-option -t "=$session:0" pane-border-status off
snap cleared
check_equal clearing-the-status-retires-the-row 0 "$(hits cleared ZC0Q)"

if [ "$check_count" -ne 9 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g PANE_BORDER_STATUS clean:9
else
    sed "s/^/pane-border-status-$side: /" "$work/failures"
fi
