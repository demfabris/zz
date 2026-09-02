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

session=display-menu-paste
work="$HOME/display-menu-paste-work-$side"
steps="$work/steps"
rm -rf "$work"
mkdir -p "$steps"
: >"$work/failures"
failed=0
check_count=0
step=0
attach_pid=""
menu_pid=""

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
    if [ -n "$menu_pid" ]; then
        step=$((step + 1))
        printf 'keys 1b\n' >"$steps/step-$step"
        sleep 0.5
        kill "$menu_pid" >/dev/null 2>&1
    fi
    step=$((step + 1))
    echo quit >"$steps/step-$step" 2>/dev/null
    main_client kill-session -t "=$session" >/dev/null 2>&1
    if [ -n "$attach_pid" ]; then
        kill "$attach_pid" >/dev/null 2>&1
        wait "$attach_pid" >/dev/null 2>&1
    fi
    main_client set-environment -gu DISPLAY_MENU_PASTE_ROW >/dev/null 2>&1
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

# One bracketed paste, written to the client's own pty the way a terminal
# delivers one.
paste() {
    drive "keys $(printf '\033[200~%s\033[201~' "$1" | od -An -tx1 | tr -d ' \n')"
    sleep 1.0
}

open_menu() {
    main_client set-environment -g DISPLAY_MENU_PASTE_ROW pending
    main_client display-menu -c "$client" -x 5 -y 5 -T '' \
        alpha a 'set-environment -g DISPLAY_MENU_PASTE_ROW alpha' \
        bravo b 'set-environment -g DISPLAY_MENU_PASTE_ROW bravo' &
    menu_pid=$!
    sleep 1.0
}

menu_alive() {
    if [ -n "$menu_pid" ] && kill -0 "$menu_pid" 2>/dev/null; then
        printf alive
    else
        printf gone
    fi
}

chosen() {
    main_client show-environment -g DISPLAY_MENU_PASTE_ROW 2>/dev/null
}

pane_text() {
    main_client capture-pane -p -t "=$session:.0" | tr -d ' \n'
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session" -x 80 -y 24 cat
main_client set-option -g status off

env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    -u TERM_PROGRAM -u TERM_PROGRAM_VERSION \
    TERM=xterm-256color \
    python3 "$HOME/pty-drive.py" "$steps" 80 24 \
    "$binary" $prefix_args attach-session -t "=$session" \
    >"$work/attach.out" 2>&1 &
attach_pid=$!

attempt=0
client=""
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
    echo "display-menu-paste-$side: attach-client"
    exit 0
fi
sleep 0.8

# A run with no row key and no navigation key leaves the menu up and reaches
# nothing: every key of the paste goes to the overlay first.
open_menu
paste 'ZW'
check_equal inert-run-leaves-the-menu-up alive "$(menu_alive)"
check_equal inert-run-runs-nothing 'DISPLAY_MENU_PASTE_ROW=pending' "$(chosen)"
check_equal inert-run-reaches-no-pane '' "$(pane_text)"
drive 'keys 1b'
sleep 0.8
wait "$menu_pid" >/dev/null 2>&1 || true
menu_pid=""
check_equal inert-run-still-reaches-no-pane '' "$(pane_text)"

# The cancel key inside a paste closes the menu without running a row.
open_menu
paste 'q'
sleep 0.5
check_equal cancel-key-closes-the-menu gone "$(menu_alive)"
check_equal cancel-key-runs-nothing 'DISPLAY_MENU_PASTE_ROW=pending' "$(chosen)"
wait "$menu_pid" >/dev/null 2>&1 || true
menu_pid=""

# A row key inside a paste runs that row, and the characters after it arrive at
# the pane as ordinary keys: the bracket markers were consumed by the overlay.
open_menu
paste 'ZaXY'
sleep 0.8
check_equal row-key-closes-the-menu gone "$(menu_alive)"
check_equal row-key-runs-its-row 'DISPLAY_MENU_PASTE_ROW=alpha' "$(chosen)"
wait "$menu_pid" >/dev/null 2>&1 || true
menu_pid=""
attempt=0
while [ "$attempt" -lt 60 ]; do
    if [ "$(pane_text)" = XY ]; then
        break
    fi
    attempt=$((attempt + 1))
    sleep 0.1
done
check_equal tail-after-the-close-reaches-the-pane XY "$(pane_text)"

if [ "$check_count" -ne 9 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g DISPLAY_MENU_PASTE clean:9
else
    sed "s/^/display-menu-paste-$side: /" "$work/failures"
fi
