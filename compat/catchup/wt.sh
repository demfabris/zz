#!/usr/bin/env bash
set -euo pipefail

MAIN="$(cd "$(git rev-parse --path-format=absolute --git-common-dir)/.." && pwd)"
BASE="$(dirname "$MAIN")"
LEDGER="$MAIN/compat/catchup/ledger.py"

usage() {
  echo "usage: wt.sh add SLOT | item SLOT ID | cache SLOT | rm SLOT | list | prune" >&2
  exit 2
}

path_of() { echo "$BASE/zz-cu-$1"; }

busy() {
  local dir="$1" pid cwd
  for pid in $(ls /proc | rg '^[0-9]+$'); do
    cwd="$(readlink "/proc/$pid/cwd" 2>/dev/null || true)"
    if [ "$cwd" = "$dir" ] || [[ "$cwd" == "$dir/"* ]]; then
      echo "$pid $(tr '\0' ' ' < "/proc/$pid/cmdline" 2>/dev/null | cut -c1-120)"
    fi
  done
}

case "${1:-}" in
  add)
    [ $# -eq 2 ] || usage
    WT="$(path_of "$2")"
    [ -d "$WT" ] || git -C "$MAIN" worktree add --detach "$WT" main >/dev/null
    if [ ! -d "$WT/target" ] && [ -d "$MAIN/target" ]; then
      cp -a --reflink=always "$MAIN/target" "$WT/target" 2>/dev/null || echo "target copied partly (a build in $MAIN moved files); cargo rebuilds the rest" >&2
    fi
    if [ ! -d "$WT/compat/.cache" ] && [ -d "$MAIN/compat/.cache" ]; then
      cp -a --reflink=auto "$MAIN/compat/.cache" "$WT/compat/.cache"
    fi
    echo "ready $WT $(git -C "$WT" rev-parse --short HEAD)"
    ;;
  item)
    [ $# -eq 3 ] || usage
    WT="$(path_of "$2")"
    [ -d "$WT" ] || "$0" add "$2" >/dev/null
    BR="catchup/$3"
    if git -C "$MAIN" show-ref --quiet "refs/heads/$BR"; then
      git -C "$WT" switch "$BR"
    elif git -C "$MAIN" show-ref --quiet "refs/remotes/origin/$BR"; then
      git -C "$WT" switch -c "$BR" --track "origin/$BR"
    else
      git -C "$WT" switch -c "$BR" main
    fi
    if ! cmp -s "$MAIN/compat/.cache/tmux-build.stamp" "$WT/compat/.cache/tmux-build.stamp"; then
      "$0" cache "$2" >/dev/null
    fi
    echo "$WT on $BR at $(git -C "$WT" rev-parse --short HEAD)"
    ;;
  cache)
    [ $# -eq 2 ] || usage
    WT="$(path_of "$2")"
    [ -d "$WT" ] || { echo "no worktree $WT" >&2; exit 1; }
    mkdir -p "$WT/compat/.cache/tmux-src"
    rsync -a --delete "$MAIN/compat/.cache/tmux-src/" "$WT/compat/.cache/tmux-src/"
    cp -a "$MAIN/compat/.cache/tmux-build.stamp" "$WT/compat/.cache/tmux-build.stamp"
    (cd "$WT" && python3 compat/tmux-oracle.py --check 2>&1 | tail -1)
    ;;
  rm)
    [ $# -eq 2 ] || usage
    WT="$(path_of "$2")"
    [ -d "$WT" ] || { echo "no worktree $WT"; exit 0; }
    HOLDERS="$(busy "$WT")"
    if [ -n "$HOLDERS" ]; then
      echo "processes still inside $WT:" >&2
      echo "$HOLDERS" >&2
      exit 1
    fi
    git -C "$MAIN" worktree remove --force "$WT"
    echo "removed $WT"
    ;;
  list)
    git -C "$MAIN" worktree list | rg "/zz-cu-" || true
    ;;
  prune)
    LIVE="$(python3 "$LEDGER" live)"
    git -C "$MAIN" worktree list --porcelain | awk '/^worktree /{w=$2} /^branch /{print w, $2} /^detached/{print w, "detached"}' |
      while read -r WT REF; do
        case "$WT" in "$BASE"/zz-cu-*) ;; *) continue ;; esac
        BR="${REF#refs/heads/}"
        if [ -n "$LIVE" ] && echo "$LIVE" | rg -qx "$BR"; then
          echo "keep $WT ($BR is live)"
        else
          "$0" rm "${WT#"$BASE"/zz-cu-}" || true
        fi
      done
    git -C "$MAIN" worktree prune
    ;;
  *) usage ;;
esac
