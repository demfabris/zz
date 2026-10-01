import json
import sys

events, start, end = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
steps = 0
kinds = {}
usage = {}
for line in open(events, errors="replace"):
    try:
        event = json.loads(line)
    except ValueError:
        continue
    if event.get("type") == "item.completed":
        kind = (event.get("item") or {}).get("type", "?")
        kinds[kind] = kinds.get(kind, 0) + 1
        steps += 1
    if event.get("type") == "turn.completed":
        for key, value in (event.get("usage") or {}).items():
            usage[key] = usage.get(key, 0) + value
print(json.dumps({"wall_min": round((end - start) / 60, 1), "steps": steps, "items": kinds, "usage": usage}))
