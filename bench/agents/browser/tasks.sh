#!/usr/bin/env bash
# tasks.sh MODEL zz|own TASK  - one agent-driven browser task through the Antigravity CLI (agy)
# against a zz browser pane over CDP or agent-browser's own Chromium. Appends a row to out/tasks.tsv.
set -uo pipefail
MODEL=$1 BACKEND=$2 TASK=$3
HERE=$(cd "$(dirname "$0")" && pwd)
SITE=${SITE:-http://127.0.0.1:4173}
CDP_PORT=${CDP_PORT:-9222}
AB=${AGENT_BROWSER:-$(command -v agent-browser || echo "npx -y agent-browser@0.37.1")}
now() { if [ -n "${EPOCHREALTIME:-}" ]; then printf "%s" "$EPOCHREALTIME"; else perl -MTime::HiRes=time -e 'printf "%.6f", time'; fi; }
id=$(printf '%s' "$MODEL-$BACKEND-$TASK" | tr -c 'a-zA-Z0-9.-' '_')
RUN=$HERE/out/tasks/$id; rm -rf "$RUN"; mkdir -p "$RUN/bin" "$RUN/ws"
SESSION=bench-task-$$
case $BACKEND in
  zz) FLAGS="--cdp $CDP_PORT --session $SESSION" ;;
  own) FLAGS="--session $SESSION" ;;
  *) echo "unknown backend $BACKEND" >&2; exit 2 ;;
esac
read -r -a ABV <<<"$AB"
cat > "$RUN/bin/agent-browser" <<WRAP
#!/usr/bin/env bash
printf '%s\n' "\$*" >> "$RUN/ab.log"
${ABV[*]} "\$@" 2> >(tee -a "$RUN/ab.err" >&2)
rc=\$?
printf 'rc=%s\n' "\$rc" >> "$RUN/ab.log"
exit \$rc
WRAP
chmod +x "$RUN/bin/agent-browser"
unset TMUX
if [ "$BACKEND" = zz ]; then
  TARGET=$(curl -s "http://127.0.0.1:$CDP_PORT/json/list" | jq -r --arg s "$SITE" '[.[] | select(.type=="page" and (.url|startswith($s)))][0].id // empty')
  [ -n "$TARGET" ] || { echo "no zz browser pane on $SITE (run matrix.sh zz once, or zz new-browser -n bench $SITE/)" >&2; exit 2; }
  BEFORE=$(curl -s "http://127.0.0.1:$CDP_PORT/json/list" | jq -r '.[] | select(.type=="page") | .id' | sort)
  "${ABV[@]}" --cdp "$CDP_PORT" --session "$SESSION" tab "$TARGET" >/dev/null 2>&1
fi
"${ABV[@]}" $FLAGS open "$SITE/" >/dev/null 2>&1
case $TASK in
  b1) PROMPT="What is the price of the Cedar monitor listed on the current page? Reply with the number only."; EXPECT='389' ;;
  b2) PROMPT="Open the Form page, fill the order form with name Ada, email a@b.c, role Engineer, newsletter checked, plan Pro, bio hi, submit it, and reply with only the confirmation code shown."; EXPECT='CONF-f3a81e31' ;;
  b3) PROMPT="Open the Table page, move to the last page of the table, and reply with only the name shown in the last row."; EXPECT='Zeta' ;;
  b4) PROMPT="Open the Delayed page, wait until the banner appears, and reply with only the token it contains (it starts with TOKEN-)."; EXPECT='TOKEN-7f3a9c' ;;
  b5) PROMPT="Open the Hover page, reveal the hidden link under Menu by hovering it, follow that link, and reply with only the paragraph text on the page you reach."; EXPECT='reached via hover' ;;
  b6) PROMPT="Open the Dialogs page, click the Confirm button, accept the dialog, and reply with only the result text the page shows."; EXPECT='confirmed: true' ;;
  b7) PROMPT="Open the Popup page, follow the link that opens the child page in a new tab, and reply with only the title of that child page."; EXPECT='Popup Child' ;;
  b8) PROMPT="Open the Shadow page, click the button inside the widget, and reply with only the text the widget shows after the click."; EXPECT='shadow clicked' ;;
  b9) PROMPT="Open the Console page and reply with only the text of the console error message that page logs."; EXPECT='bench-error-1' ;;
  b10) PROMPT="Open the Download page, download report.csv, and reply with only the number of data rows in the file, not counting the header."; EXPECT='3' ;;
  *) echo "unknown task $TASK" >&2; exit 2 ;;
esac
FULL="You control a web browser through the agent-browser CLI, which is on PATH. Every call must be spelled exactly as: agent-browser $FLAGS <command> ... (keep those flags on every call). The browser is already open on $SITE/ and that site has a navigation bar. Do not open new tabs or windows unless following a link that opens one is part of the task. If you need the command reference, run: agent-browser $FLAGS skills get core --full. Complete the task, then reply exactly as asked.

Task: $PROMPT"
printf '%s\n' "$FULL" > "$RUN/prompt.txt"
start=$(now)
( cd "$RUN/ws" && PATH="$RUN/bin:$PATH" timeout 260 agy -p "$FULL" --model "$MODEL" --dangerously-skip-permissions --output-format json --print-timeout 240s > "$RUN/agy.json" 2> "$RUN/agy.err" )
wall=$(awk -v s="$start" -v e="$(now)" 'BEGIN{printf "%.0f", e-s}')
resp=$(jq -r '.response // ""' "$RUN/agy.json" 2>/dev/null); printf '%s\n' "$resp" > "$RUN/response.txt"
turns=$(jq -r '.num_turns // 0' "$RUN/agy.json" 2>/dev/null)
calls=$(grep -c -v '^rc=' "$RUN/ab.log" 2>/dev/null); calls=${calls:-0}; fails=$(grep -c '^rc=[1-9]' "$RUN/ab.log" 2>/dev/null); fails=${fails:-0}
pass=0; grep -qiF -- "$EXPECT" <<<"$resp" && pass=1
if [ "$BACKEND" = zz ]; then
  AFTER=$(curl -s "http://127.0.0.1:$CDP_PORT/json/list" | jq -r '.[] | select(.type=="page") | .id' | sort)
  for t in $(comm -13 <(printf '%s\n' "$BEFORE") <(printf '%s\n' "$AFTER")); do curl -s "http://127.0.0.1:$CDP_PORT/json/close/$t" >/dev/null; done
  "${ABV[@]}" --cdp "$CDP_PORT" --session "$SESSION" tab "$TARGET" >/dev/null 2>&1; "${ABV[@]}" --cdp "$CDP_PORT" --session "$SESSION" open "$SITE/" >/dev/null 2>&1
else
  "${ABV[@]}" --session "$SESSION" close >/dev/null 2>&1
fi
snippet=$(tr '\n\t' '  ' <<<"$resp" | cut -c1-90)
mkdir -p "$HERE/out"; printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$MODEL" "$BACKEND" "$TASK" "$pass" "$wall" "$turns" "$calls" "$fails" "$snippet" >> "$HERE/out/tasks.tsv"
printf '%-24s %-4s %-4s pass=%s %4ss turns=%s calls=%s fail=%s | %s\n' "$MODEL" "$BACKEND" "$TASK" "$pass" "$wall" "$turns" "$calls" "$fails" "$snippet"
