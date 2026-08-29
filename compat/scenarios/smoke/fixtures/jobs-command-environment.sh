#!/bin/sh
set -eu

export LC_ALL=C

startup_environment_matches() {
    test "$TERM" = outside-term &&
        test "$TERM_PROGRAM" = outside-program &&
        test "$TERM_PROGRAM_VERSION" = outside-version &&
        test "$COLORTERM" = outside-colour &&
        test -n "${JOB_EXPECTED_VERSION:-}" &&
        test -z "${TMUX_PANE+x}" &&
        case "$TMUX" in
        *,*,-1) true ;;
        *) false ;;
        esac
}

case "${1:-}" in
startup-run)
    startup_result=broken
    if startup_environment_matches; then
        startup_result=clean
    fi
    echo "$startup_result" >"$HOME/jobs-command-environment-work/startup-run-${2:-missing}"
    exit 0
    ;;
startup-condition)
    startup_environment_matches
    exit
    ;;
positive-startup)
    result=broken
    if test "$JOB_DELAY_STARTUP_GLOBAL" = launch-global &&
        test "$TERM" = "$3" &&
        test "$TERM_PROGRAM" = tmux &&
        test "$TERM_PROGRAM_VERSION" = "$JOB_EXPECTED_VERSION" &&
        test "$COLORTERM" = truecolor &&
        test -z "${ZZ_STARTUP_REENTRY+x}" &&
        test "${TMUX##*,}" = -1; then
        result=clean
    fi
    echo "$result" >"$HOME/jobs-command-environment-work/positive-startup-$2"
    exit 0
    ;;
positive-live)
    result=broken
    if test "$3" = scheduled-format &&
        test "$4" = scheduled-argument &&
        test "$(pwd -P)" = "$6" &&
        test "$JOB_DELAY_GLOBAL" = launch-global &&
        test "$JOB_DELAY_SESSION" = launch-session &&
        test -z "${JOB_DELAY_HIDDEN+x}" &&
        test -z "${JOB_DELAY_UNSET+x}" &&
        test "$TMUX_PANE" = launch-pane &&
        test "$TERM" = jobs-delay-live-terminal &&
        test "${TMUX##*,}" = "$5"; then
        result=clean
    fi
    echo "$result" >"$HOME/jobs-command-environment-work/positive-live-$2"
    exit 0
    ;;
positive-destroyed)
    result=broken
    if test "$3" = scheduled-format &&
        test "$4" = scheduled-argument &&
        test "$JOB_DELAY_DESTROYED_GLOBAL" = launch-global &&
        test "$JOB_DELAY_DESTROYED_SESSION" = before-kill &&
        test "$TMUX_PANE" = original-pane &&
        test "$TERM" = jobs-delay-destroyed-terminal &&
        test "${TMUX##*,}" = "$5" &&
        test "$(pwd -P)" != "$6" &&
        test -d "$(pwd -P)"; then
        result=clean
    fi
    echo "$result" >"$HOME/jobs-command-environment-work/positive-destroyed-$2"
    exit 0
    ;;
positive-missing)
    result=broken
    if test "$3" = scheduled-format &&
        test "$4" = scheduled-argument &&
        test "$JOB_DELAY_MISSING_GLOBAL" = launch-global &&
        test -z "${JOB_DELAY_MISSING_SESSION+x}" &&
        test "$TMUX_PANE" = launch-global-pane &&
        test "$TERM" = jobs-delay-missing-terminal &&
        test "${TMUX##*,}" = -1; then
        result=clean
    fi
    echo "$result" >"$HOME/jobs-command-environment-work/positive-missing-$2"
    exit 0
    ;;
esac

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    side=zz
    main_client() {
        "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" "$@"
    }
    cold_socket="/tmp/zzjce-$$.sock"
    cold_client() {
        env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
            "$ZZ_SMOKE_ZZ_BIN" --socket "$cold_socket" "$@"
    }
    version_line="$("$ZZ_SMOKE_ZZ_BIN" -V)"
else
    side=tmux
    main_client() {
        "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" "$@"
    }
    cold_label="zzjce-$$"
    cold_client() {
        env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
            "$ZZ_SMOKE_TMUX_BIN" -L "$cold_label" "$@"
    }
    version_line="$("$ZZ_SMOKE_TMUX_BIN" -V)"
fi

case "$version_line" in
tmux\ *) side_version=${version_line#tmux } ;;
*) side_version=invalid ;;
esac

work="$HOME/jobs-command-environment-work"
mkdir -p "$work"
: >"$work/failures-$side"
failed=0
check_count=0
cold_started=0
cold_pid=""
launch_canary="${ZZ_SMOKE_CANARY:-}"
default_terminal=""
live_cwd=""
removed_cwd=""

record_failure() {
    failed=1
    echo "$1" >>"$work/failures-$side"
}

check_equal() {
    check_count=$((check_count + 1))
    if [ "$2" != "$3" ]; then
        record_failure "$1"
    fi
}

wait_for_marker() {
    marker=$1
    value=""
    attempt=0
    while [ "$attempt" -lt 200 ]; do
        if [ -f "$marker" ]; then
            value="$(sed -n '1p' "$marker")"
            case "$value" in
            clean | broken) break ;;
            esac
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    printf '%s' "$value"
}

cleanup() {
    cleanup_status=$?
    trap - EXIT
    set +e
    for name in JOB_GLOBAL_ONLY JOB_PRECEDENCE JOB_GLOBAL_HIDDEN \
        JOB_GLOBAL_UNSET JOB_SESSION_HIDDEN JOB_SESSION_UNSET \
        JOB_EXPECTED_TERM JOB_EXPECTED_VERSION TMUX_PANE JOB_IF_RESULT \
        JOB_MISSING_IF_RESULT JOB_DELAY_STARTUP_GLOBAL JOB_DELAY_GLOBAL \
        JOB_DELAY_SESSION JOB_DELAY_HIDDEN JOB_DELAY_UNSET \
        JOB_DELAY_DESTROYED_GLOBAL JOB_DELAY_DESTROYED_SESSION \
        JOB_DELAY_MISSING_GLOBAL JOB_DELAY_MISSING_SESSION; do
        main_client set-environment -gu "$name" >/dev/null 2>&1
        main_client set-environment -u -t =w "$name" >/dev/null 2>&1
    done
    for option in @job_delay_live @job_delay_destroyed @job_delay_missing; do
        main_client set-option -gu "$option" >/dev/null 2>&1
    done
    for session in jobs-delay-live jobs-delay-destroyed jobs-delay-missing; do
        main_client kill-session -t "=$session" >/dev/null 2>&1
    done
    if [ -n "$default_terminal" ]; then
        main_client set-option -g default-terminal "$default_terminal" \
            >/dev/null 2>&1
    fi
    if [ -n "$live_cwd" ]; then
        rmdir -- "$live_cwd" >/dev/null 2>&1
    fi
    if [ -n "$removed_cwd" ]; then
        rmdir -- "$removed_cwd" >/dev/null 2>&1
    fi
    if [ -n "$launch_canary" ]; then
        main_client set-environment -g ZZ_SMOKE_CANARY "$launch_canary" \
            >/dev/null 2>&1
    fi
    if [ "$cold_started" -eq 1 ]; then
        cold_client kill-server >/dev/null 2>&1
    fi
    if [ -n "$cold_pid" ]; then
        kill "$cold_pid" >/dev/null 2>&1
        wait "$cold_pid" >/dev/null 2>&1
    fi
    if [ "$side" = zz ]; then
        case "$cold_socket" in
        /tmp/zzjce-[0-9]*.sock)
            rm -f -- "$cold_socket" "${cold_socket}.identity" "${cold_socket}.lock"
            ;;
        esac
    fi
    exit "$cleanup_status"
}
trap cleanup EXIT

if [ -z "$launch_canary" ] || [ "$side_version" = invalid ]; then
    record_failure launch-preconditions
fi

default_terminal="$(main_client show-options -gv default-terminal)"
main_client set-environment -gu ZZ_SMOKE_CANARY
main_client set-environment -g JOB_GLOBAL_ONLY global
main_client set-environment -g JOB_PRECEDENCE global
main_client set-environment -g -h JOB_GLOBAL_HIDDEN hidden
main_client set-environment -g JOB_GLOBAL_UNSET inherited
main_client set-environment -g -r JOB_GLOBAL_UNSET
main_client set-environment -g JOB_SESSION_UNSET inherited
main_client set-environment -g JOB_EXPECTED_TERM "$default_terminal"
main_client set-environment -g JOB_EXPECTED_VERSION "$side_version"
main_client set-environment -g TMUX_PANE global-pane
main_client set-environment -t =w JOB_PRECEDENCE session
main_client set-environment -h -t =w JOB_SESSION_HIDDEN hidden
main_client set-environment -r -t =w JOB_SESSION_UNSET
main_client set-environment -t =w TMUX_PANE session-pane

session_condition='test -z "${ZZ_SMOKE_CANARY+x}" &&
test "$JOB_GLOBAL_ONLY" = global &&
test "$JOB_PRECEDENCE" = session &&
test -z "${JOB_GLOBAL_HIDDEN+x}" &&
test -z "${JOB_GLOBAL_UNSET+x}" &&
test -z "${JOB_SESSION_HIDDEN+x}" &&
test -z "${JOB_SESSION_UNSET+x}" &&
test "$TMUX_PANE" = session-pane &&
test "$TERM" = "$JOB_EXPECTED_TERM" &&
test "$TERM_PROGRAM" = tmux &&
test "$TERM_PROGRAM_VERSION" = "$JOB_EXPECTED_VERSION" &&
test "$COLORTERM" = truecolor &&
case "$TMUX" in *,*,-1) false;; *,*,*) true;; *) false;; esac'

session_run=failed
if session_run_output="$(main_client run-shell "$session_condition && echo JOB_SESSION_RUN_CLEAN")"; then
    session_run=$session_run_output
fi
check_equal session-run JOB_SESSION_RUN_CLEAN "$session_run"

main_client set-environment -gu JOB_IF_RESULT
main_client if-shell "$session_condition" \
    'set-environment -g JOB_IF_RESULT clean' \
    'set-environment -g JOB_IF_RESULT broken'
session_if="$(main_client show-environment -g JOB_IF_RESULT 2>/dev/null || true)"
check_equal session-if JOB_IF_RESULT=clean "$session_if"

missing_condition='test -z "${ZZ_SMOKE_CANARY+x}" &&
test "$JOB_GLOBAL_ONLY" = global &&
test "$JOB_PRECEDENCE" = global &&
test -z "${JOB_GLOBAL_HIDDEN+x}" &&
test -z "${JOB_GLOBAL_UNSET+x}" &&
test -z "${JOB_SESSION_HIDDEN+x}" &&
test "$JOB_SESSION_UNSET" = inherited &&
test "$TMUX_PANE" = global-pane &&
test "$TERM" = "$JOB_EXPECTED_TERM" &&
test "$TERM_PROGRAM" = tmux &&
test "$TERM_PROGRAM_VERSION" = "$JOB_EXPECTED_VERSION" &&
test "$COLORTERM" = truecolor &&
case "$TMUX" in *,*,-1) true;; *) false;; esac'

missing_run=failed
if missing_run_output="$(main_client run-shell -t =jobs-explicit-missing: \
    "$missing_condition && echo JOB_MISSING_RUN_CLEAN")"; then
    missing_run=$missing_run_output
fi
check_equal missing-run JOB_MISSING_RUN_CLEAN "$missing_run"

main_client set-environment -gu JOB_MISSING_IF_RESULT
main_client if-shell -t =jobs-explicit-missing: "$missing_condition" \
    'set-environment -g JOB_MISSING_IF_RESULT clean' \
    'set-environment -g JOB_MISSING_IF_RESULT broken'
missing_if="$(main_client show-environment -g JOB_MISSING_IF_RESULT 2>/dev/null || true)"
check_equal missing-if JOB_MISSING_IF_RESULT=clean "$missing_if"

main_client kill-session -t =jobs-delay-live >/dev/null 2>&1 || true
main_client new-session -d -s jobs-delay-live
live_session_token="$(main_client display-message -p -t =jobs-delay-live: '#{session_id}')"
live_session_numeric=${live_session_token#\$}
case "$live_session_numeric" in
''|*[!0-9]*) record_failure positive-live-session-id ;;
esac
work_physical="$(cd "$work" && pwd -P)"
live_cwd="$work_physical/positive-live-cwd-$side-$$"
live_marker="$work/positive-live-$side"
rm -f -- "$live_marker"
main_client set-environment -g JOB_DELAY_GLOBAL scheduled-global
main_client set-environment -t =jobs-delay-live JOB_DELAY_SESSION scheduled-session
main_client set-environment -t =jobs-delay-live JOB_DELAY_HIDDEN scheduled-visible
main_client set-environment -t =jobs-delay-live JOB_DELAY_UNSET scheduled-visible
main_client set-environment -t =jobs-delay-live TMUX_PANE scheduled-pane
main_client set-option -g @job_delay_live scheduled-format
main_client set-option -g default-terminal jobs-delay-live-scheduled
live_command="sh \"\$HOME/jobs-command-environment.sh\" positive-live $side '#{@job_delay_live}' '#{1}' '$live_session_numeric' '#{2}'"
main_client run-shell -b -d 1.0 -c "$live_cwd" -t =jobs-delay-live: \
    "$live_command" scheduled-argument "$live_cwd"
mkdir "$live_cwd"
main_client set-environment -g JOB_DELAY_GLOBAL launch-global
main_client set-environment -t =jobs-delay-live JOB_DELAY_SESSION launch-session
main_client set-environment -h -t =jobs-delay-live JOB_DELAY_HIDDEN hidden
main_client set-environment -r -t =jobs-delay-live JOB_DELAY_UNSET
main_client set-environment -t =jobs-delay-live TMUX_PANE launch-pane
main_client set-option -g @job_delay_live launch-format
main_client set-option -g default-terminal jobs-delay-live-terminal
live_value="$(wait_for_marker "$live_marker")"
check_equal positive-live clean "$live_value"

main_client kill-session -t =jobs-delay-destroyed >/dev/null 2>&1 || true
main_client new-session -d -s jobs-delay-destroyed
destroyed_session_token="$(main_client display-message -p \
    -t =jobs-delay-destroyed: '#{session_id}')"
destroyed_session_numeric=${destroyed_session_token#\$}
case "$destroyed_session_numeric" in
''|*[!0-9]*) record_failure positive-destroyed-session-id ;;
esac
removed_cwd="$work_physical/positive-removed-cwd-$side-$$"
mkdir "$removed_cwd"
destroyed_marker="$work/positive-destroyed-$side"
rm -f -- "$destroyed_marker"
main_client set-environment -g JOB_DELAY_DESTROYED_GLOBAL scheduled-global
main_client set-environment -t =jobs-delay-destroyed \
    JOB_DELAY_DESTROYED_SESSION scheduled-session
main_client set-environment -t =jobs-delay-destroyed TMUX_PANE scheduled-pane
main_client set-option -g @job_delay_destroyed scheduled-format
main_client set-option -g default-terminal jobs-delay-destroyed-scheduled
destroyed_command="sh \"\$HOME/jobs-command-environment.sh\" positive-destroyed $side '#{@job_delay_destroyed}' '#{1}' '$destroyed_session_numeric' '#{2}'"
main_client run-shell -b -d 1.2 -c "$removed_cwd" -t =jobs-delay-destroyed: \
    "$destroyed_command" scheduled-argument "$removed_cwd"
main_client set-environment -t =jobs-delay-destroyed \
    JOB_DELAY_DESTROYED_SESSION before-kill
main_client set-environment -t =jobs-delay-destroyed TMUX_PANE original-pane
rmdir -- "$removed_cwd"
main_client kill-session -t =jobs-delay-destroyed
main_client new-session -d -s jobs-delay-destroyed
replacement_session_token="$(main_client display-message -p \
    -t =jobs-delay-destroyed: '#{session_id}')"
replacement_session_numeric=${replacement_session_token#\$}
case "$replacement_session_numeric" in
''|*[!0-9]*) record_failure positive-replacement-session-id ;;
esac
if [ "$replacement_session_numeric" = "$destroyed_session_numeric" ]; then
    record_failure positive-replacement-session-reuse
fi
main_client set-environment -t =jobs-delay-destroyed \
    JOB_DELAY_DESTROYED_SESSION replacement
main_client set-environment -t =jobs-delay-destroyed TMUX_PANE replacement-pane
main_client set-environment -g JOB_DELAY_DESTROYED_GLOBAL launch-global
main_client set-option -g @job_delay_destroyed launch-format
main_client set-option -g default-terminal jobs-delay-destroyed-terminal
destroyed_value="$(wait_for_marker "$destroyed_marker")"
check_equal positive-destroyed clean "$destroyed_value"

main_client kill-session -t =jobs-delay-missing >/dev/null 2>&1 || true
missing_marker="$work/positive-missing-$side"
rm -f -- "$missing_marker"
main_client set-environment -g JOB_DELAY_MISSING_GLOBAL scheduled-global
main_client set-environment -g TMUX_PANE scheduled-global-pane
main_client set-option -g @job_delay_missing scheduled-format
main_client set-option -g default-terminal jobs-delay-missing-scheduled
missing_command="sh \"\$HOME/jobs-command-environment.sh\" positive-missing $side '#{@job_delay_missing}' '#{1}'"
main_client run-shell -b -d 1.0 -t =jobs-delay-missing: \
    "$missing_command" scheduled-argument
main_client new-session -d -s jobs-delay-missing
main_client set-environment -t =jobs-delay-missing \
    JOB_DELAY_MISSING_SESSION replacement
main_client set-environment -t =jobs-delay-missing TMUX_PANE replacement-pane
main_client set-environment -g JOB_DELAY_MISSING_GLOBAL launch-global
main_client set-environment -g TMUX_PANE launch-global-pane
main_client set-option -g @job_delay_missing launch-format
main_client set-option -g default-terminal jobs-delay-missing-terminal
missing_value="$(wait_for_marker "$missing_marker")"
check_equal positive-missing clean "$missing_value"

cold_config="$work/cold-$side.conf"
positive_startup_marker="$work/positive-startup-$side"
rm -f -- "$positive_startup_marker"
{
    echo "run-shell 'sh \"\$HOME/jobs-command-environment.sh\" startup-run $side'"
    echo "if-shell 'sh \"\$HOME/jobs-command-environment.sh\" startup-condition' 'set-environment -g JOB_STARTUP_IF clean' 'set-environment -g JOB_STARTUP_IF broken'"
    echo "set-environment -g JOB_DELAY_STARTUP_GLOBAL scheduled-global"
    echo "set-option -g default-terminal jobs-delay-startup-scheduled"
    echo "run-shell -b -d 1.0 'sh \"\$HOME/jobs-command-environment.sh\" positive-startup $side jobs-delay-startup-terminal'"
    echo "set-environment -g JOB_DELAY_STARTUP_GLOBAL launch-global"
    echo "set-option -g default-terminal jobs-delay-startup-terminal"
} >"$cold_config"

if [ "$side" = zz ]; then
    env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
        TERM=outside-term TERM_PROGRAM=outside-program \
        TERM_PROGRAM_VERSION=outside-version COLORTERM=outside-colour \
        JOB_EXPECTED_VERSION="$side_version" \
        "$ZZ_SMOKE_ZZ_BIN" --socket "$cold_socket" -f "$cold_config" daemon \
        >"$work/cold-$side.out" 2>"$work/cold-$side.err" &
    cold_pid=$!
    cold_started=1
    socket_ready=0
    socket_attempt=0
    while [ "$socket_attempt" -lt 200 ]; do
        if [ -S "$cold_socket" ]; then
            socket_ready=1
            break
        fi
        if ! kill -0 "$cold_pid" 2>/dev/null; then
            break
        fi
        socket_attempt=$((socket_attempt + 1))
        sleep 0.05
    done
    if [ "$socket_ready" -eq 1 ]; then
        cold_client new-session -d -s jobs-cold
    else
        record_failure cold-socket
    fi
else
    cold_started=1
    if ! env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
        TERM=outside-term TERM_PROGRAM=outside-program \
        TERM_PROGRAM_VERSION=outside-version COLORTERM=outside-colour \
        JOB_EXPECTED_VERSION="$side_version" \
        "$ZZ_SMOKE_TMUX_BIN" -L "$cold_label" -f "$cold_config" \
        new-session -d -s jobs-cold \
        >"$work/cold-$side.out" 2>"$work/cold-$side.err"; then
        record_failure cold-start
    fi
fi

startup_marker="$work/startup-run-$side"
startup_value="$(wait_for_marker "$startup_marker")"
check_equal startup-run clean "$startup_value"

startup_if="$(cold_client show-environment -g JOB_STARTUP_IF 2>/dev/null || true)"
check_equal startup-if JOB_STARTUP_IF=clean "$startup_if"

positive_startup_value="$(wait_for_marker "$positive_startup_marker")"
check_equal positive-startup clean "$positive_startup_value"

cold_default_terminal="$(cold_client show-options -gv default-terminal 2>/dev/null || true)"
post_condition='test "$TERM" = "$JOB_POST_EXPECTED_TERM" &&
test "$TERM_PROGRAM" = tmux &&
test "$TERM_PROGRAM_VERSION" = "$JOB_EXPECTED_VERSION" &&
test "$COLORTERM" = truecolor &&
test -z "${TMUX_PANE+x}" &&
case "$TMUX" in *,*,-1) false;; *,*,*) true;; *) false;; esac'
cold_client set-environment -g JOB_POST_EXPECTED_TERM "$cold_default_terminal"
post_run=failed
if post_run_output="$(cold_client run-shell "$post_condition && echo JOB_POST_RUN_CLEAN")"; then
    post_run=$post_run_output
fi
check_equal post-run JOB_POST_RUN_CLEAN "$post_run"

cold_client set-environment -gu JOB_POST_IF_RESULT
cold_client if-shell "$post_condition" \
    'set-environment -g JOB_POST_IF_RESULT clean' \
    'set-environment -g JOB_POST_IF_RESULT broken'
post_if="$(cold_client show-environment -g JOB_POST_IF_RESULT 2>/dev/null || true)"
check_equal post-if JOB_POST_IF_RESULT=clean "$post_if"

if [ "$check_count" -ne 12 ]; then
    record_failure total-checks
fi
if [ "$failed" -eq 0 ]; then
    main_client set-environment -g JOBS_COMMAND_ENVIRONMENT clean:12
else
    sed "s/^/jobs-command-environment-$side: /" "$work/failures-$side"
fi
