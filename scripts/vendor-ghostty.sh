#!/usr/bin/env bash
set -euo pipefail

usage() {
    echo "usage: scripts/vendor-ghostty.sh <upstream-rev> [repo-url]" >&2
    echo "  <upstream-rev> is a full commit id, branch, or tag of ghostty-org/ghostty" >&2
    exit 2
}

[[ $# -ge 1 && $# -le 2 ]] || usage
rev=$1
repo=${2:-https://github.com/ghostty-org/ghostty}
dir=third_party/ghostty

keep=(
    build.zig
    build.zig.zon
    LICENSE
    include
    src
    ':(exclude)src/font/res'
    ':(exclude)src/crash/testdata'
    pkg/apple-sdk
    pkg/highway
    pkg/simdutf
    pkg/translate-c
    pkg/wuffs
    ':(exclude)pkg/wuffs/src/too_big.*'
    ':(glob)pkg/*/build.zig'
    ':(glob)pkg/*/build.zig.zon'
)

root=$(git rev-parse --show-toplevel)
cd "$root"

if [[ -n "$(git status --porcelain -- "$dir")" ]]; then
    echo "$dir has uncommitted changes" >&2
    exit 1
fi

cache=${XDG_CACHE_HOME:-$HOME/.cache}/zz/ghostty.git
[[ -d $cache ]] || git clone -q --bare "$repo" "$cache"
git -C "$cache" fetch -q "$repo" "$rev"
upstream=$(git -C "$cache" rev-parse --verify 'FETCH_HEAD^{commit}')

last=$(git log -1 --format=%H --grep='^Ghostty-Upstream: ' HEAD)

tmp=$(mktemp -d)
cleanup() {
    if [[ -d $tmp/wt ]]; then
        git worktree remove --force "$tmp/wt"
    fi
    rm -rf "$tmp"
}
trap cleanup EXIT

if [[ -n $last ]]; then
    git worktree add -q --detach "$tmp/wt" "$last"
    target=$tmp/wt
else
    target=$root
fi

rm -rf "${target:?}/$dir"
mkdir -p "$target/$dir"
git -C "$cache" archive --format=tar "$upstream" -- "${keep[@]}" | tar -x -C "$target/$dir"
git -C "$target" add -A -- "$dir"
git -C "$target" commit -q -m "Import Ghostty ${upstream:0:10}" -m "Ghostty-Upstream: $upstream"
import=$(git -C "$target" rev-parse HEAD)

echo "imported $upstream as $import ($(git -C "$target" ls-files -- "$dir" | wc -l | tr -d ' ') files)"
[[ -n $last ]] || exit 0

echo "zz changes being carried over:"
git log --oneline "$last..HEAD" -- "$dir"
if ! git merge -q --no-ff -m "Merge Ghostty ${upstream:0:10}" "$import"; then
    echo "conflicts under $dir: resolve them, then git commit" >&2
    exit 1
fi
