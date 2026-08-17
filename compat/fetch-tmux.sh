#!/usr/bin/env bash
# Fetches and builds the pinned tmux behavioral reference, caching the binary
# under compat/.cache. ZZ_COMPAT_TMUX may provide an already-built binary.
set -euo pipefail

COMPAT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
CACHE_DIR="$COMPAT_DIR/.cache"
SOURCE_DIR="$CACHE_DIR/tmux-src"
TMUX_BIN="$SOURCE_DIR/tmux"
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

if verify_tmux "$TMUX_BIN"; then
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

log "building $TMUX_VERSION"
(
  cd "$SOURCE_DIR"
  sh autogen.sh
  # The pin hard-errors on macOS unless the utf8proc choice is explicit; the
  # harness diffs topology and geometry, never glyph widths, so pick the
  # dependency-free build everywhere.
  ./configure --disable-utf8proc
  make
) >&2

verify_tmux "$TMUX_BIN" ||
  die "built tmux did not report '$TMUX_VERSION' from -V"
printf '%s\n' "$TMUX_BIN"
