#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

die() { echo "error: $*" >&2; exit 1; }

if [[ $# -lt 2 || $# -gt 4 ]]; then
    die "usage: package-appimage.sh <CEF bundle directory> <output.AppImage> [appimagetool] [runtime]"
fi

[[ "$(uname -s)" == "Linux" ]] || die "AppImage packaging requires Linux"

bundle_dir="$1"
output="$2"
appimagetool="${3:-${APPIMAGETOOL:-}}"
runtime="${4:-${APPIMAGE_RUNTIME:-}}"

[[ -n "$appimagetool" ]] || die "set APPIMAGETOOL or pass the appimagetool path as argument 3"
if [[ "$appimagetool" != */* ]]; then
    appimagetool="$(command -v "$appimagetool" || true)"
fi
[[ -x "$appimagetool" ]] || die "appimagetool is not executable: ${appimagetool:-<unset>}"
appimagetool="$(cd "$(dirname "$appimagetool")" && pwd)/$(basename "$appimagetool")"

[[ -n "$runtime" ]] || die "set APPIMAGE_RUNTIME or pass the type-2 runtime path as argument 4"
if [[ "$runtime" != */* ]]; then
    runtime="$(command -v "$runtime" || true)"
fi
[[ -s "$runtime" ]] || die "AppImage runtime is missing or empty: ${runtime:-<unset>}"
runtime="$(cd "$(dirname "$runtime")" && pwd)/$(basename "$runtime")"

mkdir -p "$(dirname "$output")"
output="$(cd "$(dirname "$output")" && pwd)/$(basename "$output")"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/zz-appimage.XXXXXX")"
cleanup() { rm -rf "$work_dir"; }
trap cleanup EXIT

app_dir="$work_dir/zz.AppDir"
"$REPO_ROOT/scripts/stage-linux-usr.sh" "$bundle_dir" "$app_dir"
install -m 755 "$REPO_ROOT/packaging/linux/AppRun" "$app_dir/AppRun"
ln -s usr/share/applications/zz.desktop "$app_dir/zz.desktop"
ln -s usr/share/icons/hicolor/scalable/apps/zz.svg "$app_dir/zz.svg"
ln -s usr/share/icons/hicolor/256x256/apps/zz.png "$app_dir/zz.png"
ln -s zz.png "$app_dir/.DirIcon"

case "$(uname -m)" in
    x86_64|amd64) appimage_arch="x86_64" ;;
    aarch64|arm64) appimage_arch="aarch64" ;;
    *) die "unsupported AppImage architecture: $(uname -m)" ;;
esac

ARCH="$appimage_arch" APPIMAGE_EXTRACT_AND_RUN=1 \
    "$appimagetool" --no-appstream --runtime-file "$runtime" "$app_dir" "$output"

[[ -s "$output" && -x "$output" ]] || die "appimagetool did not create an executable: $output"

verify_dir="$work_dir/verify"
mkdir "$verify_dir"
(
    cd "$verify_dir"
    "$output" --appimage-extract >/dev/null
    [[ -x squashfs-root/AppRun ]]
    [[ -x squashfs-root/usr/lib/zz/zz ]]
    [[ -x squashfs-root/usr/lib/zz/cli ]]
    [[ -L squashfs-root/usr/bin/zz ]]
    [[ -s squashfs-root/usr/lib/zz/libcef.so ]]
    [[ -L squashfs-root/zz.desktop ]]
    [[ -L squashfs-root/zz.svg ]]
    [[ -L squashfs-root/zz.png ]]
    [[ -L squashfs-root/.DirIcon ]]
    [[ -s squashfs-root/usr/share/icons/hicolor/scalable/apps/zz.svg ]]
    [[ -s squashfs-root/usr/share/licenses/zz/LICENSE-MIT ]]
    for size in 16 24 32 48 64 128 256 512; do
        [[ -s "squashfs-root/usr/share/icons/hicolor/${size}x${size}/apps/zz.png" ]]
    done
)

echo "AppImage ready: $output"
