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

session=menu-style-refresh
work="$HOME/display-menu-style-refresh-work-$side"
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
    main_client set-environment -gu DISPLAY_MENU_STYLE_ROW >/dev/null 2>&1
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

# The SGR sequences a snapshot carries, one per line, deduplicated: a redraw
# that lands twice in one snapshot cannot move this set, and a colour that
# reaches the drawn menu must.
sgr_set() {
    python3 -c 'import re, sys
data = open(sys.argv[1], "rb").read()
found = sorted({m.decode("latin-1") for m in re.findall(rb"\x1b\[[0-9;:]*m", data)})
sys.stdout.write("\n".join(found))' "$snaps/$1"
}

# The printable characters a snapshot drew, deduplicated: the box glyphs move
# with the border lines and stay put when only a colour changes.
glyph_set() {
    python3 -c 'import re, sys
data = open(sys.argv[1], "rb").read()
plain = re.sub(rb"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b[()][AB0]|\x1b[]][^\x07\x1b]*(\x07|\x1b\\)|\x1b.", b"", data)
found = sorted({bytes([c]).decode("latin-1") for c in plain if c > 32 and c != 127})
sys.stdout.write("".join(found))' "$snaps/$1"
}

compare_sgr() {
    if [ "$(sgr_set "$1")" = "$(sgr_set "$2")" ]; then
        printf same
    else
        printf differ
    fi
}

compare_glyphs() {
    if [ "$(glyph_set "$1")" = "$(glyph_set "$2")" ]; then
        printf same
    else
        printf differ
    fi
}

restyle() {
    main_client set-window-option "$1" "$2" >/dev/null 2>&1
    sleep 0.7
    drive "snap $3"
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session"
main_client set-option -g status off
main_client set-window-option menu-style 'bg=colour17'
main_client set-window-option menu-selected-style 'fg=colour46'
main_client set-window-option menu-border-style 'fg=colour201'

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
    echo "display-menu-style-refresh-$side: attach-client"
    exit 0
fi
sleep 0.8

main_client set-environment -g DISPLAY_MENU_STYLE_ROW pending
main_client display-menu -c "$client" -x 5 -y 5 -T '' \
    alpha a 'set-environment -g DISPLAY_MENU_STYLE_ROW alpha' \
    bravo b 'set-environment -g DISPLAY_MENU_STYLE_ROW bravo' &
menu_pid=$!
sleep 1.0
drive "snap opened"

# The restyles below repaint the menu while the cursor sits on the second row;
# menu_reapply_styles never touches md->choice, so Enter at the end must still
# run bravo.
drive "keys 1b5b42"
sleep 0.5

# menu_reapply_styles runs from menu_draw_cb, so a set-option that lands while
# the menu is up repaints it with the new colours and no key pressed. The box
# keeps its glyphs: only the styling moves.
restyle menu-style 'bg=colour52' style-b
restyle menu-style 'bg=colour17' style-a
restyle menu-style 'bg=colour52' style-b-again
check_equal style-reaches-the-live-menu differ "$(compare_sgr style-b style-a)"
check_equal style-tracks-the-option-back same "$(compare_sgr style-b style-b-again)"
check_equal style-leaves-the-box-alone same "$(compare_glyphs style-b style-a)"

restyle menu-selected-style 'fg=colour214' selected-b
restyle menu-selected-style 'fg=colour46' selected-a
check_equal selected-style-reaches-the-live-menu differ "$(compare_sgr selected-b selected-a)"
check_equal selected-style-leaves-the-box-alone same "$(compare_glyphs selected-b selected-a)"

restyle menu-border-style 'fg=colour45' border-b
restyle menu-border-style 'fg=colour201' border-a
check_equal border-style-reaches-the-live-menu differ "$(compare_sgr border-b border-a)"
check_equal border-style-leaves-the-box-alone same "$(compare_glyphs border-b border-a)"

# menu_prepare copies menu-border-lines into the menu once and menu_draw_cb
# never reads the option again, so the live box keeps the lines it opened with.
main_client set-window-option menu-border-lines double
sleep 0.7
drive "snap lines-set"
restyle menu-style 'bg=colour17' lines-style-a
check_equal border-lines-are-not-reread same "$(compare_glyphs style-a lines-style-a)"
check_equal border-lines-leave-the-colours-alone same "$(compare_sgr style-a lines-style-a)"

drive "keys 0d"
attempt=0
while [ "$attempt" -lt 200 ]; do
    chosen="$(main_client show-environment -g DISPLAY_MENU_STYLE_ROW 2>/dev/null || true)"
    if [ "$chosen" != "DISPLAY_MENU_STYLE_ROW=pending" ]; then
        break
    fi
    attempt=$((attempt + 1))
    sleep 0.05
done
wait "$menu_pid" >/dev/null 2>&1 || true
menu_pid=""
check_equal cursor-survives-the-restyles "DISPLAY_MENU_STYLE_ROW=bravo" "$chosen"

if [ "$check_count" -ne 10 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g DISPLAY_MENU_STYLE_REFRESH clean:10
else
    sed "s/^/display-menu-style-refresh-$side: /" "$work/failures"
fi
