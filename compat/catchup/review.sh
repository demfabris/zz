#!/usr/bin/env bash
set -uo pipefail

[ $# -eq 2 ] || { echo "usage: review.sh SLOT ID" >&2; exit 2; }
MAIN="$(cd "$(git rev-parse --path-format=absolute --git-common-dir)/.." && pwd)"
WT="$(dirname "$MAIN")/zz-cu-$1"
ID="$2"
STATE="${ZZ_CATCHUP_STATE:-$HOME/.cache/zz-catchup}"
mkdir -p "$STATE/reviews"
N=1
while [ -e "$STATE/reviews/$ID-$N.md" ]; do N=$((N + 1)); done
OUT="$STATE/reviews/$ID-$N.md"

PROMPT="$(cat "$MAIN/compat/catchup/review.md")

Ledger item:
$(python3 "$MAIN/compat/catchup/ledger.py" show "$ID")"

cd "$WT" || exit 1
printf '%s\n' "$PROMPT" | timeout 1800 codex review --base main -c model_reasoning_effort='"high"' - > "$OUT" 2>&1
rc=$?
echo "CODEX-REVIEW-DONE exit $rc" >> "$OUT"
echo "$OUT"
