#!/usr/bin/env bash
# mcp-check.sh - attach Playwright MCP and chrome-devtools-mcp to the zz CDP endpoint and exercise
# navigate, snapshot, click, and screenshot on the fixture site. Appends rows to out/results.tsv
# under the backends "zz-playwright-mcp" and "zz-chrome-devtools-mcp".
set -uo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
SITE=${SITE:-http://127.0.0.1:4173}
CDP_PORT=${CDP_PORT:-9222}
RESULTS=${RESULTS:-$HERE/out/results.tsv}
now() { if [ -n "${EPOCHREALTIME:-}" ]; then printf "%s" "$EPOCHREALTIME"; else perl -MTime::HiRes=time -e 'printf "%.6f", time'; fi; }
ms() { awk -v s="$1" -v e="$2" 'BEGIN{printf "%.0f", (e-s)*1000}'; }
row() { local backend=$1 name=$2 pass=$3 elapsed=$4 note=$5; printf '%s\t%s\t%s\t%s\t%s\n' "$backend" "$name" "$pass" "$elapsed" "$note" >> "$RESULTS"; printf '%-24s %-22s %s %6s ms  %s\n' "$backend" "$name" "$([ "$pass" = 1 ] && echo PASS || echo FAIL)" "$elapsed" "$note"; }
BEFORE=$(curl -s "http://127.0.0.1:$CDP_PORT/json/list" | jq -r '.[] | select(.type=="page") | .id' | sort)
TARGET=$(curl -s "http://127.0.0.1:$CDP_PORT/json/list" | jq -r --arg s "$SITE" '[.[] | select(.type=="page" and (.url|startswith($s)))][0].id // empty')
[ -n "$TARGET" ] || { echo "no zz browser pane on $SITE" >&2; exit 2; }
call() { node "$HERE/mcp-call.mjs" "$SERVER" "$@" 2>&1; }

SERVER="npx -y @playwright/mcp@latest --cdp-endpoint http://127.0.0.1:$CDP_PORT"
B=zz-playwright-mcp
t=$(now); OUT=$(call list); row $B tools-list "$([[ "$OUT" == *browser_snapshot* ]] && echo 1 || echo 0)" "$(ms $t $(now))" "$(wc -l <<<"$OUT" | tr -d ' ') tools"
t=$(now); OUT=$(call call browser_tabs '{"action":"list"}'); row $B tabs-list "$([[ "$OUT" == *"$SITE"* ]] && echo 1 || echo 0)" "$(ms $t $(now))" "$(rg -c 'http' <<<"$OUT") tabs listed"
IDX=$(rg -o '^- ([0-9]+):.*'"$SITE" -r '$1' <<<"$OUT" | head -1); [ -z "$IDX" ] && IDX=$(rg -o '\b([0-9]+)\b.*'"$SITE" -r '$1' <<<"$OUT" | head -1)
t=$(now); OUT=$(call call browser_tabs "{\"action\":\"select\",\"index\":${IDX:-0}}" call browser_navigate "{\"url\":\"$SITE/form\"}" call browser_snapshot '{}'); row $B navigate-snapshot "$([[ "$OUT" == *"Place order"* ]] && echo 1 || echo 0)" "$(ms $t $(now))" "$(rg -o 'ref=[a-z0-9]+' <<<"$OUT" | wc -l | tr -d ' ') refs"
t=$(now); OUT=$(call call browser_tabs "{\"action\":\"select\",\"index\":${IDX:-0}}" call browser_navigate "{\"url\":\"$SITE/form\"}" call browser_snapshot '{}' call browser_click '{"element":"Home link","target":"{{last:link .Home. .ref=([a-z0-9]+).}}"}' call browser_evaluate '{"function":"() => document.title"}'); row $B click-ref "$([[ "$OUT" == *"Bench Home"* ]] && echo 1 || echo 0)" "$(ms $t $(now))" "$(rg -o 'Bench [A-Za-z]+' <<<"$OUT" | tail -1)"
t=$(now); OUT=$(call call browser_tabs "{\"action\":\"select\",\"index\":${IDX:-0}}" call browser_take_screenshot "{\"filename\":\"$HERE/out/pw-home.png\"}"); row $B screenshot "$([ -s "$HERE/out/pw-home.png" ] && [ "$(stat -f %z "$HERE/out/pw-home.png")" -gt 5000 ] && echo 1 || echo 0)" "$(ms $t $(now))" "$(stat -f %z "$HERE/out/pw-home.png" 2>/dev/null) bytes"

SERVER="npx -y chrome-devtools-mcp@latest --browserUrl http://127.0.0.1:$CDP_PORT"
B=zz-chrome-devtools-mcp
t=$(now); OUT=$(call list); row $B tools-list "$([[ "$OUT" == *take_snapshot* ]] && echo 1 || echo 0)" "$(ms $t $(now))" "$(wc -l <<<"$OUT" | tr -d ' ') tools"
t=$(now); OUT=$(call call list_pages '{}'); row $B pages-list "$([[ "$OUT" == *"$SITE"* ]] && echo 1 || echo 0)" "$(ms $t $(now))" "$(rg -c 'http' <<<"$OUT") pages listed"
PID=$(rg -o '([0-9]+):[^\n]*'"$SITE" -r '$1' <<<"$OUT" | head -1); [ -z "$PID" ] && PID=$(rg -o 'pageId[^0-9]*([0-9]+)' -r '$1' <<<"$OUT" | head -1)
t=$(now); OUT=$(call call select_page "{\"pageId\":${PID:-1}}" call navigate_page "{\"pageId\":${PID:-1},\"type\":\"url\",\"url\":\"$SITE/form\"}" call take_snapshot "{\"pageId\":${PID:-1}}"); row $B navigate-snapshot "$([[ "$OUT" == *"Place order"* ]] && echo 1 || echo 0)" "$(ms $t $(now))" "$(rg -o 'uid=[0-9_]+' <<<"$OUT" | wc -l | tr -d ' ') uids"
t=$(now); OUT=$(call call select_page "{\"pageId\":${PID:-1}}" call navigate_page "{\"pageId\":${PID:-1},\"type\":\"url\",\"url\":\"$SITE/form\"}" call take_snapshot "{\"pageId\":${PID:-1}}" call click "{\"pageId\":${PID:-1},\"uid\":\"{{last:uid=([0-9_]+) link .Home.}}\"}" call evaluate_script "{\"pageId\":${PID:-1},\"function\":\"() => document.title\"}"); row $B click-uid "$([[ "$OUT" == *"Bench Home"* ]] && echo 1 || echo 0)" "$(ms $t $(now))" "$(rg -o 'Bench [A-Za-z]+' <<<"$OUT" | tail -1)"
t=$(now); OUT=$(call call select_page "{\"pageId\":${PID:-1}}" call take_screenshot "{\"pageId\":${PID:-1}}"); printf '%s\n' "$OUT" > "$HERE/out/cdt-screenshot.txt"; row $B screenshot "$([ "$(rg -o 'image/[a-z]+ ([0-9]+) bytes' -r '$1' <<<"$OUT" | head -1)" -gt 5000 ] 2>/dev/null && echo 1 || echo 0)" "$(ms $t $(now))" "$(rg -o 'image/[a-z]+ [0-9]+ bytes' <<<"$OUT" | head -1)$(rg -o 'ERROR.*' <<<"$OUT" | head -1)"

AFTER=$(curl -s "http://127.0.0.1:$CDP_PORT/json/list" | jq -r '.[] | select(.type=="page") | .id' | sort)
for t in $(comm -13 <(printf '%s\n' "$BEFORE") <(printf '%s\n' "$AFTER")); do echo "closing stray target $t"; curl -s "http://127.0.0.1:$CDP_PORT/json/close/$t" >/dev/null; done
