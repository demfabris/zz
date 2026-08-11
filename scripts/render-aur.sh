#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEMPLATE_DIR="$REPO_ROOT/packaging/arch/aur"

die() { echo "error: $*" >&2; exit 1; }

if [[ $# -ne 4 ]]; then
    cat >&2 <<'EOF'
usage: render-aur.sh <version> <x86_64-sha256> <aarch64-sha256> <output directory>

Render the AUR zz-bin PKGBUILD and .SRCINFO for a release. The version must
match the tag without its leading `v`, and the checksums are the two Linux
release tarballs'.
EOF
    exit 2
fi

version="$1"
x86_64_sha256="$2"
aarch64_sha256="$3"
out_dir="$4"

# A hyphen in pkgver is invalid to pacman, so prereleases can never render.
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || \
    die "version must look like 1.2.3 (the AUR takes no prereleases): $version"
[[ "$x86_64_sha256" =~ ^[0-9a-f]{64}$ ]] || die "malformed x86_64 sha256: $x86_64_sha256"
[[ "$aarch64_sha256" =~ ^[0-9a-f]{64}$ ]] || die "malformed aarch64 sha256: $aarch64_sha256"
[[ -d "$out_dir" ]] || die "output directory does not exist: $out_dir"
[[ -s "$TEMPLATE_DIR/PKGBUILD" ]] || die "PKGBUILD template is missing: $TEMPLATE_DIR/PKGBUILD"
[[ -s "$TEMPLATE_DIR/.SRCINFO" ]] || die ".SRCINFO template is missing: $TEMPLATE_DIR/.SRCINFO"

pkgbuild="$(sed \
    -e "s/^pkgver=.*/pkgver=$version/" \
    -e "s/^sha256sums_x86_64=.*/sha256sums_x86_64=('$x86_64_sha256')/" \
    -e "s/^sha256sums_aarch64=.*/sha256sums_aarch64=('$aarch64_sha256')/" \
    "$TEMPLATE_DIR/PKGBUILD")"
grep -qx "pkgver=$version" <<<"$pkgbuild" || \
    die "the PKGBUILD template has no pkgver to replace"
grep -qx "sha256sums_x86_64=('$x86_64_sha256')" <<<"$pkgbuild" || \
    die "the PKGBUILD template has no x86_64 checksum to replace"
grep -qx "sha256sums_aarch64=('$aarch64_sha256')" <<<"$pkgbuild" || \
    die "the PKGBUILD template has no aarch64 checksum to replace"

# The version appears expanded in .SRCINFO's source URLs, not only in pkgver.
srcinfo="$(sed \
    -e "s/0\.0\.0/$version/g" \
    -e "s/^\tsha256sums_x86_64 = .*/\tsha256sums_x86_64 = $x86_64_sha256/" \
    -e "s/^\tsha256sums_aarch64 = .*/\tsha256sums_aarch64 = $aarch64_sha256/" \
    "$TEMPLATE_DIR/.SRCINFO")"
grep -qx $'\tpkgver = '"$version" <<<"$srcinfo" || \
    die "the .SRCINFO template has no pkgver to replace"
grep -q "download/v$version/zz-$version-linux-x86_64.tar.gz" <<<"$srcinfo" || \
    die "the .SRCINFO template has no x86_64 source URL to replace"
grep -qx $'\tsha256sums_aarch64 = '"$aarch64_sha256" <<<"$srcinfo" || \
    die "the .SRCINFO template has no aarch64 checksum to replace"

printf '%s\n' "$pkgbuild" > "$out_dir/PKGBUILD"
printf '%s\n' "$srcinfo" > "$out_dir/.SRCINFO"
echo "AUR package rendered into $out_dir" >&2
