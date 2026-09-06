#!/bin/bash
set -euo pipefail
root=$(cd "$(dirname "$0")/../../../.." && pwd)
pin=${ZZ_COMPAT_TMUX:-$root/compat/.cache/tmux-src/tmux}
zz=${ZZ_COMPAT_ZZ:-$root/target/debug/zz}
work=$(mktemp -d /tmp/zzprobe-keys-XXXXXX)
label=zzprobe-$$
cleanup() {
    "$pin" -L "$label" kill-server >/dev/null 2>&1 || true
    "$zz" --socket "$work/zz.sock" kill-server >/dev/null 2>&1 || true
    rm -r "$work"
}
trap cleanup EXIT
mkdir -p "$work/home" "$work/config" "$work/pin" "$work/zz"
export HOME="$work/home"
export XDG_CONFIG_HOME="$work/config"
for mode in find-window activity; do
    export KEYS_CONTRACT_MODE="$mode"
    echo "Pinned tmux ($mode):"
    python3 "$root/compat/scenarios/smoke/fixtures/keys-contract.py" "$work/pin" "$pin" -L "$label" -f /dev/null
    echo "zz ($mode):"
    python3 "$root/compat/scenarios/smoke/fixtures/keys-contract.py" "$work/zz" "$zz" --socket "$work/zz.sock" -f /dev/null
done
for engine in pin zz; do
    if [ "$engine" = pin ]; then
        client=("$pin" -L "$label" -f /dev/null)
        expected='1|tree-mode'
    else
        client=("$zz" --socket "$work/zz.sock" -f /dev/null)
        expected='0|'
    fi
    "${client[@]}" new-session -d -s keysdetached -n source
    "${client[@]}" new-window -d -t keysdetached -n needle
    before=$("${client[@]}" display-message -p -t keysdetached:source '#{pane_in_mode}|#{pane_mode}')
    [ "$before" = '0|' ]
    "${client[@]}" find-window -t keysdetached:source needle
    after=$("${client[@]}" display-message -p -t keysdetached:source '#{pane_in_mode}|#{pane_mode}')
    [ "$after" = "$expected" ]
    printf '%s detached find-window mode=%s\n' "$engine" "$after"
    backref=$("${client[@]}" display-message -p '#{m/r:(a)\1,aa}')
    digit_class=$("${client[@]}" display-message -p '#{m/r:\d+,123}')
    if [ "$engine" = pin ]; then
        [ "$backref|$digit_class" = '1|0' ]
    else
        [ "$backref|$digit_class" = '0|1' ]
    fi
    printf '%s inherited regex backref=%s digit-class=%s\n' "$engine" "$backref" "$digit_class"
    "${client[@]}" kill-session -t keysdetached
done
