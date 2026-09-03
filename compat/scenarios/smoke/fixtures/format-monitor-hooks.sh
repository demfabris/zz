#!/bin/sh
set -eu

export LC_ALL=C

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    side=zz
    main_client() {
        "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" "$@"
    }
else
    side=tmux
    main_client() {
        "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" "$@"
    }
fi

session=montest
work="$HOME/format-monitor-hooks-work-$side"
rm -rf "$work"
mkdir -p "$work"
: >"$work/failures"
failed=0
check_count=0

record_failure() {
    failed=1
    printf '%s\n' "$1" >>"$work/failures"
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

names='hook=#{hook} value=#{hook_value} last=#{hook_last} window=#{hook_window} window_name=#{hook_window_name} window_index=#{hook_window_index} session=#{hook_session} session_name=#{hook_session_name} pane=#{hook_pane}'

probe() {
    label="$1"
    shift
    status=0
    output="$(main_client "$@" 2>&1)" || status=$?
    printf 'format-monitor-hooks: %s status=%s output=[%s]\n' "$label" "$status" "$output"
}

read_option() {
    main_client show-options -gqv "$1"
}

await_option() {
    attempt=0
    while [ "$attempt" -lt 60 ]; do
        value="$(read_option "$1")"
        if [ -n "$value" ]; then
            printf '%s' "$value"
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.1
    done
    printf ''
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session" -n first
session_id="$(main_client display-message -p -t "=$session:0.0" '#{session_id}')"
window_id="$(main_client display-message -p -t "=$session:0.0" '#{window_id}')"
pane_id="$(main_client display-message -p -t "=$session:0.0" '#{pane_id}')"

# Outside a monitor fire none of the nine names answer.
check_equal quiescent '<|||||||| >' \
    "$(main_client display-message -p -t "=$session" \
        '<#{hook}|#{hook_value}|#{hook_last}|#{hook_window}|#{hook_window_name}|#{hook_window_index}|#{hook_session}|#{hook_session_name}|#{hook_pane} >')"

# An all-windows subscription arms without firing, and stays quiet while the
# value it watches does not move.
main_client set-hook -B "@watch:@*:#{window_name}" "set -g @fired \"$names\""
sleep 2.2
check_equal window-arm-quiet '' "$(read_option @fired)"

main_client rename-window -t "=$session:0" renamed
check_equal window-fire \
    "hook=@watch value=renamed last=first window=$window_id window_name=renamed window_index=0 session=$session_id session_name=montest pane=" \
    "$(await_option @fired)"

# The subscription reads back as name:what:format.
check_equal show-one "@watch:@*:#{window_name}" "$(main_client show-hooks -B)"

# A session subscription carries the session names and leaves the window and
# pane names empty.
main_client set-hook -B "@sess::#{session_name}" "set -g @sessfired \"$names\""
sleep 1.8
main_client rename-session -t "=$session" montest2
session=montest2
check_equal session-fire \
    "hook=@sess value=montest2 last=montest window= window_name= window_index= session=$session_id session_name=montest2 pane=" \
    "$(await_option @sessfired)"

# An all-panes subscription carries the pane as well.
main_client select-pane -t "=$session:0.0" -T before
main_client set-hook -B "@panes:%*:#{pane_title}" "set -g @panefired \"$names\""
sleep 1.8
main_client select-pane -t "=$session:0.0" -T after
check_equal pane-fire \
    "hook=@panes value=after last=before window=$window_id window_name=renamed window_index=0 session=$session_id session_name=montest2 pane=$pane_id" \
    "$(await_option @panefired)"

# Every subscribed object of an all-windows subscription is polled on its own.
main_client new-window -t "=$session" -n second
main_client set-hook -B "@each:@*:#{window_name}" \
    'set -ga @each_fired "[#{hook_window_index}:#{hook_value}]"'
sleep 1.8
main_client rename-window -t "=$session:0" firstwin
main_client rename-window -t "=$session:1" secondwin
attempt=0
while [ "$attempt" -lt 60 ]; do
    each="$(read_option @each_fired)"
    if [ "$each" = "[0:firstwin][1:secondwin]" ]; then
        break
    fi
    attempt=$((attempt + 1))
    sleep 0.1
done
check_equal each-window '[0:firstwin][1:secondwin]' "$(read_option @each_fired)"

# A subscription with no body still creates its own empty option.
main_client set-hook -B "@bodyless:@*:#{window_name}"
check_equal bodyless-option '' "$(main_client show-options -qv @bodyless)"

# show-hooks -B lists the live subscriptions in name order, one per line.
check_equal show-all \
    "@bodyless:@*:#{window_name}
@each:@*:#{window_name}
@panes:%*:#{pane_title}
@sess::#{session_name}
@watch:@*:#{window_name}" \
    "$(main_client show-hooks -B)"
check_equal show-named "@sess::#{session_name}" "$(main_client show-hooks -B @sess)"

# monitor_parse needs both colons, and the name must be a user option.
probe missing-colons set-hook -B nocolons "set -g @x y"
probe one-colon set-hook -B one:colon "set -g @x y"
probe bad-name set-hook -B "noat:@*:#{window_name}" "set -g @x y"
probe show-unknown show-hooks -B @nomonitor

# -B takes a value, so -Bu swallows the u and removes nothing; -u -B does
# remove the subscription.
probe swallowed-u set-hook -Bu @watch
check_equal swallowed-u-survives "@watch:@*:#{window_name}" \
    "$(main_client show-hooks -B @watch)"
main_client set-hook -u -B @watch
check_equal removed '' "$(main_client show-hooks -B @watch)"
main_client set-hook -u -B @never

# A removed subscription stops firing.
main_client set-option -gu @fired
main_client rename-window -t "=$session:0" thirdwin
sleep 2.2
check_equal removed-quiet '' "$(read_option @fired)"

if [ "$check_count" -ne 13 ]; then
    record_failure "total-checks $check_count"
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g FORMAT_MONITOR_HOOKS clean:13
else
    sed "s/^/format-monitor-hooks-$side: /" "$work/failures"
fi
