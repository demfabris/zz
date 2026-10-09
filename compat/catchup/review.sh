#!/usr/bin/env bash
set -uo pipefail

[ $# -ge 2 ] || { echo "usage: review.sh SLOT ID [EXTRA]" >&2; exit 2; }
MAIN="$(cd "$(git rev-parse --path-format=absolute --git-common-dir)/.." && pwd)"
WT="$(dirname "$MAIN")/zz-cu-$1"
ID="$2"
STATE="${ZZ_CATCHUP_STATE:-$HOME/.cache/zz-catchup}"
mkdir -p "$STATE/reviews"
N=1
while [ -e "$STATE/reviews/$ID-$N.md" ]; do N=$((N + 1)); done
OUT="$STATE/reviews/$ID-$N.md"

PROMPT="Review the changes on this branch: \`git diff main...HEAD\` in $WT.

$(cat "$MAIN/compat/catchup/review.md")

Ledger item:
$(python3 "$MAIN/compat/catchup/ledger.py" show "$ID")
${3:-}"

printf '%s\n' "$PROMPT" | timeout 1800 codex exec --skip-git-repo-check --sandbox read-only \
  -c model_reasoning_effort='"high"' -C "$WT" -o "$OUT" - > "$OUT.log" 2>&1
rc=$?
[ -s "$OUT" ] || cp "$OUT.log" "$OUT"
echo "CODEX-REVIEW-DONE exit $rc" >> "$OUT"
echo "$OUT"
