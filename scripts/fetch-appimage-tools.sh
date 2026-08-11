#!/usr/bin/env bash
# Download the pinned appimagetool and type-2 runtime for this machine's
# architecture and verify their checksums. Inside GitHub Actions ($GITHUB_ENV
# set) the APPIMAGETOOL/APPIMAGE_RUNTIME paths land in the job environment;
# elsewhere they print as export lines to eval.
set -euo pipefail

APPIMAGETOOL_VERSION=1.9.1
APPIMAGE_RUNTIME_VERSION=20251108
APPIMAGETOOL_X86_64_SHA256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
APPIMAGETOOL_AARCH64_SHA256=f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158
APPIMAGE_RUNTIME_X86_64_SHA256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d
APPIMAGE_RUNTIME_AARCH64_SHA256=00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444

die() { echo "error: $*" >&2; exit 1; }

case "$(uname -m)" in
    x86_64 | amd64)
        arch="x86_64"
        tool_checksum="$APPIMAGETOOL_X86_64_SHA256"
        runtime_checksum="$APPIMAGE_RUNTIME_X86_64_SHA256"
        ;;
    aarch64 | arm64)
        arch="aarch64"
        tool_checksum="$APPIMAGETOOL_AARCH64_SHA256"
        runtime_checksum="$APPIMAGE_RUNTIME_AARCH64_SHA256"
        ;;
    *)
        die "unsupported AppImage architecture: $(uname -m)"
        ;;
esac

dest="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
tool="$dest/appimagetool-$arch.AppImage"
runtime="$dest/runtime-$arch"

curl --fail --location --retry 3 \
    "https://github.com/AppImage/appimagetool/releases/download/$APPIMAGETOOL_VERSION/appimagetool-$arch.AppImage" \
    --output "$tool"
curl --fail --location --retry 3 \
    "https://github.com/AppImage/type2-runtime/releases/download/$APPIMAGE_RUNTIME_VERSION/runtime-$arch" \
    --output "$runtime"
echo "$tool_checksum  $tool" | sha256sum --check
echo "$runtime_checksum  $runtime" | sha256sum --check
chmod +x "$tool"

if [[ -n "${GITHUB_ENV:-}" ]]; then
    {
        echo "APPIMAGETOOL=$tool"
        echo "APPIMAGE_RUNTIME=$runtime"
    } >>"$GITHUB_ENV"
else
    printf 'export APPIMAGETOOL=%q\nexport APPIMAGE_RUNTIME=%q\n' "$tool" "$runtime"
fi
