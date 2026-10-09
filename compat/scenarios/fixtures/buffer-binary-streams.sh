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

client() {
    # shellcheck disable=SC2086
    env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
        "$binary" $prefix_args "$@"
}

utf8_client() {
    # shellcheck disable=SC2086
    env -u TMUX -u TMUX_PANE -u ZZ_SOCKET -u ZZ_SESSION -u ZZ_PANE \
        "$binary" -u $prefix_args "$@"
}

transcript="$HOME/buffer-binary-streams-$side.txt"
: >"$transcript"

hex() {
    od -An -tx1 -v | tr -s ' \n' '  ' | sed 's/^ //;s/ $//'
}

record() {
    printf '%s=[%s]\n' "$1" "$2" >>"$transcript"
}

last_block() {
    awk '/^%begin / { body = ""; inside = 1; next }
        /^%(end|error) / { if (inside) last = body; inside = 0; next }
        inside { body = body $0 "\n" }
        END { printf "%s", last }'
}

cleanup() {
    cleanup_status=$?
    trap - EXIT
    set +e
    client delete-buffer -b binary >/dev/null 2>&1
    client delete-buffer -b sourced >/dev/null 2>&1
    client set-option -gu @binary >/dev/null 2>&1
    client set-environment -gu BINARY_STREAMS >/dev/null 2>&1
    exit "$cleanup_status"
}
trap cleanup EXIT

printf 'a\376b\nc\001d' | client load-buffer -b binary -

status=0
bytes="$(client show-buffer -b binary | hex)" || status=$?
record command-show "$status:$bytes"
record command-show-utf8 "$(utf8_client show-buffer -b binary | hex)"

record control-show "$(printf 'show-buffer -b binary\n' |
    client -C attach 2>/dev/null | last_block | hex)"
record control-show-utf8 "$(printf 'show-buffer -b binary\n' |
    utf8_client -C attach 2>/dev/null | last_block | hex)"

status=0
message="$(printf 'set -g @binary a\376b\n' | client source-file - 2>&1)" || status=$?
record source-stdin "$status:$message"
record source-stdin-value "$(client show-options -gv @binary | hex)"

status=0
message="$(printf 'set-buffer -b sourced a\376b\nset-environment -g BINARY_STREAMS c\375d\n' |
    client source-file - 2>&1)" || status=$?
record source-stdin-bytes "$status:$message"
record source-stdin-buffer "$(client show-buffer -b sourced | hex)"
record source-stdin-environment "$(client show-environment -g BINARY_STREAMS | hex)"

client delete-buffer -b binary
client delete-buffer -b sourced
client set-environment -gu BINARY_STREAMS
client set-option -gu @binary
client load-buffer -b transcript "$transcript"
