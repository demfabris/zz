#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
package="$repo_root/clients/macos"
mode="${1:-run}"
configuration="${2:-debug}"
shift "$(( $# < 2 ? $# : 2 ))"

if [[ "$(uname -s)" != Darwin ]]; then
    echo "The native macOS client requires macOS and Xcode." >&2
    exit 2
fi
if [[ "$mode" != run && "$mode" != build && "$mode" != test ]]; then
    echo "usage: macos-native.sh [run|build|test] [debug|release] [app arguments]" >&2
    exit 2
fi
if [[ "$configuration" != debug && "$configuration" != release ]]; then
    echo "configuration must be debug or release" >&2
    exit 2
fi

cd "$repo_root"
export MACOSX_DEPLOYMENT_TARGET=14.0
if [[ "$configuration" == release ]]; then
    cargo build -p zz-client-ffi --features native-browser --release
else
    cargo build -p zz-client-ffi --features native-browser
fi
target_directory="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
export ZZ_MACOS_FFI_DIR="$target_directory/$configuration"

if [[ "$mode" == test ]]; then
    cargo build -p zz-client-ffi --features native-browser --example native_fixture
    export ZZ_NATIVE_TEST_FIXTURE="$target_directory/debug/examples/native_fixture"
    exec swift test --package-path "$package"
fi

swift build --package-path "$package" --configuration "$configuration" --product ZZNative
binary_dir="$(swift build --package-path "$package" --configuration "$configuration" --show-bin-path)"
app="$package/dist/zz Native.app"
staging="$package/dist/.native-build"
mkdir -p "$staging"
cargo xtask bundle-native-macos "$binary_dir" "$ZZ_MACOS_FFI_DIR/zz_native_helper" "$staging"
next_app="$staging/ZZNative.app"
cp "$package/Resources/NativeInfo.plist" "$next_app/Contents/Info.plist"
version="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "zz"))')"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $version" "$next_app/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $version" "$next_app/Contents/Info.plist"
cp "$repo_root/packaging/mac/zz.icns" "$next_app/Contents/Resources/zz.icns"
cp "$repo_root/assets/zz-light-512.png" "$repo_root/assets/zz-dark-512.png" "$next_app/Contents/Resources/"
codesign --force --sign - "$next_app/Contents/Frameworks/Chromium Embedded Framework.framework"
for helper in "$next_app/Contents/Frameworks/"*.app; do
    codesign --force --sign - "$helper"
done
codesign --force --sign - "$next_app"
previous_app="$package/dist/.native-previous.app"
if [[ -d "$previous_app" ]]; then rm -rf "$previous_app"; fi
if [[ -d "$app" ]]; then mv "$app" "$previous_app"; fi
mv "$next_app" "$app"
echo "$app"
if [[ "$mode" == run ]]; then open -n "$app" --args "$@"; fi
