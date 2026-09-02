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

session=paneinput
work="$HOME/pane-engine-knobs-input-$side"
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
    main_client set-option -su backspace >/dev/null 2>&1
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

named() {
    case "$1" in
    "$2") echo renamed ;;
    *) echo untouched ;;
    esac
}

raw="sh -c 'stty -echo -icanon min 1 time 0; exec cat'"
visible="sh -c 'stty -echo -icanon min 1 time 0; exec cat -v'"

main_client new-session -d -s "$session" -n renamer -x 40 -y 6 "$raw"
rename_pane="$(pane_of "=$session:renamer")"
main_client set-option -w -t "$rename_pane" automatic-rename on

# input_exit_rename returns before touching the window when allow-rename is
# off, and the collected string never reaches the screen either way.
main_client send-keys -t "$rename_pane" -H 1b 6b
main_client send-keys -t "$rename_pane" -l RENOFF
main_client send-keys -t "$rename_pane" -H 1b 5c
sleep 0.5
check_equal allow-rename-off-leaves-the-name untouched \
    "$(named "$(main_client display-message -p -t "$rename_pane" '#{window_name}')" RENOFF)"
check_equal allow-rename-off-keeps-automatic-rename 1 \
    "$(main_client display-message -p -t "$rename_pane" '#{automatic-rename}')"
check_equal allow-rename-off-prints-nothing "" \
    "$(main_client capture-pane -p -t "$rename_pane" | tr -d ' \n')"

main_client set-option -p -t "$rename_pane" allow-rename on
check_equal allow-rename-reads-back-on on "$(main_client show-options -p -t "$rename_pane" -v allow-rename)"
main_client send-keys -t "$rename_pane" -H 1b 6b
main_client send-keys -t "$rename_pane" -l RENON
main_client send-keys -t "$rename_pane" -H 1b 5c
sleep 0.5
check_equal allow-rename-on-renames RENON \
    "$(main_client display-message -p -t "$rename_pane" '#{window_name}')"
check_equal allow-rename-on-clears-automatic-rename 0 \
    "$(main_client display-message -p -t "$rename_pane" '#{automatic-rename}')"
check_equal allow-rename-on-prints-nothing "" \
    "$(main_client capture-pane -p -t "$rename_pane" | tr -d ' \n')"

# An empty string removes the window's own automatic-rename entry instead of
# setting a name, so the option falls back to the global default.
main_client send-keys -t "$rename_pane" -H 1b 6b 1b 5c
sleep 0.5
check_equal allow-rename-empty-restores-automatic-rename 1 \
    "$(main_client display-message -p -t "$rename_pane" '#{automatic-rename}')"

main_client set-option -p -t "$rename_pane" allow-rename off
check_equal allow-rename-reads-back-off off "$(main_client show-options -p -t "$rename_pane" -v allow-rename)"
main_client send-keys -t "$rename_pane" -H 1b 6b
main_client send-keys -t "$rename_pane" -l RENAGAIN
main_client send-keys -t "$rename_pane" -H 1b 5c
sleep 0.5
check_equal allow-rename-off-again-leaves-the-name untouched \
    "$(named "$(main_client display-message -p -t "$rename_pane" '#{window_name}')" RENAGAIN)"
main_client set-option -p -t "$rename_pane" allow-rename on

# BEL does not terminate the rename string: the pin's rename state waits for
# ST, so the name is still the one the last ST committed.
main_client send-keys -t "$rename_pane" -H 1b 6b
main_client send-keys -t "$rename_pane" -l BELNAME
main_client send-keys -t "$rename_pane" -H 07
sleep 0.5
check_equal allow-rename-ignores-bel untouched \
    "$(named "$(main_client display-message -p -t "$rename_pane" '#{window_name}')" BELNAME)"
main_client send-keys -t "$rename_pane" -H 1b 5c
sleep 0.5
# The rename table drops every C0 byte but its terminators, so the BEL never
# joins the name and the ST commits BELNAME.
check_equal allow-rename-drops-bel-from-the-name BELNAME \
    "$(main_client display-message -p -t "$rename_pane" '#{window_name}')"

# Every exit from the rename state runs input_exit_rename: an ESC commits the
# collected name and starts its own sequence, and CAN commits it too.
main_client send-keys -t "$rename_pane" -H 1b 6b
main_client send-keys -t "$rename_pane" -l ESCCSI
main_client send-keys -t "$rename_pane" -H 1b 5b 30 6d
sleep 0.5
check_equal allow-rename-commits-on-esc ESCCSI \
    "$(main_client display-message -p -t "$rename_pane" '#{window_name}')"
main_client send-keys -t "$rename_pane" -H 1b 6b
main_client send-keys -t "$rename_pane" -l CAN
main_client send-keys -t "$rename_pane" -H 18
sleep 0.5
check_equal allow-rename-commits-on-can CAN \
    "$(main_client display-message -p -t "$rename_pane" '#{window_name}')"

# input_exit_rename writes the name with window_set_name, so the rename raises
# window-renamed and never the after-rename-window command hook.
main_client set-environment -g RENHOOK none
main_client set-environment -g RENEVENT none
main_client set-hook -g after-rename-window 'set-environment -g RENHOOK after'
main_client set-hook -g window-renamed 'set-environment -g RENEVENT renamed'
main_client send-keys -t "$rename_pane" -H 1b 6b
main_client send-keys -t "$rename_pane" -l HOOKED
main_client send-keys -t "$rename_pane" -H 1b 5c
sleep 0.5
check_equal allow-rename-raises-window-renamed renamed \
    "$(main_client show-environment -g RENEVENT | sed 's/^RENEVENT=//')"
check_equal allow-rename-skips-after-rename-window none \
    "$(main_client show-environment -g RENHOOK | sed 's/^RENHOOK=//')"
main_client set-hook -gu after-rename-window
main_client set-hook -gu window-renamed

check_equal backspace-defaults-to-c-question "C-?" "$(main_client show-options -s -v backspace)"
main_client new-window -t "=$session" -n bspace "$visible"
bspace_pane="$(pane_of "=$session:bspace")"
main_client send-keys -t "$bspace_pane" BSpace
await_line "$bspace_pane" '\^?'
main_client set-option -s backspace C-h
check_equal backspace-reads-back-c-h "C-h" "$(main_client show-options -s -v backspace)"
main_client send-keys -t "$bspace_pane" BSpace
await_line "$bspace_pane" '\^H'
main_client set-option -s backspace C-w
main_client send-keys -t "$bspace_pane" BSpace
await_line "$bspace_pane" '\^W'
sleep 0.3
check_equal backspace-writes-one-byte-per-key '^?^H^W' \
    "$(main_client capture-pane -p -t "$bspace_pane" | tr -d ' \n')"

# spawn.c writes the backspace key into the child's VERASE, but only a key
# code below 0x7f survives: C-h carries a modifier bit and lands on 0x7f,
# while the 0x08 spelling is a bare code and reaches the child.
probe_verase() {
    main_client set-option -s backspace "$2"
    main_client new-window -t "=$session" -n "$1" "sh $HOME/pane-erase-probe.sh"
    probe_pane="$(pane_of "=$session:$1")"
    await_line "$probe_pane" 'ERASE'
    main_client capture-pane -p -t "$probe_pane" | tr -d ' \n'
}

check_equal verase-follows-the-hex-spelling 'ERASE[^H]' "$(probe_verase erasehex 0x08)"
check_equal verase-rejects-the-modifier-spelling 'ERASE[^?]' "$(probe_verase erasech C-h)"
check_equal verase-rejects-a-literal-above-31 'ERASE[^?]' "$(probe_verase eraselit 0x41)"

main_client set-option -su backspace
main_client new-window -t "=$session" -n erasedef "sh $HOME/pane-erase-probe.sh"
erase_default="$(pane_of "=$session:erasedef")"
await_line "$erase_default" 'ERASE'
check_equal verase-follows-the-default 'ERASE[^?]' \
    "$(main_client capture-pane -p -t "$erase_default" | tr -d ' \n')"

# input_key writes the option's own byte for an unmodified BSpace even when
# VERASE could not take it.
main_client set-option -s backspace 0x41
main_client new-window -t "=$session" -n bsliteral "$visible"
literal_pane="$(pane_of "=$session:bsliteral")"
main_client send-keys -t "$literal_pane" BSpace
await_line "$literal_pane" 'A'
check_equal backspace-writes-a-literal-code A \
    "$(main_client capture-pane -p -t "$literal_pane" | tr -d ' \n')"

if [ "$check_count" -ne 24 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g PANE_ENGINE_KNOBS_INPUT "clean:$check_count"
else
    sed "s/^/pane-engine-knobs-input-$side: /" "$work/failures"
fi
