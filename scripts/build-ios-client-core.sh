#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
case "${PLATFORM_NAME:-iphonesimulator}" in
    iphonesimulator) rust_target="aarch64-apple-ios-sim" ;;
    iphoneos) rust_target="aarch64-apple-ios" ;;
    *) echo "error: unsupported Apple platform: ${PLATFORM_NAME:-unset}" >&2; exit 2 ;;
esac

profile="debug"
if [[ "${CONFIGURATION:-Debug}" == "Release" ]]; then
    profile="release"
fi

cd "$repo_root"
if [[ "$profile" == "release" ]]; then
    cargo build -p zz-client-ffi --target "$rust_target" --release
else
    cargo build -p zz-client-ffi --target "$rust_target"
fi
cp "target/$rust_target/$profile/libzz_client_ffi.a" "$BUILT_PRODUCTS_DIR/libzz_client_ffi.a"
