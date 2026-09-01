#!/bin/sh
set -eu

export LC_ALL=C

case "${1:-}" in
popup-overrides)
    marker=$2
    cwd=$3
    result=broken
    if test "$POPUP_GLOBAL" = global &&
        test "$POPUP_PRECEDENCE" = session &&
        test "$POPUP_EXTRA" = extra &&
        test "$POPUP_REPEATED" = last &&
        test "$POPUP_HIDDEN" = unhidden &&
        test -z "${POPUP_NO_EQUALS+x}" &&
        test "$TMUX" = popup-tmux &&
        test "$TERM" = popup-term &&
        test "$PWD" = "$cwd" &&
        test "$(pwd -P)" = "$cwd"; then
        result=clean
    fi
    printf '%s' "$result" >"$marker"
    exit 0
    ;;
popup-scope)
    marker=$2
    expected_version=$3
    expected_session=$4
    expected_terminal=$5
    result=broken
    if test "$POPUP_GLOBAL" = global &&
        test "$POPUP_PRECEDENCE" = session &&
        test "$POPUP_SESSION" = session &&
        test -z "${POPUP_HIDDEN+x}" &&
        test -z "${POPUP_UNSET+x}" &&
        test "$TERM" = "$expected_terminal" &&
        test "$TERM_PROGRAM" = tmux &&
        test "$TERM_PROGRAM_VERSION" = "$expected_version" &&
        test "$COLORTERM" = truecolor &&
        test "$TMUX_PANE" = global-pane &&
        test "${TMUX##*,}" = "$expected_session"; then
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

script="$HOME/jobs-display-popup-environment.sh"
session=popup-env
work="$HOME/jobs-display-popup-environment-work-$side"
steps="$work/steps"
rm -rf "$work"
mkdir -p "$steps"
: >"$work/failures"
failed=0
check_count=0
attach_pid=""
client=""
default_terminal=""

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
    if [ -n "$client" ]; then
        main_client display-popup -c "$client" -C >/dev/null 2>&1
    fi
    echo quit >"$steps/step-1" 2>/dev/null
    main_client kill-session -t "=$session" >/dev/null 2>&1
    if [ -n "$attach_pid" ]; then
        kill "$attach_pid" >/dev/null 2>&1
        wait "$attach_pid" >/dev/null 2>&1
    fi
    for name in POPUP_GLOBAL POPUP_PRECEDENCE POPUP_HIDDEN POPUP_UNSET TMUX_PANE; do
        main_client set-environment -gu "$name" >/dev/null 2>&1
    done
    if [ -n "$default_terminal" ]; then
        main_client set-option -g default-terminal "$default_terminal" >/dev/null 2>&1
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
main_client set-environment -g POPUP_GLOBAL global
main_client set-environment -g POPUP_PRECEDENCE global
main_client set-environment -g POPUP_UNSET inherited
main_client set-environment -g -r POPUP_UNSET
main_client set-environment -g TMUX_PANE global-pane

main_client kill-session -t "=$session" >/dev/null 2>&1 || true
main_client new-session -d -s "$session"
main_client set-environment -t "=$session" POPUP_PRECEDENCE session
main_client set-environment -t "=$session" POPUP_SESSION session
main_client set-environment -h -t "=$session" POPUP_HIDDEN hidden
main_client set-option -g default-terminal popup-env-terminal
session_token="$(main_client display-message -p -t "=$session:" '#{session_id}')"
session_numeric=${session_token#\$}

env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    TERM=xterm-256color \
    python3 "$HOME/pty-drive.py" "$steps" 80 24 \
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
    echo "jobs-display-popup-environment-$side: attach-client"
    exit 0
fi

popup_cwd="$(cd "$work" && pwd -P)/cwd"
mkdir -p "$popup_cwd"
overrides_marker="$work/overrides"
rm -f "$overrides_marker"
overrides_status=0
main_client display-popup -c "$client" -E -d "$popup_cwd" \
    -e POPUP_EXTRA=extra \
    -e POPUP_REPEATED=first \
    -e POPUP_REPEATED=last \
    -e POPUP_HIDDEN=unhidden \
    -e TMUX=popup-tmux \
    -e TERM=popup-term \
    -e PWD=/nowhere \
    -e POPUP_NO_EQUALS \
    -e =nameless \
    "sh '$script' popup-overrides '$overrides_marker' '$popup_cwd'" ||
    overrides_status=$?
check_equal overrides-exit 0 "$overrides_status"
check_equal overrides clean "$(cat "$overrides_marker" 2>/dev/null || true)"

scope_marker="$work/scope"
rm -f "$scope_marker"
scope_status=0
main_client display-popup -c "$client" -E -d "$popup_cwd" \
    "sh '$script' popup-scope '$scope_marker' '$side_version' '$session_numeric' popup-env-terminal" ||
    scope_status=$?
check_equal scope-exit 0 "$scope_status"
check_equal scope clean "$(cat "$scope_marker" 2>/dev/null || true)"

if [ "$check_count" -ne 4 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g JOBS_DISPLAY_POPUP_ENVIRONMENT clean:4
else
    sed "s/^/jobs-display-popup-environment-$side: /" "$work/failures"
fi
