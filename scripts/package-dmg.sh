#!/usr/bin/env bash
set -euo pipefail

die() { echo "error: $*" >&2; exit 1; }

if [[ $# -ne 2 ]]; then
    die "usage: package-dmg.sh <application.app> <output.dmg>"
fi

[[ "$(uname -s)" == "Darwin" ]] || die "DMG packaging requires macOS"

app="$1"
output="$2"

[[ -d "$app/Contents" ]] || die "application bundle does not exist: $app"
app="$(cd "$(dirname "$app")" && pwd)/$(basename "$app")"
app_name="$(basename "$app")"

mkdir -p "$(dirname "$output")"
output="$(cd "$(dirname "$output")" && pwd)/$(basename "$output")"

codesign --verify --deep --strict "$app"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/zz-dmg.XXXXXX")"
device=""
cleanup() {
    if [[ -n "$device" ]]; then
        hdiutil detach "$device" >/dev/null 2>&1 || true
    fi
    rm -rf "$work_dir"
}
trap cleanup EXIT

staging="$work_dir/staging"
mount_dir="$work_dir/mount"
mkdir "$staging" "$mount_dir"
ditto "$app" "$staging/$app_name"
ln -s /Applications "$staging/Applications"

hdiutil create -quiet \
    -volname zz \
    -srcfolder "$staging" \
    -fs HFS+ \
    -format UDZO \
    -ov "$output"
hdiutil verify -quiet "$output"

attach_output="$(hdiutil attach -readonly -nobrowse -mountpoint "$mount_dir" "$output")"
device="$(awk '$1 ~ /^\/dev\// { print $1; exit }' <<<"$attach_output")"
[[ -n "$device" ]] || die "could not determine the mounted DMG device"
[[ -d "$mount_dir/$app_name/Contents" ]] || die "DMG is missing $app_name"
[[ -L "$mount_dir/Applications" ]] || die "DMG is missing the Applications shortcut"
[[ "$(readlink "$mount_dir/Applications")" == "/Applications" ]] || \
    die "DMG Applications shortcut has the wrong target"
codesign --verify --deep --strict "$mount_dir/$app_name"

hdiutil detach "$device" >/dev/null
device=""

echo "DMG ready: $output"
