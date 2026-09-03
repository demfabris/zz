#!/usr/bin/env bash
set -euo pipefail

die() { echo "error: $*" >&2; exit 1; }
note() { echo "==> $*"; }

[[ "$(uname -s)" == "Darwin" ]] || die "TestFlight builds require macOS"
command -v xcodebuild >/dev/null 2>&1 || die "xcodebuild not found (install Xcode)"
command -v xcodegen >/dev/null 2>&1 || die "xcodegen not found (brew install xcodegen)"

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
project_dir="$repo_root/clients/ios"
project="$project_dir/ZZMobile.xcodeproj"
spec="$project_dir/project.yml"
export_options="$project_dir/Support/TestFlightExportOptions.plist"
output_root="$repo_root/target/ios-preview"
derived="$output_root/DerivedData"
workspace_version="$(sed -nE 's/^version = "([^"]+)"$/\1/p' "$repo_root/Cargo.toml" | head -1)"
marketing_version="${workspace_version%%[-+]*}"
build_number="${1:-$(date -u +%Y%m%d%H%M%S)}"
archive="$output_root/ZZ-$marketing_version-$build_number.xcarchive"
export_path="$output_root/upload-$marketing_version-$build_number"

[[ "$marketing_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || \
    die "workspace version does not have an App Store version core: $workspace_version"
[[ "$build_number" =~ ^[0-9]+(\.[0-9]+){0,2}$ ]] || \
    die "build number must contain one to three dot-separated integers"
[[ ! -e "$archive" ]] || die "archive already exists: $archive"
[[ ! -e "$export_path" ]] || die "export path already exists: $export_path"
[[ -f "$export_options" ]] || die "missing export options: $export_options"

auth_args=()
api_key_path="${APPLE_API_KEY_PATH:-}"
api_key_id="${APPLE_API_KEY_ID:-}"
api_issuer_id="${APPLE_API_ISSUER_ID:-}"
if [[ -n "$api_key_path" || -n "$api_key_id" || -n "$api_issuer_id" ]]; then
    [[ -f "$api_key_path" ]] || die "APPLE_API_KEY_PATH must name an App Store Connect .p8 key"
    [[ -n "$api_key_id" ]] || die "APPLE_API_KEY_ID is required with APPLE_API_KEY_PATH"
    [[ -n "$api_issuer_id" ]] || die "APPLE_API_ISSUER_ID is required with APPLE_API_KEY_PATH"
    auth_args=(
        -authenticationKeyPath "$api_key_path"
        -authenticationKeyID "$api_key_id"
        -authenticationKeyIssuerID "$api_issuer_id"
    )
fi

mkdir -p "$output_root"
xcodegen generate --spec "$spec" --project "$project_dir" >/dev/null

note "Archiving zz $marketing_version ($build_number)"
xcodebuild \
    -project "$project" \
    -scheme ZZMobile \
    -configuration Release \
    -destination "generic/platform=iOS" \
    -derivedDataPath "$derived" \
    -archivePath "$archive" \
    -allowProvisioningUpdates \
    "${auth_args[@]}" \
    MARKETING_VERSION="$marketing_version" \
    CURRENT_PROJECT_VERSION="$build_number" \
    archive

[[ -d "$archive" ]] || die "archive finished but $archive is missing"

note "Uploading internal-only TestFlight build"
xcodebuild \
    -exportArchive \
    -archivePath "$archive" \
    -exportPath "$export_path" \
    -exportOptionsPlist "$export_options" \
    -allowProvisioningUpdates \
    "${auth_args[@]}"

note "Uploaded zz $marketing_version ($build_number) for internal TestFlight processing"
