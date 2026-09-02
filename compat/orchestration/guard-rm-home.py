import json
import re
import sys

try:
    data = json.load(sys.stdin)
except Exception:
    sys.exit(0)
if data.get("tool_name") != "Bash":
    sys.exit(0)
command = (data.get("tool_input") or {}).get("command") or ""
pattern = re.compile(r'\brm\s+(?:-\S+\s+)*(?:"?\$\{?HOME\}?"?|~)(?=\s|;|&|\||$)')
if not pattern.search(command):
    sys.exit(0)
reason = (
    "Blocked by a local hook: 'rm -rf $HOME' (or ~) trips Claude Code's critical-path guard and "
    "prompts the user even when HOME was re-exported to a scratch path a line earlier. Put the "
    "scratch directory in a plain variable and delete that instead: "
    "D=/tmp/<name>; rm -rf \"$D\"; mkdir -p \"$D\"; export HOME=\"$D\". Then rerun the command."
)
print(json.dumps({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": reason}}))
