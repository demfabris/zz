#!/usr/bin/env bash
set -euo pipefail

die() { echo "error: $*" >&2; exit 1; }

[[ "$(uname -s)" == "Darwin" ]] || die "device builds require macOS"
command -v xcodegen >/dev/null 2>&1 || die "xcodegen not found (brew install xcodegen)"
command -v xcrun >/dev/null 2>&1 || die "xcrun not found (install Xcode)"

device="${1:-ipad}"
spec="crates/zz-ios/ios/project.yml"
project="crates/zz-ios/ios/ZZ.xcodeproj"
derived="target/ios-device"
app="$derived/Build/Products/Release-iphoneos/ZZ.app"
workspace_version="$(sed -nE 's/^version = "([^"]+)"$/\1/p' Cargo.toml | head -1)"
marketing_version="${workspace_version%%[-+]*}"
[[ "$marketing_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || \
    die "workspace version is not valid SemVer: $workspace_version"

xcodegen generate --spec "$spec" --project "$(dirname "$project")" >/dev/null
echo "building + signing (cargo release build runs inside Xcode's script phase)..."
xcodebuild \
    -project "$project" \
    -scheme ZZ \
    -configuration Release \
    -destination "generic/platform=iOS" \
    -derivedDataPath "$derived" \
    -allowProvisioningUpdates \
    MARKETING_VERSION="$marketing_version" \
    build

[[ -d "$app" ]] || die "build finished but $app is missing"

echo "installing on '$device'..."
xcrun devicectl device install app --device "$device" "$app" || die \
    "install failed - is the iPad connected (USB or same Wi-Fi), unlocked, and paired?
     first run: enable Settings > Privacy & Security > Developer Mode on the iPad, then rerun"

echo "launching..."
xcrun devicectl device process launch --device "$device" dev.zz.ios || die \
    "launch failed - if iOS blocked the app, trust it under Settings > General > VPN & Device Management"

echo
echo "on the iPad: the local host row will sit in an error state (there is no daemon"
echo "on the device - expected). add this Mac from the sidebar: ssh://demfabris@$(hostname -s).local"
echo "after enabling Remote Login on the Mac (System Settings > General > Sharing)."
