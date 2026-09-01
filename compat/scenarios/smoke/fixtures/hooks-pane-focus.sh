#!/bin/sh
set -eu

export LC_ALL=C

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    side=zz
    binary="$ZZ_SMOKE_ZZ_BIN"
else
    side=tmux
    binary="$ZZ_SMOKE_TMUX_BIN"
fi

work="$HOME/hooks-pane-focus-$side"
rm -rf "$work"
mkdir -p "$work/s1" "$work/s2"
log="$work/log"
: >"$log"

probe_socket=""
probe_label=""
probe_daemon_pid=""
first_pid=""
second_pid=""

main_client() {
    if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
        "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" "$@"
    else
        "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" "$@"
    fi
}

probe_command() {
    if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
        "$binary" --socket "$probe_socket" "$@"
    else
        "$binary" -L "$probe_label" "$@"
    fi
}

stop() {
    stop_pid=$1
    [ -n "$stop_pid" ] || return 0
    kill "$stop_pid" >/dev/null 2>&1 || true
    wait "$stop_pid" >/dev/null 2>&1 || true
}

cleanup() {
    cleanup_status=$?
    trap - EXIT
    set +e
    probe_command kill-server >/dev/null 2>&1
    stop "$first_pid"
    stop "$second_pid"
    stop "$probe_daemon_pid"
    case "$probe_socket" in
    /tmp/zzpf-[0-9]*.sock) rm -f -- "$probe_socket" ;;
    esac
    exit "$cleanup_status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    probe_socket="/tmp/zzpf-$$.sock"
    "$binary" --socket "$probe_socket" -f /dev/null daemon \
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
    probe_command new-session -d -s pf 'sleep 300'
else
    probe_label="zzpf-$$"
    "$binary" -L "$probe_label" -f /dev/null new-session -d -s pf 'sleep 300'
fi

probe_command set-option -g focus-events on
for hook in pane-focus-in pane-focus-out; do
    probe_command set-hook -g "$hook" \
        "run-shell \"printf '$hook %s hc=[%s]\\n' '#{pane_id}' '#{hook_client}' >> $log\""
done

attach() {
    env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
        TERM=xterm-256color \
        python3 "$HOME/pty-drive.py" "$1" 80 24 \
        "$binary" $2 attach-session -t "=pf" >"$3" 2>&1 &
}

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    attach_args="--socket $probe_socket"
else
    attach_args="-L $probe_label"
fi

# shellcheck disable=SC2086
attach "$work/s1" "$attach_args" "$work/a1.out"
first_pid=$!
sleep 2
# shellcheck disable=SC2086
attach "$work/s2" "$attach_args" "$work/a2.out"
second_pid=$!
sleep 2

first_step=0
second_step=0
drive() {
    if [ "$1" = 1 ]; then
        first_step=$((first_step + 1))
        step_dir="$work/s1"
        step_number=$first_step
    else
        second_step=$((second_step + 1))
        step_dir="$work/s2"
        step_number=$second_step
    fi
    printf '%s\n' "$2" >"$step_dir/step-$step_number"
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if [ -f "$step_dir/ack-$step_number" ]; then
            break
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    sleep 0.8
}

focus_in=1b5b49
focus_out=1b5b4f

# The pane flag is the OR across focused clients, so only the first client that
# gains focus and the last one that loses it may move it.
drive 1 "keys $focus_in"
drive 1 "keys $focus_out"
drive 2 "keys $focus_out"
drive 1 "keys $focus_in"
drive 2 "keys $focus_in"
drive 2 "keys $focus_out"
drive 1 "keys $focus_out"

drive 1 quit
drive 2 quit

timeline=$(tr '\n' '|' <"$log")
expected="pane-focus-in %0 hc=[]|pane-focus-out %0 hc=[]|pane-focus-in %0 hc=[]|pane-focus-out %0 hc=[]|"

result=broken
if [ "$timeline" = "$expected" ]; then
    result=clean:4
fi

main_client set-environment -g HOOKS_PANE_FOCUS "$result"
