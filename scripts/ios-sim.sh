#!/usr/bin/env bash
set -euo pipefail

die() { echo "error: $*" >&2; exit 1; }

[[ "$(uname -s)" == "Darwin" ]] || die "the iOS simulator requires macOS"
command -v xcodegen >/dev/null 2>&1 || die "xcodegen not found (brew install xcodegen)"
command -v xcrun >/dev/null 2>&1 || die "xcrun not found (install Xcode)"

mode="${1:-run}"
[[ "$mode" == "run" || "$mode" == "--build-only" || "$mode" == "--test" ]] || die "usage: scripts/ios-sim.sh [--build-only|--test]"

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
spec="$repo_root/clients/ios/project.yml"
project_dir="$repo_root/clients/ios"
project="$project_dir/ZZMobile.xcodeproj"
derived="$repo_root/target/ios-sim"
app="$derived/Build/Products/Debug-iphonesimulator/ZZ.app"
bundle_id="dev.zz.ios"
simulator_family="${ZZ_IOS_SIMULATOR_FAMILY:-iPhone}"
workspace_version="$(sed -nE 's/^version = "([^"]+)"$/\1/p' "$repo_root/Cargo.toml" | head -1)"
marketing_version="${workspace_version%%[-+]*}"

[[ "$simulator_family" == "iPhone" || "$simulator_family" == "iPad" ]] || die "ZZ_IOS_SIMULATOR_FAMILY must be iPhone or iPad"

simulator_udid() {
    local udid
    udid="$(xcrun simctl list devices available --json | python3 -c '
import json, sys
family = sys.argv[1]
prefix = "com.apple.CoreSimulator.SimRuntime.iOS-"
runtimes = [
    (tuple(map(int, runtime.removeprefix(prefix).split("-"))), devices)
    for runtime, devices in json.load(sys.stdin)["devices"].items()
    if runtime.startswith(prefix)
]
for _, devices in sorted(runtimes, key=lambda item: item[0], reverse=True):
    candidates = [device for device in devices if device["name"].startswith(family)]
    if candidates:
        selected = next((device for device in candidates if device["state"] == "Booted"), candidates[0])
        print(selected["udid"])
        break
' "$simulator_family")"
    [[ -n "$udid" ]] || die "no $simulator_family simulator is available"
    echo "$udid"
}

xcodegen generate --spec "$spec" --project "$project_dir" >/dev/null
if [[ "$mode" == "--test" ]]; then
    udid="$(simulator_udid)"
    xcodebuild \
        -project "$project" \
        -scheme ZZMobile \
        -configuration Debug \
        -destination "platform=iOS Simulator,id=$udid" \
        -derivedDataPath "$derived" \
        MARKETING_VERSION="$marketing_version" \
        CODE_SIGNING_ALLOWED=NO \
        test
    exit 0
fi

xcodebuild \
    -project "$project" \
    -scheme ZZMobile \
    -configuration Debug \
    -destination "generic/platform=iOS Simulator" \
    -derivedDataPath "$derived" \
    MARKETING_VERSION="$marketing_version" \
    CODE_SIGNING_ALLOWED=NO \
    build

[[ -d "$app" ]] || die "build finished but $app is missing"
[[ "$mode" == "run" ]] || exit 0

udid="$(simulator_udid)"
xcrun simctl bootstatus "$udid" -b
developer_dir="${DEVELOPER_DIR:-$(xcode-select -p)}"
device_hub="$developer_dir/../Applications/DeviceHub.app"
if [[ -d "$device_hub" ]]; then
    open "$device_hub"
else
    open -a Simulator
fi

socket="${ZZ_SOCKET:-}"
if [[ -z "$socket" ]]; then
    if [[ -n "${XDG_RUNTIME_DIR:-}" ]]; then
        socket="$XDG_RUNTIME_DIR/zz/default.sock"
    else
        socket="${TMPDIR:-/tmp}/zz-${USER}/default.sock"
    fi
fi
[[ -S "$socket" ]] || echo "warning: no daemon socket at $socket; start zz first" >&2

xcrun simctl terminate "$udid" "$bundle_id" 2>/dev/null || true
xcrun simctl install "$udid" "$app"
exec env SIMCTL_CHILD_ZZ_SOCKET="$socket" xcrun simctl launch --console-pty "$udid" "$bundle_id"
