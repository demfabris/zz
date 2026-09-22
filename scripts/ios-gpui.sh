#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"
family="${1:-iPad}"
mode="${2:-run}"
[[ "$(uname -s)" == Darwin ]] || { echo "iOS GPUI requires macOS" >&2; exit 2; }
[[ "$family" == iPhone || "$family" == iPad ]] || { echo "expected iPhone or iPad" >&2; exit 2; }
[[ "$mode" == run || "$mode" == build ]] || { echo "expected run or build" >&2; exit 2; }
[[ "$(zig version)" == 0.16.0 ]] || { echo "Zig 0.16.0 is required" >&2; exit 2; }

IPHONEOS_DEPLOYMENT_TARGET=26.0 cargo build --locked -p zz-gpui-ios --example terminal --target aarch64-apple-ios-sim
app="$repo_root/target/ios-gpui/ZZ GPUI.app"
mkdir -p "$app"
cp "$repo_root/target/aarch64-apple-ios-sim/debug/examples/terminal" "$app/ZZGPUI"
cp "$repo_root/clients/ios-gpui/Info.plist" "$app/Info.plist"
codesign --force --sign - "$app"
[[ "$mode" == run ]] || exit 0

endpoint="${ZZ_GPUI_ENDPOINT:-${ZZ_DEV_SOCKET:-}}"
if [[ -z "$endpoint" ]]; then
    darwin_tmp="$(getconf DARWIN_USER_TEMP_DIR 2>/dev/null || true)"
    for candidate in "${XDG_RUNTIME_DIR:-/tmp}/zz-dev/default.sock" "${TMPDIR:-/tmp}/zz-dev-${USER:-user}/default.sock" "${darwin_tmp%/}/zz-dev-${USER:-user}/default.sock" "/tmp/zz-dev-${USER:-user}/default.sock"; do
        if [[ -S "$candidate" ]]; then endpoint="$candidate"; break; fi
    done
fi
if [[ -n "$endpoint" ]]; then
    export SIMCTL_CHILD_ZZ_GPUI_ENDPOINT="$endpoint"
fi
if [[ -n "${ZZ_GPUI_SESSION:-}" ]]; then
    export SIMCTL_CHILD_ZZ_GPUI_SESSION="$ZZ_GPUI_SESSION"
fi

udid="${ZZ_GPUI_SIMULATOR:-}"
if [[ -z "$udid" ]]; then
    udid="$(xcrun simctl list devices available --json | python3 -c '
import json, sys
family = sys.argv[1]
prefix = "com.apple.CoreSimulator.SimRuntime.iOS-"
runtimes = [(tuple(map(int, key.removeprefix(prefix).split("-"))), devices)
    for key, devices in json.load(sys.stdin)["devices"].items() if key.startswith(prefix)]
for _, devices in sorted(runtimes, reverse=True):
    candidates = [device for device in devices if device["name"].startswith(family)]
    if candidates:
        print(next((device for device in candidates if device["state"] == "Booted"), candidates[0])["udid"])
        break
' "$family")"
fi
[[ -n "$udid" ]] || { echo "no $family simulator available" >&2; exit 1; }
xcrun simctl bootstatus "$udid" -b
developer_dir="${DEVELOPER_DIR:-$(xcode-select -p)}"
if [[ -d "$developer_dir/../Applications/DeviceHub.app" ]]; then
    open "$developer_dir/../Applications/DeviceHub.app"
else
    open -a Simulator
fi
xcrun simctl terminate "$udid" dev.zz.gpui-poc 2>/dev/null || true
xcrun simctl install "$udid" "$app"
exec xcrun simctl launch --console-pty "$udid" dev.zz.gpui-poc
