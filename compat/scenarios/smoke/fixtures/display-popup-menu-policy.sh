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

session=display-popup-menu-policy
work="$HOME/display-popup-menu-policy-work-$side"
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
cursor = re.compile(rb"\x1b\[(\d+);(\d+)H|\x1b\[H")
corner = "┌".encode("utf-8")
index = data.rfind(corner)
if index == -1:
    sys.stdout.write("none")
    raise SystemExit
found = None
for match in cursor.finditer(data, 0, index):
    found = (int(match.group(1)), int(match.group(2))) if match.group(1) else (1, 1)
sys.stdout.write("%d,%d" % found if found else "none")' "$snaps/$1"
}

# The run of horizontal border the client drew after the box's top-left
# corner, which is the box's inside width without assuming where either binary
# put the box.
box_width() {
    python3 -c 'import sys
data = open(sys.argv[1], "rb").read()
top = "┌".encode("utf-8")
dash = "─".encode("utf-8")
index = data.rfind(top)
if index == -1:
    sys.stdout.write("0")
    raise SystemExit
index += len(top)
count = 0
while data.startswith(dash, index):
    count += 1
    index += len(dash)
sys.stdout.write(str(count))' "$snaps/$1"
}

drawn() {
    if grep -qF "$2" "$snaps/$1" 2>/dev/null; then
        printf yes
    else
        printf no
    fi
}

winched() {
    if [ -s "$work/winch" ]; then
        printf yes
    else
        printf no
    fi
}

sgr() {
    printf 'keys %s\n' "$(printf '\033[<%s;%s;%sM' "$1" "$2" "$3" | od -An -tx1 | tr -d ' \n')"
}

open_popup() {
    main_client display-popup -c "$client" "$@" &
    popup_pid=$!
    sleep 2.0
}

close_popup() {
    main_client display-popup -C -c "$client" >/dev/null 2>&1 || true
    if [ -n "$popup_pid" ]; then
        wait "$popup_pid" >/dev/null 2>&1 || true
        popup_pid=""
    fi
    sleep 1.5
}

# A popup job that records every SIGWINCH the pty hands it, so a resize of the
# box that does not reach the job is measurable.
cat >"$work/popup.sh" <<POPUP
trap 'printf winch >>"$work/winch"' WINCH
echo POPUPBODY
while : ; do sleep 0.2 ; done
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
    echo "display-popup-menu-policy-$side: attach-client"
    exit 0
fi
sleep 1.2

open_popup -x 5 -y 5 -w 60 -h 8 "sh $work/popup.sh"
drive "snap opened"
opened="$(corner opened)"
check_equal popup-box-reaches-the-client yes "$([ "$opened" = none ] && printf no || printf yes)"
if [ "$opened" = none ]; then
    check_count=$((check_count + 9))
    record_failure no-popup-box
    sed "s/^/display-popup-menu-policy-$side: /" "$work/failures"
    exit 0
fi
row="${opened%,*}"
column="${opened#*,}"

# The control for the two Fill Space checks below: popup_resize_cb does call
# job_resize, so a client narrower than the box does reach the job.
rm -f "$work/winch"
drive "size 50 30"
sleep 1.5
check_equal a-client-resize-reaches-the-popup-job yes "$(winched)"
drive "size 100 30"
sleep 1.5

# Fill Space is a box move: popup_menu_done sets pd->sx/sy/px/py and redraws,
# and pd->s and the job keep the size they had.
rm -f "$work/winch"
drive "$(sgr 2 $((column + 40)) $((row + 12)))"
sleep 0.8
drive "snap menu"
check_equal outside-button-three-raises-the-popup-menu yes "$(drawn menu 'Fill Space')"
drive "keys 46"
sleep 1.5
drive "snap filled"
check_equal fill-space-widens-the-box yes \
    "$([ "$(box_width filled)" -gt "$(box_width opened)" ] && printf yes || printf no)"
check_equal fill-space-leaves-the-popup-job-at-its-size no "$(winched)"
close_popup

# Centre moves the box without touching ppx/ppy/psx/psy, so the next
# owning-client resize puts it back where display-popup asked for it.
open_popup -x 5 -y 5 -w 30 -h 8 "sh $work/popup.sh"
drive "snap reopened"
reopened="$(corner reopened)"
drive "$(sgr 2 $((column + 40)) $((row + 12)))"
sleep 0.8
drive "snap menu-again"
check_equal the-menu-is-up-over-the-reopened-popup yes "$(drawn menu-again Centre)"
drive "keys 43"
sleep 1.2
drive "snap centred"
centred="$(corner centred)"
case "$centred" in
none | "$reopened") moved=same ;;
*) moved=moved ;;
esac
check_equal centre-moves-the-popup moved "$moved"
drive "size 90 26"
sleep 1.5
drive "snap refit"
refit="$(corner refit)"
case "$refit" in
"$reopened") refit=origin ;;
"$centred") refit=centred ;;
esac
check_equal a-resize-restores-the-placement-display-popup-asked-for origin "$refit"
drive "size 100 30"
sleep 1.0
close_popup

# The popup's menu is pd->md: it is drawn by popup_draw_cb and freed by
# popup_free, so it cannot outlive the popup and cannot swallow the keys that
# follow an -E popup whose job has exited.
rm -f "$work/orphan"
open_popup -E -x 5 -y 5 -w 30 -h 8 "sleep 5"
drive "$(sgr 2 $((column + 40)) $((row + 12)))"
sleep 0.8
drive "snap orphan"
check_equal the-menu-is-up-when-the-job-exits yes "$(drawn orphan Close)"
sleep 6.0
drive "keys $(printf 'printf ORPHAN >%s\r' "$work/orphan" | od -An -tx1 | tr -d ' \n')"
sleep 2.0
check_equal keys-after-the-popup-reach-the-pane yes \
    "$([ -s "$work/orphan" ] && printf yes || printf no)"
popup_pid=""

if [ "$check_count" -ne 10 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g DISPLAY_POPUP_MENU_POLICY clean:10
else
    sed "s/^/display-popup-menu-policy-$side: /" "$work/failures"
fi
