#!/usr/bin/env bash
# Fetches the pinned tmux plugin corpus used by alias smoke scenarios.
# ZZ_COMPAT_CORPUS may provide a directory containing the seven checkouts.
set -euo pipefail

COMPAT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
DEFAULT_CORPUS_DIR="$COMPAT_DIR/.cache/plugins"

plugins=(
  "tpm|https://github.com/tmux-plugins/tpm.git|e261deb1b47614eed3400089ce7197dc68acc4eb"
  "tmux-sensible|https://github.com/tmux-plugins/tmux-sensible.git|25cb91f42d020f675bb0a2ce3fbd3a5d96119efa"
  "vim-tmux-navigator|https://github.com/christoomey/vim-tmux-navigator.git|e41c431a0c7b7388ae7ba341f01a0d217eb3a432"
  "tmux-yank|https://github.com/tmux-plugins/tmux-yank.git|acfd36e4fcba99f8310a7dfb432111c242fe7392"
  "tmux-resurrect|https://github.com/tmux-plugins/tmux-resurrect.git|cff343cf9e81983d3da0c8562b01616f12e8d548"
  "tmux-continuum|https://github.com/tmux-plugins/tmux-continuum.git|0698e8f4b17d6454c71bf5212895ec055c578da0"
  "tmux-fpp|https://github.com/tmux-plugins/tmux-fpp.git|878302f228ee14f0fa59717f63743d396b327a21"
)

log() { printf '\033[1;36m==>\033[0m %s\n' "$*" >&2; }
skip() {
  printf '\033[1;33mSKIP:\033[0m %s\n' "$*" >&2
  exit 3
}
die() {
  printf '\033[1;31merror:\033[0m %s\n' "$*" >&2
  exit 1
}

verify_corpus() {
  local corpus_dir="$1"
  local entry name url commit checkout actual

  for entry in "${plugins[@]}"; do
    IFS='|' read -r name url commit <<<"$entry"
    checkout="$corpus_dir/$name"
    [ -d "$checkout/.git" ] || return 1
    actual="$(git -C "$checkout" rev-parse HEAD 2>/dev/null || true)"
    [ "$actual" = "$commit" ] || return 1
  done
}

if [ -n "${ZZ_COMPAT_CORPUS:-}" ]; then
  [ -d "$ZZ_COMPAT_CORPUS" ] ||
    die "ZZ_COMPAT_CORPUS is not a directory: $ZZ_COMPAT_CORPUS"
  corpus_dir="$(cd -- "$ZZ_COMPAT_CORPUS" && pwd)"
  verify_corpus "$corpus_dir" ||
    die "ZZ_COMPAT_CORPUS does not contain all seven pinned plugin checkouts"
  printf '%s\n' "$corpus_dir"
  exit 0
fi

command -v git >/dev/null 2>&1 ||
  skip "git is unavailable and the plugin corpus is not cached"

mkdir -p "$DEFAULT_CORPUS_DIR"
for entry in "${plugins[@]}"; do
  IFS='|' read -r name url commit <<<"$entry"
  checkout="$DEFAULT_CORPUS_DIR/$name"

  if [ ! -d "$checkout/.git" ]; then
    [ ! -e "$checkout" ] || die "$checkout exists but is not a git checkout"
    log "cloning $name"
    if ! git clone --quiet "$url" "$checkout"; then
      skip "could not fetch $name and no pinned checkout is cached"
    fi
  fi

  if ! git -C "$checkout" cat-file -e "$commit^{commit}" 2>/dev/null; then
    log "fetching $name commit $commit"
    if ! git -C "$checkout" fetch --quiet origin "$commit"; then
      skip "could not fetch pinned $name commit $commit"
    fi
  fi

  log "checking out $name at $commit"
  git -C "$checkout" checkout --quiet --detach "$commit"
  actual="$(git -C "$checkout" rev-parse HEAD 2>/dev/null || true)"
  [ "$actual" = "$commit" ] || die "$name resolved to $actual instead of $commit"
done

verify_corpus "$DEFAULT_CORPUS_DIR" || die "plugin corpus verification failed"
printf '%s\n' "$DEFAULT_CORPUS_DIR"
