import argparse
import json
import re
import sys
from collections import Counter
from datetime import date
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
MANIFEST = Path("compat/tui/campaign.json")
REPORT = Path("knowledge/tmux/tui-parity.md")
STATUSES = ("unmeasured", "different", "active", "review", "blocked", "verified")
DESCRIPTION = "TUI parity obligations, their proof status, and progress against the fixed baseline."


def require(condition, message):
    if not condition:
        raise ValueError(message)


def text(value, label):
    require(isinstance(value, str) and bool(value.strip()), f"{label} must be nonempty text")


def strings(value, label, nonempty=False):
    require(isinstance(value, list), f"{label} must be a list")
    require(not nonempty or bool(value), f"{label} must not be empty")
    for entry in value:
        text(entry, label)
    require(len(value) == len(set(value)), f"{label} contains duplicates")
    return value


def fields(value, expected, label):
    require(isinstance(value, dict), f"{label} must be an object")
    require(set(value) == set(expected.split()), f"{label} has missing or unknown fields")


def commit(value, label):
    require(isinstance(value, str) and re.fullmatch(r"[0-9a-f]{40}", value), f"{label} must be a full commit hash")


def file_path(root, value, label):
    text(value, label)
    path = Path(value)
    require(not path.is_absolute() and ".." not in path.parts and path.as_posix() == value, f"{label} must be a relative repository path")
    require((root / path).resolve().is_relative_to(root.resolve()), f"{label} leaves the repository")
    require((root / path).is_file(), f"{label} references a missing file: {value}")


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def validate(root, data, check_report=False):
    fields(data, "schema_version title updated_on tmux_commit baseline milestones items", "campaign")
    require(type(data["schema_version"]) is int and data["schema_version"] == 1, "schema_version must be 1")
    text(data["title"], "title")
    require(isinstance(data["updated_on"], str) and date.fromisoformat(data["updated_on"]).isoformat() == data["updated_on"], "updated_on must be YYYY-MM-DD")
    commit(data["tmux_commit"], "tmux_commit")
    oracle = read_json(root / "compat/tmux-oracle.json")
    require(data["tmux_commit"] == oracle["pin"], "tmux_commit differs from the oracle pin")
    registry = read_json(root / "compat/tmux-gaps.json")
    gap_ids = {gap["id"] for gap in registry["gaps"] + registry["closed"]}
    baseline = strings(data["baseline"], "baseline", nonempty=True)
    require(isinstance(data["milestones"], list) and data["milestones"], "milestones must be a nonempty list")
    milestones = set()
    for milestone in data["milestones"]:
        fields(milestone, "id title", "milestone")
        text(milestone["id"], "milestone.id")
        text(milestone["title"], "milestone.title")
        require(milestone["id"] not in milestones, "duplicate milestone id")
        milestones.add(milestone["id"])
    require(isinstance(data["items"], list), "items must be a list")
    items = {}
    for item in data["items"]:
        fields(item, "id title milestone priority status depends_on tmux_gaps sources acceptance evidence_note next_action proof", "item")
        item_id = item["id"]
        require(isinstance(item_id, str) and re.fullmatch(r"TUI-[0-9]{3,}", item_id), "item.id must have the form TUI-001")
        require(item_id not in items, f"duplicate item id: {item_id}")
        items[item_id] = item
        for name in ("title", "milestone", "evidence_note", "next_action"):
            text(item[name], f"{item_id}.{name}")
        require(item["milestone"] in milestones, f"{item_id} references an unknown milestone")
        require(type(item["priority"]) is int and item["priority"] >= 0, f"{item_id}.priority must be a nonnegative integer")
        require(item["status"] in STATUSES, f"{item_id} has an unknown status")
        strings(item["depends_on"], f"{item_id}.depends_on")
        strings(item["acceptance"], f"{item_id}.acceptance", nonempty=True)
        for gap in strings(item["tmux_gaps"], f"{item_id}.tmux_gaps"):
            require(gap in gap_ids, f"{item_id} references unknown tmux gap: {gap}")
        for source in strings(item["sources"], f"{item_id}.sources", nonempty=True):
            file_path(root, source, f"{item_id}.sources")
        proof = item["proof"]
        require(item["status"] != "verified" or proof is not None, f"{item_id}: verified requires proof")
        if proof is not None:
            fields(proof, "revision tmux_commit environment commands artifacts review", f"{item_id}.proof")
            commit(proof["revision"], f"{item_id}.proof.revision")
            require(proof["tmux_commit"] == data["tmux_commit"], f"{item_id}.proof pin differs from campaign")
            text(proof["environment"], f"{item_id}.proof.environment")
            strings(proof["commands"], f"{item_id}.proof.commands", nonempty=True)
            for artifact in strings(proof["artifacts"], f"{item_id}.proof.artifacts", nonempty=True):
                file_path(root, artifact, f"{item_id}.proof.artifacts")
            file_path(root, proof["review"], f"{item_id}.proof.review")
    require(set(baseline) <= items.keys(), "baseline obligations must be retained in items")
    for item_id, item in items.items():
        require(set(item["depends_on"]) <= items.keys(), f"{item_id} references an unknown dependency")
        if item["status"] == "verified":
            require(all(items[dep]["status"] == "verified" for dep in item["depends_on"]), f"{item_id}: verified requires verified dependencies")
    visiting, visited = set(), set()

    def visit(item_id):
        require(item_id not in visiting, f"dependency cycle reaches {item_id}")
        if item_id in visited:
            return
        visiting.add(item_id)
        for dependency in items[item_id]["depends_on"]:
            visit(dependency)
        visiting.remove(item_id)
        visited.add(item_id)

    for item_id in items:
        visit(item_id)
    if check_report:
        path = root / REPORT
        require(path.is_file() and path.read_text(encoding="utf-8") == render_report(data), f"{REPORT} is stale; run write-report")


def progress(data):
    baseline = set(data["baseline"])
    verified = {item["id"] for item in data["items"] if item["status"] == "verified"}
    return {
        "verified": len(baseline & verified),
        "baseline": len(baseline),
        "added": len(data["items"]) - len(baseline),
        "added_verified": len(verified - baseline),
        "statuses": dict(Counter(item["status"] for item in data["items"])),
    }


def ready_items(data):
    verified = {item["id"] for item in data["items"] if item["status"] == "verified"}
    return sorted(
        (item for item in data["items"] if item["status"] not in ("verified", "blocked") and set(item["depends_on"]) <= verified),
        key=lambda item: (item["priority"], item["id"]),
    )


def cell(value):
    return str(value).replace("|", "\\|").replace("\n", " ")


def render_report(data):
    counts = progress(data)
    ready = ", ".join(item["id"] for item in ready_items(data)) or "none"
    lines = [
        "---", "type: Reference", f"title: {json.dumps(data['title'])}",
        f"description: {json.dumps(DESCRIPTION)}",
        "resource: compat/tui/campaign.json", "tags: [tmux, tui, compatibility, campaign]",
        f"timestamp: {data['updated_on']}T00:00:00Z", "---", "", f"# {data['title']}", "",
        "Generated by `python3 compat/tui/tracker.py write-report`. Edit `compat/tui/campaign.json`.", "",
        f"Tmux reference: `{data['tmux_commit']}`.", "",
        "The contract compares CLI stdout, stderr and exit codes exactly, and rendered cells, styles, cursor, geometry and interactions. Raw terminal escape streams are outside the contract.", "",
        "Existing tmux gap decisions remain in `compat/tmux-gaps.json`. Accepted or closed source gaps do not establish TUI parity. Only obligations with verified proof count as complete.", "",
        f"Fixed baseline: **{counts['verified']}/{counts['baseline']} verified**. Added scope: **{counts['added_verified']}/{counts['added']} verified**.", "",
        "Status counts: " + ", ".join(f"{status}: {counts['statuses'].get(status, 0)}" for status in STATUSES) + ".", "",
        f"Dependency-ready obligations, by priority: {ready}.",
    ]
    for milestone in data["milestones"]:
        lines.extend(["", f"## {milestone['id']}: {milestone['title']}", "", "| Obligation | Status | Priority | Dependencies |", "| --- | --- | ---: | --- |"])
        for item in data["items"]:
            if item["milestone"] == milestone["id"]:
                lines.append("| " + " | ".join(cell(value) for value in (f"{item['id']}: {item['title']}", item["status"], item["priority"], ", ".join(item["depends_on"]) or "none")) + " |")
    lines.extend(["", "## Obligation details", ""])
    for item in data["items"]:
        lines.extend([f"### {item['id']}: {item['title']}", "", f"Status: {item['status']}.", "", "Acceptance:", ""])
        lines.extend(f"- {criterion}" for criterion in item["acceptance"])
        lines.extend(["", "Sources:", ""])
        lines.extend(f"- `{source}`" for source in item["sources"])
        lines.extend(["", "Tmux gap references: " + (", ".join(f"`{gap}`" for gap in item["tmux_gaps"]) or "none") + ".", "", item["evidence_note"], "", f"Next action: {item['next_action']}", ""])
        proof = item["proof"]
        if proof:
            lines.extend([f"Proof revision: `{proof['revision']}`. Tmux: `{proof['tmux_commit']}`.", "", f"Environment: {proof['environment']}", "", "Proof commands:", ""])
            lines.extend(f"- `{command}`" for command in proof["commands"])
            lines.extend(["", "Artifacts: " + ", ".join(f"`{path}`" for path in proof["artifacts"]) + ".", "", f"Review: `{proof['review']}`.", ""])
    return "\n".join(lines).rstrip("\n") + "\n"


def main(argv=None, root=ROOT):
    parser = argparse.ArgumentParser(description="Validate the TUI parity ledger and generate its report.")
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("check")
    commands.add_parser("write-report")
    commands.add_parser("ready")
    args = parser.parse_args(argv)
    try:
        data = read_json(root / MANIFEST)
        validate(root, data, check_report=args.command == "check")
        if args.command == "check":
            print(f"{MANIFEST} is valid and {REPORT} is current")
        elif args.command == "write-report":
            (root / REPORT).parent.mkdir(parents=True, exist_ok=True)
            (root / REPORT).write_text(render_report(data), encoding="utf-8")
            print(f"Wrote {REPORT}")
        else:
            ready = ready_items(data)
            print("\n".join(f"{item['id']} p{item['priority']} {item['status']}: {item['title']}" for item in ready) if ready else "No ready obligation.")
        return 0
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
