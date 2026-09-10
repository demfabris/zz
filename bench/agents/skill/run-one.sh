#!/usr/bin/env bash
# run-one.sh MODEL ARM TASK  (ARM = none | skill | inline)
set -uo pipefail
MODEL=$1 ARM=$2 TASK=$3
S=$(cd "$(dirname "$0")" && pwd)
REAL=${ZZ_BINARY:-$(command -v zz)}
SKILL=$(cd "$S/../../.." && pwd)/.agents/skills/zz-workspace/SKILL.md
id=$(printf '%s' "$MODEL-$ARM-$TASK" | tr -c 'a-zA-Z0-9.-' '_')
RUN=$S/out/runs/$id; rm -rf "$RUN"; mkdir -p "$RUN/ws" "$RUN/bin"; mkdir -p "$S/out"
export ZZ_SOCKET=/tmp/zze-$$-$RANDOM.sock
unset TMUX
Z() { "$REAL" "$@"; }
cat > "$RUN/bin/zz" <<WRAP
#!/usr/bin/env bash
printf '%s %s\n' "\$(basename "\$0")" "\$*" >> "$RUN/zz.log"
"$REAL" "\$@" 2> >(tee -a "$RUN/zz.err" >&2)
rc=\$?
printf 'rc=%s\n' "\$rc" >> "$RUN/zz.log"
exit \$rc
WRAP
chmod +x "$RUN/bin/zz"; ln -s "$RUN/bin/zz" "$RUN/bin/tmux"
ADD_DIR=""
case $ARM in
  skill) mkdir -p "$RUN/ws/.agents/skills/zz-workspace"; cp "$SKILL" "$RUN/ws/.agents/skills/zz-workspace/SKILL.md"; ADD_DIR="--add-dir $RUN/ws" ;;
  inline|none) ;;
  *) echo "unknown arm $ARM" >&2; exit 2 ;;
esac
Z kill-server 2>/dev/null
P0=$(Z new-session -d -P -F '#{pane_id}' -s work -n main -x 160 -y 48 -c "$RUN/ws") || { echo "daemon start failed" >&2; exit 2; }
P1=$(Z split-window -d -P -F '#{pane_id}' -t "$P0" -c "$RUN/ws")
P2=$(Z new-window -d -P -F '#{pane_id}' -t work -n jobs -c "$RUN/ws")
sleep 2
TOKEN=$(od -An -N4 -tx1 /dev/urandom | tr -d ' \n')
Z send-keys -t "$P1" 'echo 4711' Enter
Z send-keys -t "$P2" "sleep 12; echo DONE-$TOKEN" Enter
sleep 1
TMO=150
case $TASK in
  t1) PROMPT="Run the command \`touch $RUN/t1.ok\` inside pane $P1 (not in your own shell), wait until the file exists, then reply with the word done." ;;
  t2) PROMPT="Pane $P1 recently ran a command that printed a single number. Read pane $P1 to find it and reply with only that number." ;;
  t3) PROMPT="Create a new window named build that runs the command \`sleep 300\`. Reply with the word done." ;;
  t4) PROMPT="Pane $P2 is running a job that prints one line starting with DONE- when it finishes. Wait for it to finish and reply with only that line." ;;
  t5) PROMPT="Split pane $P1 so that a new pane appears to its right, then run \`pwd\` in the new pane. The new pane must still be open when you reply. Reply with the word done." ;;
  t6) PROMPT="Set the pane option @name to reviewer on pane $P1. Reply with the word done." ;;
  t7) PROMPT="Close the window named jobs. Reply with the word done." ;;
  t8) PROMPT="Open a new zz agent pane (an AI agent pane, not a terminal) and send it this prompt: Reply with exactly the word pong. Wait for its answer and reply with only the agent's answer."; TMO=300 ;;
  t9) PROMPT="Run \`ls /definitely-missing-dir-zz\` inside pane $P1 and reply with only the exact error line it printed." ;;
  *) echo "unknown task $TASK" >&2; exit 2 ;;
esac
PRE="You are working inside a zz terminal multiplexer workspace. zz is tmux-compatible; the \`zz\` CLI is on PATH and already targets the right server. Your own pane is $P0 in session \"work\". Complete the task with shell commands, then reply exactly as asked."
FULL="$PRE

Task: $PROMPT"
if [ "$ARM" = inline ]; then FULL="$FULL

The following skill describes the zz-specific commands available to you:

$(awk 'BEGIN{c=0} /^---$/{c++; next} c>=2' "$SKILL")"; fi
printf '%s\n' "$FULL" > "$RUN/prompt.txt"
now() { if [ -n "${EPOCHREALTIME:-}" ]; then printf "%s" "$EPOCHREALTIME"; else perl -MTime::HiRes=time -e 'printf "%.6f", time'; fi; }
start=$(now)
( cd "$RUN/ws" && ZZ_PANE=$P0 ZZ_SESSION=work PATH="$RUN/bin:$PATH" timeout $((TMO+20)) agy -p "$FULL" $ADD_DIR --model "$MODEL" --dangerously-skip-permissions --output-format json --print-timeout "${TMO}s" > "$RUN/agy.json" 2> "$RUN/agy.err" )
wall=$(awk -v s="$start" -v e="$(now)" 'BEGIN{printf "%.0f", e-s}')
resp=$(jq -r '.response // ""' "$RUN/agy.json" 2>/dev/null); printf '%s\n' "$resp" > "$RUN/response.txt"
turns=$(jq -r '.num_turns // 0' "$RUN/agy.json" 2>/dev/null); itok=$(jq -r '.usage.input_tokens // 0' "$RUN/agy.json" 2>/dev/null); otok=$(jq -r '.usage.output_tokens // 0' "$RUN/agy.json" 2>/dev/null)
calls=$(rg -c -v '^rc=' "$RUN/zz.log" 2>/dev/null || echo 0); fails=$(rg -c '^rc=[1-9]' "$RUN/zz.log" 2>/dev/null || echo 0)
pass=0
case $TASK in
  t1) [ -f "$RUN/t1.ok" ] && Z capture-pane -pJ -t "$P1" | rg -q 'touch .*t1\.ok' && pass=1 ;;
  t2) rg -q '4711' <<<"$resp" && pass=1 ;;
  t3) Z list-windows -t work -F '#{window_name} #{pane_current_command}' | rg -q '^build sleep' && pass=1 ;;
  t4) rg -q "DONE-$TOKEN" <<<"$resp" && pass=1 ;;
  t5) [ "$(Z list-panes -t work:main | wc -l | tr -d ' ')" = 3 ] && pass=1 ;;
  t6) [ "$(Z show-options -pqv -t "$P1" @name)" = reviewer ] && pass=1 ;;
  t7) ! Z list-windows -t work -F '#{window_name}' | rg -qx 'jobs' && pass=1 ;;
  t8) Z list-panes -a -F '#{pane_kind}' | rg -qx agent && sed -E 's/(exactly )?the word pong//Ig' <<<"$resp" | rg -qi 'pong' && pass=1 ;;
  t9) rg -q 'No such file or directory' <<<"$resp" && pass=1 ;;
esac
Z kill-server 2>/dev/null; rm -f "$ZZ_SOCKET"
snippet=$(tr '\n\t' '  ' <<<"$resp" | cut -c1-90)
printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$MODEL" "$ARM" "$TASK" "$pass" "$wall" "$turns" "$calls" "$fails" "$itok" "$otok" "$snippet" >> "$S/out/results.tsv"
printf '%-24s %-6s %s pass=%s %ss turns=%s zz=%s fail=%s | %s\n' "$MODEL" "$ARM" "$TASK" "$pass" "$wall" "$turns" "$calls" "$fails" "$snippet"
