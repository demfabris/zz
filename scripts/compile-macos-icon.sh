#!/usr/bin/env bash
# Regenerate packaging/mac/Assets.car from assets/zz.icon. Run this after
# editing the icon and commit the result: actool renders the layered Icon
# Composer icon through the GPU and fails sporadically on virtualized CI
# runners, so the release bundles the checked-in artifact instead of rolling
# those dice per release. Compiling the .icon format needs Xcode 26+.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

die() { echo "error: $*" >&2; exit 1; }

[[ "$(uname -s)" == "Darwin" ]] || die "compiling the macOS icon requires macOS"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/zz-icon.XXXXXX")"
cleanup() { rm -rf "$work_dir"; }
trap cleanup EXIT

# --app-icon and the deployment target mirror what the bundle's Info.plist
# carries (CFBundleIconName "zz", set by zz-xtask).
xcrun actool "$REPO_ROOT/assets/zz.icon" \
    --compile "$work_dir" \
    --platform macosx \
    --minimum-deployment-target 15.0 \
    --app-icon zz \
    --output-partial-info-plist "$work_dir/partial.plist" \
    >/dev/null

[[ -s "$work_dir/Assets.car" ]] || die "actool produced no Assets.car"
install -m 644 "$work_dir/Assets.car" "$REPO_ROOT/packaging/mac/Assets.car"
echo "packaging/mac/Assets.car updated"
