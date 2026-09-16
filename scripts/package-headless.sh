#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

die() { echo "error: $*" >&2; exit 1; }

if [[ $# -ne 2 ]]; then
    die "usage: package-headless.sh <zz_cli-binary> <zz-VERSION-headless-OS-ARCH.tar.gz> (VERSION, OS and ARCH come from the filename; OS is linux or macos)"
fi

binary="$1"
output="$2"

[[ -x "$binary" ]] || die "binary is not executable: $binary"
version_output="$("$binary" --version)" || die "binary --version failed: $binary"
grep -q '^zz ' <<<"$version_output" || die "binary --version must print a line starting with 'zz ': $binary"

filename="$(basename "$output")"
[[ "$filename" =~ ^zz-(.+)-headless-(linux|macos)-(x86_64|aarch64|arm64)\.tar\.gz$ ]] \
    || die "output must be zz-<version>-headless-<linux|macos>-<arch>.tar.gz: $output"
version="${BASH_REMATCH[1]}"
os="${BASH_REMATCH[2]}"
arch="${BASH_REMATCH[3]}"
case "$os/$arch" in
    linux/x86_64|linux/aarch64|macos/arm64) ;;
    *) die "unsupported headless platform: $os/$arch" ;;
esac
name="zz-$version-headless-$os-$arch"

mkdir -p "$(dirname "$output")"
output="$(cd "$(dirname "$output")" && pwd)/$(basename "$output")"

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/zz-headless.XXXXXX")"
cleanup() { rm -rf "$work_dir"; }
trap cleanup EXIT

mkdir -p "$work_dir/$name"
install -m 755 "$binary" "$work_dir/$name/zz"
install -m 644 "$REPO_ROOT/LICENSE-MIT" "$REPO_ROOT/LICENSE-APACHE" "$work_dir/$name/"
tar -C "$work_dir" -czf "$output" "$name"
[[ -s "$output" ]] || die "tar did not create the archive: $output"

echo "$output"
