#!/usr/bin/env bash
set -u
if [ $# -lt 7 ]; then
  echo "usage: lane-run.sh <worktree> <brief.md> <out.json> <effort> <budget-min> <idle-min> <base-sha> [zone-globs-file]" >&2
  exit 2
fi
WT=$1 BRIEF=$2 OUT=$3 EFFORT=$4 BUDGET=$5 IDLE=$6 BASE=$7 ZONE=${8:--}
HERE=$(cd "$(dirname "$0")" && pwd)
START=$(date +%s)
python3 "$HERE/lane-watchdog.py" "$WT" "$BRIEF" "$OUT" "$START" "$BUDGET" "$IDLE" "$BASE" "$ZONE" &
WATCHDOG=$!
codex exec -m gpt-6.1-sol -c model_reasoning_effort="\"$EFFORT\"" -c service_tier='"priority"' \
  -s danger-full-access -C "$WT" --json --output-schema "$HERE/lane-answer.schema.json" -o "$OUT" \
  "Read $BRIEF and do exactly what it says." < /dev/null > "$OUT.events.jsonl" 2> "$OUT.stderr.log"
STATUS=$?
END=$(date +%s)
kill "$WATCHDOG" 2>/dev/null
python3 "$HERE/lane-cost.py" "$OUT.events.jsonl" "$START" "$END" > "$OUT.cost.json"
if ! python3 -c "import json,sys; d=json.load(open(sys.argv[1])); sys.exit(0 if isinstance(d.get('done'), bool) else 1)" "$OUT" 2>/dev/null; then
  echo "no valid answer (codex exit $STATUS)" > "$OUT.failed"
fi
echo "exit $STATUS" > "$OUT.exit"
