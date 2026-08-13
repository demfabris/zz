#!/usr/bin/env bash
# Package the Linux CEF bundle as a release tarball. The archive carries a
# single root-owned <name>/usr/ tree (name = the archive filename without
# .tar.gz), so `tar -x --strip-components=2 -C /usr/local` installs it by hand
# and the AUR zz-bin package copies <name>/usr verbatim into the package root.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

die() { echo "error: $*" >&2; exit 1; }

if [[ $# -ne 2 ]]; then
    die "usage: package-linux-tarball.sh <CEF bundle directory> <output.tar.gz>"
fi

bundle_dir="$1"
output="$2"

[[ "$output" == *.tar.gz ]] || die "output must end in .tar.gz: $output"

mkdir -p "$(dirname "$output")"
output="$(cd "$(dirname "$output")" && pwd)/$(basename "$output")"
name="$(basename "$output" .tar.gz)"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/zz-tarball.XXXXXX")"
cleanup() { rm -rf "$work_dir"; }
trap cleanup EXIT

"$REPO_ROOT/scripts/stage-linux-usr.sh" "$bundle_dir" "$work_dir/$name"
tar -C "$work_dir" --owner=0 --group=0 --numeric-owner -czf "$output" "$name"
[[ -s "$output" ]] || die "tar did not create the archive: $output"

listing="$(tar -tzf "$output")"
for entry in \
    "$name/usr/bin/zz" \
    "$name/usr/lib/zz/zz" \
    "$name/usr/lib/zz/libcef.so" \
    "$name/usr/share/applications/zz.desktop" \
    "$name/usr/share/licenses/zz/LICENSE-MIT"; do
    grep -qx "$entry" <<<"$listing" || die "the archive is missing $entry"
done

echo "tarball ready: $output"
