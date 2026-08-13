#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CARGO_RELEASE_VERSION="1.1.5"

die() { echo "error: $*" >&2; exit 1; }
note() { echo "==> $*"; }

usage() {
    cat >&2 <<'EOF'
usage: release.sh <level-or-version> [--execute]

Examples:
  scripts/release.sh 1.2.0-beta.1
  scripts/release.sh beta
  scripts/release.sh beta --execute

The default is a dry run. --execute commits the version bump, creates an
annotated v<version> tag, and pushes the commit and tag to origin. The working
tree must be clean, so commit release-flow changes before using this command.
EOF
    exit 2
}

[[ $# -ge 1 && $# -le 2 ]] || usage
target="$1"
mode="${2:-}"
[[ -z "$mode" || "$mode" == "--execute" ]] || usage

case "$target" in
    major|minor|patch|release|alpha|beta|rc) ;;
    *)
        [[ "$target" =~ ^[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$ ]] || \
            die "expected major, minor, patch, release, alpha, beta, rc, or an exact SemVer"
        ;;
esac

installed="$(cargo release --version 2>/dev/null || true)"
[[ "$installed" == "cargo-release $CARGO_RELEASE_VERSION" ]] || \
    die "cargo-release $CARGO_RELEASE_VERSION is required; run 'just release-setup'"

[[ -z "$(git -C "$REPO_ROOT" status --porcelain)" ]] || \
    die "working tree is not clean; commit or remove local changes before releasing"
[[ "$(git -C "$REPO_ROOT" branch --show-current)" == "main" ]] || \
    die "releases must run from main"

command=(
    cargo release "$target"
    --package zz
    --config "$REPO_ROOT/release.toml"
)

if [[ "$mode" == "--execute" ]]; then
    note "Execute mode will commit, create an annotated tag, and push both to origin"
    command+=(--execute)
else
    note "Dry run only; no files, commits, tags, or remotes will change"
fi

cd "$REPO_ROOT"
"${command[@]}"
