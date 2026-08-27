#!/bin/sh
# Install zz from the latest GitHub release.
#
#   curl -fsSL https://zzmux.sh/install.sh | sh
#   curl -fsSL https://zzmux.sh/install.sh | sh -s -- --beta
#
# macOS (Apple Silicon): the notarized disk image, copied to /Applications and
# linked onto your PATH. Linux with dpkg and apt: the .deb, which also carries
# the AppArmor profile Ubuntu 24.04+ needs for browser panes. Any other Linux:
# the release tarball unpacked under ~/.local, no root required. Rerun the same
# command to upgrade.
set -eu

repo=demfabris/zz
channel=stable
version=
prefix=
tmp=
mount=

say() { printf '%s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }
has() { command -v "$1" >/dev/null 2>&1; }

usage() {
    cat <<'EOF'
usage: install.sh [--beta] [--version <version>] [--prefix <dir>]

  --beta               newest release including betas (default: newest stable)
  --version <version>  an exact release, for example 0.3.0 or 0.3.0-beta.2
  --prefix <dir>       Linux: unpack the tarball here instead of using apt (default: ~/.local)
EOF
}

parse_arguments() {
    while [ $# -gt 0 ]; do
        case "$1" in
            --beta) channel=beta ;;
            --version) [ $# -ge 2 ] || die "--version needs a value"; version="$2"; shift ;;
            --version=*) version="${1#*=}" ;;
            --prefix) [ $# -ge 2 ] || die "--prefix needs a value"; prefix="$2"; shift ;;
            --prefix=*) prefix="${1#*=}" ;;
            -h|--help) usage; exit 0 ;;
            *) usage >&2; die "unknown option: $1" ;;
        esac
        shift
    done
    if [ -n "$version" ]; then
        version="${version#v}"
        case "$version" in
            *[!0-9A-Za-z.-]*|"") die "not a release version: $version" ;;
        esac
    fi
}

cleanup() {
    if [ -n "$mount" ]; then
        hdiutil detach -quiet "$mount" >/dev/null 2>&1 || true
    fi
    if [ -n "$tmp" ]; then
        rm -rf "$tmp"
    fi
}

resolve_version() {
    [ -z "$version" ] || return 0
    releases="$(curl -fsSL -H 'Accept: application/vnd.github+json' \
        "https://api.github.com/repos/$repo/releases?per_page=30")" \
        || die "could not list the releases of $repo"
    tags="$(printf '%s\n' "$releases" | grep -o '"tag_name": *"v[^"]*"' | sed 's/.*"v//; s/"$//')"
    case "$channel" in
        stable) version="$(printf '%s\n' "$tags" | grep -v -- '-' | head -n 1)" ;;
        beta) version="$(printf '%s\n' "$tags" | head -n 1)" ;;
    esac
    [ -n "$version" ] || die "no $channel release found for $repo"
}

fetch() {
    if [ -t 2 ]; then
        curl -fL --progress-bar -o "$2" "$1"
    else
        curl -fsSL -o "$2" "$1"
    fi
}

download() {
    base="https://github.com/$repo/releases/download/v$version"
    say "downloading $1"
    fetch "$base/$1" "$tmp/$1" || die "no such release asset: $base/$1"
    fetch "$base/$1.sha256" "$tmp/$1.sha256" || die "the release carries no checksum for $1"
    if has sha256sum; then
        (cd "$tmp" && sha256sum -c "$1.sha256" >/dev/null) || die "checksum mismatch for $1"
    else
        (cd "$tmp" && shasum -a 256 -c "$1.sha256" >/dev/null) || die "checksum mismatch for $1"
    fi
}

gui_pids() {
    pgrep -f "^$1" 2>/dev/null | while read -r pid; do
        case "$(ps -o command= -p "$pid" 2>/dev/null)" in
            *" daemon") ;;
            *) echo "$pid" ;;
        esac
    done
}

on_path() {
    case ":$PATH:" in
        *":$1:"*) return 0 ;;
        *) return 1 ;;
    esac
}

link_cli() {
    cli="$1"
    for dir in /opt/homebrew/bin /usr/local/bin "$HOME/.local/bin"; do
        if [ "$dir" = "$HOME/.local/bin" ]; then
            mkdir -p "$dir"
        fi
        [ -d "$dir" ] && [ -w "$dir" ] || continue
        link="$dir/zz"
        if [ -e "$link" ] || [ -L "$link" ]; then
            existing="$(readlink "$link" 2>/dev/null || true)"
            [ "$existing" = "$cli" ] && return 0
            case "$existing" in
                */zz.app/Contents/MacOS/*) ;;
                *) say "note: $link already exists and is not zz's launcher; leaving it alone"; return 0 ;;
            esac
        fi
        ln -sf "$cli" "$link"
        say "linked $link -> $cli"
        on_path "$dir" || warn "$dir is not on your PATH; add it to run zz from a shell"
        return 0
    done
    warn "found no writable directory for the zz CLI; link it yourself: ln -s \"$cli\" /usr/local/bin/zz"
}

install_macos() {
    case "$(uname -m)" in
        arm64) ;;
        *) die "zz ships for Apple Silicon only; Intel Macs build from source" ;;
    esac
    for caskroom in /opt/homebrew/Caskroom /usr/local/Caskroom; do
        for cask in zz zz@beta; do
            [ -d "$caskroom/$cask" ] && die "Homebrew manages this install; run: brew upgrade --cask $cask"
        done
    done
    has hdiutil || die "hdiutil is missing"

    asset="zz-$version-macos-arm64.dmg"
    download "$asset"

    mount="$tmp/mnt"
    mkdir "$mount"
    hdiutil attach -quiet -nobrowse -readonly -mountpoint "$mount" "$tmp/$asset" \
        || die "could not mount $asset"
    [ -d "$mount/zz.app/Contents" ] || die "the disk image carries no zz.app"

    if [ -w /Applications ]; then
        target=/Applications/zz.app
    else
        mkdir -p "$HOME/Applications"
        target="$HOME/Applications/zz.app"
    fi

    if [ -n "$(gui_pids "$target/Contents/MacOS/zz")" ]; then
        say "quitting the running zz (the daemon keeps your sessions)"
        osascript -e 'tell application id "dev.zz.app" to quit' >/dev/null 2>&1 || true
        i=0
        while [ -n "$(gui_pids "$target/Contents/MacOS/zz")" ]; do
            i=$((i + 1))
            [ $i -le 30 ] || die "zz did not quit; close it and rerun"
            sleep 0.5
        done
    fi

    rm -rf "$target"
    ditto "$mount/zz.app" "$target"
    hdiutil detach -quiet "$mount"
    mount=
    link_cli "$target/Contents/MacOS/cli"
    say "installed zz $version -> $target"
    say "open it from Launchpad or run: zz"
}

as_root() {
    if [ "$(id -u)" -eq 0 ]; then
        "$@"
    else
        sudo "$@"
    fi
}

install_linux_deb() {
    asset="zz-$version-linux-$arch.deb"
    download "$asset"
    chmod 755 "$tmp"
    say "installing with apt (this needs root)"
    as_root env DEBIAN_FRONTEND=noninteractive apt-get install -y "$tmp/$asset" </dev/null
    say "installed zz $version with apt; remove it with: sudo apt remove zz"
    say "open it from your launcher or run: zz"
}

install_linux_tarball() {
    prefix="${prefix:-$HOME/.local}"
    asset="zz-$version-linux-$arch.tar.gz"
    download "$asset"

    if [ -n "$(gui_pids "$prefix/lib/zz/zz")" ]; then
        warn "zz is running from $prefix; restart it after the install"
    fi

    mkdir -p "$prefix"
    rm -rf "$prefix/lib/zz"
    tar -xzf "$tmp/$asset" --strip-components=2 -C "$prefix"
    [ -x "$prefix/lib/zz/zz" ] || die "the tarball did not unpack as expected under $prefix"
    sed -i "s|^Exec=zz app\$|Exec=$prefix/bin/zz app|" "$prefix/share/applications/zz.desktop"
    if has update-desktop-database; then
        update-desktop-database -q "$prefix/share/applications" 2>/dev/null || true
    fi

    say "installed zz $version -> $prefix (bin/zz, lib/zz, share/applications/zz.desktop)"
    if [ "$(cat /proc/sys/kernel/apparmor_restrict_unprivileged_userns 2>/dev/null)" = 1 ]; then
        warn "this kernel restricts unprivileged user namespaces; browser panes need an AppArmor profile for $prefix/lib/zz/zz (see https://zzmux.sh/docs/getting-started#linux)"
    fi
    if [ -x /usr/bin/zz ]; then
        warn "/usr/bin/zz is also installed; $prefix/bin/zz shadows it when $prefix/bin comes first on PATH"
    fi
    on_path "$prefix/bin" || warn "$prefix/bin is not on your PATH; add it to run zz from a shell"
    say "open it from your launcher or run: zz"
}

install_linux() {
    case "$(uname -m)" in
        x86_64|amd64) arch=x86_64 ;;
        aarch64|arm64) arch=aarch64 ;;
        *) die "zz ships Linux builds for x86_64 and aarch64 only" ;;
    esac
    if [ -z "$prefix" ] && has dpkg && has apt-get && { [ "$(id -u)" -eq 0 ] || has sudo; }; then
        install_linux_deb
    else
        install_linux_tarball
    fi
}

main() {
    parse_arguments "$@"
    has curl || die "curl is missing"
    case "$(uname -s)" in
        Darwin|Linux) ;;
        *) die "this installer covers macOS and Linux; Windows builds are at https://github.com/$repo/releases" ;;
    esac

    resolve_version
    say "zz $version"
    tmp="$(mktemp -d "${TMPDIR:-/tmp}/zz-install.XXXXXX")"
    trap cleanup EXIT

    case "$(uname -s)" in
        Darwin) install_macos ;;
        Linux) install_linux ;;
    esac
}

main "$@"
