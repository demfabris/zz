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
if [[ "$configuration" == release ]]; then
    cargo build -p zz-client-ffi --release
else
    cargo build -p zz-client-ffi
fi
target_directory="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
export ZZ_MACOS_FFI_DIR="$target_directory/$configuration"

if [[ "$mode" == test ]]; then
    cargo build -p zz-client-ffi --example native_fixture
    export ZZ_NATIVE_TEST_FIXTURE="$target_directory/debug/examples/native_fixture"
    exec swift test --package-path "$package"
fi

swift build --package-path "$package" --configuration "$configuration" --product ZZNative
binary_dir="$(swift build --package-path "$package" --configuration "$configuration" --show-bin-path)"
app="$package/dist/zz Native.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary_dir/ZZNative" "$app/Contents/MacOS/ZZNative.new"
mv -f "$app/Contents/MacOS/ZZNative.new" "$app/Contents/MacOS/ZZNative"
cp "$package/Resources/NativeInfo.plist" "$app/Contents/Info.plist"
cp "$repo_root/packaging/mac/zz.icns" "$app/Contents/Resources/zz.icns"
codesign --force --sign - "$app"
echo "$app"
if [[ "$mode" == run ]]; then open -n "$app" --args "$@"; fi
