#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
package="$repo_root/clients/macos"
mode="${1:-run}"
configuration="${2:-debug}"

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "The native macOS gallery requires macOS and Xcode." >&2
    exit 2
fi

if [[ "$mode" == "test" ]]; then
    exec bash "$repo_root/scripts/macos-native.sh" test
fi

if [[ "$mode" != "run" && "$mode" != "build" ]]; then
    echo "usage: macos-gallery.sh [run|build|test] [debug|release]" >&2
    exit 2
fi

if [[ "$configuration" != "debug" && "$configuration" != "release" ]]; then
    echo "configuration must be debug or release" >&2
    exit 2
fi

swift build --package-path "$package" --configuration "$configuration" --product ZZComponentGallery
binary_dir="$(swift build --package-path "$package" --configuration "$configuration" --show-bin-path)"
app="$package/dist/zz Native Gallery.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary_dir/ZZComponentGallery" "$app/Contents/MacOS/ZZComponentGallery"
cp "$package/Resources/Info.plist" "$app/Contents/Info.plist"
cp "$repo_root/packaging/mac/zz.icns" "$app/Contents/Resources/zz.icns"
codesign --force --sign - "$app"
echo "$app"

if [[ "$mode" == "run" ]]; then
    open -n "$app"
fi
