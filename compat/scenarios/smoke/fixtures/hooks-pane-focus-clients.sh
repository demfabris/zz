#!/bin/sh
set -eu

export LC_ALL=C

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    side=zz
    binary="$ZZ_SMOKE_ZZ_BIN"
    probe_socket="/tmp/zzpfc-$$.sock"
    probe_args="--socket $probe_socket"
else
    side=tmux
    binary="$ZZ_SMOKE_TMUX_BIN"
    probe_label="zzpfc-$$"
    probe_args="-L $probe_label"
    probe_socket=""
fi

work="$HOME/hooks-pane-focus-clients-$side"
rm -rf "$work"
mkdir -p "$work/first" "$work/second"
log="$work/log"
: >"$log"

probe_daemon_pid=""
first_pid=""
second_pid=""
first_step=0
second_step=0

main_client() {
    if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
        "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" "$@"
    else
        "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" "$@"
    fi
}

probe() {
    # shellcheck disable=SC2086
    "$binary" $probe_args "$@"
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
    stop "$first_pid"
    stop "$second_pid"
    stop "$probe_daemon_pid"
    case "$probe_socket" in
    /tmp/zzpfc-[0-9]*.sock) rm -f -- "$probe_socket" ;;
    esac
    exit "$cleanup_status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
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
    probe -f /dev/null new-session -d -s pf -x 80 -y 24 'sleep 300'
fi
probe new-session -d -s other -x 80 -y 24 'sleep 300'
probe set-option -g focus-events on
for hook in pane-focus-in pane-focus-out client-focus-in client-focus-out \
    client-session-changed; do
    probe set-hook -g "$hook" \
        "run-shell \"printf '%s:%s\\n' '$hook' '#{pane_id}' >>$log\""
done

mark() {
    printf '|%s|\n' "$1" >>"$log"
}

attach() {
    env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
        TERM=xterm-256color \
        python3 "$HOME/pty-drive.py" "$work/$1" 80 24 \
        $binary $probe_args attach-session -t "=pf" >"$work/$1.out" 2>&1 &
}

await_clients() {
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if [ "$(probe list-clients -t "=pf" -F x 2>/dev/null | wc -l | tr -d ' ')" = "$1" ]; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    printf 'attach-timeout:%s\n' "$1" >>"$log"
}

drive() {
    if [ "$1" = first ]; then
        first_step=$((first_step + 1))
        step_dir="$work/first"
        step_number=$first_step
    else
        second_step=$((second_step + 1))
        step_dir="$work/second"
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
    sleep 0.9
}

focus_in=1b5b49
focus_out=1b5b4f

attach first
first_pid=$!
await_clients 1
attach second
second_pid=$!
await_clients 2
sleep 0.9
mark attached

# The pane flag is the OR across attached, focused, overlay-free clients, so
# only the last client to lose focus and the first to regain it move it, and
# window_update_focus runs before the client-focus-out notify and after the
# client-focus-in one.
drive first "keys $focus_out"
mark first-out
drive second "keys $focus_out"
mark second-out
drive first "keys $focus_in"
mark first-in
drive second "keys $focus_in"
mark second-in
drive second "keys $focus_out"
mark second-out-again

# server_client_set_overlay and server_client_clear_overlay evaluate the
# client's current window unconditionally, and the only focused client left is
# the one taking the overlay.
first_tty="$(probe list-clients -t "=pf" -F '#{client_tty}' | sed -n '1p')"
probe display-panes -t "$first_tty" -d 0 'select-pane -t %%' >/dev/null 2>&1 &
sleep 1.2
mark overlay-open
drive first "keys 30"
mark overlay-closed

# server_client_set_session evaluates the old session's window and then the new
# one, both ahead of the client-session-changed notify.
probe switch-client -c "$first_tty" -t other
sleep 0.9
mark switch-away
probe switch-client -c "$first_tty" -t pf
sleep 0.9
mark switch-back
probe detach-client -s pf
sleep 1.2
mark detach

drive first quit
drive second quit
main_client set-environment -g HOOKS_PANE_FOCUS_CLIENTS "$(tr '\n' ' ' <"$log")"
