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

session=display-popup-drag
work="$HOME/display-popup-drag-work-$side"
steps="$work/steps"
snaps="$work/snaps"
rm -rf "$work"
mkdir -p "$steps" "$snaps"
: >"$work/failures"
failed=0
check_count=0
step=0
attach_pid=""
popup_pid=""
client=""

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
    if [ -n "$popup_pid" ]; then
        kill "$popup_pid" >/dev/null 2>&1
    fi
    step=$((step + 1))
    echo quit >"$steps/step-$step" 2>/dev/null
    main_client kill-session -t "=$session" >/dev/null 2>&1
    if [ -n "$attach_pid" ]; then
        kill "$attach_pid" >/dev/null 2>&1
        wait "$attach_pid" >/dev/null 2>&1
    fi
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

# The cursor position the client wrote before the popup's top-left corner
# glyph, which is where the box is drawn from.
corner() {
    python3 -c 'import re, sys
data = open(sys.argv[1], "rb").read()
cursor = re.compile(rb"\x1b\[(\d+);(\d+)H")
corner = "┌".encode("utf-8")
index = data.rfind(corner)
if index == -1:
    sys.stdout.write("none")
    raise SystemExit
found = None
for match in cursor.finditer(data, 0, index):
    found = (int(match.group(1)), int(match.group(2)))
sys.stdout.write("%d,%d" % found if found else "none")' "$snaps/$1"
}

sgr() {
    printf 'keys %s\n' "$(printf '\033[<%s;%s;%sM' "$1" "$2" "$3" | od -An -tx1 | tr -d ' \n')"
}

sgr_up() {
    printf 'keys %s\n' "$(printf '\033[<%s;%s;%sm' "$1" "$2" "$3" | od -An -tx1 | tr -d ' \n')"
}

cat >"$work/popup.sh" <<'POPUP'
echo POPUPBODY
sleep 120
POPUP

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session"
main_client set-option -g status off
main_client set-option -g mouse on

env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    TERM=xterm-256color LC_ALL=en_US.UTF-8 LANG=en_US.UTF-8 \
    python3 "$HOME/chooser-drive.py" "$steps" "$snaps" 100 30 \
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
    echo "display-popup-drag-$side: attach-client"
    exit 0
fi
sleep 1.2

main_client display-popup -c "$client" -x 5 -y 5 -w 30 -h 8 "sh $work/popup.sh" &
popup_pid=$!
sleep 2.0
drive "snap opened"
opened="$(corner opened)"
check_equal popup-box-reaches-the-client yes "$([ "$opened" = none ] && printf no || printf yes)"
if [ "$opened" = none ]; then
    check_count=$((check_count + 3))
    record_failure no-popup-box
    sed "s/^/display-popup-drag-$side: /" "$work/failures"
    exit 0
fi
row="${opened%,*}"
column="${opened#*,}"

# The arming condition reads the previous report from the tty, which
# tty_keys_mouse refreshes from every report it decodes. A drag that began on
# the border, wandered into the content and came back onto the border is
# therefore still a drag as far as m->lb is concerned, so it never arms a move.
drive "$(sgr 0 $((column + 4)) "$row")"
sleep 0.3
drive "$(sgr 32 $((column + 6)) $((row + 3)))"
sleep 0.3
drive "$(sgr 32 $((column + 8)) "$row")"
sleep 0.3
drive "$(sgr 32 $((column + 14)) $((row + 5)))"
sleep 0.5
drive "$(sgr_up 0 $((column + 14)) $((row + 5)))"
sleep 0.8
drive "snap reentry"
reentry="$(corner reentry)"
case "$reentry" in
none | "$opened") reentry=unmoved ;;
esac
check_equal a-drag-back-onto-the-border-does-not-arm-a-move unmoved "$reentry"

# A press on the top border, a drag still on the border - which is what arms
# mode MOVE, because popup_key_cb reads the border from the drag report's own
# position and the button from the previous one - then a drag away from it.
drive "$(sgr 0 $((column + 4)) "$row")"
sleep 0.3
drive "$(sgr 32 $((column + 8)) "$row")"
sleep 0.3
drive "$(sgr 32 $((column + 14)) $((row + 5)))"
sleep 0.5
drive "$(sgr_up 0 $((column + 14)) $((row + 5)))"
sleep 0.8
drive "snap dragged"
moved="$(corner dragged)"
check_equal border-drag-moves-the-popup "$((row + 5)),$((column + 10))" "$moved"

# A drag that starts inside the content box, away from every border, is the
# job's and never moves the popup.
drive "$(sgr 0 $((column + 4)) $((row + 7)))"
sleep 0.3
drive "$(sgr 32 $((column + 6)) $((row + 8)))"
sleep 0.3
drive "$(sgr 32 $((column + 20)) $((row + 12)))"
sleep 0.5
drive "$(sgr_up 0 $((column + 20)) $((row + 12)))"
sleep 0.8
drive "snap inside"
inside="$(corner inside)"
case "$inside" in
none | "$((row + 5)),$((column + 10))") inside=unmoved ;;
esac
check_equal an-inside-drag-leaves-the-popup-where-it-is unmoved "$inside"

if [ "$check_count" -ne 4 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g DISPLAY_POPUP_DRAG clean:4
else
    sed "s/^/display-popup-drag-$side: /" "$work/failures"
fi
