import "scripts/just/settings.just"

mod compat "scripts/just/compat.just"
mod install "scripts/just/install.just"
mod ios "scripts/just/ios.just"
mod package "scripts/just/package.just"
mod perf "scripts/just/perf.just"
mod profile "scripts/just/profile.just"
mod release "scripts/just/release.just"
mod storybook "scripts/just/storybook.just"
mod tools "scripts/just/tools.just"
mod vendor "scripts/just/vendor.just"
mod web "scripts/just/web.just"

# Launch a fresh debug instance; append --verbose for continuous diagnostics.
run platform *args:
    @ZZ_ZIG_VERSION="{{ zig_version }}" scripts/run.sh {{ platform }} {{ args }}

# Rebuild and relaunch the development app whenever workspace sources change.
watch platform *args:
    @scripts/run-watch.sh {{ platform }} {{ args }}

hot platform *args:
    @scripts/run-hot.sh {{ platform }} {{ args }}

# Build a release bundle for a supported platform (must run on that platform).
# Extra args after `--` pass through to bundle-cef.
build platform *args:
    @if [[ "{{ platform }}" != "mac" && "{{ platform }}" != "linux" && "{{ platform }}" != "windows" ]]; then echo "unsupported build platform: {{ platform }} (expected: mac|linux|windows)" >&2; exit 2; fi
    @if [[ "{{ platform }}" == "mac" && "$(uname -s)" != "Darwin" ]]; then echo "just build mac requires macOS" >&2; exit 2; fi
    @if [[ "{{ platform }}" == "linux" && "$(uname -s)" != "Linux" ]]; then echo "just build linux requires Linux" >&2; exit 2; fi
    @if [[ "{{ platform }}" == "windows" && "$(uname -s)" != MINGW* && "$(uname -s)" != MSYS* ]]; then echo "just build windows requires Windows" >&2; exit 2; fi
    @if [[ "{{ platform }}" == "mac" || "{{ platform }}" == "windows" ]]; then version="$(zig version)"; if [[ "$version" != "{{ zig_version }}" ]]; then echo "Zig {{ zig_version }} is required, found $version" >&2; exit 2; fi; fi
    @cargo xtask bundle-cef --release --output dist/zz {{ args }}

headless platform *args:
    #!/usr/bin/env bash
    set -euo pipefail
    case "{{ platform }}/$(uname -s)" in
        mac/Darwin) os=macos ;;
        linux/Linux) os=linux ;;
        *) echo "just headless requires mac on macOS or linux on Linux" >&2; exit 2 ;;
    esac
    case "$os/$(uname -m)" in
        linux/x86_64|linux/amd64) arch=x86_64 ;;
        linux/aarch64|linux/arm64) arch=aarch64 ;;
        macos/arm64|macos/aarch64) arch=arm64 ;;
        *) echo "unsupported headless architecture: $(uname -m)" >&2; exit 2 ;;
    esac
    version="$(sed -nE 's/^version = "([^"]+)"$/\1/p' Cargo.toml | head -1)"
    [[ -n "$version" ]] || { echo "workspace version is missing from Cargo.toml" >&2; exit 1; }
    cargo build --release -p zz-cli {{ args }}
    scripts/package-headless.sh target/release/zz_cli "dist/zz-$version-headless-$os-$arch.tar.gz"

# Serve the landing page + docs site with live reload (localhost:4321/zz).
site:
    npm --prefix site install
    npm --prefix site run dev
