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
workspace_version="$(sed -nE 's/^version = "([^"]+)"$/\1/p' "$repo_root/Cargo.toml" | head -1)"
marketing_version="${workspace_version%%[-+]*}"

simulator_udid() {
    local udid
    udid="$(xcrun simctl list devices booted | grep 'iPhone' | grep -Eo '[0-9A-F]{8}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{12}' | head -1 || true)"
    if [[ -z "$udid" ]]; then
        udid="$(xcrun simctl list devices available \
            | grep 'iPhone' \
            | grep -Eo '[0-9A-F]{8}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{12}' \
            | head -1)"
    fi
    [[ -n "$udid" ]] || die "no iPhone simulator is available"
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
xcrun simctl boot "$udid" 2>/dev/null || true
open -a Simulator

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
