#!/usr/bin/env bash
# Stage the Linux CEF bundle, desktop entry, icons, and licenses as a usr/
# tree under the destination directory. This is the single source of the
# installed layout: the AppImage AppDir (package-appimage.sh), the release
# tarball (package-linux-tarball.sh), and the Arch packages all carry it.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

die() { echo "error: $*" >&2; exit 1; }

if [[ $# -ne 2 ]]; then
    die "usage: stage-linux-usr.sh <CEF bundle directory> <destination directory>"
fi

bundle_dir="$1"
dest="$2"
icon_root="$REPO_ROOT/assets/linux/hicolor"

[[ -d "$bundle_dir" ]] || die "CEF bundle directory does not exist: $bundle_dir"
bundle_dir="$(cd "$bundle_dir" && pwd)"

for required in \
    zz cli libcef.so icudtl.dat resources.pak locales/en-US.pak chrome-sandbox \
    CREDITS.html CEF_LICENSE.txt; do
    [[ -s "$bundle_dir/$required" ]] || die "CEF bundle file is missing or empty: $required"
done
[[ -x "$bundle_dir/zz" ]] || die "CEF bundle executable is not executable: $bundle_dir/zz"
[[ -x "$bundle_dir/cli" ]] || die "CLI launcher is not executable: $bundle_dir/cli"
[[ -s "$icon_root/scalable/apps/zz.svg" ]] || die "scalable Linux icon is missing or empty"
for size in 16 24 32 48 64 128 256 512; do
    [[ -s "$icon_root/${size}x${size}/apps/zz.png" ]] \
        || die "${size}x${size} Linux icon is missing or empty"
done

install -d \
    "$dest/usr/bin" \
    "$dest/usr/lib/zz" \
    "$dest/usr/share/applications" \
    "$dest/usr/share/icons/hicolor" \
    "$dest/usr/share/licenses/zz"

cp -a "$bundle_dir/." "$dest/usr/lib/zz/"
ln -s ../lib/zz/cli "$dest/usr/bin/zz"
install -m 644 "$REPO_ROOT/packaging/linux/zz.desktop" \
    "$dest/usr/share/applications/zz.desktop"
cp -a "$icon_root/." "$dest/usr/share/icons/hicolor/"
install -m 644 "$REPO_ROOT/LICENSE-MIT" "$REPO_ROOT/LICENSE-APACHE" \
    "$dest/usr/share/licenses/zz/"

if command -v desktop-file-validate >/dev/null 2>&1; then
    desktop-file-validate "$dest/usr/share/applications/zz.desktop"
fi
