#!/usr/bin/env bash
# matrix.sh zz|own  - run the browser primitive parity matrix through agent-browser
# against a zz browser pane (CDP attach) or agent-browser's own managed Chromium.
set -uo pipefail
BACKEND=${1:?usage: matrix.sh zz|own}
HERE=$(cd "$(dirname "$0")" && pwd)
SITE=${SITE:-http://127.0.0.1:4173}
CDP_PORT=${CDP_PORT:-9222}
OUT_DIR=${OUT_DIR:-$HERE/out/$BACKEND}; rm -rf "$OUT_DIR"; mkdir -p "$OUT_DIR"
RESULTS=${RESULTS:-$HERE/out/results.tsv}; mkdir -p "$(dirname "$RESULTS")"
read -r -a AB_BIN <<<"${AGENT_BROWSER:-$(command -v agent-browser || echo "npx -y agent-browser@0.37.1")}"
SESSION=bench-$BACKEND
case $BACKEND in
  zz) AB_BASE=("${AB_BIN[@]}" --cdp "$CDP_PORT" --session "$SESSION") ;;
  own) AB_BASE=("${AB_BIN[@]}" --session "$SESSION") ;;
  *) echo "unknown backend $BACKEND" >&2; exit 2 ;;
esac
unset TMUX
now() { if [ -n "${EPOCHREALTIME:-}" ]; then printf "%s" "$EPOCHREALTIME"; else perl -MTime::HiRes=time -e 'printf "%.6f", time'; fi; }
ms() { awk -v s="$1" -v e="$2" 'BEGIN{printf "%.0f", (e-s)*1000}'; }
ab() { timeout "${AB_TIMEOUT:-30}" "${AB_BASE[@]}" "$@"; }
run() { OUT=$(ab "$@" 2>&1); RC=$?; }
begin() { T0=$(now); }
check() {
  local name=$1 expr=$2 note=""; local elapsed; elapsed=$(ms "$T0" "$(now)")
  if eval "$expr"; then pass=1; else pass=0; note="rc=$RC $(tr '\n' ' ' <<<"${OUT:-}" | cut -c1-140)"; fi
  printf '%s\t%s\t%s\t%s\t%s\n' "$BACKEND" "$name" "$pass" "$elapsed" "$note" >> "$RESULTS"
  printf '%-30s %s %6s ms  %s\n' "$name" "$([ $pass = 1 ] && echo PASS || echo FAIL)" "$elapsed" "$note"
}
text() { run get text "$1"; printf '%s' "$OUT"; }
title() { run get title; printf '%s' "$OUT"; }
url() { run get url; printf '%s' "$OUT"; }
evaljs() { run eval "$1"; printf '%s' "$OUT" | sed -e 's/^"//' -e 's/"$//'; }

curl -sf "$SITE/" >/dev/null || { echo "fixture site not reachable at $SITE (run: node site/server.mjs)" >&2; exit 2; }
if [ "$BACKEND" = zz ]; then
  command -v zz >/dev/null || { echo "zz CLI missing" >&2; exit 2; }
  curl -sf "http://127.0.0.1:$CDP_PORT/json/version" >/dev/null || { echo "no CDP endpoint on $CDP_PORT (enable browser-remote-debugging-port and open a browser pane)" >&2; exit 2; }
  PANE=$(zz list-panes -a -F '#{pane_id} #{pane_kind} #{browser_url}' | awk -v s="$SITE" '$2=="browser" && index($3,s)==1 {print $1; exit}')
  if [ -z "$PANE" ]; then PANE=$(zz new-browser -d -n bench "$SITE/" >/dev/null && sleep 2 && zz list-panes -a -F '#{pane_id} #{pane_kind} #{browser_url}' | awk -v s="$SITE" '$2=="browser" && index($3,s)==1 {print $1; exit}'); fi
  [ -n "$PANE" ] || { echo "could not create a bench browser pane" >&2; exit 2; }
  zz select-window -t "$(zz display-message -p -t "$PANE" '#{session_name}:#{window_index}')" >/dev/null 2>&1
  for _ in 1 2 3 4 5 6 7 8 9 10; do TARGET=$(curl -s "http://127.0.0.1:$CDP_PORT/json/list" | jq -r --arg s "$SITE" '[.[] | select(.type=="page" and (.url|startswith($s)))][0].id // empty'); [ -n "$TARGET" ] && break; sleep 0.5; done
  [ -n "$TARGET" ] || { echo "the bench pane has no CDP target yet (is the zz window drawn?)" >&2; exit 2; }
  echo "zz backend: pane $PANE target $TARGET"
  run tab "$TARGET"; [ $RC -eq 0 ] || echo "tab switch: $OUT"
  BEFORE_TARGETS=$(curl -s "http://127.0.0.1:$CDP_PORT/json/list" | jq -r '.[] | select(.type=="page") | .id' | sort)
fi
EXPECTED_CSV_SHA=$(curl -s "$SITE/report.csv" | shasum -a 256 | cut -d' ' -f1)
head -c 1234 /dev/zero | tr '\0' 'x' > "$OUT_DIR/upload.txt"
echo "== $BACKEND: ${AB_BASE[*]} =="

begin; run open "$SITE/"; check open '[ "$(title)" = "Bench Home" ]'
begin; check get-url '[ "$(url)" = "'"$SITE"'/" ]'
begin; run snapshot -i; check snapshot-refs 'rg -q "ref=e[0-9]+" <<<"$OUT" && rg -q "Double-click me" <<<"$OUT"'
begin; check get-text '[[ "$(text "#intro")" == *"Fixture site"* ]]'
begin; run get attr "#secret-holder" data-secret; check get-attr '[ "$OUT" = TOKEN-7f3a9c ]'
begin; run get count .product; check get-count '[ "$(tr -dc 0-9 <<<"$OUT")" = 5 ]'
begin; run get box "#dbl"; check get-box 'rg -q "[0-9]" <<<"$OUT"'
begin; run read "$SITE/"; check read-page '[[ "$OUT" == *"Bench Home"* ]]'
begin; run snapshot -i; REF=$(rg -o 'link "Form" \[ref=(e[0-9]+)\]' -r '$1' <<<"$OUT" | head -1); run click "@$REF"; check click-ref '[ "$(title)" = "Bench Form" ]'
begin; run open "$SITE/"; run click "a[href='/delayed']"; check click-css '[ "$(title)" = "Bench Delayed" ]'
begin; run open "$SITE/"; run find role link click --name Table; check find-role '[[ "$(title)" == "Bench Table 1" ]]'
begin; run open "$SITE/"; run find text "Hover" click; check find-text '[ "$(title)" = "Bench Hover" ]'
begin; run open "$SITE/"; run dblclick "#dbl"; check dblclick '[ "$(text "#dbl-out")" = double-clicked ]'
begin; run open "$SITE/form"; run fill "#name" Ada; run fill "#email" a@b.c; run select "#role" engineer; run check "#newsletter"; run click "#plan-pro"; run fill "#bio" hi; run is checked "#newsletter"; ISCHK=$OUT; run click "#submit"; check form-fill-submit '[ "$(text "#code")" = "Confirmation CONF-f3a81e31" ]'
begin; check is-checked '[[ "$ISCHK" == *true* ]]'
begin; run open "$SITE/keys"; run click "#box"; run type "#box" abc; run press Enter; run press Escape; LOG=$(text "#log"); check type-and-press '[[ "$LOG" == *ENTER* && "$LOG" == *ESC* ]]'
begin; run fill "#box" ""; run focus "#box"; run keyboard type hello; run get value "#box"; check keyboard-type '[ "$OUT" = hello ]'
begin; run open "$SITE/hover"; run hover "#menu-label"; run click "#hidden-link"; check hover-reveal '[ "$(title)" = "Hover Target" ]'
begin; run open "$SITE/long"; run scroll down 4000; check scroll '[ "$(evaljs "window.scrollY > 1000")" = true ]'
begin; run scrollintoview "#bottom-btn"; run click "#bottom-btn"; check scrollintoview-click '[ "$(text "#bottom-btn")" = "bottom clicked" ]'
begin; run mouse move 200 200; run mouse wheel -3000; check mouse-wheel '[ "$(evaljs "window.scrollY < 4000")" = true ]'
begin; run open "$SITE/delayed"; run wait --text "Banner ready"; check wait-text '[[ "$(text "#banner")" == *TOKEN-7f3a9c* ]]'
begin; run click "#fetch"; run wait --fn "document.getElementById('fetched').textContent.length>0"; check wait-fn '[[ "$(text "#fetched")" == *"slow TOKEN-7f3a9c"* ]]'
begin; run open "$SITE/console"; run wait --load networkidle; check wait-networkidle '[[ "$(text "#net")" == "data: TOKEN-7f3a9c" ]]'
begin; run open "$SITE/spa"; run click "#go-two"; run wait --url "**/spa/two"; check wait-url-spa '[[ "$(url)" == */spa/two && "$(title)" = "SPA Two" ]]'
begin; run open "$SITE/"; run open "$SITE/form"; run back; B=$(title); run forward; F=$(title); run reload; R=$(title); check back-forward-reload '[ "$B" = "Bench Home" ] && [ "$F" = "Bench Form" ] && [ "$R" = "Bench Form" ]'
begin; run open "$SITE/redirect"; check redirect '[[ "$(url)" == */redirected && "$(title)" = Redirected ]]'
begin; run open "$SITE/iframe"; run snapshot -i; IREF=$(rg -o 'button "Inner button" \[ref=(e[0-9]+)\]' -r '$1' <<<"$OUT" | head -1); [ -n "$IREF" ] && run click "@$IREF"; check iframe-click-ref '[ "$(evaljs "document.getElementById(\"frame\").contentDocument.getElementById(\"inner-out\").textContent")" = "inner clicked" ]'
begin; run open "$SITE/iframe"; run frame "#frame"; run click "#inner"; run get text "#inner-out"; IT=$OUT; run frame main; check iframe-frame-switch '[ "$IT" = "inner clicked" ]'
begin; run open "$SITE/shadow"; run snapshot -i; SREF=$(rg -o 'button "Shadow button" \[ref=(e[0-9]+)\]' -r '$1' <<<"$OUT" | head -1); [ -n "$SREF" ] && run click "@$SREF"; check shadow-dom-click-ref '[ "$(evaljs "document.getElementById(\"widget\").shadowRoot.getElementById(\"shadow-out\").textContent")" = "shadow clicked" ]'
begin; run open "$SITE/shadow"; run click "#shadow-btn"; check shadow-dom-click-css '[ "$(evaljs "document.getElementById(\"widget\").shadowRoot.getElementById(\"shadow-out\").textContent")" = "shadow clicked" ]'
begin; run eval "1+1"; check eval-js '[ "$(tr -dc 0-9 <<<"$OUT")" = 2 ]'
begin; run open "$SITE/storage"; run click "#set"; run cookies get; C=$OUT; run storage local get bench; check cookies-and-storage '[[ "$C" == *cookie-TOKEN-7f3a9c* && "$OUT" == *local-TOKEN-7f3a9c* ]]'
begin; run cookies set benchset yes --url "$SITE/"; run reload; check cookie-set '[[ "$(text "#cookie")" == *benchset=yes* ]]'
begin; run open "$SITE/console"; run wait 800; run console; check console-messages '[[ "$OUT" == *bench-error-1* ]]'
begin; run network requests; check network-requests '[[ "$OUT" == */api/data* && "$OUT" == */missing* ]]'
begin; run network route "**/api/data" --body '{"value":"mocked"}'; run open "$SITE/console"; run wait --load networkidle; run network unroute; check network-mock '[ "$(text "#net")" = "data: mocked" ]'
begin; run open "$SITE/"; run screenshot "$OUT_DIR/home.png"; run open "$SITE/long"; run screenshot "$OUT_DIR/long.png"; check screenshot '[ -s "$OUT_DIR/home.png" ] && [ "$(stat -f %z "$OUT_DIR/home.png")" -gt 5000 ] && ! cmp -s "$OUT_DIR/home.png" "$OUT_DIR/long.png"'
begin; run screenshot --full "$OUT_DIR/long-full.png"; check screenshot-full '[ -s "$OUT_DIR/long-full.png" ] && [ "$(sips -g pixelHeight "$OUT_DIR/long-full.png" 2>/dev/null | awk "/pixelHeight/{print \$2}")" -gt "$(sips -g pixelHeight "$OUT_DIR/long.png" 2>/dev/null | awk "/pixelHeight/{print \$2}")" ]'
begin; run pdf "$OUT_DIR/page.pdf"; check pdf '[ "$(head -c 4 "$OUT_DIR/page.pdf" 2>/dev/null)" = "%PDF" ]'
begin; run set viewport 800 600; run open "$SITE/"; check set-viewport '[ "$(evaljs "window.innerWidth")" = 800 ]'; run set viewport 1280 800
begin; run set media dark; check media-dark '[ "$(evaljs "matchMedia(\"(prefers-color-scheme: dark)\").matches")" = true ]'; run set media light
begin; run open "$SITE/upload"; run upload "#file" "$OUT_DIR/upload.txt"; run click "#send"; run wait --text "file="; check upload '[[ "$(text "#upload-result")" == "file=upload.txt bytes="* ]]'
begin; run open "$SITE/download"; run download "#report" "$OUT_DIR/report.csv"; check download-click '[ "$(shasum -a 256 "$OUT_DIR/report.csv" 2>/dev/null | cut -d" " -f1)" = "$EXPECTED_CSV_SHA" ]'
begin; run open "$SITE/dialogs"; run click "#alert"; run dialog accept; check dialog-alert '[ "$(text "#result")" = "alert closed" ]'
begin; run click "#confirm"; run dialog accept; check dialog-confirm '[ "$(text "#result")" = "confirmed: true" ]'
begin; run click "#prompt"; run dialog accept zz; check dialog-prompt '[ "$(text "#result")" = "prompt: zz" ]'
begin; run open "$SITE/popup"; run click "#blank-link"; run tab list; TABS=$OUT; NEW=$(rg -o '^\s*(t[0-9]+)' -r '$1' <<<"$TABS" | tail -1); run tab "$NEW"; CT=$(title); run tab close; run tab list; check tab-popup-link '[ "$CT" = "Popup Child" ] && [ "$(rg -c "t[0-9]+" <<<"$TABS")" -ge 2 ]'
begin; run tab new "$SITE/table?page=2"; NT=$(title); run tab close; check tab-new-close '[ "$NT" = "Bench Table 2" ]'
begin; run open "$SITE/popup"; run click "#open-window"; run wait 500; run tab list; WT=$OUT; NEW=$(rg -o '^\s*(t[0-9]+)' -r '$1' <<<"$WT" | tail -1); run tab "$NEW"; WTT=$(title); run tab close; check window-open '[ "$WTT" = "Popup Child" ]'
begin; run open "$SITE/table?page=1"; run click "#next"; run click "#next"; run get text "#items tr:last-child td:last-child"; check paginate-extract '[ "$OUT" = Zeta ]'

if [ "$BACKEND" = zz ]; then
  AFTER=$(curl -s "http://127.0.0.1:$CDP_PORT/json/list" | jq -r '.[] | select(.type=="page") | "\(.id) \(.url)"')
  STRAY=$(comm -13 <(printf '%s\n' "$BEFORE_TARGETS") <(printf '%s\n' "$AFTER" | cut -d' ' -f1 | sort))
  for id in $STRAY; do printf 'closing stray CDP target %s (%s)\n' "$id" "$(printf '%s\n' "$AFTER" | awk -v i="$id" '$1==i{print $2}')"; curl -s "http://127.0.0.1:$CDP_PORT/json/close/$id" >/dev/null; done
  printf 'zz\tstray-targets-after-run\t%s\t0\t%s\n' "$([ -z "$STRAY" ] && echo 1 || echo 0)" "$(printf '%s' "$STRAY" | wc -w | tr -d ' ') native windows opened by tab/popup rows" >> "$RESULTS"
  run open "$SITE/"
else
  run close
fi
echo "results appended to $RESULTS"
