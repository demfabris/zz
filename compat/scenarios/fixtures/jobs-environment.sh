#!/bin/sh
set -eu

export LC_ALL=C

case "${1:-}" in
copy-check)
    marker=$2
    expected_version=$3
    expected_session=$4
    expected_cwd=$5
    selection="$(cat)"
    result=broken
    if test "$JOB_COPY_GLOBAL" = global &&
        test "$JOB_COPY_PRECEDENCE" = session &&
        test "$JOB_COPY_SESSION" = session &&
        test -z "${JOB_COPY_HIDDEN+x}" &&
        test -z "${JOB_COPY_UNSET+x}" &&
        test "$TERM" = copy-pipe-terminal &&
        test "$TERM_PROGRAM" = tmux &&
        test "$TERM_PROGRAM_VERSION" = "$expected_version" &&
        test "$COLORTERM" = truecolor &&
        test "${TMUX##*,}" = "$expected_session" &&
        test "$(pwd -P)" = "$expected_cwd" &&
        test -n "$selection"; then
        result=clean
    fi
    printf '%s' "$result" >"$marker"
    exit 0
    ;;
esac

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

script="$HOME/jobs-environment.sh"
attach="$HOME/pty-attach.py"
session=copy-pipe-env
work="$HOME/jobs-environment-work-$side"
rm -rf "$work"
mkdir -p "$work"
: >"$work/failures"
failed=0
check_count=0
default_terminal=""
attach_pid=""

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
    main_client kill-session -t "=$session" >/dev/null 2>&1
    if [ -n "$attach_pid" ]; then
        kill "$attach_pid" >/dev/null 2>&1
        wait "$attach_pid" >/dev/null 2>&1
    fi
    for name in JOB_COPY_GLOBAL JOB_COPY_PRECEDENCE JOB_COPY_HIDDEN \
        JOB_COPY_UNSET JOB_COPY_SESSION; do
        main_client set-environment -gu "$name" >/dev/null 2>&1
    done
    if [ -n "$default_terminal" ]; then
        main_client set-option -g default-terminal "$default_terminal" \
            >/dev/null 2>&1
    fi
    exit "$cleanup_status"
}
trap cleanup EXIT

version_line="$("$binary" -V)"
case "$version_line" in
tmux\ *) side_version=${version_line#tmux } ;;
*) side_version=invalid ;;
esac
if [ "$side_version" = invalid ]; then
    record_failure version-line
fi

default_terminal="$(main_client show-options -gv default-terminal)"
main_client set-environment -g JOB_COPY_GLOBAL global
main_client set-environment -g JOB_COPY_PRECEDENCE global
main_client set-environment -g -h JOB_COPY_HIDDEN hidden
main_client set-environment -g JOB_COPY_UNSET inherited
main_client set-environment -g -r JOB_COPY_UNSET

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session"
main_client set-environment -t "=$session" JOB_COPY_PRECEDENCE session
main_client set-environment -t "=$session" JOB_COPY_SESSION session
main_client set-option -g default-terminal copy-pipe-terminal
session_token="$(main_client display-message -p -t "=$session:" '#{session_id}')"
session_numeric=${session_token#\$}

env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    TERM=xterm-256color \
    python3 "$attach" "$binary" $prefix_args attach-session -t "=$session" \
    >"$work/attach.out" 2>&1 &
attach_pid=$!

attempt=0
while [ "$attempt" -lt 300 ]; do
    if [ -n "$(main_client list-clients -t "=$session" -F '#{client_name}')" ]; then
        break
    fi
    attempt=$((attempt + 1))
    sleep 0.05
done
if [ "$attempt" -ge 300 ]; then
    record_failure attach-client
fi

main_client send-keys -t "=$session:0.0" 'printf COPY_PIPE_PAYLOAD; printf "\n"' Enter
attempt=0
while [ "$attempt" -lt 300 ]; do
    if main_client capture-pane -p -t "=$session:0.0" |
        grep -q '^COPY_PIPE_PAYLOAD$'; then
        break
    fi
    attempt=$((attempt + 1))
    sleep 0.05
done
if [ "$attempt" -ge 300 ]; then
    record_failure payload
fi

marker="$work/copy-check"
server_physical="$(pwd -P)"
main_client copy-mode -t "=$session:0.0"
main_client send-keys -X -t "=$session:0.0" top-line
main_client send-keys -X -t "=$session:0.0" begin-selection
main_client send-keys -X -t "=$session:0.0" end-of-line
main_client send-keys -X -t "=$session:0.0" copy-pipe \
    "sh '$script' copy-check '$marker' '$side_version' '$session_numeric' '$server_physical'"

attempt=0
while [ "$attempt" -lt 300 ]; do
    if [ -s "$marker" ]; then
        break
    fi
    attempt=$((attempt + 1))
    sleep 0.05
done
check_equal copy-pipe-environment clean "$(cat "$marker" 2>/dev/null)"

if [ "$check_count" -ne 1 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g JOBS_ENVIRONMENT clean:1
else
    sed "s/^/jobs-environment-$side: /" "$work/failures"
fi
