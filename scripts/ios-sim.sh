#!/usr/bin/env bash
set -euo pipefail
unset ZZ_SOCKET ZZ_PANE ZZ_SESSION TMUX TMUX_PANE ZZ_TMUX_EXECUTABLE ZZ_APP_STARTUP_DIRECTORY ZZ_STARTUP_REENTRY ZZ_DEV_BUILD

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
derived="$repo_root/target/ios-sim-dev"
app="$derived/Build/Products/Debug-iphonesimulator/ZZ.app"
bundle_id="dev.zz.ios.dev"
simulator_family="${ZZ_IOS_SIMULATOR_FAMILY:-iPhone}"
workspace_version="$(sed -nE 's/^version = "([^"]+)"$/\1/p' "$repo_root/Cargo.toml" | head -1)"
marketing_version="${workspace_version%%[-+]*}"

[[ "$simulator_family" == "iPhone" || "$simulator_family" == "iPad" ]] || die "ZZ_IOS_SIMULATOR_FAMILY must be iPhone or iPad"

if [[ "$mode" == "run" ]]; then
    socket="${ZZ_DEV_SOCKET:-}"
    if [[ -z "$socket" ]]; then
        darwin_tmp="$(getconf DARWIN_USER_TEMP_DIR 2>/dev/null || true)"
        darwin_tmp="${darwin_tmp:-/tmp}"
        if [[ -n "${XDG_RUNTIME_DIR:-}" ]]; then
            socket="$XDG_RUNTIME_DIR/zz-dev/default.sock"
        else
            socket="${TMPDIR:-$darwin_tmp}/zz-dev-${USER:-user}/default.sock"
        fi
        for temp_dir in "$darwin_tmp" /tmp; do
            [[ -S "$socket" ]] && break
            candidate="${temp_dir%/}/zz-dev-${USER:-user}/default.sock"
            [[ ! -S "$candidate" ]] || socket="$candidate"
        done
    fi
    [[ -S "$socket" ]] || die "no dev daemon socket at $socket; run just run mac first or set ZZ_DEV_SOCKET"
    echo "Using dev daemon socket: $socket"
fi

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
        ZZ_DEV_BUILD=1 ZZ_APP_BUNDLE_ID="$bundle_id" ZZ_APP_DISPLAY_NAME="zz Dev" ZZ_APP_URL_SCHEME=zz-dev ZZ_APP_ICON=zz-dev \
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
    ZZ_DEV_BUILD=1 ZZ_APP_BUNDLE_ID="$bundle_id" ZZ_APP_DISPLAY_NAME="zz Dev" ZZ_APP_URL_SCHEME=zz-dev ZZ_APP_ICON=zz-dev \
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

xcrun simctl terminate "$udid" "$bundle_id" 2>/dev/null || true
xcrun simctl install "$udid" "$app"
exec env SIMCTL_CHILD_ZZ_SOCKET="$socket" xcrun simctl launch --console-pty "$udid" "$bundle_id"
