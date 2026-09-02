#!/bin/sh
set -eu

export LC_ALL=C

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    side=zz
    binary="$ZZ_SMOKE_ZZ_BIN"
    set -- --socket "$ZZ_SMOKE_ZZ_SOCKET"
    want_place=yes
    want_anchor=inside
    want_retire=yes
else
    side=tmux
    binary="$ZZ_SMOKE_TMUX_BIN"
    set -- -L "$ZZ_SMOKE_TMUX_LABEL"
    want_place=no
    want_anchor=none
    want_retire=no
fi
prefix_args="$*"
main_client() {
    # shellcheck disable=SC2086
    "$binary" $prefix_args "$@"
}

session=display-popup-kitty
work="$HOME/display-popup-kitty-work-$side"
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

drawn() {
    if grep -q "$2" "$snaps/$1" 2>/dev/null; then
        printf yes
    else
        printf no
    fi
}

placed() {
    python3 -c 'import sys
data = open(sys.argv[1], "rb").read()
sys.stdout.write("yes" if b"\x1b_Ga=p," in data else "no")' "$snaps/$1"
}

retired() {
    python3 -c 'import sys
data = open(sys.argv[1], "rb").read()
sys.stdout.write("yes" if b"\x1b_Ga=d,d=i," in data else "no")' "$snaps/$1"
}

# Where the placement sits relative to the popup box. The box is drawn from its
# frame origin and the placement from the content origin inside the border, so
# a placement one row down and one column right of the corner is anchored the
# way the renderer intends.
anchor() {
    python3 -c 'import re, sys
data = open(sys.argv[1], "rb").read()
cursor = re.compile(rb"\x1b\[(\d+);(\d+)H")
corner = "\u250c".encode("utf-8")
for frame in data.split(b"\x1b[?2026h"):
    place = frame.find(b"\x1b_Ga=p,")
    box = frame.rfind(corner, 0, place) if place != -1 else -1
    if place == -1 or box == -1:
        continue
    def last_before(index):
        found = None
        for match in cursor.finditer(frame, 0, index):
            found = (int(match.group(1)), int(match.group(2)))
        return found
    origin = last_before(box)
    image = last_before(place)
    if origin is None or image is None:
        continue
    if image == (origin[0] + 1, origin[1] + 1):
        sys.stdout.write("inside")
    else:
        sys.stdout.write("%s-vs-%s" % (image, origin))
    raise SystemExit
sys.stdout.write("none")' "$snaps/$1"
}

cat >"$work/popup.sh" <<'POPUP'
printf '\033_Ga=T,f=24,s=1,v=1,i=77,q=2;/wAA\033\\'
printf '\nPOPUPMARK\n'
sleep 4
printf '\033[2J\033[H'
printf '\033_Ga=T,f=24,s=1,v=1,i=78,q=2;AP8A\033\\'
sleep 60
POPUP

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session"
main_client set-option -g status off

env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    TERM=xterm-256color LC_ALL=en_US.UTF-8 LANG=en_US.UTF-8 \
    python3 "$HOME/graphics-drive.py" "$steps" "$snaps" 100 30 \
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
    echo "display-popup-kitty-$side: attach-client"
    exit 0
fi
sleep 1.2

main_client display-popup -c "$client" -x 5 -y 5 -w 40 -h 14 "sh $work/popup.sh" &
popup_pid=$!
sleep 2.5
# A client resize republishes the popup's per-view terminal snapshot. zz only
# carries a pane's Kitty placements on that snapshot - TerminalSession's
# fallback viewport publishes an empty placement list - so the resize is what
# hands the popup's image to the client. The residue is recorded in the
# registry; the assertions below are about what the client does with the image
# once it has it.
drive "size 100 29"
sleep 2.5
drive "snap opened"
check_equal popup-content-reaches-the-client yes "$(drawn opened POPUPMARK)"
check_equal image-is-placed-inside-the-popup "$want_place" "$(placed opened)"
check_equal placement-is-anchored-inside-the-border "$want_anchor" "$(anchor opened)"

sleep 3.5
drive "size 100 30"
sleep 2.0
drive "snap replaced"
check_equal replacement-retires-the-previous-placement "$want_retire" "$(retired replaced)"
check_equal replacement-places-the-new-image "$want_place" "$(placed replaced)"

main_client display-popup -C -c "$client" >/dev/null 2>&1 || true
sleep 1.0
kill "$popup_pid" >/dev/null 2>&1 || true
wait "$popup_pid" >/dev/null 2>&1 || true
popup_pid=""
sleep 1.5
drive "snap closed"
check_equal closing-the-popup-deletes-what-it-placed "$want_retire" "$(retired closed)"
check_equal closing-the-popup-places-nothing-new no "$(placed closed)"

if [ "$check_count" -ne 7 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g DISPLAY_POPUP_KITTY clean:7
else
    sed "s/^/display-popup-kitty-$side: /" "$work/failures"
fi
