#!/usr/bin/env bash
set -euo pipefail

die() { echo "error: $*" >&2; exit 1; }

[[ "$(uname -s)" == "Darwin" ]] || die "device builds require macOS"
command -v xcodegen >/dev/null 2>&1 || die "xcodegen not found (brew install xcodegen)"
command -v xcrun >/dev/null 2>&1 || die "xcrun not found (install Xcode)"

device="${1:-iphone}"
repo_root="$(cd "$(dirname "$0")/.." && pwd)"
spec="$repo_root/clients/ios/project.yml"
project_dir="$repo_root/clients/ios"
project="$project_dir/ZZMobile.xcodeproj"
derived="$repo_root/target/ios-device"
app="$derived/Build/Products/Release-iphoneos/ZZ.app"
workspace_version="$(sed -nE 's/^version = "([^"]+)"$/\1/p' "$repo_root/Cargo.toml" | head -1)"
marketing_version="${workspace_version%%[-+]*}"

xcodegen generate --spec "$spec" --project "$project_dir" >/dev/null
xcodebuild \
    -project "$project" \
    -scheme ZZMobile \
    -configuration Release \
    -destination "generic/platform=iOS" \
    -derivedDataPath "$derived" \
    -allowProvisioningUpdates \
    MARKETING_VERSION="$marketing_version" \
    build

[[ -d "$app" ]] || die "build finished but $app is missing"
xcrun devicectl device install app --device "$device" "$app"
xcrun devicectl device process launch --device "$device" dev.zz.ios
