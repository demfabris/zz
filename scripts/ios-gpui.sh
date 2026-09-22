#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"
family="${1:-iPad}"
mode="${2:-run}"
demo="${ZZ_GPUI_DEMO:-app}"
[[ "$(uname -s)" == Darwin ]] || { echo "iOS GPUI requires macOS" >&2; exit 2; }
[[ "$family" == iPhone || "$family" == iPad ]] || { echo "expected iPhone or iPad" >&2; exit 2; }
[[ "$mode" == run || "$mode" == build || "$mode" == device || "$mode" == testflight ]] || { echo "expected run, build, device, or testflight" >&2; exit 2; }
[[ "$demo" == app || "$demo" == terminal ]] || { echo "ZZ_GPUI_DEMO must be app or terminal" >&2; exit 2; }
[[ "$(zig version)" == 0.16.0 ]] || { echo "Zig 0.16.0 is required" >&2; exit 2; }

if [[ "$demo" == terminal ]]; then
    build_target=(--example terminal)
    binary="examples/terminal"
else
    build_target=(--bin zz-gpui-ios)
    binary="zz-gpui-ios"
fi
compile_icon() {
    local app="$1" platform="$2" icon="$3" partial
    partial="$(mktemp)"
    xcrun actool "$repo_root/assets/$icon.icon" --compile "$app" --platform "$platform" \
        --minimum-deployment-target 26.0 --app-icon "$icon" --target-device iphone --target-device ipad \
        --output-partial-info-plist "$partial" --output-format human-readable-text >/dev/null
    /usr/libexec/PlistBuddy -c "Merge $partial" "$app/Info.plist" >/dev/null
    rm -f "$partial"
}

signing_identity() {
    local identity="${ZZ_GPUI_SIGN_IDENTITY:-$(security find-identity -v -p codesigning | sed -n 's/.*"\(Apple Development: .*\)"/\1/p' | head -1)}"
    [[ -n "$identity" ]] || { echo "no Apple Development signing identity; sign in to Xcode with your Apple ID" >&2; exit 1; }
    printf '%s' "$identity"
}

write_entitlements() {
    local path="$1" team="$2" bundle="$3" debuggable="$4"
    {
        echo '<?xml version="1.0" encoding="UTF-8"?>'
        echo '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">'
        echo '<plist version="1.0"><dict>'
        echo "    <key>application-identifier</key><string>$team.$bundle</string>"
        echo "    <key>com.apple.developer.team-identifier</key><string>$team</string>"
        [[ "$debuggable" == 1 ]] && echo '    <key>get-task-allow</key><true/>'
        echo "    <key>keychain-access-groups</key><array><string>$team.$bundle</string></array>"
        echo '</dict></plist>'
    } > "$path"
}

if [[ "$mode" == testflight ]]; then
    [[ "$demo" == app ]] || { echo "TestFlight ships the app, not the terminal example" >&2; exit 2; }
    bundle_id="${ZZ_IOS_BUNDLE_ID:-dev.zz.ios}"
    team="${ZZ_IOS_TEAM:-Y4JYPG9TS8}"
    workspace_version="$(sed -nE 's/^version = "([^"]+)"$/\1/p' "$repo_root/Cargo.toml" | head -1)"
    marketing_version="${workspace_version%%[-+]*}"
    build_number="${ZZ_IOS_BUILD_NUMBER:-$(date -u +%Y%m%d%H%M%S)}"
    [[ "$marketing_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "workspace version has no App Store version core: $workspace_version" >&2; exit 1; }
    [[ "$build_number" =~ ^[0-9]+(\.[0-9]+){0,2}$ ]] || { echo "build number must be one to three dot-separated integers" >&2; exit 1; }
    identity="$(signing_identity)"
    auth_args=()
    signing_dir="$repo_root/../.zz-signing"
    api_key_path="${APPLE_API_KEY_PATH:-$(ls "$signing_dir"/AuthKey_*.p8 2>/dev/null | head -1)}"
    api_key_id="${APPLE_API_KEY_ID:-$(basename "${api_key_path%.p8}" | sed 's/^AuthKey_//')}"
    if [[ -n "${APPLE_API_ISSUER_ID:-}" && -f "$api_key_path" ]]; then
        auth_args=(-authenticationKeyPath "$api_key_path" -authenticationKeyID "$api_key_id" -authenticationKeyIssuerID "$APPLE_API_ISSUER_ID")
    fi

    ZZ_DEV_BUILD=0 IPHONEOS_DEPLOYMENT_TARGET=26.0 cargo build --locked --release -p zz-gpui-ios "${build_target[@]}" --target aarch64-apple-ios
    out="$repo_root/target/ios-testflight"
    archive="$out/zz-$marketing_version-$build_number.xcarchive"
    export_path="$out/export-$marketing_version-$build_number"
    [[ ! -e "$archive" ]] || { echo "archive already exists: $archive" >&2; exit 1; }
    app="$archive/Products/Applications/ZZ.app"
    mkdir -p "$app"
    cp "$repo_root/target/aarch64-apple-ios/release/$binary" "$app/ZZ"
    cp "$repo_root/clients/ios-gpui/Info.plist" "$app/Info.plist"
    plist="$app/Info.plist"
    sdk_version="$(xcrun --sdk iphoneos --show-sdk-version)"
    sdk_build="$(xcrun --sdk iphoneos --show-sdk-build-version)"
    xcode_version="$(xcodebuild -version | sed -n 's/^Xcode \([0-9]*\)\.\([0-9]*\).*/\1\2/p')"
    xcode_build="$(xcodebuild -version | sed -n 's/^Build version //p')"
    for entry in \
        "CFBundleExecutable ZZ" "CFBundleIdentifier $bundle_id" "CFBundleName ZZ" "CFBundleDisplayName zz" \
        "CFBundleShortVersionString $marketing_version" "CFBundleVersion $build_number" \
        "DTPlatformName iphoneos" "DTPlatformVersion $sdk_version" "DTPlatformBuild $sdk_build" \
        "DTSDKName iphoneos$sdk_version" "DTSDKBuild $sdk_build" "DTXcode ${xcode_version}00" \
        "DTXcodeBuild $xcode_build" "DTCompiler com.apple.compilers.llvm.clang.1_0" \
        "BuildMachineOSBuild $(sw_vers -buildVersion)"; do
        plutil -replace "${entry%% *}" -string "${entry#* }" "$plist"
    done
    plutil -replace CFBundleSupportedPlatforms -json '["iPhoneOS"]' "$plist"
    plutil -replace UIRequiredDeviceCapabilities -json '["arm64"]' "$plist"
    compile_icon "$app" iphoneos zz
    entitlements="$out/entitlements-$build_number.plist"
    write_entitlements "$entitlements" "$team" "$bundle_id" 0
    codesign --force --timestamp=none --sign "$identity" --entitlements "$entitlements" "$app"
    plutil -create xml1 "$archive/Info.plist"
    plutil -insert ApplicationProperties -json "{\"ApplicationPath\": \"Applications/ZZ.app\", \"CFBundleIdentifier\": \"$bundle_id\", \"CFBundleShortVersionString\": \"$marketing_version\", \"CFBundleVersion\": \"$build_number\", \"SigningIdentity\": \"$identity\", \"Team\": \"$team\", \"Architectures\": [\"arm64\"]}" "$archive/Info.plist"
    plutil -insert ArchiveVersion -integer 2 "$archive/Info.plist"
    plutil -insert CreationDate -date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$archive/Info.plist"
    plutil -insert Name -string ZZ "$archive/Info.plist"
    plutil -insert SchemeName -string ZZ "$archive/Info.plist"

    options="$out/export-options-$build_number.plist"
    cp "$repo_root/clients/ios-gpui/TestFlightExportOptions.plist" "$options"
    plutil -replace teamID -string "$team" "$options"
    if [[ "${ZZ_IOS_UPLOAD:-1}" != 1 ]]; then
        plutil -replace destination -string export "$options"
    fi
    echo "==> exporting zz $marketing_version ($build_number) as $bundle_id"
    xcodebuild -exportArchive -archivePath "$archive" -exportPath "$export_path" \
        -exportOptionsPlist "$options" -allowProvisioningUpdates ${auth_args[@]+"${auth_args[@]}"}
    if [[ "${ZZ_IOS_UPLOAD:-1}" == 1 ]]; then
        echo "==> uploaded zz $marketing_version ($build_number) for internal TestFlight processing"
    else
        echo "==> exported $(ls "$export_path"/*.ipa)"
    fi
    exit 0
fi

if [[ "$mode" == device ]]; then
    bundle_id=dev.zz.gpui-poc
    devices_json="$(mktemp)"
    trap 'rm -f "$devices_json"' EXIT
    xcrun devicectl list devices --json-output "$devices_json" >/dev/null
    udid="${ZZ_GPUI_DEVICE:-$(python3 - "$devices_json" "$family" <<'PY'
import json, sys
devices = json.load(open(sys.argv[1]))["result"]["devices"]
for device in devices:
    hardware = device.get("hardwareProperties", {})
    if (hardware.get("reality") == "physical" and hardware.get("deviceType") == sys.argv[2]
            and device.get("connectionProperties", {}).get("pairingState") == "paired"):
        print(hardware["udid"])
        break
PY
)}"
    [[ -n "$udid" ]] || { echo "no paired physical $family; pair it in Xcode or set ZZ_GPUI_DEVICE" >&2; exit 1; }
    identity="$(signing_identity)"
    profile="${ZZ_GPUI_PROFILE:-$(python3 - "$udid" "$bundle_id" <<'PY'
import datetime, glob, os, plistlib, subprocess, sys
udid, bundle = sys.argv[1], sys.argv[2]
home = os.path.expanduser("~")
candidates = []
for path in glob.glob(f"{home}/Library/Developer/Xcode/UserData/Provisioning Profiles/*.mobileprovision") + glob.glob(f"{home}/Library/MobileDevice/Provisioning Profiles/*.mobileprovision"):
    decoded = subprocess.run(["security", "cms", "-D", "-i", path], capture_output=True).stdout
    if not decoded:
        continue
    profile = plistlib.loads(decoded)
    team = profile["TeamIdentifier"][0]
    app_id = profile["Entitlements"].get("application-identifier", "")
    if udid not in (profile.get("ProvisionedDevices") or []):
        continue
    if profile["ExpirationDate"] < datetime.datetime.now():
        continue
    if app_id == f"{team}.{bundle}":
        candidates.insert(0, path)
    elif app_id == f"{team}.*":
        candidates.append(path)
print(candidates[0] if candidates else "")
PY
)}"
    [[ -n "$profile" ]] || { echo "no provisioning profile covers $bundle_id on $udid; build any app to it once from Xcode, or set ZZ_GPUI_PROFILE" >&2; exit 1; }
    team="$(security cms -D -i "$profile" | plutil -extract TeamIdentifier.0 raw -)"
    endpoint="${ZZ_GPUI_ENDPOINT:-ssh://${USER}@$(scutil --get LocalHostName).local}"
    dev_build="${ZZ_DEV_BUILD-1}"
    ZZ_DEV_BUILD="$dev_build" IPHONEOS_DEPLOYMENT_TARGET=26.0 cargo build --locked --release -p zz-gpui-ios "${build_target[@]}" --target aarch64-apple-ios
    app="$repo_root/target/ios-gpui-device/ZZ GPUI.app"
    rm -rf "$app"
    mkdir -p "$app"
    cp "$repo_root/target/aarch64-apple-ios/release/$binary" "$app/ZZGPUI"
    cp "$repo_root/clients/ios-gpui/Info.plist" "$app/Info.plist"
    plutil -replace CFBundleSupportedPlatforms -json '["iPhoneOS"]' "$app/Info.plist"
    compile_icon "$app" iphoneos zz-dev
    cp "$profile" "$app/embedded.mobileprovision"
    entitlements="$repo_root/target/ios-gpui-device/entitlements.plist"
    write_entitlements "$entitlements" "$team" "$bundle_id" 1
    codesign --force --timestamp=none --sign "$identity" --entitlements "$entitlements" "$app"
    [[ "${3:-}" == --build-only ]] && exit 0
    xcrun devicectl device install app --device "$udid" "$app"
    echo "launching on $udid with endpoint $endpoint"
    exec xcrun devicectl device process launch --device "$udid" --terminate-existing --console \
        --environment-variables "{\"ZZ_GPUI_ENDPOINT\": \"$endpoint\"}" "$bundle_id"
fi

IPHONEOS_DEPLOYMENT_TARGET=26.0 cargo build --locked -p zz-gpui-ios "${build_target[@]}" --target aarch64-apple-ios-sim
app="$repo_root/target/ios-gpui/ZZ GPUI.app"
mkdir -p "$app"
cp "$repo_root/target/aarch64-apple-ios-sim/debug/$binary" "$app/ZZGPUI"
cp "$repo_root/clients/ios-gpui/Info.plist" "$app/Info.plist"
compile_icon "$app" iphonesimulator zz-dev
codesign --force --sign - "$app"
[[ "$mode" == run ]] || exit 0

endpoint="${ZZ_GPUI_ENDPOINT:-${ZZ_DEV_SOCKET:-${ZZ_SOCKET:-}}}"
if [[ -z "$endpoint" ]]; then
    darwin_tmp="$(getconf DARWIN_USER_TEMP_DIR 2>/dev/null || true)"
    for candidate in "${XDG_RUNTIME_DIR:-/tmp}/zz-dev/default.sock" "${TMPDIR:-/tmp}/zz-dev-${USER:-user}/default.sock" "${darwin_tmp%/}/zz-dev-${USER:-user}/default.sock" "/tmp/zz-dev-${USER:-user}/default.sock"; do
        if [[ -S "$candidate" ]] && python3 - "$candidate" <<'PY'
import socket, sys

with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
    client.settimeout(0.25)
    try:
        client.connect(sys.argv[1])
    except OSError:
        sys.exit(1)
PY
        then endpoint="$candidate"; break; fi
    done
fi
unset SIMCTL_CHILD_ZZ_GPUI_ENDPOINT SIMCTL_CHILD_ZZ_GPUI_SESSION
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
