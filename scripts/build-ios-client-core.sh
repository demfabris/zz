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
library="target/$rust_target/$profile/libzz_client_ffi.a"
if [[ "${ZZ_IOS_REUSE_CLIENT_CORE:-0}" != "1" ]]; then
    if [[ "$profile" == "release" ]]; then
        cargo build -p zz-client-ffi --target "$rust_target" --release
    else
        cargo build -p zz-client-ffi --target "$rust_target"
    fi
fi
[[ -f "$library" ]] || {
    echo "error: reusable iOS client core is missing: $library" >&2
    exit 1
}
cp "$library" "$BUILT_PRODUCTS_DIR/libzz_client_ffi.a"
