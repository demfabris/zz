#!/bin/sh
set -eu

export LC_ALL=C.UTF-8

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

root="$HOME/source-file-byte-name-$side"
rm -rf "$root"
mkdir -p "$root"

hex() {
    od -An -tx1 -v "$1" | tr -d ' \n'
}

path_byte="$(printf '\377')"
encoding=bytes
directory="$root/dir${path_byte}ectory"
if ! mkdir "$directory" 2>/dev/null; then
    path_byte=utf8-control
    encoding=utf8-control
    directory="$root/dir${path_byte}ectory"
    mkdir "$directory"
fi

plain="$root/real${path_byte}name.conf"
printf 'set-environment -g ZZ_SOURCE_BYTE_RAN yes\n' >"$plain"
main_client source-file "$plain" >/dev/null 2>&1 || :
main_client show-environment -g ZZ_SOURCE_BYTE_RAN >"$root/ran" 2>&1 || :

# The byte in a directory component, reached through a glob rather than a
# literal name, so the match comes back out of glob() as bytes too.
printf 'set-environment -g ZZ_SOURCE_BYTE_GLOB yes\n' >"$directory/leaf.conf"
main_client source-file "$directory/*.conf" >/dev/null 2>&1 || :
main_client show-environment -g ZZ_SOURCE_BYTE_GLOB >"$root/glob" 2>&1 || :

# -F expands the argument as a format first, so the byte arrives from the
# environment store instead of from argv.
formatted="$root/format${path_byte}ted.conf"
printf 'set-environment -g ZZ_SOURCE_BYTE_FORMAT yes\n' >"$formatted"
main_client set-environment -g ZZ_SOURCE_BYTE_PATH "$formatted"
main_client source-file -F '#{ZZ_SOURCE_BYTE_PATH}' >/dev/null 2>&1 || :
main_client show-environment -g ZZ_SOURCE_BYTE_FORMAT >"$root/format" 2>&1 || :

# -q on a byte path that is not there says nothing at all.
main_client source-file -q "$root/$(printf 'miss\377ing').conf" >"$root/quiet" 2>&1 || :

result=broken
if [ "$(hex "$root/ran")" = 5a5a5f534f555243455f425954455f52414e3d7965730a ] &&
    [ "$(hex "$root/glob")" = 5a5a5f534f555243455f425954455f474c4f423d7965730a ] &&
    [ "$(hex "$root/format")" = 5a5a5f534f555243455f425954455f464f524d41543d7965730a ] &&
    [ "$(hex "$root/quiet")" = "" ]; then
    result=clean:4
fi

if [ "$encoding" != bytes ]; then
    result="$result/$encoding"
fi

main_client set-environment -g ZZ_SOURCE_BYTE_RESULT "$result"
