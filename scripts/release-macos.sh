#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_ENTITLEMENTS="$REPO_ROOT/packaging/macos-signing/app-entitlements.plist"
HELPER_JIT_ENTITLEMENTS="$REPO_ROOT/packaging/macos-signing/helper-jit-entitlements.plist"
DEFAULT_APP="$REPO_ROOT/dist/zz/zz.app"

SIGN_IDENTITY=""
NOTARY_PROFILE=""
MOUNT_DEVICE=""
MOUNT_WORK_DIR=""

die() { echo "error: $*" >&2; exit 1; }
note() { echo "==> $*"; }

usage() {
    cat >&2 <<'EOF'
usage: release-macos.sh <command> [arguments]

commands:
  preflight                  Check the Developer ID identity and Keychain profile.
  sign-app [application]     Developer ID-sign an existing zz.app bundle.
  notarize-dmg <disk-image>  Sign, submit, staple, and verify an existing DMG.
  verify-dmg <disk-image>    Verify an already-notarized DMG and its bundled app.
  release <version-label>    Build, sign, package, notarize, and verify a release.

environment:
  MACOS_SIGN_IDENTITY   Developer ID Application identity or SHA-1. Auto-detected
                        when exactly one valid Developer ID identity is installed.
  MACOS_NOTARY_PROFILE Keychain profile created by `notarytool store-credentials`.
  MACOS_NOTARY_TIMEOUT Maximum `notarytool submit --wait` duration (default: 30m).
EOF
    exit 2
}

cleanup_mount() {
    if [[ -n "$MOUNT_DEVICE" ]]; then
        hdiutil detach "$MOUNT_DEVICE" >/dev/null 2>&1 || true
        MOUNT_DEVICE=""
    fi
    if [[ -n "$MOUNT_WORK_DIR" && "$MOUNT_WORK_DIR" == "${TMPDIR:-/tmp}/zz-release."* ]]; then
        rm -rf "$MOUNT_WORK_DIR"
        MOUNT_WORK_DIR=""
    fi
}
trap cleanup_mount EXIT

require_macos() {
    [[ "$(uname -s)" == "Darwin" ]] || die "macOS release commands require macOS"
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || die "required command is unavailable: $1"
}

absolute_existing_path() {
    local path="$1"
    [[ -e "$path" ]] || die "path does not exist: $path"
    (cd "$(dirname "$path")" && printf '%s/%s\n' "$PWD" "$(basename "$path")")
}

load_signing_identity() {
    local configured="${MACOS_SIGN_IDENTITY:-}"
    local identities_output
    local identity
    local identities=()

    identities_output="$(security find-identity -v -p codesigning)"
    if [[ -n "$configured" ]]; then
        [[ "$identities_output" == *"$configured"* ]] || \
            die "MACOS_SIGN_IDENTITY does not match a valid Keychain identity"
        SIGN_IDENTITY="$configured"
        return
    fi

    while IFS= read -r identity; do
        [[ -n "$identity" ]] && identities+=("$identity")
    done < <(sed -nE 's/^ *[0-9]+\) [0-9A-F]+ "(Developer ID Application: .+)"$/\1/p' \
        <<<"$identities_output")

    case "${#identities[@]}" in
        0) die "no valid Developer ID Application identity is installed" ;;
        1) SIGN_IDENTITY="${identities[0]}" ;;
        *) die "multiple Developer ID identities found; set MACOS_SIGN_IDENTITY explicitly" ;;
    esac
}

load_notary_profile() {
    NOTARY_PROFILE="${MACOS_NOTARY_PROFILE:-}"
    [[ -n "$NOTARY_PROFILE" ]] || \
        die "set MACOS_NOTARY_PROFILE to a notarytool Keychain profile"
}

preflight_signing() {
    require_macos
    require_command codesign
    require_command file
    require_command plutil
    require_command security
    [[ -s "$APP_ENTITLEMENTS" ]] || die "missing app entitlements: $APP_ENTITLEMENTS"
    [[ -s "$HELPER_JIT_ENTITLEMENTS" ]] || \
        die "missing helper entitlements: $HELPER_JIT_ENTITLEMENTS"
    plutil -lint "$APP_ENTITLEMENTS" "$HELPER_JIT_ENTITLEMENTS" >/dev/null
    load_signing_identity
}

preflight_notary() {
    preflight_signing
    require_command hdiutil
    require_command shasum
    require_command spctl
    require_command xcrun
    load_notary_profile
    note "Checking notary credentials in Keychain profile '$NOTARY_PROFILE'"
    xcrun notarytool history \
        --keychain-profile "$NOTARY_PROFILE" \
        --output-format json >/dev/null
}

assert_app_layout() {
    local app="$1"
    local framework="$app/Contents/Frameworks/Chromium Embedded Framework.framework"
    local helper
    local helpers=(
        "zz Helper.app"
        "zz Helper (GPU).app"
        "zz Helper (Renderer).app"
        "zz Helper (Plugin).app"
        "zz Helper (Alerts).app"
    )

    [[ -d "$app/Contents" ]] || die "application bundle does not exist: $app"
    [[ -x "$app/Contents/MacOS/zz" ]] || die "zz executable is missing from $app"
    [[ -x "$app/Contents/MacOS/cli" ]] || die "PATH launcher is missing from $app"
    [[ -d "$framework" ]] || die "CEF framework is missing from $app"
    [[ -x "$framework/Chromium Embedded Framework" ]] || \
        die "CEF framework executable is missing from $app"
    for helper in "${helpers[@]}"; do
        [[ -d "$app/Contents/Frameworks/$helper/Contents" ]] || \
            die "CEF helper is missing from $app: $helper"
    done
}

sign_library_code() {
    local path="$1"
    codesign --force --sign "$SIGN_IDENTITY" --timestamp "$path"
}

sign_runtime_code() {
    local path="$1"
    local entitlements="${2:-}"
    local args=(--force --sign "$SIGN_IDENTITY" --timestamp --options runtime)
    if [[ -n "$entitlements" ]]; then
        args+=(--entitlements "$entitlements")
    fi
    codesign "${args[@]}" "$path"
}

verify_all_macho_code_is_developer_id_signed() {
    local app="$1"
    local candidate
    local kind
    local signature

    while IFS= read -r -d '' candidate; do
        kind="$(file -b "$candidate")"
        [[ "$kind" == Mach-O* ]] || continue
        signature="$(codesign -dvv "$candidate" 2>&1)" || \
            die "unsigned Mach-O code remains in the application: $candidate"
        [[ "$signature" == *"Authority=Developer ID Application:"* ]] || \
            die "non-Developer-ID Mach-O code remains in the application: $candidate"
    done < <(find "$app" -type f -print0)
}

sign_app() {
    local app
    local framework
    local candidate
    local kind
    local helper

    preflight_signing
    app="$(absolute_existing_path "$1")"
    assert_app_layout "$app"
    framework="$app/Contents/Frameworks/Chromium Embedded Framework.framework"

    note "Signing CEF libraries with '$SIGN_IDENTITY'"
    while IFS= read -r -d '' candidate; do
        kind="$(file -b "$candidate")"
        if [[ "$kind" == Mach-O* ]]; then
            sign_library_code "$candidate"
        fi
    done < <(find "$framework/Libraries" -type f -print0)
    sign_library_code "$framework/Chromium Embedded Framework"
    sign_library_code "$framework"

    note "Signing CEF helpers with Hardened Runtime"
    for helper in \
        "zz Helper.app" \
        "zz Helper (Plugin).app" \
        "zz Helper (Alerts).app"; do
        sign_runtime_code "$app/Contents/Frameworks/$helper"
    done
    for helper in \
        "zz Helper (GPU).app" \
        "zz Helper (Renderer).app"; do
        sign_runtime_code "$app/Contents/Frameworks/$helper" "$HELPER_JIT_ENTITLEMENTS"
    done

    note "Signing the PATH launcher with Hardened Runtime"
    sign_runtime_code "$app/Contents/MacOS/cli"

    note "Signing the main application with Hardened Runtime"
    sign_runtime_code "$app" "$APP_ENTITLEMENTS"
    codesign --verify --deep --strict --verbose=2 "$app"
    verify_all_macho_code_is_developer_id_signed "$app"

    local signature
    signature="$(codesign -dvv "$app" 2>&1)"
    [[ "$signature" == *"Authority=Developer ID Application:"* ]] || \
        die "main application is not Developer ID-signed"
    [[ "$signature" == *"flags=0x10000(runtime)"* ]] || \
        die "main application does not enable Hardened Runtime"
    note "Developer ID signature verified: $app"
}

verify_dmg_contents() {
    local dmg="$1"
    local mount_dir
    local attach_output
    local apps=()
    local app
    local signature

    hdiutil verify -quiet "$dmg"
    MOUNT_WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/zz-release.XXXXXX")"
    mount_dir="$MOUNT_WORK_DIR/mount"
    mkdir "$mount_dir"
    attach_output="$(hdiutil attach -readonly -nobrowse -mountpoint "$mount_dir" "$dmg")"
    MOUNT_DEVICE="$(awk '$1 ~ /^\/dev\// { print $1; exit }' <<<"$attach_output")"
    [[ -n "$MOUNT_DEVICE" ]] || die "could not determine the mounted DMG device"

    while IFS= read -r -d '' app; do
        apps+=("$app")
    done < <(find "$mount_dir" -maxdepth 1 -type d -name '*.app' -print0)
    [[ "${#apps[@]}" -eq 1 ]] || \
        die "DMG must contain exactly one top-level application bundle"
    app="${apps[0]}"
    codesign --verify --deep --strict --verbose=2 "$app"
    verify_all_macho_code_is_developer_id_signed "$app"
    signature="$(codesign -dvv "$app" 2>&1)"
    [[ "$signature" == *"Authority=Developer ID Application:"* ]] || \
        die "DMG contains an app without a Developer ID signature"
    [[ "$signature" == *"flags=0x10000(runtime)"* ]] || \
        die "DMG contains an app without Hardened Runtime"

    hdiutil detach "$MOUNT_DEVICE" >/dev/null
    MOUNT_DEVICE=""
    rm -rf "$MOUNT_WORK_DIR"
    MOUNT_WORK_DIR=""
}

sign_dmg() {
    local dmg="$1"
    note "Signing disk image with '$SIGN_IDENTITY'"
    codesign --force --sign "$SIGN_IDENTITY" --timestamp "$dmg"
    codesign --verify --strict --verbose=2 "$dmg"
}

verify_notarized_dmg() {
    local dmg
    require_macos
    require_command codesign
    require_command file
    require_command hdiutil
    require_command spctl
    require_command xcrun
    dmg="$(absolute_existing_path "$1")"

    verify_dmg_contents "$dmg"
    codesign --verify --strict --verbose=2 "$dmg"
    xcrun stapler validate "$dmg"
    spctl --assess \
        --type open \
        --context context:primary-signature \
        --verbose=4 \
        "$dmg"
    note "Notarized disk image verified: $dmg"
}

notarize_dmg() {
    local dmg
    local response
    local status
    local submission_id
    local timeout="${MACOS_NOTARY_TIMEOUT:-30m}"
    local log_path

    preflight_notary
    dmg="$(absolute_existing_path "$1")"
    [[ "$dmg" == *.dmg ]] || die "notarization input must be a .dmg file: $dmg"
    verify_dmg_contents "$dmg"
    sign_dmg "$dmg"

    note "Submitting $(basename "$dmg") to Apple and waiting up to $timeout"
    response="$(xcrun notarytool submit "$dmg" \
        --keychain-profile "$NOTARY_PROFILE" \
        --wait \
        --timeout "$timeout" \
        --output-format plist)"
    status="$(printf '%s' "$response" | plutil -extract status raw -o - -)"
    submission_id="$(printf '%s' "$response" | plutil -extract id raw -o - -)"
    note "Notarization status: $status ($submission_id)"

    log_path="${dmg%.dmg}.$submission_id.notary.json"
    xcrun notarytool log \
        "$submission_id" \
        --keychain-profile "$NOTARY_PROFILE" \
        "$log_path" >/dev/null
    [[ "$status" == "Accepted" ]] || \
        die "Apple rejected the submission; inspect $log_path"

    xcrun stapler staple "$dmg"
    verify_notarized_dmg "$dmg"
    note "Notary log: $log_path"
    shasum -a 256 "$dmg"
}

release() {
    local version="$1"
    local architecture
    local output

    [[ "$version" =~ ^[0-9A-Za-z][0-9A-Za-z._-]*$ ]] || \
        die "version label may contain only letters, numbers, dots, underscores, and hyphens"
    preflight_notary
    require_command just

    architecture="$(uname -m)"
    case "$architecture" in
        arm64|x86_64) ;;
        *) die "unsupported macOS release architecture: $architecture" ;;
    esac
    output="$REPO_ROOT/dist/zz-$version-macos-$architecture.dmg"
    [[ ! -e "$output" ]] || \
        die "release artifact already exists; choose a new version label: $output"

    note "Building macOS release bundle"
    (cd "$REPO_ROOT" && just build mac)
    sign_app "$DEFAULT_APP"
    "$REPO_ROOT/scripts/package-dmg.sh" "$DEFAULT_APP" "$output"
    notarize_dmg "$output"
    note "macOS release ready: $output"
}

command="${1:-}"
case "$command" in
    preflight)
        [[ $# -eq 1 ]] || usage
        preflight_notary
        note "macOS release credentials are ready"
        ;;
    sign-app)
        [[ $# -le 2 ]] || usage
        sign_app "${2:-$DEFAULT_APP}"
        ;;
    notarize-dmg)
        [[ $# -eq 2 ]] || usage
        notarize_dmg "$2"
        ;;
    verify-dmg)
        [[ $# -eq 2 ]] || usage
        verify_notarized_dmg "$2"
        ;;
    release)
        [[ $# -eq 2 ]] || usage
        release "$2"
        ;;
    *) usage ;;
esac
