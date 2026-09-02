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
mkdir -p "$work"
log="$work/log"

probe_socket=""
probe_label=""
probe_daemon_pid=""
attach_pid=""
step=0

main_client() {
    if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
        "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" "$@"
    else
        "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" "$@"
    fi
}

probe() {
    if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
        "$binary" --socket "$probe_socket" "$@"
    else
        "$binary" -L "$probe_label" "$@"
    fi
}

stop() {
    [ -n "$1" ] || return 0
    kill "$1" >/dev/null 2>&1 || true
    wait "$1" >/dev/null 2>&1 || true
}

cleanup() {
    cleanup_status=$?
    trap - EXIT
    set +e
    probe kill-server >/dev/null 2>&1
    stop "$attach_pid"
    stop "$probe_daemon_pid"
    case "$probe_socket" in
    /tmp/zzpf-[0-9]*.sock) rm -f -- "$probe_socket" ;;
    esac
    exit "$cleanup_status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

start_probe() {
    if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
        probe_socket="/tmp/zzpf-$$.sock"
        "$binary" --socket "$probe_socket" -f /dev/null daemon \
            >"$work/daemon.out" 2>"$work/daemon.err" &
        probe_daemon_pid=$!
        attempt=0
        until [ -S "$probe_socket" ]; do
            attempt=$((attempt + 1))
            if [ "$attempt" -ge 400 ] || ! kill -0 "$probe_daemon_pid" 2>/dev/null; then
                sed -n '1,120p' "$work/daemon.err" >&2
                exit 1
            fi
            sleep 0.05
        done
        probe new-session -d -s pf -x 80 -y 24 'sleep 300'
    else
        probe_label="zzpf-$$"
        "$binary" -L "$probe_label" -f /dev/null \
            new-session -d -s pf -x 80 -y 24 'sleep 300'
    fi
    probe split-window -d -t pf:0 'sleep 300'
}

stop_probe() {
    probe kill-server >/dev/null 2>&1 || true
    stop "$probe_daemon_pid"
    probe_daemon_pid=""
    case "$probe_socket" in
    /tmp/zzpf-[0-9]*.sock) rm -f -- "$probe_socket" ;;
    esac
    probe_socket=""
    probe_label=""
}

# Every hook body is a blocking run-shell, so the log is the queue order.
install_hooks() {
    for hook in pane-focus-in pane-focus-out window-pane-changed \
        session-window-changed client-session-changed client-attached \
        client-detached; do
        probe set-hook -g "$hook" \
            "run-shell \"printf '%s:%s\\n' '$hook' '#{pane_id}#{window_id}' >>$log\""
    done
}

drive() {
    step=$((step + 1))
    printf '%s\n' "$1" >"$work/steps/step-$step"
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if [ -f "$work/steps/ack-$step" ]; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    printf 'drive-timeout\n' >>"$log"
    return 0
}

mark() {
    printf '|%s|\n' "$1" >>"$log"
}

settle() {
    sleep 0.7
}

walk() {
    rm -rf "$work/steps"
    mkdir -p "$work/steps"
    step=0
    : >"$log"
    start_probe
    probe set-option -g focus-events "$1"
    install_hooks
    env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
        TERM=xterm-256color \
        python3 "$HOME/pty-drive.py" "$work/steps" 80 24 \
        "$binary" $2 attach-session -t "=pf" >"$work/attach-$1.out" 2>&1 &
    attach_pid=$!
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if [ -n "$(probe list-clients -t "=pf" -F '#{client_tty}' 2>/dev/null)" ]; then
            break
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    settle
    mark attach
    probe select-pane -t pf:0.1
    settle
    mark select-pane
    probe new-window -n two -t pf:
    settle
    mark new-window
    probe select-window -t pf:0
    settle
    mark select-window
    probe kill-pane -t pf:0.1
    settle
    mark kill-pane
    probe detach-client -s pf
    settle
    mark detach
    drive quit
    stop "$attach_pid"
    attach_pid=""
    stop_probe
    tr '\n' ' ' <"$log"
}

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    attach_args="--socket /tmp/zzpf-$$.sock"
else
    attach_args="-L zzpf-$$"
fi

# shellcheck disable=SC2086
off="$(walk off "$attach_args")"
# shellcheck disable=SC2086
on="$(walk on "$attach_args")"

main_client set-environment -g HOOKS_PANE_FOCUS_OFF "$off"
main_client set-environment -g HOOKS_PANE_FOCUS_ON "$on"
