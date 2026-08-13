#!/usr/bin/env bash
set -euo pipefail

# fork-sync: status and rebase for the carried-patch forks in scripts/forks.conf.
#
#   fork-sync.sh status              show carried commits, upstream drift, lock state
#   fork-sync.sh rebase <name> [rev] rebase the patch branch onto upstream (default:
#                                    upstream branch tip), force-push, refresh Cargo.lock
#
# Rebase uses a cached blobless clone under ~/.cache/zz-forks so the multi-GB
# upstream history is only paid for once.

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONF="$REPO_ROOT/scripts/forks.conf"
CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/zz-forks"

die() { echo "error: $*" >&2; exit 1; }

read_conf() {
    grep -Ev '^\s*(#|$)' "$CONF"
}

find_fork() {
    local name="$1" line
    line=$(read_conf | awk -v n="$name" '$1 == n') || true
    [[ -n "$line" ]] || die "unknown fork '$name' (see $CONF)"
    echo "$line"
}

status() {
    printf '%-6s %-14s %-8s %-10s %-12s %s\n' FORK BRANCH CARRIED BEHIND BASE LOCK
    while read -r name upstream fork branch upstream_branch _packages; do
        local compare tip ahead behind base locked lock_state
        compare=$(gh api "repos/$upstream/compare/$upstream_branch...${fork%%/*}:$branch" \
            --jq '[.ahead_by, .behind_by, .merge_base_commit.sha] | @tsv') ||
            die "compare failed for $name (is the fork branch pushed?)"
        IFS=$'\t' read -r ahead behind base <<<"$compare"
        tip=$(gh api "repos/$fork/branches/$branch" --jq .commit.sha)
        # Patch entries pin either `branch = "…"` or an explicit `rev = "…"`;
        # both serialize into Cargo.lock as `?<kind>=<value>#<full-sha>`.
        locked=$(grep -m1 -oP "github.com/$fork\?(branch=$branch|rev=[0-9a-f]+)#\K[0-9a-f]+" \
            "$REPO_ROOT/Cargo.lock" || true)
        if [[ -z "$locked" ]]; then
            lock_state="not in Cargo.lock"
        elif [[ "$locked" == "$tip" ]]; then
            lock_state="in sync (${locked:0:10})"
        else
            lock_state="STALE: lock ${locked:0:10} != tip ${tip:0:10} (run: cargo update)"
        fi
        printf '%-6s %-14s %-8s %-10s %-12s %s\n' \
            "$name" "$branch" "$ahead" "$behind" "${base:0:10}" "$lock_state"
    done < <(read_conf)
    echo
    echo "CARRIED = our patch commits on the branch; BEHIND = upstream commits since"
    echo "our base. Rebase with: just fork-rebase <name> [rev]"
}

rebase() {
    local name="$1" target="${2:-}"
    read -r _ upstream fork branch upstream_branch packages <<<"$(find_fork "$name")"
    local clone="$CACHE/$name"

    if [[ ! -d "$clone" ]]; then
        echo "one-time blobless clone of $fork into $clone ..."
        mkdir -p "$CACHE"
        git clone --filter=blob:none "git@github.com:$fork.git" "$clone"
        git -C "$clone" remote add upstream "https://github.com/$upstream"
    fi

    git -C "$clone" fetch origin "$branch"
    git -C "$clone" fetch upstream "$upstream_branch" ${target:+"$target"}
    [[ -n "$target" ]] || target="upstream/$upstream_branch"

    git -C "$clone" checkout -B "$branch" "origin/$branch"
    if ! git -C "$clone" rebase "$target"; then
        cat >&2 <<EOF

Rebase hit conflicts. Resolve them in $clone:
    git -C $clone status
    ... fix, git add, git -C $clone rebase --continue ...
then finish with:
    git -C $clone push --force-with-lease origin $branch
    (cd $REPO_ROOT && cargo update -p ${packages//,/ -p }) # then run the gates
EOF
        exit 1
    fi

    git -C "$clone" push --force-with-lease origin "$branch"
    echo "rebased $branch onto $(git -C "$clone" rev-parse --short "$target"), pushed."

    echo "refreshing Cargo.lock ..."
    local pkg args=()
    IFS=',' read -ra pkgs <<<"$packages"
    for pkg in "${pkgs[@]}"; do args+=(-p "$pkg"); done
    if ! (cd "$REPO_ROOT" && cargo update "${args[@]}"); then
        cat >&2 <<EOF

cargo update failed. If upstream bumped a crate version, refresh the
'version =' pin in the [patch] section of Cargo.toml, then re-run:
    cargo update ${args[*]}
EOF
        exit 1
    fi
    echo
    echo "done. now run the gates:"
    echo "    cargo check --workspace && cargo clippy --workspace --all-targets && cargo test --workspace"
}

case "${1:-status}" in
    status) status ;;
    rebase) shift; [[ $# -ge 1 ]] || die "usage: fork-sync.sh rebase <name> [rev]"; rebase "$@" ;;
    *) die "usage: fork-sync.sh [status|rebase <name> [rev]]" ;;
esac
