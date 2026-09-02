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

session=paneknobs
work="$HOME/pane-engine-knobs-$side"
rm -rf "$work"
mkdir -p "$work"
: >"$work/failures"
failed=0
check_count=0

record_failure() {
    failed=1
    echo "$1" >>"$work/failures"
}

check_equal() {
    check_count=$((check_count + 1))
    if [ "$2" != "$3" ]; then
        record_failure "$1 want=[$2] got=[$3]"
    fi
}

cleanup() {
    cleanup_status=$?
    trap - EXIT
    set +e
    main_client kill-session -t "=$session" >/dev/null 2>&1
    exit "$cleanup_status"
}
trap cleanup EXIT

pane_of() {
    main_client display-message -p -t "$1" '#{pane_id}'
}

await_line() {
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if main_client capture-pane -p -t "$1" 2>/dev/null | grep -q "$2"; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "await-line-$2"
    return 1
}

# The scrollback is the observable: `#{history_size}` and `#{alternate_on}`
# are still pinned to their inactive values on zz under
# formats.terminal-runtime, and the two engines give a detached pane's
# terminal different row counts, so every check below counts markers rather
# than rows.
retained() {
    main_client capture-pane -p -S - -t "$1" | grep -cE "$2" || true
}

visible() {
    main_client capture-pane -p -t "$1" | grep -cE "$2" || true
}

# `cat` under -icanon echoes every byte the fixture writes straight back into
# the pane, so send-keys -H drives the pane's own VT.
raw="sh -c 'stty -echo -icanon min 1 time 0; exec cat'"

main_client new-session -d -s "$session" -n socon -x 40 -y 6 "$raw"
main_client new-window -t "=$session" -n socoff "$raw"
on_pane="$(pane_of "=$session:socon")"
off_pane="$(pane_of "=$session:socoff")"

main_client set-option -p -t "$on_pane" scroll-on-clear on
main_client set-option -p -t "$off_pane" scroll-on-clear off
check_equal scroll-on-clear-reads-back-on on "$(main_client show-options -p -t "$on_pane" -v scroll-on-clear)"
check_equal scroll-on-clear-reads-back-off off "$(main_client show-options -p -t "$off_pane" -v scroll-on-clear)"

feed_twelve_lines() {
    for line in L01 L02 L03 L04 L05 L06 L07 L08 L09 L10 L11 L12; do
        main_client send-keys -t "$1" -l "$line"
        main_client send-keys -t "$1" -H 0a
    done
    await_line "$1" L12
}

feed_twelve_lines "$on_pane"
feed_twelve_lines "$off_pane"
check_equal all-twelve-lines-reachable-on 12 "$(retained "$on_pane" '^L(0[1-9]|1[0-2])$')"
check_equal all-twelve-lines-reachable-off 12 "$(retained "$off_pane" '^L(0[1-9]|1[0-2])$')"

# screen_write_clearscreen hands the erase to grid_view_clear_history when
# scroll-on-clear is set: every row up to the last used one is scrolled into
# history first, so nothing on the screen is lost.
main_client send-keys -t "$on_pane" -H 1b 5b 48 1b 5b 32 4a
main_client send-keys -t "$off_pane" -H 1b 5b 48 1b 5b 32 4a
sleep 0.5
check_equal clear-keeps-every-line-on 12 "$(retained "$on_pane" '^L(0[1-9]|1[0-2])$')"
check_equal clear-drops-the-screen-off 0 "$(retained "$off_pane" '^L(09|1[0-2])$')"
check_equal clear-blanks-the-screen-on "" "$(main_client capture-pane -p -t "$on_pane" | tr -d ' \n')"
check_equal clear-blanks-the-screen-off "" "$(main_client capture-pane -p -t "$off_pane" | tr -d ' \n')"
check_equal first-history-row-on L01 "$(main_client capture-pane -p -S - -t "$on_pane" | head -1)"

# screen_write_clearendofscreen takes the same branch only with the cursor at
# the origin, so an erase-to-end after printing leaves the row on the screen.
main_client send-keys -t "$on_pane" -l AWAY
main_client send-keys -t "$on_pane" -H 1b 5b 4a
sleep 0.4
check_equal ed0-away-from-the-origin-does-not-scroll 1 "$(visible "$on_pane" '^AWAY$')"
check_equal ed0-away-from-the-origin-keeps-history 12 "$(retained "$on_pane" '^L(0[1-9]|1[0-2])$')"

main_client send-keys -t "$on_pane" -H 1b 5b 48 1b 5b 4a
sleep 0.4
check_equal ed0-at-the-origin-blanks-the-screen "" "$(main_client capture-pane -p -t "$on_pane" | tr -d ' \n')"
check_equal ed0-at-the-origin-scrolls 1 "$(retained "$on_pane" '^AWAY$')"

# grid_view_clear_history finds no used row on a blank screen and falls back
# to the plain clear, so a second erase adds nothing.
main_client send-keys -t "$on_pane" -H 1b 5b 32 4a
sleep 0.4
check_equal empty-clear-adds-nothing 1 "$(retained "$on_pane" '^AWAY$')"
check_equal empty-clear-keeps-history 12 "$(retained "$on_pane" '^L(0[1-9]|1[0-2])$')"

main_client new-window -t "=$session" -n alton "$raw"
main_client new-window -t "=$session" -n altoff "$raw"
alt_on_pane="$(pane_of "=$session:alton")"
alt_off_pane="$(pane_of "=$session:altoff")"
main_client set-option -p -t "$alt_on_pane" alternate-screen on
main_client set-option -p -t "$alt_off_pane" alternate-screen off
check_equal alternate-screen-reads-back-on on "$(main_client show-options -p -t "$alt_on_pane" -v alternate-screen)"
check_equal alternate-screen-reads-back-off off "$(main_client show-options -p -t "$alt_off_pane" -v alternate-screen)"

for pane in "$alt_on_pane" "$alt_off_pane"; do
    main_client send-keys -t "$pane" -l PRIMARY
    main_client send-keys -t "$pane" -H 0a
    await_line "$pane" '^PRIMARY$'
done

# screen_write_alternateon returns before touching the screen when the pane
# option is off, so 1049 never switches and the text keeps landing on the
# primary grid beside what is already there.
for pane in "$alt_on_pane" "$alt_off_pane"; do
    main_client send-keys -t "$pane" -H 1b 5b 3f 31 30 34 39 68
    main_client send-keys -t "$pane" -l ALTERNATE
    main_client send-keys -t "$pane" -H 0a
done
await_line "$alt_off_pane" '^ALTERNATE$'
sleep 0.4
check_equal alternate-on-hides-the-primary-grid 0 "$(visible "$alt_on_pane" '^PRIMARY$')"
check_equal alternate-on-shows-the-alternate-grid 1 "$(visible "$alt_on_pane" '^ALTERNATE$')"
check_equal alternate-off-keeps-both-lines 1 "$(visible "$alt_off_pane" '^PRIMARY$')"
check_equal alternate-off-appended-in-place 1 "$(visible "$alt_off_pane" '^ALTERNATE$')"

# The paired 1049l is dropped too, so the off pane has nothing to restore.
for pane in "$alt_on_pane" "$alt_off_pane"; do
    main_client send-keys -t "$pane" -H 1b 5b 3f 31 30 34 39 6c
done
sleep 0.5
check_equal alternate-on-restores-the-primary-grid 1 "$(visible "$alt_on_pane" '^PRIMARY$')"
check_equal alternate-on-drops-the-alternate-grid 0 "$(visible "$alt_on_pane" '^ALTERNATE$')"
check_equal alternate-off-untouched-by-the-restore 1 "$(visible "$alt_off_pane" '^ALTERNATE$')"

# 47 and 1047 are the same two branches, and a mode list keeps its other
# members when the alternate-screen numbers are taken out of it.
main_client send-keys -t "$alt_off_pane" -H 1b 5b 3f 34 37 68
main_client send-keys -t "$alt_off_pane" -H 1b 5b 3f 31 30 34 37 68
main_client send-keys -t "$alt_off_pane" -H 1b 5b 3f 31 30 34 39 3b 32 30 30 34 68
main_client send-keys -t "$alt_off_pane" -l BRACKET
main_client send-keys -t "$alt_off_pane" -H 0a
await_line "$alt_off_pane" '^BRACKET$'
sleep 0.3
check_equal alternate-off-ignores-47-and-1047 1 "$(visible "$alt_off_pane" '^PRIMARY$')"
check_equal alternate-off-keeps-writing-after-a-mode-list 1 "$(visible "$alt_off_pane" '^BRACKET$')"

if [ "$check_count" -ne 26 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g PANE_ENGINE_KNOBS "clean:$check_count"
else
    sed "s/^/pane-engine-knobs-$side: /" "$work/failures"
fi
