#!/bin/sh
set -eu

export LC_ALL=C

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    side=zz
else
    side=tmux
fi

work="$HOME/control-tilde-environment-$side"
mkdir -p "$work"

raw="$work/control.raw"
errors="$work/control.err"
input="$work/control.in"
: >"$raw"
: >"$errors"
rm -f "$input"

probe_socket=""
probe_label=""
probe_daemon_pid=""
control_pid=""

current_user=$(id -un)
passwd_home=$(python3 -c 'import os, pwd; print(pwd.getpwuid(os.getuid()).pw_dir)')
missing_user=zz_control_tilde_missing_618203

wait_for_process() {
    wait_pid=$1
    wait_limit=$2
    wait_attempt=0
    while kill -0 "$wait_pid" 2>/dev/null && [ "$wait_attempt" -lt "$wait_limit" ]; do
        wait_attempt=$((wait_attempt + 1))
        sleep 0.05
    done
    if kill -0 "$wait_pid" 2>/dev/null; then
        kill -TERM "$wait_pid" 2>/dev/null || true
        wait_attempt=0
        while kill -0 "$wait_pid" 2>/dev/null && [ "$wait_attempt" -lt 20 ]; do
            wait_attempt=$((wait_attempt + 1))
            sleep 0.05
        done
    fi
    if kill -0 "$wait_pid" 2>/dev/null; then
        kill -KILL "$wait_pid" 2>/dev/null || true
    fi
}

cleanup_probe() {
    cleanup_status=$?
    set +e
    exec 3>&- 2>/dev/null
    shutdown_pid=''
    if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
        if [ -n "$probe_socket" ]; then
            "$ZZ_SMOKE_ZZ_BIN" --socket "$probe_socket" kill-server >/dev/null 2>&1 &
            shutdown_pid=$!
        fi
    elif [ -n "$probe_label" ]; then
        "$ZZ_SMOKE_TMUX_BIN" -L "$probe_label" kill-server >/dev/null 2>&1 &
        shutdown_pid=$!
    fi
    [ -n "$shutdown_pid" ] && wait_for_process "$shutdown_pid" 40
    [ -n "$control_pid" ] && wait_for_process "$control_pid" 100
    [ -n "$probe_daemon_pid" ] && wait_for_process "$probe_daemon_pid" 100
    rm -f "$input"
    case "$probe_socket" in
    /tmp/zzcte-[0-9]*.sock) rm -f -- "$probe_socket" ;;
    esac
    return "$cleanup_status"
}
trap cleanup_probe EXIT
trap 'exit 1' HUP INT TERM

# The probe server owns HOME=/server/home; the Control client below owns
# HOME=/client/home. Every `~` the Control client types must read the server's.
if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    probe_socket="/tmp/zzcte-$$.sock"
    env HOME=/server/home "$ZZ_SMOKE_ZZ_BIN" --socket "$probe_socket" -f /dev/null daemon \
        >"$work/daemon.out" 2>"$work/daemon.err" &
    probe_daemon_pid=$!
    attempt=0
    until [ -S "$probe_socket" ]; do
        attempt=$((attempt + 1))
        if [ "$attempt" -ge 200 ] || ! kill -0 "$probe_daemon_pid" 2>/dev/null; then
            sed -n '1,120p' "$work/daemon.err" >&2
            exit 1
        fi
        sleep 0.05
    done
    "$ZZ_SMOKE_ZZ_BIN" --socket "$probe_socket" new-session -d -s w 'sleep 300'
else
    probe_label="zzcte-$$"
    env HOME=/server/home "$ZZ_SMOKE_TMUX_BIN" -L "$probe_label" -f /dev/null \
        new-session -d -s w 'sleep 300'
fi

control_client() {
    if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
        env -u TMUX -u TMUX_PANE HOME=/client/home \
            "$ZZ_SMOKE_ZZ_BIN" --socket "$probe_socket" \
            -C attach-session -t =w
    else
        env -u TMUX -u TMUX_PANE HOME=/client/home \
            "$ZZ_SMOKE_TMUX_BIN" -L "$probe_label" \
            -C attach-session -t =w
    fi
}

main_client() {
    if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
        "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" "$@"
    else
        "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" "$@"
    fi
}

wait_for_payload() {
    marker_attempt=0
    until grep -qFx "$1" "$raw"; do
        marker_attempt=$((marker_attempt + 1))
        if [ "$marker_attempt" -ge 300 ] || ! kill -0 "$control_pid" 2>/dev/null; then
            printf 'control marker not reached: %s\n' "$1" >&2
            return 1
        fi
        sleep 0.05
    done
}

# Rebase the command numbers on the first flags-1 frame so the two servers'
# global counters compare, and keep only framed command traffic.
control_timeline() {
    sed "s|$passwd_home|<PASSWD>|g" "$raw" | awk '
        function emit(terminator) {
            if (payload == "") payload = "_"
            printf "%s%s:%s:%s:%s", separator, number, flags, terminator, payload
            separator = "|"
            active = 0
            payload = ""
        }
        /^%begin [0-9]+ [0-9]+ 1$/ {
            if (base == 0) base = $3 - 1
            active = 1
            number = $3 - base
            flags = $4
            payload = ""
            next
        }
        /^%end [0-9]+ [0-9]+ [0-9]+$/ { if (active) emit("end"); active = 0; next }
        /^%error [0-9]+ [0-9]+ [0-9]+$/ { if (active) emit("error"); active = 0; next }
        active && !/^%/ {
            if (payload != "") payload = payload "+"
            payload = payload $0
        }
        END { print "" }
    '
}

mkfifo "$input"
control_client <"$input" >"$raw" 2>"$errors" &
control_pid=$!
exec 3>"$input"

# Each step waits for its own payload: the pin parses whatever stdin it has
# already buffered before running any of it, so a burst would read a stale
# server HOME on both sides for reasons that have nothing to do with the route.
printf 'display-message -p ~\n' >&3
wait_for_payload /server/home
printf 'display-message -p ~/x\n' >&3
wait_for_payload /server/home/x
printf 'display-message -p ~%s/named\n' "$current_user" >&3
wait_for_payload "$passwd_home/named"
printf "display-message -p '~'\n" >&3
wait_for_payload '~'
printf 'display-message -p ~%s\n' "$missing_user" >&3
wait_for_payload 'parse error: syntax error'
printf 'display-message -p ~ ; display-message -p ~%s\n' "$current_user" >&3
wait_for_payload "$passwd_home"
printf "set-environment -g HOME ''\n" >&3
printf 'display-message -p EMPTY_HOME_SET\n' >&3
wait_for_payload EMPTY_HOME_SET
printf 'display-message -p ~/empty\n' >&3
wait_for_payload "$passwd_home/empty"
printf 'set-environment -gu HOME\n' >&3
printf 'display-message -p UNSET_HOME_SET\n' >&3
wait_for_payload UNSET_HOME_SET
printf 'display-message -p ~/unset\n' >&3
wait_for_payload "$passwd_home/unset"

exec 3>&-
wait_for_process "$control_pid" 200
control_pid=''

timeline=$(control_timeline)
expected="1:1:end:/server/home\
|2:1:end:/server/home/x\
|3:1:end:<PASSWD>/named\
|4:1:end:~\
|5:1:error:parse error: syntax error\
|6:1:end:/server/home\
|7:1:end:<PASSWD>\
|8:1:end:_\
|9:1:end:EMPTY_HOME_SET\
|10:1:end:<PASSWD>/empty\
|11:1:end:_\
|12:1:end:UNSET_HOME_SET\
|13:1:end:<PASSWD>/unset"

result=broken
if [ "$timeline" = "$expected" ] && [ ! -s "$errors" ]; then
    result=clean:13
fi

main_client set-environment -g CONTROL_TILDE_ENVIRONMENT "$result"
