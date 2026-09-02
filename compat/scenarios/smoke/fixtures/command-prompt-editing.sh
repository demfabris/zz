#!/bin/sh
set -eu

export LC_ALL=C

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    side=zz
    binary="$ZZ_SMOKE_ZZ_BIN"
    probe_socket="/tmp/zzcpe-$$.sock"
    probe_args="--socket $probe_socket"
else
    side=tmux
    binary="$ZZ_SMOKE_TMUX_BIN"
    probe_label="zzcpe-$$"
    probe_args="-L $probe_label"
    probe_socket=""
fi

work="$HOME/command-prompt-editing-$side"
rm -rf "$work"
mkdir -p "$work/steps"
log="$work/log"
: >"$log"

probe_daemon_pid=""
attach_pid=""
step=0
client=""

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
    stop "$prompt_pid"
    stop "$attach_pid"
    stop "$probe_daemon_pid"
    case "$probe_socket" in
    /tmp/zzcpe-[0-9]*.sock) rm -f -- "$probe_socket" ;;
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
    probe new-session -d -s prompt -x 80 -y 24 'sleep 3000'
else
    probe -f /dev/null new-session -d -s prompt -x 80 -y 24 'sleep 3000'
fi

env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
    TERM=xterm-256color \
    python3 "$HOME/pty-drive.py" "$work/steps" 80 24 \
    $binary $probe_args attach-session -t "=prompt" >"$work/attach.out" 2>&1 &
attach_pid=$!
attempt=0
while [ "$attempt" -lt 400 ]; do
    client="$(probe list-clients -t "=prompt" -F '#{client_tty}' 2>/dev/null | sed -n '1p')"
    if [ -n "$client" ]; then
        break
    fi
    attempt=$((attempt + 1))
    sleep 0.05
done
if [ -z "$client" ]; then
    printf 'attach-failed\n' >>"$log"
fi
sleep 0.6

drive() {
    step=$((step + 1))
    printf 'keys %s\n' "$1" >"$work/steps/step-$step"
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if [ -f "$work/steps/ack-$step" ]; then
            break
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    sleep 0.35
}

value() {
    row="$(probe show-environment -g "$1" 2>/dev/null || true)"
    printf '%s' "${row#"$1"=}"
}

row() {
    printf '%s=[%s]\n' "$1" "$2" >>"$log"
}

# There is no format for "a prompt is open", so every step raises a parked
# prompt in its own background process and waits for that process to exit.
# CMD_RETURN_WAIT holds it until the prompt is answered, cancelled or freed, so
# the wait is exactly "the prompt closed" and no row can start while the last
# one is still up.
prompt_pid=""

raise_prompt() {
    (
        # shellcheck disable=SC2086
        "$binary" $probe_args command-prompt -t "$client" "$@" >/dev/null 2>&1
    ) &
    prompt_pid=$!
    sleep 0.8
}

await_prompt_close() {
    attempt=0
    while [ "$attempt" -lt 400 ]; do
        if ! kill -0 "$prompt_pid" 2>/dev/null; then
            wait "$prompt_pid" >/dev/null 2>&1 || true
            prompt_pid=""
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.05
    done
    printf 'prompt-never-closed\n' >>"$log"
    kill "$prompt_pid" >/dev/null 2>&1 || true
    wait "$prompt_pid" >/dev/null 2>&1 || true
    prompt_pid=""
}

# `-k` answers with `key_string_lookup_key`'s own spelling, which is not the
# key's typed text: 0x20 is Space, 0x08 is C-h and never BSpace, and CSI Z is
# BTab and never a shifted Tab.
key_answer() {
    probe set-environment -gu KEY >/dev/null 2>&1 || true
    raise_prompt -k -p 'k ' "set-environment -g KEY '%%'"
    drive "$2"
    await_prompt_close
    row "$1" "$(value KEY)"
}
key_answer space 20
key_answer control-a 01
key_answer control-h 08
key_answer meta-x 1b78
key_answer f1 1b4f50
key_answer up 1b5b41
key_answer bspace 7f
key_answer letter 61
key_answer enter 0d
key_answer escape 1b
key_answer tab 09
key_answer btab 1b5b5a
key_answer delete 1b5b337e
key_answer tilde 7e
key_answer semicolon 3b

# `prompt_set_options` reads status-keys and word-separators once, at prompt
# creation, and `prompt_translate_key` is the whole vi table on top of the
# emacs one.
# `sh` has no local variables, so nothing here may reuse a caller's name.
edit_answer() {
    edit_label=$1
    edit_keys=$2
    edit_separators=$3
    edit_initial=$4
    shift 4
    probe set-option -g status-keys "$edit_keys" >/dev/null
    probe set-option -g word-separators "$edit_separators" >/dev/null
    probe set-environment -gu EDIT >/dev/null 2>&1 || true
    raise_prompt -I "$edit_initial" -p 'e ' "set-environment -g EDIT '%%'"
    for key in "$@"; do
        drive "$key"
    done
    drive 0d
    await_prompt_close
    row "$edit_label" "$(value EDIT)"
}

separators='!"#$%&'"'"'()*+,-./:;<=>?@[\]^`{|}~'
edit_answer vi-command-mode-home vi "$separators" 'aaa bbb-ccc ddd' 1b 30 69 58
edit_answer vi-backward-word vi "$separators" 'aaa bbb-ccc' 1b 62 44
edit_answer vi-backward-big-word vi "$separators" 'aaa bbb-ccc' 1b 42 44
edit_answer vi-custom-separator vi '_' 'foo_bar' 1b 62 44
edit_answer vi-default-separators vi "$separators" 'foo_bar' 1b 62 44
edit_answer vi-end-word vi "$separators" 'aaa-bbb ccc' 1b 30 65 44
edit_answer vi-end-big-word vi "$separators" 'aaa-bbb ccc' 1b 30 45 44
edit_answer vi-forward-word vi "$separators" 'aaa-bbb ccc' 1b 30 77 44
edit_answer vi-forward-big-word vi "$separators" 'aaa-bbb ccc' 1b 30 57 44
edit_answer vi-end-of-line vi "$separators" 'abc' 1b 30 24 58
edit_answer vi-start-of-line vi "$separators" 'abc' 1b 5e 58
edit_answer vi-substitute-line vi "$separators" 'abc' 1b 53 5a
edit_answer vi-append vi "$separators" 'abc' 1b 61 5a
edit_answer vi-insert-drops-unlisted vi "$separators" 'ab' 02 58
edit_answer emacs-backward-character emacs "$separators" 'ab' 02 58
edit_answer emacs-meta-b emacs "$separators" 'foo_bar baz' 1b62 0b
edit_answer emacs-delete-word emacs "$separators" 'foo_bar baz' 17
edit_answer emacs-meta-f emacs "$separators" 'foo_bar baz' 01 1b66 0b
edit_answer emacs-control-right emacs "$separators" 'foo_bar baz' 01 1b5b313b3543 0b

# `prompt_key` maps C-c and C-g onto the same close Escape takes.
close_answer() {
    probe set-environment -g EDIT untouched >/dev/null
    raise_prompt -I zz -p 'e ' "set-environment -g EDIT 'ran-%%'"
    drive "$2"
    await_prompt_close
    row "$1" "$(value EDIT)"
}
close_answer control-c 03
close_answer control-g 07

# `screen_redraw_draw_status` draws the message over the prompt line rather
# than deferring it, and `server_client_handle_key` clears the message and lets
# the same key reach the prompt unless `-N` set `message_ignore_keys`.
# `status_message_set` only assigns that flag when the delay is non-zero, so the
# `-d 0` message below inherits the `-N` one's setting and keeps eating keys: the
# parked prompt never closes and its answer never runs.
message_answer() {
    probe set-environment -g EDIT pending >/dev/null
    raise_prompt -I ab -p 'm ' "set-environment -g EDIT 'got-%%'"
}
probe set-option -g status-keys emacs >/dev/null

message_answer
probe display-message -c "$client" -d 5000 covering >/dev/null 2>&1
sleep 0.5
drive 63
drive 64
drive 0d
await_prompt_close
row message-delay "$(value EDIT)"

message_answer
probe display-message -c "$client" -N -d 1500 eating >/dev/null 2>&1
sleep 0.3
drive 58
drive 59
sleep 2.0
drive 63
drive 64
drive 0d
await_prompt_close
row message-ignore-keys "$(value EDIT)"

message_answer
probe display-message -c "$client" -d 0 waiting >/dev/null 2>&1
sleep 0.5
drive 63
drive 64
drive 0d
await_prompt_close
row message-wait-for-key "$(value EDIT)"

main_client set-environment -g COMMAND_PROMPT_EDITING "$(tr '\n' ' ' <"$log")"
