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

session=display-menu-mouse
work="$HOME/display-menu-mouse-work-$side"
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
    main_client set-environment -gu DISPLAY_MENU_MOUSE_ROW >/dev/null 2>&1
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

keys() {
    drive "keys $(printf '%s' "$1" | od -An -tx1 | tr -d ' \n')"
    sleep 0.6
}

# SGR reports, written straight to the client's pty. Encoded button 0 with a
# final `M` is a press, with a final `m` a release, and 35 with `M` is a motion
# with no button held, which is the pin's highlight-only `move`.
pointer() {
    keys "$(printf '\033[<%s;%s;%sM' "$1" "$2" "$3")"
}

pointer_release() {
    keys "$(printf '\033[<0;%s;%sm' "$1" "$2")"
}

open_menu() {
    main_client set-environment -g DISPLAY_MENU_MOUSE_ROW pending
    # shellcheck disable=SC2086
    main_client display-menu $1 -c "$client" -x 5 -y 5 -T '' \
        alpha a 'set-environment -g DISPLAY_MENU_MOUSE_ROW alpha' \
        bravo b 'set-environment -g DISPLAY_MENU_MOUSE_ROW bravo' &
    menu_pid=$!
    sleep 1.0
}

drop_menu() {
    if [ -n "$menu_pid" ]; then
        if kill -0 "$menu_pid" 2>/dev/null; then
            keys "$(printf '\033')"
        fi
        wait "$menu_pid" >/dev/null 2>&1 || true
        menu_pid=""
    fi
}

menu_alive() {
    if [ -n "$menu_pid" ] && kill -0 "$menu_pid" 2>/dev/null; then
        printf alive
    else
        printf gone
    fi
}

chosen() {
    main_client show-environment -g DISPLAY_MENU_MOUSE_ROW 2>/dev/null
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session" -x 79 -y 24
main_client set-option -g status off
# menu_key_cb reads the pointer before the mouse option decides what a mouse key
# becomes, so the menu answers pointer reports with `mouse` off.
main_client set-option -g mouse off

env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    -u TERM_PROGRAM -u TERM_PROGRAM_VERSION \
    TERM=xterm-256color \
    python3 "$HOME/pty-drive.py" "$steps" 79 24 \
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
    echo "display-menu-mouse-$side: attach-client"
    exit 0
fi
sleep 0.8

# `display-menu -x 5 -y 5` anchors -y at the bottom of a box that is
# menu->count + 2 rows tall, so a two-row menu draws its top border on row 1 and
# its rows on rows 2 and 3: one-based rows 3 and 4 for an SGR report. The client
# is 79 columns because zz's raw TUI hides its sidebar below 80 and then centres
# the overlay grid on the same viewport the pin uses, so both engines draw the
# box at the same cell.

# A display-menu typed on a command line has no invoking mouse event, so the pin
# marks it MENU_NOMOUSE: it ignores button 1 whole and leaves on anything else.
open_menu ''
pointer 0 8 3
check_equal nomouse-ignores-button-one alive "$(menu_alive)"
check_equal nomouse-button-one-runs-nothing 'DISPLAY_MENU_MOUSE_ROW=pending' "$(chosen)"
keys "$(printf '\r')"
sleep 0.8
check_equal nomouse-keeps-its-starting-row 'DISPLAY_MENU_MOUSE_ROW=alpha' "$(chosen)"
drop_menu

open_menu ''
pointer_release 8 3
sleep 0.5
check_equal nomouse-release-leaves gone "$(menu_alive)"
check_equal nomouse-release-runs-nothing 'DISPLAY_MENU_MOUSE_ROW=pending' "$(chosen)"
drop_menu

open_menu ''
pointer 35 8 3
sleep 0.5
check_equal nomouse-motion-leaves gone "$(menu_alive)"
drop_menu

open_menu ''
pointer 2 8 3
sleep 0.5
check_equal nomouse-button-three-leaves gone "$(menu_alive)"
drop_menu

# -M gives the menu the full mouse policy, and menu_prepare then leaves
# md->choice at -1 rather than resolving a starting choice.
open_menu -M
keys "$(printf '\r')"
sleep 0.8
check_equal mouse-menu-starts-with-no-highlight 'DISPLAY_MENU_MOUSE_ROW=pending' "$(chosen)"
check_equal enter-with-no-highlight-closes gone "$(menu_alive)"
drop_menu

# menu_key_cb reaches `chosen` before it rewrites md->choice from the pointer
# row, so a release runs the row the press left the highlight on.
open_menu -M
pointer 0 8 4
check_equal press-does-not-choose alive "$(menu_alive)"
check_equal press-runs-nothing 'DISPLAY_MENU_MOUSE_ROW=pending' "$(chosen)"
pointer_release 8 3
sleep 0.8
check_equal release-closes-the-menu gone "$(menu_alive)"
check_equal release-runs-the-highlighted-row 'DISPLAY_MENU_MOUSE_ROW=bravo' "$(chosen)"
drop_menu

# Outside the box a press is not a release, so it only clears the highlight.
open_menu -M
pointer 0 41 21
check_equal outside-press-leaves-the-menu-up alive "$(menu_alive)"
pointer_release 41 21
sleep 0.8
check_equal outside-release-closes-the-menu gone "$(menu_alive)"
check_equal outside-release-runs-nothing 'DISPLAY_MENU_MOUSE_ROW=pending' "$(chosen)"
drop_menu

# A motion with no button held is highlight-only: it never selects or closes,
# but it does move the highlight Enter then runs.
open_menu -M
pointer 35 8 4
check_equal motion-leaves-the-menu-up alive "$(menu_alive)"
check_equal motion-runs-nothing 'DISPLAY_MENU_MOUSE_ROW=pending' "$(chosen)"
keys "$(printf '\r')"
sleep 0.8
check_equal motion-moved-the-highlight 'DISPLAY_MENU_MOUSE_ROW=bravo' "$(chosen)"
drop_menu

# A motion outside the box clears the highlight, so Enter closes the menu with
# nothing to run.
open_menu -M
pointer 35 8 4
pointer 35 41 21
check_equal outside-motion-leaves-the-menu-up alive "$(menu_alive)"
keys "$(printf '\r')"
sleep 0.8
check_equal outside-motion-cleared-the-highlight 'DISPLAY_MENU_MOUSE_ROW=pending' "$(chosen)"
drop_menu

if [ "$check_count" -ne 21 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g DISPLAY_MENU_MOUSE clean:21
else
    sed "s/^/display-menu-mouse-$side: /" "$work/failures"
fi
