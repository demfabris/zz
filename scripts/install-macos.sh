#!/usr/bin/env bash
set -euo pipefail

die() { echo "error: $*" >&2; exit 1; }

[[ "$(uname -s)" == "Darwin" ]] || die "installing the macOS bundle requires macOS"

bundle="${1:-dist/zz/zz.app}"
target="/Applications/zz.app"
binary="$target/Contents/MacOS/zz"

[[ -d "$bundle/Contents" ]] || die "application bundle does not exist: $bundle (run: just build mac)"

# The GUI instance of the installed bundle, and only that: the daemon runs the
# same binary (`… --socket … daemon`) but must survive the swap. It keeps the
# sessions and, unlike the GUI, never loads CEF helpers by bundle path, so it
# is safe with its binary unlinked. dist/zz-dev instances don't match either.
gui_pids() {
    pgrep -f "^$binary" 2>/dev/null | while read -r pid; do
        case "$(ps -o command= -p "$pid" 2>/dev/null)" in
            *" daemon") ;;
            *) echo "$pid" ;;
        esac
    done
}

# Quit a running GUI before replacing its bundle: CEF spawns helper processes
# by bundle path at runtime, so deleting the bundle under a live GUI is how
# you get the next mystery crash report.
if [[ -n "$(gui_pids)" ]]; then
    echo "quitting running zz..."
    osascript -e 'tell application id "dev.zz.app" to quit' >/dev/null 2>&1 || true
    for _ in $(seq 1 30); do
        [[ -n "$(gui_pids)" ]] || break
        sleep 0.5
    done
    [[ -n "$(gui_pids)" ]] && die "zz did not quit; close it and rerun"
fi

# Symlink the bundle's `cli` launcher, never the real binary: macOS resolves
# the app bundle from the launch path without following symlinks, so a
# symlinked `zz` would run with no Info.plist and no CEF framework beside it.
install_cli_link() {
    local cli="$target/Contents/MacOS/cli" dir link existing
    for dir in /opt/homebrew/bin /usr/local/bin; do
        [[ -d "$dir" && -w "$dir" && ":$PATH:" == *":$dir:"* ]] || continue
        link="$dir/zz"
        existing="$(readlink "$link" 2>/dev/null || true)"
        if [[ -e "$link" || -L "$link" ]]; then
            [[ "$existing" == "$cli" ]] && return 0
            if [[ "$existing" != *"/zz.app/Contents/MacOS/"* ]]; then
                echo "note: $link already exists and is not zz's cli launcher; leaving it alone"
                return 0
            fi
        fi
        ln -sf "$cli" "$link"
        echo "linked $link -> $cli"
        return 0
    done
    echo "note: found no writable PATH directory for the zz CLI; link it yourself:"
    echo "  ln -s \"$cli\" /usr/local/bin/zz"
}

rm -rf "$target"
ditto "$bundle" "$target"
install_cli_link
echo "installed $bundle -> $target (daemon keeps running the previous build until restarted)"
open "$target"
