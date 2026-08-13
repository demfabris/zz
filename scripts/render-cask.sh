#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEMPLATE="$REPO_ROOT/packaging/homebrew/zz.rb"
PLACEHOLDER_VERSION='0.0.0'
PLACEHOLDER_SHA256='0000000000000000000000000000000000000000000000000000000000000000'

die() { echo "error: $*" >&2; exit 1; }

if [[ $# -ne 2 ]]; then
    cat >&2 <<'EOF'
usage: render-cask.sh <version> <disk-image>

Print the Homebrew cask for a released DMG. The version must match the tag
without its leading `v`, and the disk image is the artifact the cask points at.
EOF
    exit 2
fi

version="$1"
dmg="$2"

[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$ ]] || \
    die "version must look like 1.2.3 or 1.2.3-rc.1: $version"
[[ -s "$dmg" ]] || die "disk image does not exist: $dmg"
[[ -s "$TEMPLATE" ]] || die "cask template is missing: $TEMPLATE"

sha256="$(shasum -a 256 "$dmg" | cut -d' ' -f1)"
[[ "$sha256" =~ ^[0-9a-f]{64}$ ]] || die "could not compute the disk image checksum"

cask="$(sed \
    -e "s/\"$PLACEHOLDER_VERSION\"/\"$version\"/" \
    -e "s/\"$PLACEHOLDER_SHA256\"/\"$sha256\"/" \
    "$TEMPLATE")"

grep -qx "  version \"$version\"" <<<"$cask" || \
    die "the cask template has no version stanza to replace"
grep -qx "  sha256 \"$sha256\"" <<<"$cask" || \
    die "the cask template has no sha256 stanza to replace"

printf '%s\n' "$cask"
