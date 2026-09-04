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

# A path is bytes on Unix and this file system takes them, so the file below
# exists under a name no UTF-8 string can spell. cmd_source_file hands the
# argument to glob() as it stood in argv, so the pin runs it.
plain="$root/$(printf 'real\377name').conf"
printf 'set-environment -g ZZ_SOURCE_BYTE_RAN yes\n' >"$plain"
main_client source-file "$plain" >/dev/null 2>&1 || :
main_client show-environment -g ZZ_SOURCE_BYTE_RAN >"$root/ran" 2>&1 || :

# The byte in a directory component, reached through a glob rather than a
# literal name, so the match comes back out of glob() as bytes too.
directory="$root/$(printf 'dir\377ectory')"
mkdir -p "$directory"
printf 'set-environment -g ZZ_SOURCE_BYTE_GLOB yes\n' >"$directory/leaf.conf"
main_client source-file "$directory/*.conf" >/dev/null 2>&1 || :
main_client show-environment -g ZZ_SOURCE_BYTE_GLOB >"$root/glob" 2>&1 || :

# -F expands the argument as a format first, so the byte arrives from the
# environment store instead of from argv.
formatted="$root/$(printf 'format\377ted').conf"
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

main_client set-environment -g ZZ_SOURCE_BYTE_RESULT "$result"
