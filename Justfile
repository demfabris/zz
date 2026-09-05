set shell := ["bash", "-euo", "pipefail", "-c"]

macos_notary_profile := env_var_or_default("MACOS_NOTARY_PROFILE", "zz-notary")
zig_version := "0.16.0"

# Show carried-patch fork status: our commits, upstream drift, Cargo.lock sync.
forks:
    @scripts/fork-sync.sh status

# Rebase a fork's patch branch onto upstream (default: upstream branch tip).
fork-rebase name target="":
    @scripts/fork-sync.sh rebase {{ name }} {{ target }}

compat *args:
    @compat/run.sh {{ args }}

compat-check:
    @compat/check.sh

# Build a release bundle for a supported platform (must run on that platform).
# Extra args after `--` pass through to bundle-cef.
build platform *args:
    @if [[ "{{ platform }}" != "mac" && "{{ platform }}" != "linux" && "{{ platform }}" != "windows" ]]; then echo "unsupported build platform: {{ platform }} (expected: mac|linux|windows)" >&2; exit 2; fi
    @if [[ "{{ platform }}" == "mac" && "$(uname -s)" != "Darwin" ]]; then echo "just build mac requires macOS" >&2; exit 2; fi
    @if [[ "{{ platform }}" == "linux" && "$(uname -s)" != "Linux" ]]; then echo "just build linux requires Linux" >&2; exit 2; fi
    @if [[ "{{ platform }}" == "windows" && "$(uname -s)" != MINGW* && "$(uname -s)" != MSYS* ]]; then echo "just build windows requires Windows" >&2; exit 2; fi
    @if [[ "{{ platform }}" == "mac" || "{{ platform }}" == "windows" ]]; then version="$(zig version)"; if [[ "$version" != "{{ zig_version }}" ]]; then echo "Zig {{ zig_version }} is required, found $version" >&2; exit 2; fi; fi
    @cargo xtask bundle-cef --release --output dist/zz {{ args }}

# Build the release bundle and install it as /Applications/zz.app, quitting a
# running instance first and relaunching after. The daemon (and its sessions)
# survives the swap but runs the previous build until restarted.
install platform *args:
    @if [[ "{{ platform }}" != "mac" ]]; then echo "just install currently supports mac only (Arch Linux: just pacman-install; Debian/Ubuntu: just deb-install)" >&2; exit 2; fi
    @just build mac {{ args }}
    @scripts/install-macos.sh

# ctrl-c detaches the console and the app keeps running; `ZZ_SOCKET`
# overrides the daemon socket the app dials.
ios:
    @if [[ "$(uname -s)" != "Darwin" ]]; then echo "just ios requires macOS" >&2; exit 2; fi
    @scripts/ios-sim.sh

ios-build:
    @if [[ "$(uname -s)" != "Darwin" ]]; then echo "just ios-build requires macOS" >&2; exit 2; fi
    @scripts/ios-sim.sh --build-only

ios-test:
    @if [[ "$(uname -s)" != "Darwin" ]]; then echo "just ios-test requires macOS" >&2; exit 2; fi
    @scripts/ios-sim.sh --test

ipad:
    @if [[ "$(uname -s)" != "Darwin" ]]; then echo "just ipad requires macOS" >&2; exit 2; fi
    @ZZ_IOS_SIMULATOR_FAMILY=iPad scripts/ios-sim.sh

ipad-build:
    @if [[ "$(uname -s)" != "Darwin" ]]; then echo "just ipad-build requires macOS" >&2; exit 2; fi
    @ZZ_IOS_SIMULATOR_FAMILY=iPad scripts/ios-sim.sh --build-only

ipad-test:
    @if [[ "$(uname -s)" != "Darwin" ]]; then echo "just ipad-test requires macOS" >&2; exit 2; fi
    @ZZ_IOS_SIMULATOR_FAMILY=iPad scripts/ios-sim.sh --test

ios-device device="iphone":
    @if [[ "$(uname -s)" != "Darwin" ]]; then echo "just ios-device requires macOS" >&2; exit 2; fi
    @scripts/ios-device.sh {{ device }}

ios-preview build="":
    @if [[ "$(uname -s)" != "Darwin" ]]; then echo "just ios-preview requires macOS" >&2; exit 2; fi
    @scripts/ios-testflight.sh "{{ build }}"

# Build a release-optimized macOS bundle with matching source-level dSYMs.
profile-build platform:
    @if [[ "{{ platform }}" != "mac" ]]; then echo "profiling bundles currently support macOS only" >&2; exit 2; fi
    @if [[ "$(uname -s)" != "Darwin" ]]; then echo "just profile-build mac requires macOS" >&2; exit 2; fi
    @version="$(zig version)"; if [[ "$version" != "{{ zig_version }}" ]]; then echo "Zig {{ zig_version }} is required, found $version" >&2; exit 2; fi; cargo xtask bundle-cef --profile profiling --output dist/zz-profile

# Profile GUI, daemon, or all-process CPU work in a fresh isolated macOS run.
profile-cpu platform target="gui" duration="20s":
    @if [[ "{{ platform }}" != "mac" ]]; then echo "CPU capture currently supports macOS only" >&2; exit 2; fi
    @scripts/profile-macos.sh cpu "{{ target }}" "{{ duration }}"

# Export and summarize CPU used by the isolated zz GUI, daemon, and CEF helpers.
profile-cpu-summary run:
    @python3 scripts/summarize-macos-cpu.py "{{ run }}"

profile-memory platform duration="60s":
    @if [[ "{{ platform }}" != "mac" ]]; then echo "Memory capture currently supports macOS only" >&2; exit 2; fi
    @scripts/profile-macos.sh memory all "{{ duration }}"

# Profile scheduling, waits, wakeups, and IPC in a fresh isolated macOS run.
profile-system platform duration="20s":
    @if [[ "{{ platform }}" != "mac" ]]; then echo "System Trace capture currently supports macOS only" >&2; exit 2; fi
    @scripts/profile-macos.sh system all "{{ duration }}"

# Profile CEF and GPUI Metal work in a fresh isolated macOS run.
profile-metal platform duration="20s":
    @if [[ "{{ platform }}" != "mac" ]]; then echo "Metal capture currently supports macOS only" >&2; exit 2; fi
    @scripts/profile-macos.sh metal gui "{{ duration }}"

# Export and summarize zz-owned work from one captured Metal run.
profile-metal-summary run:
    @python3 scripts/summarize-macos-metal.py "{{ run }}"

# Collect terminal row-cache diagnostics without treating logged timings as a clean CPU benchmark.
profile-terminal-diagnostics platform duration="20s":
    @if [[ "{{ platform }}" != "mac" ]]; then echo "Terminal diagnostic capture currently supports macOS only" >&2; exit 2; fi
    @scripts/profile-macos.sh diagnostics gui "{{ duration }}"

# Summarize terminal row-cache hits, misses, and prepaint timing from one diagnostic run.
profile-terminal-summary run:
    @python3 scripts/summarize-terminal-render.py "{{ run }}"

# Build and validate the macOS release bundle, then emit dist/zz-macos.dmg.
dmg:
    @if [[ "$(uname -s)" != "Darwin" ]]; then echo "just dmg requires macOS" >&2; exit 2; fi
    @just build mac
    @scripts/package-dmg.sh dist/zz/zz.app dist/zz-macos.dmg

# Build and validate the Windows release bundle, then emit dist/zz-windows.zip.
zip-windows:
    @if [[ "$(uname -s)" != MINGW* && "$(uname -s)" != MSYS* ]]; then echo "just zip-windows requires Windows" >&2; exit 2; fi
    @just build windows
    @powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/package-windows.ps1 dist/zz dist/zz-windows.zip

# Build and validate the native Arch Linux package in packaging/arch.
pacman-package *args:
    @if [[ "$(uname -s)" != "Linux" ]] || ! command -v makepkg >/dev/null 2>&1; then echo "just pacman-package requires an Arch Linux system with makepkg" >&2; exit 2; fi
    @just build linux -- {{ args }}
    @cd packaging/arch && makepkg --force --cleanbuild --clean

# Build and install the native Arch Linux package, syncing missing runtime dependencies.
pacman-install *args:
    @if [[ "$(uname -s)" != "Linux" ]] || ! command -v makepkg >/dev/null 2>&1; then echo "just pacman-install requires an Arch Linux system with makepkg" >&2; exit 2; fi
    @just build linux -- {{ args }}
    @cd packaging/arch && makepkg --force --cleanbuild --clean --syncdeps --install

# Build and validate the Debian/Ubuntu package, then emit dist/zz-linux.deb.
deb-package *args:
    @if [[ "$(uname -s)" != "Linux" ]] || ! command -v dpkg-deb >/dev/null 2>&1; then echo "just deb-package requires a Debian/Ubuntu system with dpkg-dev" >&2; exit 2; fi
    @just build linux -- {{ args }}
    @scripts/package-deb.sh dist/zz dist/zz-linux.deb

# Build the Debian/Ubuntu package and install it, resolving runtime dependencies through apt.
deb-install *args:
    @if [[ "$(uname -s)" != "Linux" ]] || ! command -v dpkg-deb >/dev/null 2>&1; then echo "just deb-install requires a Debian/Ubuntu system with dpkg-dev" >&2; exit 2; fi
    @just deb-package {{ args }}
    @sudo apt-get install --yes --reinstall "$PWD/dist/zz-linux.deb"

# Store Apple notarization credentials in Keychain (one-time interactive setup).
notary-setup-mac profile=macos_notary_profile:
    @if [[ "$(uname -s)" != "Darwin" ]]; then echo "just notary-setup-mac requires macOS" >&2; exit 2; fi
    @xcrun notarytool store-credentials "{{ profile }}"

# Check the Developer ID identity and saved notary profile without uploading anything.
release-mac-check profile=macos_notary_profile:
    @MACOS_NOTARY_PROFILE="{{ profile }}" scripts/release-macos.sh preflight

# Replace an existing bundle's ad-hoc signature with a Developer ID signature.
sign-mac app="dist/zz/zz.app":
    @scripts/release-macos.sh sign-app "{{ app }}"

# Sign, submit, staple, and verify an existing Developer ID-signed DMG.
notarize-mac dmg profile=macos_notary_profile:
    @MACOS_NOTARY_PROFILE="{{ profile }}" scripts/release-macos.sh notarize-dmg "{{ dmg }}"

# Recheck an already-notarized DMG locally without uploading it again.
verify-notarized-mac dmg:
    @scripts/release-macos.sh verify-dmg "{{ dmg }}"

# Build, Developer ID-sign, package, notarize, staple, and verify a versioned DMG.
release-mac version profile=macos_notary_profile:
    @MACOS_NOTARY_PROFILE="{{ profile }}" scripts/release-macos.sh release "{{ version }}"

# Install the pinned dry-run-first release driver.
release-setup:
    cargo install cargo-release --version 1.1.5 --locked

# Preview a SemVer bump, release commit, tag, and push; --execute applies and publishes it (the pushed tag starts release CI).
release target *flags:
    @scripts/release.sh "{{ target }}" {{ flags }}

# Launch a fresh debug instance; append --verbose for continuous diagnostics.
run platform *args:
    @ZZ_ZIG_VERSION="{{ zig_version }}" scripts/run.sh {{ platform }} {{ args }}

# Rebuild and relaunch the development app whenever workspace sources change.
watch platform *args:
    @scripts/run-watch.sh {{ platform }} {{ args }}

# Serve the landing page + docs site with live reload (localhost:4321/zz).
site:
    npm --prefix site install
    npm --prefix site run dev

# Run the zz UI showcase with Cargo watch and Vite live reload.
showcase:
    @scripts/showcase-dev.sh

showcase-capture path:
    ZZ_PREVIEW_CAPTURE="{{path}}" cargo run --locked --features native-capture --manifest-path examples/ui-showcase/Cargo.toml --target-dir target/ui-showcase-native

showcase-native:
    cargo run --locked --manifest-path examples/ui-showcase/Cargo.toml --target-dir target/ui-showcase-native

# Install the toolchain used by the browser showcase.
showcase-setup:
    rustup toolchain install nightly --profile minimal --component rustfmt --component clippy
    rustup target add --toolchain nightly wasm32-unknown-unknown
    command -v cargo-watch >/dev/null 2>&1 || cargo install cargo-watch --locked
    cargo install wasm-bindgen-cli --version 0.2.126 --locked
    npm --prefix examples/ui-showcase/web install
    python3 scripts/prepare-showcase-fonts.py

# Build browser-ready debug assets into examples/ui-showcase/web/src/wasm.
showcase-build:
    @scripts/build-showcase-wasm.sh

# Build optimized browser assets into examples/ui-showcase/web/src/wasm.
showcase-build-release:
    @scripts/build-showcase-wasm.sh --release
