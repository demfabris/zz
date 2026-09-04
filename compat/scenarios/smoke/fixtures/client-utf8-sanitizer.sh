#!/bin/sh
set -eu

# The store keeps bytes; this fixture is about what the server hands a client,
# so setup runs through a client whose locale names UTF-8 and the probes run
# through clients whose locales do not.
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

# tmux.c raises CLIENT_UTF8 from $TMUX being set at all, or from the first of
# LC_ALL, LC_CTYPE and LANG that is set and non-empty holding UTF-8 or UTF8.
# The fixture runs inside a job, so $TMUX is already set and every probe that
# wants the flag down has to drop it.
utf8_client() {
    # shellcheck disable=SC2086
    LC_ALL=C.UTF-8 "$binary" $prefix_args "$@"
}
ascii_client() {
    # shellcheck disable=SC2086
    env -u TMUX LC_ALL=C LANG=C LC_CTYPE=C "$binary" $prefix_args "$@"
}
tmux_client() {
    # shellcheck disable=SC2086
    env TMUX=/tmp/zz-sanitizer-fake,1,0 LC_ALL=C LANG=C LC_CTYPE=C \
        "$binary" $prefix_args "$@"
}
lang_client() {
    # shellcheck disable=SC2086
    env -u TMUX -u LC_ALL -u LC_CTYPE LANG=en_US.UTF-8 "$binary" $prefix_args "$@"
}
lc_ctype_client() {
    # shellcheck disable=SC2086
    env -u TMUX -u LC_ALL LC_CTYPE=C LANG=en_US.UTF-8 "$binary" $prefix_args "$@"
}

root="$HOME/client-utf8-sanitizer-$side"
rm -rf "$root"
mkdir -p "$root"

hex() {
    od -An -tx1 -v "$1" | tr -d ' \n'
}

# One byte no UTF-8 string can hold, one wide character, one zero-width
# combining mark behind a plain letter, and one control byte.
bytes=$(printf 'a\377b')
wide=$(printf 'x\346\274\242y')
combining=$(printf 'e\314\201z')
tabbed=$(printf 'a\tb')

utf8_client set-environment -g ZZSAN_BYTES "$bytes"
utf8_client set-environment -g ZZSAN_WIDE "$wide"
utf8_client set-environment -g ZZSAN_COMB "$combining"
utf8_client set-environment -g ZZSAN_TAB "$tabbed"
utf8_client new-window -d -n sanitize 'sleep 30'

# utf8_sanitize: one underscore per column a complete sequence would take, so
# the wide character becomes two and the combining mark becomes none; printable
# ASCII passes; every other byte, the control byte and the byte no sequence
# opens alike, becomes one underscore.
ascii_client show-environment -g ZZSAN_BYTES >"$root/ascii-bytes" 2>&1 || :
ascii_client show-environment -g ZZSAN_WIDE >"$root/ascii-wide" 2>&1 || :
ascii_client show-environment -g ZZSAN_COMB >"$root/ascii-comb" 2>&1 || :
ascii_client show-environment -g ZZSAN_TAB >"$root/ascii-tab" 2>&1 || :
ascii_client display-message -p 'x#{ZZSAN_WIDE}y' >"$root/ascii-format" 2>&1 || :

# A listing is one cmdq_print per row, so each row is sanitized on its own and
# the separator between them is not.
ascii_client list-panes -a -F '#{ZZSAN_BYTES}' >"$root/ascii-list" 2>&1 || :

# The same command from a client that did raise the flag, four ways: an
# explicit UTF-8 LC_ALL, a $TMUX that outranks a C locale, a LANG that answers
# because LC_ALL and LC_CTYPE are unset, and an LC_CTYPE=C that answers first
# and so masks a UTF-8 LANG behind it.
utf8_client show-environment -g ZZSAN_BYTES >"$root/utf8-bytes" 2>&1 || :
tmux_client show-environment -g ZZSAN_BYTES >"$root/tmux-bytes" 2>&1 || :
lang_client show-environment -g ZZSAN_BYTES >"$root/lang-bytes" 2>&1 || :
lc_ctype_client show-environment -g ZZSAN_BYTES >"$root/lcctype-bytes" 2>&1 || :

result=broken
if [ "$(hex "$root/ascii-bytes")" = 5a5a53414e5f42595445533d615f620a ] &&
    [ "$(hex "$root/ascii-wide")" = 5a5a53414e5f574944453d785f5f790a ] &&
    [ "$(hex "$root/ascii-comb")" = 5a5a53414e5f434f4d423d657a0a ] &&
    [ "$(hex "$root/ascii-tab")" = 5a5a53414e5f5441423d615f620a ] &&
    [ "$(hex "$root/ascii-format")" = 78785f5f79790a ] &&
    [ "$(hex "$root/ascii-list")" = 615f620a615f620a ] &&
    [ "$(hex "$root/utf8-bytes")" = 5a5a53414e5f42595445533d61ff620a ] &&
    [ "$(hex "$root/tmux-bytes")" = 5a5a53414e5f42595445533d61ff620a ] &&
    [ "$(hex "$root/lang-bytes")" = 5a5a53414e5f42595445533d61ff620a ] &&
    [ "$(hex "$root/lcctype-bytes")" = 5a5a53414e5f42595445533d615f620a ]; then
    result=clean:10
fi

utf8_client set-environment -g ZZ_SANITIZE_RESULT "$result"
