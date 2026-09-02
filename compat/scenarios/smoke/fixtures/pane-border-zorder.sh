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
label="zzzorderrec-$side-$$"
record() {
    "$recorder" -L "$label" -f /dev/null "$@"
}

session=pane-border-zorder
work="$HOME/pane-border-zorder-work-$side"
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

lines_snap() {
    main_client set-window-option -t "=$session:joined" pane-border-lines "$1"
    sleep 1.0
    record capture-pane -p -t recorder >"$snaps/$2" 2>/dev/null || : >"$snaps/$2"
}

# The one cell where the middle row's vertical divider ends on the row below it
# is the only tie: the panes on both sides of that divider each own the cell's
# top. Locate it by its up tee under `single` and read the owner's index off the
# same cell under `number`, which is a plain ASCII digit on both clients.
junction_owner() {
    python3 - "$snaps/single" "$snaps/number" <<'PY'
import io, sys

single = io.open(sys.argv[1], encoding="utf-8", errors="replace").read().split("\n")
number = io.open(sys.argv[2], encoding="utf-8", errors="replace").read().split("\n")
for row, line in enumerate(single):
    column = line.find("┴")
    if column == -1:
        continue
    target = number[row] if row < len(number) else ""
    sys.stdout.write(target[column] if column < len(target) else "short")
    break
else:
    sys.stdout.write("missing")
PY
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session"
main_client set-option -g status off
main_client new-window -t "=$session" -n joined
main_client split-window -v -t "=$session:joined.0"
main_client split-window -v -t "=$session:joined.0"
main_client set-window-option -t "=$session:joined" pane-border-indicators off

record new-session -d -x 100 -y 34 -s recorder \
    "env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
     LC_ALL=en_US.UTF-8 LANG=en_US.UTF-8 \
     $binary $prefix_args attach-session -t =$session"
recorder_started=1
record set-option -g status off
sleep 1.8

# The three panes were all created in this window, so w->z_index is their
# creation order and matches their ids. Split the middle row and the tie at the
# junction goes to the older of the two panes beside it, which is the middle
# row's original pane at index 1.
main_client split-window -h -t "=$session:joined.1"
main_client select-pane -t "=$session:joined.0"
lines_snap single single
lines_snap number number
check_equal created-here-gives-the-junction-to-the-older-pane 1 "$(junction_owner)"
main_client kill-pane -t "=$session:joined.2"

# Now put the same pane there by joining the oldest pane on the server instead.
# window_add_pane appends every tiled pane to the tail of z_index and join-pane
# inserts the moved pane right after its destination, so the joined pane is
# after the middle row's own pane in z_index even though it is older and carries
# the lower pane id. The junction stays with the destination pane at index 1.
main_client join-pane -h -s "=$session:0.0" -t "=$session:joined.1"
main_client select-pane -t "=$session:joined.0"
lines_snap single single
lines_snap number number
check_equal joined-in-gives-the-junction-to-the-destination 1 "$(junction_owner)"

if [ "$check_count" -ne 2 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g PANE_BORDER_ZORDER clean:2
else
    sed "s/^/pane-border-zorder-$side: /" "$work/failures"
fi
