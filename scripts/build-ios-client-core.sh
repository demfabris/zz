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
dev_build="${ZZ_DEV_BUILD:-0}"
[[ "$dev_build" == 0 || "$dev_build" == 1 ]] || {
    echo "error: ZZ_DEV_BUILD must be 0 or 1" >&2
    exit 2
}
library="target/ios-client-core/$dev_build/$rust_target/$profile/libzz_client_ffi.a"
if [[ "${ZZ_IOS_REUSE_CLIENT_CORE:-0}" != "1" ]]; then
    if [[ "$profile" == "release" ]]; then
        ZZ_DEV_BUILD="$dev_build" cargo build -p zz-client-ffi --target-dir "$repo_root/target" --target "$rust_target" --release
    else
        ZZ_DEV_BUILD="$dev_build" cargo build -p zz-client-ffi --target-dir "$repo_root/target" --target "$rust_target"
    fi
    mkdir -p "$(dirname "$library")"
    cp "target/$rust_target/$profile/libzz_client_ffi.a" "$library"
fi
[[ -f "$library" ]] || {
    echo "error: reusable iOS client core is missing: $library" >&2
    exit 1
}
cp "$library" "$BUILT_PRODUCTS_DIR/libzz_client_ffi.a"
