#!/usr/bin/env bash
# Fetches and builds the pinned tmux behavioral reference, caching the binary
# under compat/.cache. ZZ_COMPAT_TMUX may provide an already-built binary.
set -euo pipefail

COMPAT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
CACHE_DIR="$COMPAT_DIR/.cache"
SOURCE_DIR="$CACHE_DIR/tmux-src"
TMUX_BIN="$SOURCE_DIR/tmux"
BUILD_STAMP="$CACHE_DIR/tmux-build.stamp"
LEGACY_BUILD_STAMP="$SOURCE_DIR/.zz-build-stamp"
TMUX_COMMIT="d77c9dc6aa021e4bc61f0da128c591af695e6466"
TMUX_VERSION="tmux next-3.8"

log() { printf '\033[1;36m==>\033[0m %s\n' "$*" >&2; }
die() {
  printf '\033[1;31merror:\033[0m %s\n' "$*" >&2
  exit 1
}

verify_tmux() {
  local binary="$1"
  local version

  [ -x "$binary" ] || return 1
  version="$("$binary" -V 2>/dev/null || true)"
  [ "$version" = "$TMUX_VERSION" ]
}

build_stamp() {
  local binary_checksum script_checksum

  script_checksum="$(cksum <"${BASH_SOURCE[0]}")" || return 1
  binary_checksum="$(cksum <"$TMUX_BIN")" || return 1
  printf 'commit=%s\nversion=%s\nscript-cksum=%s\nbinary-cksum=%s\n' \
    "$TMUX_COMMIT" "$TMUX_VERSION" "$script_checksum" "$binary_checksum"
}

verify_cached_tmux() {
  local actual_commit actual_stamp dirty expected_stamp

  verify_tmux "$TMUX_BIN" || return 1
  [ -d "$SOURCE_DIR/.git" ] || return 1
  actual_commit="$(git -C "$SOURCE_DIR" rev-parse HEAD 2>/dev/null || true)"
  [ "$actual_commit" = "$TMUX_COMMIT" ] || return 1
  dirty="$(git -C "$SOURCE_DIR" status --porcelain --untracked-files=all 2>/dev/null)" || return 1
  [ -z "$dirty" ] || return 1
  [ -f "$BUILD_STAMP" ] || return 1
  actual_stamp="$(cat "$BUILD_STAMP")" || return 1
  expected_stamp="$(build_stamp)" || return 1
  [ "$actual_stamp" = "$expected_stamp" ]
}

if [ -f "$LEGACY_BUILD_STAMP" ]; then
  mv "$LEGACY_BUILD_STAMP" "$BUILD_STAMP"
fi

if [ -n "${ZZ_COMPAT_TMUX:-}" ]; then
  if [[ "$ZZ_COMPAT_TMUX" == */* ]]; then
    override="$ZZ_COMPAT_TMUX"
  else
    override="$(command -v "$ZZ_COMPAT_TMUX" || true)"
  fi
  [ -n "$override" ] || die "ZZ_COMPAT_TMUX does not resolve to an executable: $ZZ_COMPAT_TMUX"
  [ -x "$override" ] || die "ZZ_COMPAT_TMUX is not executable: $override"
  verify_tmux "$override" ||
    die "ZZ_COMPAT_TMUX must report '$TMUX_VERSION' from -V"
  printf '%s\n' "$override"
  exit 0
fi

if verify_cached_tmux; then
  printf '%s\n' "$TMUX_BIN"
  exit 0
fi

missing=()
for command in git make autoconf automake pkg-config; do
  command -v "$command" >/dev/null 2>&1 || missing+=("$command")
done

if command -v pkg-config >/dev/null 2>&1; then
  pkg-config --exists libevent || missing+=("libevent (pkg-config)")
  if ! pkg-config --exists ncurses && ! pkg-config --exists ncursesw; then
    missing+=("ncurses (pkg-config)")
  fi
fi

if ! command -v bison >/dev/null 2>&1 && ! command -v yacc >/dev/null 2>&1; then
  missing+=("bison or yacc")
fi

if [ "${#missing[@]}" -gt 0 ]; then
  printf '\033[1;31merror:\033[0m missing dependencies required to build tmux:\n' >&2
  printf '  - %s\n' "${missing[@]}" >&2
  exit 1
fi

mkdir -p "$CACHE_DIR"
if [ ! -d "$SOURCE_DIR/.git" ]; then
  [ ! -e "$SOURCE_DIR" ] || die "$SOURCE_DIR exists but is not a git checkout"
  log "cloning tmux"
  git clone https://github.com/tmux/tmux "$SOURCE_DIR" >&2
fi

if ! git -C "$SOURCE_DIR" cat-file -e "$TMUX_COMMIT^{commit}" 2>/dev/null; then
  log "fetching tmux commit $TMUX_COMMIT"
  git -C "$SOURCE_DIR" fetch origin "$TMUX_COMMIT" >&2
fi

log "checking out tmux commit $TMUX_COMMIT"
git -C "$SOURCE_DIR" checkout --quiet --detach "$TMUX_COMMIT" >&2
dirty="$(git -C "$SOURCE_DIR" status --porcelain --untracked-files=all)"
[ -z "$dirty" ] || die "tmux source checkout is dirty; refusing to attest a non-pin build: $SOURCE_DIR"

log "building $TMUX_VERSION"
(
  cd "$SOURCE_DIR"
  sh autogen.sh
  # The pin hard-errors on macOS unless the utf8proc choice is explicit; the
  # harness diffs topology and geometry, never glyph widths, so pick the
  # dependency-free build everywhere.
  ./configure --disable-utf8proc
  make clean
  make
) >&2

verify_tmux "$TMUX_BIN" ||
  die "built tmux did not report '$TMUX_VERSION' from -V"
stamp_tmp="$BUILD_STAMP.$$"
build_stamp >"$stamp_tmp"
mv "$stamp_tmp" "$BUILD_STAMP"
printf '%s\n' "$TMUX_BIN"
