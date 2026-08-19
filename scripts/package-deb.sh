#!/usr/bin/env bash
# Package the Linux CEF bundle as a Debian/Ubuntu binary package. The installed
# layout is the shared one from stage-linux-usr.sh, so the .deb, the AppImage,
# the release tarball, and the Arch packages all place the same files; what this
# script adds on top is Debian metadata: DEBIAN/control rendered from
# packaging/deb/control, the maintainer scripts, a policy-shaped copyright file,
# and the AppArmor profile Ubuntu 24.04+ needs before Chromium's user-namespace
# sandbox is allowed to unshare (packaging/deb/zz.apparmor explains why).
#
# Dependencies are computed, not curated: dpkg-shlibdeps reads the ELF NEEDED
# entries of the bundle's binaries and resolves them against the build host's
# packages, which is why the resulting .deb targets the distribution release it
# was built on. Only libraries opened with dlopen are invisible to it, and those
# are the hand-written tail of the template's Depends line.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

die() { echo "error: $*" >&2; exit 1; }

if [[ $# -ne 2 ]]; then
    die "usage: package-deb.sh <CEF bundle directory> <output.deb>"
fi

[[ "$(uname -s)" == "Linux" ]] || die "Debian packaging requires Linux"
command -v dpkg-deb >/dev/null 2>&1 || die "dpkg-deb is missing (apt install dpkg-dev)"
command -v dpkg-shlibdeps >/dev/null 2>&1 || die "dpkg-shlibdeps is missing (apt install dpkg-dev)"

bundle_dir="$1"
output="$2"
template_dir="$REPO_ROOT/packaging/deb"

[[ "$output" == *.deb ]] || die "output must end in .deb: $output"
for required in control postinst postrm zz.apparmor; do
    [[ -s "$template_dir/$required" ]] || die "packaging file is missing: packaging/deb/$required"
done

mkdir -p "$(dirname "$output")"
output="$(cd "$(dirname "$output")" && pwd)/$(basename "$output")"

# dpkg orders `~` before everything, so a prerelease sorts below the release it
# leads to; a bare `-` would read as the start of a Debian revision instead.
version="$(sed -nE 's/^version = "([^"]+)"$/\1/p' "$REPO_ROOT/Cargo.toml" | head -1)"
[[ -n "$version" ]] || die "could not read the workspace version from Cargo.toml"
version="${version//-/\~}-1"

case "$(uname -m)" in
    x86_64|amd64) arch="amd64" ;;
    aarch64|arm64) arch="arm64" ;;
    *) die "unsupported Debian architecture: $(uname -m)" ;;
esac

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/zz-deb.XXXXXX")"
cleanup() { rm -rf "$work_dir"; }
trap cleanup EXIT

# The staged tree lives where debhelper would put it, because dpkg-shlibdeps
# resolves the bundle's own $ORIGIN rpath through debian/<package>/ and warns
# about binaries analyzed anywhere else.
pkg_root="$work_dir/debian/zz"
"$REPO_ROOT/scripts/stage-linux-usr.sh" "$bundle_dir" "$pkg_root"

# Cargo builds under the developer's umask, so the executable can arrive
# group-writable; Debian policy wants nothing in a package to be.
find "$pkg_root" \( -type f -o -type d \) -exec chmod go-w {} +

# Debian keeps licenses in /usr/share/doc/<package>/copyright, in a machine-
# readable header followed by the texts themselves, so the shared staging tree's
# Arch-shaped /usr/share/licenses goes away rather than shipping them twice.
install -d "$pkg_root/usr/share/doc/zz"
{
    cat <<'EOF'
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: zz
Source: https://github.com/demfabris/zz

Files: *
Copyright: 2026 Fabricio Dematte
License: MIT OR Apache-2.0

License: MIT
EOF
    sed -e 's/^/ /' -e 's/^ $/ ./' "$REPO_ROOT/LICENSE-MIT"
    echo
    echo "License: Apache-2.0"
    sed -e 's/^/ /' -e 's/^ $/ ./' "$REPO_ROOT/LICENSE-APACHE"
} > "$pkg_root/usr/share/doc/zz/copyright"
chmod 0644 "$pkg_root/usr/share/doc/zz/copyright"
rm -rf "$pkg_root/usr/share/licenses"

install -Dm0644 "$template_dir/zz.apparmor" "$pkg_root/etc/apparmor.d/zz"

install -d "$pkg_root/DEBIAN"
install -m0755 "$template_dir/postinst" "$pkg_root/DEBIAN/postinst"
install -m0755 "$template_dir/postrm" "$pkg_root/DEBIAN/postrm"
echo /etc/apparmor.d/zz > "$pkg_root/DEBIAN/conffiles"

# dpkg-shlibdeps reads a source control file relative to its working directory,
# so it runs from the work directory above the staged tree.
# --ignore-missing-info keeps the bundle's own unpackaged libraries (libcef.so
# and friends) from failing the run.
cat > "$work_dir/debian/control" <<EOF
Source: zz

Package: zz
Architecture: $arch
EOF

mapfile -t binaries < <(
    find "$pkg_root/usr/lib/zz" -maxdepth 1 -type f \
        \( -name 'zz' -o -name 'chrome-sandbox' -o -name '*.so' -o -name '*.so.*' \) \
        | sort
)
[[ ${#binaries[@]} -gt 0 ]] || die "no ELF objects found under usr/lib/zz"

shlibdeps="$(
    cd "$work_dir" && dpkg-shlibdeps -O --ignore-missing-info \
        -l"$pkg_root/usr/lib/zz" "${binaries[@]}"
)"
[[ "$shlibdeps" == shlibs:Depends=?* ]] || die "unexpected dpkg-shlibdeps output: $shlibdeps"
shlibdeps="${shlibdeps#shlibs:Depends=}"

installed_size="$(du -sk --exclude=DEBIAN "$pkg_root" | cut -f1)"

control="$(sed \
    -e "s/@VERSION@/$version/" \
    -e "s/@ARCH@/$arch/" \
    -e "s/@INSTALLED_SIZE@/$installed_size/" \
    -e "s|@SHLIBDEPS@|$shlibdeps|" \
    "$template_dir/control")"
if grep -qE '@[A-Z_]+@' <<<"$control"; then
    die "the control template has unfilled placeholders"
fi
printf '%s\n' "$control" > "$pkg_root/DEBIAN/control"

# The payload is a 1.4 GB Chromium that compresses slowly and barely shrinks;
# level 3 keeps a local `just deb-package` under a minute. Release builds raise
# it through ZZ_DEB_COMPRESSION_LEVEL.
dpkg-deb --build --root-owner-group \
    -Zzstd -z"${ZZ_DEB_COMPRESSION_LEVEL:-3}" --threads-max="$(nproc)" \
    "$pkg_root" "$output"

[[ -s "$output" ]] || die "dpkg-deb did not create the package: $output"

listing="$(dpkg-deb -c "$output" | awk '{ print $6 }')"
for entry in \
    ./usr/bin/zz \
    ./usr/lib/zz/zz \
    ./usr/lib/zz/cli \
    ./usr/lib/zz/libcef.so \
    ./usr/lib/zz/chrome-sandbox \
    ./usr/share/applications/zz.desktop \
    ./usr/share/icons/hicolor/scalable/apps/zz.svg \
    ./usr/share/doc/zz/copyright \
    ./etc/apparmor.d/zz; do
    grep -qx "$entry" <<<"$listing" || die "the package is missing $entry"
done
dpkg-deb -I "$output" > /dev/null

echo "deb ready: $output"
dpkg-deb -f "$output" Package Version Architecture Installed-Size Depends
