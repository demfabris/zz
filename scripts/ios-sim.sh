#!/usr/bin/env bash
set -euo pipefail

die() { echo "error: $*" >&2; exit 1; }

[[ "$(uname -s)" == "Darwin" ]] || die "the iOS simulator requires macOS"
command -v xcrun >/dev/null 2>&1 || die "xcrun not found (install Xcode)"

bundle_id="dev.zz.ios"

# Boot an iPad if nothing is booted; the Simulator app must be open either
# way, or a booted device has no window to show.
if ! xcrun simctl list devices booted | grep -q "(Booted)"; then
    # Device names carry their own parentheses ("iPad Pro 11-inch (M5)"), so
    # take the UDID by shape, not by position.
    udid="$(xcrun simctl list devices available \
        | grep iPad \
        | grep -Eo '[0-9A-F]{8}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{12}' \
        | head -1)"
    [[ -n "$udid" ]] || die "no available iPad simulator (create one in Xcode > Devices and Simulators)"
    echo "booting iPad simulator $udid..."
    xcrun simctl boot "$udid"
fi
open -a Simulator

# The host daemon's socket, derived the way zz derives it, so the sim app
# attaches to the same daemon the desktop does.
socket="${ZZ_SOCKET:-}"
if [[ -z "$socket" ]]; then
    if [[ -n "${XDG_RUNTIME_DIR:-}" ]]; then
        socket="$XDG_RUNTIME_DIR/zz/default.sock"
    else
        socket="${TMPDIR:-/tmp}/zz-${USER}/default.sock"
    fi
fi
[[ -S "$socket" ]] || echo "warning: no daemon socket at $socket — start zz (or \`zz daemon\`) to give the app something to attach to" >&2

cargo xtask ios-sim

xcrun simctl terminate booted "$bundle_id" 2>/dev/null || true
xcrun simctl install booted target/ios-app/ZZ.app

# The real config, so fleet hosts and settings match the desktop; ssh state
# stays in the app container unless ZZ_IOS_SSH_DIR overrides it. Remote hosts
# authenticate with the app's own generated key, so they read as unreachable
# until its .pub is installed on them.
echo "launching $bundle_id (socket: $socket) — ctrl-c detaches the console, the app keeps running"
exec env \
    SIMCTL_CHILD_ZZ_SOCKET="$socket" \
    SIMCTL_CHILD_XDG_CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}" \
    xcrun simctl launch --console-pty booted "$bundle_id"
