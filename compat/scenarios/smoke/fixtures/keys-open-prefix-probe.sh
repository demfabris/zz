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
mkdir -p "$work/home" "$work/pin" "$work/zz"
export HOME="$work/home"
for mode in find-window activity; do
    export KEYS_CONTRACT_MODE="$mode"
    echo "Pinned tmux ($mode):"
    python3 "$root/compat/scenarios/smoke/fixtures/keys-contract.py" "$work/pin" "$pin" -L "$label" -f /dev/null
    echo "zz (known open $mode bindings):"
    python3 "$root/compat/scenarios/smoke/fixtures/keys-contract.py" "$work/zz" "$zz" --socket "$work/zz.sock"
done
