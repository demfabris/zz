#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEMPLATE_DIR="$REPO_ROOT/packaging/arch/aur"

die() { echo "error: $*" >&2; exit 1; }

if [[ $# -lt 4 || $# -gt 5 ]]; then
    cat >&2 <<'EOF'
usage: render-aur.sh <version> <x86_64-sha256> <aarch64-sha256> <output directory> [stable|beta]

Render the AUR PKGBUILD and .SRCINFO for a release. The version must match
the tag without its leading `v`, and the checksums are the two Linux release
tarballs'. The stable channel renders zz-bin; beta renders zz-beta-bin, the
package prereleases publish to and stable releases keep current.
EOF
    exit 2
fi

version="$1"
x86_64_sha256="$2"
aarch64_sha256="$3"
out_dir="$4"
channel="${5:-stable}"

[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] || \
    die "version must look like 1.2.3 or 1.2.3-beta.1: $version"
case "$channel" in
    stable)
        [[ "$version" != *-* ]] || die "prerelease versions render only the beta channel: $version"
        pkgname=zz-bin
        conflicting=zz-beta-bin
        ;;
    beta)
        pkgname=zz-beta-bin
        conflicting=zz-bin
        ;;
    *) die "channel must be stable or beta: $channel" ;;
esac
# pacman rejects a hyphen in pkgver; 1.2.3-beta.1 becomes 1.2.3beta.1, which
# vercmp orders before 1.2.3 like any trailing alphabetic segment.
pkgver="${version//-/}"
[[ "$x86_64_sha256" =~ ^[0-9a-f]{64}$ ]] || die "malformed x86_64 sha256: $x86_64_sha256"
[[ "$aarch64_sha256" =~ ^[0-9a-f]{64}$ ]] || die "malformed aarch64 sha256: $aarch64_sha256"
[[ -d "$out_dir" ]] || die "output directory does not exist: $out_dir"
[[ -s "$TEMPLATE_DIR/PKGBUILD" ]] || die "PKGBUILD template is missing: $TEMPLATE_DIR/PKGBUILD"
[[ -s "$TEMPLATE_DIR/.SRCINFO" ]] || die ".SRCINFO template is missing: $TEMPLATE_DIR/.SRCINFO"

pkgbuild="$(sed \
    -e "s/^pkgname=zz-bin$/pkgname=$pkgname/" \
    -e "s/^_version=.*/_version=$version/" \
    -e "s/^pkgver=.*/pkgver=$pkgver/" \
    -e "s/^conflicts=('zz' 'zz-beta-bin')$/conflicts=('zz' '$conflicting')/" \
    -e "s/^sha256sums_x86_64=.*/sha256sums_x86_64=('$x86_64_sha256')/" \
    -e "s/^sha256sums_aarch64=.*/sha256sums_aarch64=('$aarch64_sha256')/" \
    "$TEMPLATE_DIR/PKGBUILD")"
grep -qx "pkgname=$pkgname" <<<"$pkgbuild" || \
    die "the PKGBUILD template has no pkgname to replace"
grep -qx "_version=$version" <<<"$pkgbuild" || \
    die "the PKGBUILD template has no _version to replace"
grep -qx "pkgver=$pkgver" <<<"$pkgbuild" || \
    die "the PKGBUILD template has no pkgver to replace"
grep -qx "conflicts=('zz' '$conflicting')" <<<"$pkgbuild" || \
    die "the PKGBUILD template has no conflicts to replace"
grep -qx "sha256sums_x86_64=('$x86_64_sha256')" <<<"$pkgbuild" || \
    die "the PKGBUILD template has no x86_64 checksum to replace"
grep -qx "sha256sums_aarch64=('$aarch64_sha256')" <<<"$pkgbuild" || \
    die "the PKGBUILD template has no aarch64 checksum to replace"

# The tag version appears expanded in .SRCINFO's source URLs; pkgver is
# rewritten first so the global replacement only touches the URLs.
srcinfo="$(sed \
    -e "s/^pkgbase = zz-bin$/pkgbase = $pkgname/" \
    -e "s/^pkgname = zz-bin$/pkgname = $pkgname/" \
    -e "s/^\tpkgver = 0\.0\.0$/\tpkgver = $pkgver/" \
    -e "s/0\.0\.0/$version/g" \
    -e "s/^\tconflicts = zz-beta-bin$/\tconflicts = $conflicting/" \
    -e "s/^\tsha256sums_x86_64 = .*/\tsha256sums_x86_64 = $x86_64_sha256/" \
    -e "s/^\tsha256sums_aarch64 = .*/\tsha256sums_aarch64 = $aarch64_sha256/" \
    "$TEMPLATE_DIR/.SRCINFO")"
grep -qx "pkgbase = $pkgname" <<<"$srcinfo" || \
    die "the .SRCINFO template has no pkgbase to replace"
grep -qx "pkgname = $pkgname" <<<"$srcinfo" || \
    die "the .SRCINFO template has no pkgname to replace"
grep -qx $'\tpkgver = '"$pkgver" <<<"$srcinfo" || \
    die "the .SRCINFO template has no pkgver to replace"
grep -qx $'\tconflicts = '"$conflicting" <<<"$srcinfo" || \
    die "the .SRCINFO template has no conflicts to replace"
grep -q "download/v$version/zz-$version-linux-x86_64.tar.gz" <<<"$srcinfo" || \
    die "the .SRCINFO template has no x86_64 source URL to replace"
grep -qx $'\tsha256sums_aarch64 = '"$aarch64_sha256" <<<"$srcinfo" || \
    die "the .SRCINFO template has no aarch64 checksum to replace"

printf '%s\n' "$pkgbuild" > "$out_dir/PKGBUILD"
printf '%s\n' "$srcinfo" > "$out_dir/.SRCINFO"
echo "AUR $pkgname $pkgver rendered into $out_dir" >&2
