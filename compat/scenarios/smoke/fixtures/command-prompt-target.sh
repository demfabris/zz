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

session=prompt-target
work="$HOME/command-prompt-target-work-$side"
steps="$work/steps"
rosteps="$work/rosteps"
rm -rf "$work"
mkdir -p "$steps" "$rosteps"
: >"$work/failures"
failed=0
check_count=0
step=0
rostep=0
attach_pid=""
ro_pid=""
client=""
ro_client=""

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
    step=$((step + 1))
    echo quit >"$steps/step-$step" 2>/dev/null
    rostep=$((rostep + 1))
    echo quit >"$rosteps/step-$rostep" 2>/dev/null
    main_client kill-session -t "=$session" >/dev/null 2>&1
    for pid in $attach_pid $ro_pid; do
        kill "$pid" >/dev/null 2>&1
        wait "$pid" >/dev/null 2>&1
    done
    for name in CP_ANSWER CP_CHAIN CP_AFTER CP_FORMAT CP_CANCEL CP_BACKGROUND \
        CP_SECOND CP_READONLY CP_NOCLIENT; do
        main_client set-environment -gu "$name" >/dev/null 2>&1
    done
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

drive_readonly() {
    rostep=$((rostep + 1))
    printf '%s\n' "$1" >"$rosteps/step-$rostep"
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if [ -f "$rosteps/ack-$rostep" ]; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "ro-drive-$rostep"
    return 0
}

await_pid() {
    attempt=0
    while [ "$attempt" -lt 300 ]; do
        if ! kill -0 "$1" 2>/dev/null; then
            wait "$1" >/dev/null 2>&1 || true
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    record_failure "$2"
    kill "$1" >/dev/null 2>&1
    wait "$1" >/dev/null 2>&1 || true
    return 0
}

value() {
    row="$(main_client show-environment -g "$1" 2>/dev/null || true)"
    printf '%s' "${row#"$1"=}"
}

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session"
# The prompt reads status-keys at prompt_set_options time, and an editor in the
# environment would otherwise decide it, so pin it for every check below.
main_client set-option -g status-keys emacs

# cmd_find_current_client has no client to fall back on before the attach, so
# an untargeted prompt is a loud miss that runs nothing.
rm -f "$work/none.err"
none_rc=0
main_client command-prompt -p none 'set-environment -g CP_NOCLIENT hit' \
    >/dev/null 2>"$work/none.err" || none_rc=$?
check_equal no-client-exit 1 "$none_rc"
check_equal no-client-error "no current client" "$(cat "$work/none.err")"
check_equal no-client-ran-nothing "" "$(value CP_NOCLIENT)"

env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    TERM=xterm-256color \
    python3 "$HOME/pty-drive.py" "$steps" 80 24 \
    "$binary" $prefix_args attach-session -t "=$session" \
    >"$work/attach.out" 2>&1 &
attach_pid=$!

attempt=0
while [ "$attempt" -lt 400 ]; do
    client="$(main_client list-clients -t "=$session" -F '#{client_tty}' | sed -n '1p')"
    if [ -n "$client" ]; then
        break
    fi
    attempt=$((attempt + 1))
    sleep 0.05
done
if [ -z "$client" ]; then
    record_failure attach-client
    echo "command-prompt-target-$side: attach-client"
    exit 0
fi
sleep 0.5

# CMD_CLIENT_TFLAG resolves the client by tty and CMD_RETURN_WAIT parks the
# issuing queue, so a chained command waits behind the answer.
main_client set-environment -g CP_ANSWER pending
main_client set-environment -g CP_AFTER pending
rm -f "$work/answer.exit"
(
    main_client command-prompt -t "$client" -p 'ask ' \
        "set-environment -g CP_ANSWER 'said-%%'" \; \
        set-environment -g CP_AFTER after
    echo "$?" >"$work/answer.exit"
) &
answer_pid=$!
sleep 1.0
check_equal blocks-the-answer pending "$(value CP_ANSWER)"
check_equal blocks-the-chain pending "$(value CP_AFTER)"
drive "keys 68656c6c6f0d"
sleep 1.2
await_pid "$answer_pid" answer-parked
check_equal answer-exit 0 "$(cat "$work/answer.exit" 2>/dev/null)"
check_equal answer-substituted said-hello "$(value CP_ANSWER)"
check_equal chain-continued after "$(value CP_AFTER)"

# A target that matches no client is cmd_find_client's own diagnostic.
rm -f "$work/miss.err"
miss_rc=0
main_client command-prompt -t /dev/nosuchclient -p miss \
    'set-environment -g CP_CHAIN hit' >/dev/null 2>"$work/miss.err" || miss_rc=$?
check_equal miss-exit 1 "$miss_rc"
check_equal miss-error "can't find client: /dev/nosuchclient" "$(cat "$work/miss.err")"

# Escape leaves the prompt without an answer: the queue resumes at zero and
# the template never runs.
main_client set-environment -g CP_CANCEL pending
rm -f "$work/cancel.exit"
(
    main_client command-prompt -t "$client" -p 'cancel ' \
        "set-environment -g CP_CANCEL 'ran-%%'"
    echo "$?" >"$work/cancel.exit"
) &
cancel_pid=$!
sleep 1.0
drive "keys 1b"
sleep 1.2
await_pid "$cancel_pid" cancel-parked
check_equal cancel-exit 0 "$(cat "$work/cancel.exit" 2>/dev/null)"
check_equal cancel-ran-nothing pending "$(value CP_CANCEL)"

# -b clears the wait, so the command returns while the prompt is still up and
# the answer runs later.
main_client set-environment -g CP_BACKGROUND pending
bg_rc=0
main_client command-prompt -b -t "$client" -p 'bg ' \
    "set-environment -g CP_BACKGROUND 'bg-%%'" || bg_rc=$?
check_equal background-exit 0 "$bg_rc"
check_equal background-returned-early pending "$(value CP_BACKGROUND)"

# A client that already has a prompt takes no second one and blocks nothing.
main_client set-environment -g CP_SECOND pending
second_rc=0
main_client command-prompt -t "$client" -p 'second ' \
    "set-environment -g CP_SECOND 'second-%%'" || second_rc=$?
check_equal second-prompt-exit 0 "$second_rc"
drive "keys 6f6e650d"
sleep 1.0
check_equal background-answered bg-one "$(value CP_BACKGROUND)"
check_equal second-prompt-never-opened pending "$(value CP_SECOND)"

# -F runs the template through format_single_from_target before the answer
# pass, and format_expand leaves %% alone because it is not the time variant.
# The expansion lands before cmd_template_replace, so a #{pane_id} that expands
# to %1 is eaten by the first answer; a session name cannot collide that way.
name="$session"
main_client set-environment -g CP_FORMAT pending
(
    main_client command-prompt -F -t "$client" -p 'fmt ' \
        "set-environment -g CP_FORMAT '#{session_name}/%%'"
) &
fmt_pid=$!
sleep 1.0
drive "keys 7a65640d"
sleep 1.2
await_pid "$fmt_pid" format-parked
check_equal format-expanded "$name/zed" "$(value CP_FORMAT)"

main_client set-environment -g CP_FORMAT pending
(
    main_client command-prompt -t "$client" -p 'plain ' \
        "set-environment -g CP_FORMAT '#{session_name}/%%'"
) &
plain_pid=$!
sleep 1.0
drive "keys 7a65640d"
sleep 1.2
await_pid "$plain_pid" plain-parked
check_equal format-left-literal '#{session_name}/zed' "$(value CP_FORMAT)"

# A read-only client takes the prompt but never processes its keys, so the
# queue stays parked until the client leaves and the template never runs.
env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    TERM=xterm-256color \
    python3 "$HOME/pty-drive.py" "$rosteps" 80 24 \
    "$binary" $prefix_args attach-session -r -t "=$session" \
    >"$work/ro-attach.out" 2>&1 &
ro_pid=$!

attempt=0
while [ "$attempt" -lt 400 ]; do
    ro_client="$(main_client list-clients -t "=$session" \
        -F '#{client_tty} #{client_readonly}' | sed -n 's/ 1$//p' | sed -n '1p')"
    if [ -n "$ro_client" ]; then
        break
    fi
    attempt=$((attempt + 1))
    sleep 0.05
done
if [ -z "$ro_client" ]; then
    record_failure readonly-attach
else
    main_client set-environment -g CP_READONLY pending
    rm -f "$work/readonly.exit"
    (
        main_client command-prompt -t "$ro_client" -p 'ro ' \
            "set-environment -g CP_READONLY 'ro-%%'"
        echo "$?" >"$work/readonly.exit"
    ) &
    ro_prompt_pid=$!
    sleep 1.0
    drive_readonly "keys 726f760d"
    sleep 1.0
    check_equal readonly-keys-ignored pending "$(value CP_READONLY)"
    check_equal readonly-still-parked "" "$(cat "$work/readonly.exit" 2>/dev/null)"
    main_client detach-client -t "$ro_client" >/dev/null 2>&1 || true
    sleep 1.2
    await_pid "$ro_prompt_pid" readonly-parked
    check_equal readonly-detach-exit 0 "$(cat "$work/readonly.exit" 2>/dev/null)"
    check_equal readonly-ran-nothing pending "$(value CP_READONLY)"
fi

if [ "$check_count" -ne 23 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g COMMAND_PROMPT_TARGET clean:23
else
    sed "s/^/command-prompt-target-$side: /" "$work/failures"
fi
