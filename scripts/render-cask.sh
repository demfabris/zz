#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEMPLATE="$REPO_ROOT/packaging/homebrew/zz.rb"
PLACEHOLDER_VERSION='0.0.0'
PLACEHOLDER_SHA256='0000000000000000000000000000000000000000000000000000000000000000'

die() { echo "error: $*" >&2; exit 1; }

if [[ $# -lt 2 || $# -gt 3 ]]; then
    cat >&2 <<'EOF'
usage: render-cask.sh <version> <disk-image-or-sha256> [stable|beta]

Print the Homebrew cask for a released DMG. The version must match the tag
without its leading `v`. The second argument is the disk image the cask points
at, or its sha256 directly, which is what the release workflow passes now that
it renders the cask on a runner that never held the DMG. The stable channel
renders the `zz` cask; beta renders `zz@beta`, the cask prereleases publish to
and stable releases keep current.
EOF
    exit 2
fi

version="$1"
image="$2"
channel="${3:-stable}"

[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$ ]] || \
    die "version must look like 1.2.3 or 1.2.3-rc.1: $version"
[[ "$channel" == stable || "$channel" == beta ]] || die "channel must be stable or beta: $channel"
[[ "$channel" == beta || "$version" != *-* ]] || \
    die "prerelease versions render only the beta channel: $version"
[[ -s "$TEMPLATE" ]] || die "cask template is missing: $TEMPLATE"

if [[ "$image" =~ ^[0-9a-f]{64}$ ]]; then
    sha256="$image"
else
    [[ -s "$image" ]] || die "disk image does not exist: $image"
    sha256="$(shasum -a 256 "$image" | cut -d' ' -f1)"
    [[ "$sha256" =~ ^[0-9a-f]{64}$ ]] || die "could not compute the disk image checksum"
fi

cask="$(sed \
    -e "s/\"$PLACEHOLDER_VERSION\"/\"$version\"/" \
    -e "s/\"$PLACEHOLDER_SHA256\"/\"$sha256\"/" \
    "$TEMPLATE")"

grep -qx "  version \"$version\"" <<<"$cask" || \
    die "the cask template has no version stanza to replace"
grep -qx "  sha256 \"$sha256\"" <<<"$cask" || \
    die "the cask template has no sha256 stanza to replace"

if [[ "$channel" == beta ]]; then
    cask="$(sed \
        -e 's/^cask "zz" do$/cask "zz@beta" do/' \
        -e 's/^  name "zz"$/  name "zz beta"/' \
        -e 's/^  conflicts_with cask: "zz@beta"$/  conflicts_with cask: "zz"/' \
        -e '/^  livecheck do$/,/^$/d' \
        <<<"$cask")"
    grep -qx 'cask "zz@beta" do' <<<"$cask" || \
        die "the cask template has no zz token to retarget"
    grep -qx '  conflicts_with cask: "zz"' <<<"$cask" || \
        die "the cask template has no zz@beta conflict to retarget"
    ! grep -qx '  livecheck do' <<<"$cask" || die "the beta cask must not carry livecheck"
fi

printf '%s\n' "$cask"
