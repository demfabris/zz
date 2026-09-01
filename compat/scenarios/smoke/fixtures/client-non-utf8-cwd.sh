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
main_client() {
    # shellcheck disable=SC2086
    "$binary" $prefix_args "$@"
}

root="$HOME/client-non-utf8-cwd-$side"
rm -rf "$root"
mkdir -p "$root"
# One byte the daemon cannot carry as UTF-8 text. The client's cwd is the fact
# under test, so the directory name holds it and every probe below is a
# relative path resolved against that cwd.
work="$root/$(printf 'cwd-\377-dir')"
mkdir -p "$work/nested"
printf '%s\n' 'set-environment -g NON_UTF8_CWD_DIRECT hit' >"$work/rel.conf"
printf '%s\n' 'set-environment -g NON_UTF8_CWD_NESTED hit' >"$work/nested/leaf.conf"

main_client set-environment -g NON_UTF8_CWD_DIRECT miss
main_client set-environment -g NON_UTF8_CWD_NESTED miss

direct_rc=0
(cd "$work" && main_client source-file rel.conf) >"$root/direct.out" 2>"$root/direct.err" ||
    direct_rc=$?

nested_rc=0
(cd "$work" && main_client source-file ./nested/leaf.conf) >"$root/nested.out" \
    2>"$root/nested.err" || nested_rc=$?

quiet_rc=0
(cd "$work" && main_client source-file -q missing.conf) >"$root/quiet.out" \
    2>"$root/quiet.err" || quiet_rc=$?

missing_rc=0
(cd "$work" && main_client source-file missing.conf) >"$root/missing.out" \
    2>"$root/missing.err" || missing_rc=$?

environment_value() {
    main_client show-environment -g "$1" 2>/dev/null | sed "s/^$1=//" || :
}

result=broken
if [ "$direct_rc" -eq 0 ] &&
    [ "$nested_rc" -eq 0 ] &&
    [ "$quiet_rc" -eq 0 ] &&
    [ "$missing_rc" -eq 1 ] &&
    [ "$(environment_value NON_UTF8_CWD_DIRECT)" = hit ] &&
    [ "$(environment_value NON_UTF8_CWD_NESTED)" = hit ] &&
    [ ! -s "$root/direct.out" ] &&
    [ ! -s "$root/direct.err" ] &&
    [ ! -s "$root/nested.out" ] &&
    [ ! -s "$root/nested.err" ] &&
    [ ! -s "$root/quiet.out" ] &&
    [ ! -s "$root/quiet.err" ] &&
    grep -q 'No such file or directory' "$root/missing.err"; then
    result=clean:12
fi

main_client set-environment -g NON_UTF8_CWD "$result"
