#!/usr/bin/env python3
"""Catch-up campaign ledger.

    ledger.py status            every item with its state
    ledger.py ready             todo items whose deps are merged
    ledger.py show ID           one item in full
    ledger.py live              branches of active and review items
    ledger.py set ID STATUS [--branch B] [--sha S] [--note TEXT]
    ledger.py check             validate the ledger
"""
import argparse
import datetime
import json
import sys
from pathlib import Path

PATH = Path(__file__).with_name("ledger.json")
STATUSES = ["todo", "active", "review", "merged", "deferred", "dropped"]


def load():
    return json.loads(PATH.read_text(encoding="utf-8"))


def save(data):
    PATH.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def by_id(data):
    return {item["id"]: item for item in data["items"]}


def ready(data):
    items = by_id(data)
    return [i for i in data["items"]
            if i["status"] == "todo" and all(items[d]["status"] == "merged" for d in i["deps"])]


def problems(data):
    out = []
    items = by_id(data)
    if len(items) != len(data["items"]):
        out.append("duplicate ids")
    for item in data["items"]:
        if item["status"] not in STATUSES:
            out.append(f"{item['id']}: bad status {item['status']}")
        for dep in item["deps"]:
            if dep not in items:
                out.append(f"{item['id']}: unknown dep {dep}")
        if item["status"] in ("active", "review") and not item["branch"]:
            out.append(f"{item['id']}: {item['status']} without a branch")
        if item["status"] == "merged" and not item["sha"]:
            out.append(f"{item['id']}: merged without a sha")
    seen, stack = set(), set()

    def visit(node):
        if node in stack:
            out.append(f"dependency cycle through {node}")
            return
        if node in seen or node not in items:
            return
        stack.add(node)
        for dep in items[node]["deps"]:
            visit(dep)
        stack.discard(node)
        seen.add(node)

    for node in items:
        visit(node)
    return out


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="cmd", required=True)
    sub.add_parser("status")
    sub.add_parser("ready")
    sub.add_parser("check")
    sub.add_parser("live")
    show = sub.add_parser("show")
    show.add_argument("id")
    setp = sub.add_parser("set")
    setp.add_argument("id")
    setp.add_argument("status", choices=STATUSES)
    setp.add_argument("--branch")
    setp.add_argument("--sha")
    setp.add_argument("--note")
    args = parser.parse_args()
    data = load()

    if args.cmd == "status":
        for item in data["items"]:
            extra = item["branch"] or ""
            if item["sha"]:
                extra = f"{extra} {item['sha']}".strip()
            print(f"{item['status']:<9} {item['id']:<22} {item['title']}  {extra}".rstrip())
    elif args.cmd == "ready":
        for item in ready(data):
            print(f"{item['id']:<22} {item['title']}")
    elif args.cmd == "live":
        for item in data["items"]:
            if item["status"] in ("active", "review") and item["branch"]:
                print(item["branch"])
    elif args.cmd == "show":
        print(json.dumps(by_id(data)[args.id], indent=2, ensure_ascii=False))
    elif args.cmd == "set":
        item = by_id(data)[args.id]
        item["status"] = args.status
        if args.branch is not None:
            item["branch"] = args.branch or None
        if args.sha is not None:
            item["sha"] = args.sha or None
        if args.note:
            item["notes"].append(f"{datetime.date.today().isoformat()}: {args.note}")
        found = problems(data)
        if found:
            sys.exit("\n".join(found))
        save(data)
    elif args.cmd == "check":
        found = problems(data)
        if found:
            sys.exit("\n".join(found))
        print(f"ok: {len(data['items'])} items")


if __name__ == "__main__":
    main()
