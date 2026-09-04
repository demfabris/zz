#!/bin/sh
set -eu

# tmux takes a UTF-8 client from LC_ALL, LC_CTYPE or LANG and sanitizes byte
# output to a non-UTF-8 client, so pin the locale rather than inherit one.
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

root="$HOME/environment-bytes-$side"
rm -rf "$root"
mkdir -p "$root"

# One byte no UTF-8 string can hold. tmux keeps an environment name and value as
# a C string, so it rides argv into the store and comes back out of
# show-environment unchanged.
value=$(printf 'a\377b')
name=$(printf 'N\377')
session_value=$(printf 'x\377y')

hex() {
    od -An -tx1 -v "$1" | tr -d ' \n'
}

main_client set-environment -g ZZBYTES "$value"
main_client show-environment -g ZZBYTES >"$root/global" 2>&1 || :
main_client show-environment -gs ZZBYTES >"$root/global-s" 2>&1 || :
main_client set-environment -g "$name" plain
main_client show-environment -g "$name" >"$root/name" 2>&1 || :
main_client set-environment ZZSESS "$session_value"
main_client show-environment ZZSESS >"$root/session" 2>&1 || :

# The spawn environment a later pane inherits: the pane's own shell reads the
# variable back out of its process environment and reports it.
cat >"$root/inherit.sh" <<INHERIT
#!/bin/sh
LC_ALL=C.UTF-8 "$binary" $prefix_args set-environment -g ZZINHERIT "\${ZZBYTES-unset}"
sleep 30
INHERIT
main_client new-window -d -n inherit "sh $root/inherit.sh"
attempt=0
while [ "$attempt" -lt 80 ]; do
    main_client show-environment -g ZZINHERIT >"$root/inherit" 2>/dev/null || :
    if [ -s "$root/inherit" ]; then
        break
    fi
    attempt=$((attempt + 1))
    sleep 0.25
done

# The attach shape: a real client on a pty whose own environment holds the byte,
# with update-environment naming it beside a plain neighbour.
main_client set-option -g update-environment "ZZBYTES ZZPLAIN"
ZZBYTES="$value" ZZPLAIN=neighbour \
    ZZ_ENVBYTES_BIN="$binary" ZZ_ENVBYTES_ARGS="$prefix_args" ZZ_ENVBYTES_ROOT="$root" \
    python3 "$HOME/environment-bytes-attach.py" || :

result=broken
if [ "$(hex "$root/global")" = 5a5a42595445533d61ff620a ] &&
    [ "$(hex "$root/global-s")" = 5a5a42595445533d2261ff62223b206578706f7274205a5a42595445533b0a ] &&
    [ "$(hex "$root/name")" = 4eff3d706c61696e0a ] &&
    [ "$(hex "$root/session")" = 5a5a534553533d78ff790a ] &&
    [ "$(hex "$root/inherit")" = 5a5a494e48455249543d61ff620a ] &&
    [ "$(hex "$root/attach-bytes")" = 5a5a42595445533d61ff620a ] &&
    [ "$(hex "$root/attach-plain")" = 5a5a504c41494e3d6e65696768626f75720a ]; then
    result=clean:7
fi

main_client set-environment -g ZZ_ENVBYTES_RESULT "$result"
