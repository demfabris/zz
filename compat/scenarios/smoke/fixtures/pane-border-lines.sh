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
label="zzlinesrec-$side-$$"
record() {
    "$recorder" -L "$label" -f /dev/null "$@"
}

session=pane-border-lines
work="$HOME/pane-border-lines-work-$side"
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

present() {
    if grep -q "$2" "$snaps/$1" 2>/dev/null; then
        printf yes
    else
        printf no
    fi
}

# A run of one repeated digit, which is what the `number` family draws along a
# border and nothing else on either screen draws.
digit_run() {
    if grep -qE '(0{8}|1{8}|2{8}|3{8})' "$snaps/$1" 2>/dev/null; then
        printf yes
    else
        printf no
    fi
}

lines() {
    main_client set-window-option -t "=$session:0" pane-border-lines "$1"
    snap "$1"
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session"
main_client set-option -g status off
main_client split-window -v -t "=$session:0.0"
main_client split-window -h -t "=$session:0.1"
main_client set-window-option -t "=$session:0" pane-border-status top

record new-session -d -x 100 -y 30 -s recorder \
    "env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
     LC_ALL=en_US.UTF-8 LANG=en_US.UTF-8 \
     $binary $prefix_args attach-session -t =$session"
recorder_started=1
record set-option -g status off
sleep 1.8

# A T split: the horizontal divider meets the vertical one from below, so the
# junction cell is CELL_LRD and screen_redraw_type_of_cell picks the family's
# down tee rather than letting one divider overwrite the other.
lines single
check_equal single-draws-light "yes" "$(present single '─')"
check_equal single-draws-the-tee "yes" "$(present single '┬')"
check_equal single-draws-no-double "no" "$(present single '═')"

lines double
check_equal double-draws-double "yes" "$(present double '═')"
check_equal double-draws-the-tee "yes" "$(present double '╦')"
check_equal double-draws-no-light "no" "$(present double '─')"

lines heavy
check_equal heavy-draws-heavy "yes" "$(present heavy '━')"
check_equal heavy-draws-the-tee "yes" "$(present heavy '┳')"
check_equal heavy-draws-no-light "no" "$(present heavy '─')"

lines simple
check_equal simple-draws-the-ascii-tee "yes" "$(present simple '+')"
check_equal simple-draws-no-light "no" "$(present simple '─')"

lines number
check_equal number-draws-a-digit-run "yes" "$(digit_run number)"
check_equal number-draws-no-light "no" "$(present number '─')"

lines spaces
check_equal spaces-draws-no-light "no" "$(present spaces '─')"
check_equal spaces-draws-no-tee "no" "$(present spaces '┬')"
check_equal spaces-draws-no-digit-run "no" "$(digit_run spaces)"

lines none
check_equal none-draws-no-light "no" "$(present none '─')"
check_equal none-draws-no-tee "no" "$(present none '┬')"

# A cross split: both halves are split at the same column, so the horizontal
# divider cell there continues in all four directions and takes CELL_LRUD.
main_client kill-pane -t "=$session:0.2"
main_client split-window -h -t "=$session:0.0"
main_client split-window -h -t "=$session:0.2"
lines single
check_equal cross-draws-the-single-cross "yes" "$(present single '┼')"
lines double
check_equal cross-draws-the-double-cross "yes" "$(present double '╬')"
lines heavy
check_equal cross-draws-the-heavy-cross "yes" "$(present heavy '╋')"

if [ "$check_count" -ne 21 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g PANE_BORDER_LINES clean:21
else
    sed "s/^/pane-border-lines-$side: /" "$work/failures"
fi
