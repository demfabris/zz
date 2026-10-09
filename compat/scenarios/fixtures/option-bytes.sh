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

transcript="$HOME/option-bytes-$side.txt"
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

block_conf="$HOME/option-bytes-block-$side.conf"
assign_conf="$HOME/option-bytes-assign-$side.conf"
use_conf="$HOME/option-bytes-use-$side.conf"

cleanup() {
    cleanup_status=$?
    trap - EXIT
    set +e
    client set-option -gu @option_bytes_argv >/dev/null 2>&1
    client set-option -gu @option_bytes_stream >/dev/null 2>&1
    client delete-buffer -b option_bytes_block >/dev/null 2>&1
    client delete-buffer -b option_bytes_variable >/dev/null 2>&1
    client set-environment -gu OPTION_BYTES >/dev/null 2>&1
    exit "$cleanup_status"
}
trap cleanup EXIT

client set-option -g @option_bytes_argv "$(printf 'a\376b')"
record argv-show "$(client show-options -g @option_bytes_argv | hex)"
record argv-show-utf8 "$(utf8_client show-options -g @option_bytes_argv | hex)"
record argv-value "$(client show-options -gv @option_bytes_argv | hex)"
record argv-value-utf8 "$(utf8_client show-options -gv @option_bytes_argv | hex)"
record argv-format "$(client display-message -p '#{@option_bytes_argv}' | hex)"
record argv-format-utf8 "$(utf8_client display-message -p '#{@option_bytes_argv}' | hex)"
record control-value "$(printf 'show-options -gv @option_bytes_argv\n' |
    client -C attach 2>/dev/null | last_block | hex)"
record control-value-utf8 "$(printf 'show-options -gv @option_bytes_argv\n' |
    utf8_client -C attach 2>/dev/null | last_block | hex)"
record control-inserted-format-utf8 "$(printf "if-shell -F 1 \"display-message -p '#{@option_bytes_argv}'\"\n" |
    utf8_client -C attach 2>/dev/null | last_block | hex)"

status=0
message="$(printf 'set -g @option_bytes_stream a\376b\n' | client source-file - 2>&1)" || status=$?
record stream-status "$status:$message"
record stream-show "$(client show-options -g @option_bytes_stream | hex)"
record stream-format-utf8 "$(utf8_client display-message -p '#{@option_bytes_stream}' | hex)"

printf 'if-shell -F 1 { set-buffer -b option_bytes_block a\376b }\n' >"$block_conf"
status=0
message="$(client source-file "$block_conf" 2>&1)" || status=$?
record block-status "$status:$message"
record block-buffer "$(client show-buffer -b option_bytes_block | hex)"

printf 'OPTION_BYTES=a\375b\n' >"$assign_conf"
printf 'set-buffer -b option_bytes_variable "$OPTION_BYTES"\n' >"$use_conf"
client source-file "$assign_conf"
status=0
message="$(client source-file "$use_conf" 2>&1)" || status=$?
record variable-status "$status:$message"
record variable-buffer "$(client show-buffer -b option_bytes_variable | hex)"
record variable-environment "$(client show-environment -g OPTION_BYTES | hex)"

printf 'OPTION_BYTES=a\\374b\n' >"$assign_conf"
client source-file "$assign_conf"
status=0
message="$(client source-file "$use_conf" 2>&1)" || status=$?
record octal-variable-status "$status:$message"
record octal-variable-buffer "$(client show-buffer -b option_bytes_variable | hex)"
record octal-variable-environment "$(utf8_client show-environment -g OPTION_BYTES | hex)"

client set-option -gu @option_bytes_argv
client set-option -gu @option_bytes_stream
client delete-buffer -b option_bytes_block
client delete-buffer -b option_bytes_variable
client set-environment -gu OPTION_BYTES
client load-buffer -b transcript "$transcript"
