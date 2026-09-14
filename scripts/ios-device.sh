#!/usr/bin/env bash
set -euo pipefail
unset ZZ_SOCKET ZZ_PANE ZZ_SESSION TMUX TMUX_PANE ZZ_TMUX_EXECUTABLE ZZ_APP_STARTUP_DIRECTORY ZZ_STARTUP_REENTRY ZZ_DEV_BUILD

die() { echo "error: $*" >&2; exit 1; }

[[ "$(uname -s)" == "Darwin" ]] || die "device builds require macOS"
command -v xcodegen >/dev/null 2>&1 || die "xcodegen not found (brew install xcodegen)"
command -v xcrun >/dev/null 2>&1 || die "xcrun not found (install Xcode)"

device="${1:-iphone}"
configuration="${ZZ_IOS_CONFIGURATION:-Debug}"
repo_root="$(cd "$(dirname "$0")/.." && pwd)"
spec="$repo_root/clients/ios/project.yml"
project_dir="$repo_root/clients/ios"
project="$project_dir/ZZMobile.xcodeproj"
derived="$repo_root/target/ios-device-dev"
app="$derived/Build/Products/$configuration-iphoneos/ZZ.app"
workspace_version="$(sed -nE 's/^version = "([^"]+)"$/\1/p' "$repo_root/Cargo.toml" | head -1)"
marketing_version="${workspace_version%%[-+]*}"

[[ "$configuration" == "Debug" || "$configuration" == "Release" ]] || die "ZZ_IOS_CONFIGURATION must be Debug or Release"

xcodegen generate --spec "$spec" --project "$project_dir" >/dev/null
xcodebuild \
    -project "$project" \
    -scheme ZZMobile \
    -configuration "$configuration" \
    -destination "generic/platform=iOS" \
    -derivedDataPath "$derived" \
    -allowProvisioningUpdates \
    MARKETING_VERSION="$marketing_version" \
    ZZ_DEV_BUILD=1 ZZ_APP_BUNDLE_ID=dev.zz.ios.dev ZZ_APP_DISPLAY_NAME="zz Dev" ZZ_APP_URL_SCHEME=zz-dev ZZ_APP_ICON=zz-dev \
    build

[[ -d "$app" ]] || die "build finished but $app is missing"
xcrun devicectl device install app --device "$device" "$app"
xcrun devicectl device process launch --device "$device" dev.zz.ios.dev
