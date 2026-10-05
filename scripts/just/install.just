import "settings.just"

set working-directory := "../.."

# Build the release bundle and install it as /Applications/zz.app, quitting a
# running instance first and relaunching after. The daemon (and its sessions)
# survives the swap but runs the previous build until restarted.
mac *args:
    @just build mac {{ args }}
    @scripts/install-macos.sh

# Build and install the native Arch Linux package, syncing missing runtime dependencies.
arch *args:
    @if [[ "$(uname -s)" != "Linux" ]] || ! command -v makepkg >/dev/null 2>&1; then echo "just install arch requires an Arch Linux system with makepkg" >&2; exit 2; fi
    @just build linux -- {{ args }}
    @cd packaging/arch && makepkg --force --cleanbuild --clean --syncdeps --install

# Build the Debian/Ubuntu package and install it, resolving runtime dependencies through apt.
deb *args:
    @if [[ "$(uname -s)" != "Linux" ]] || ! command -v dpkg-deb >/dev/null 2>&1; then echo "just install deb requires a Debian/Ubuntu system with dpkg-dev" >&2; exit 2; fi
    @just package deb {{ args }}
    @sudo apt-get install --yes --reinstall "$PWD/dist/zz-linux.deb"
