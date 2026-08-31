#!/usr/bin/env python3
"""Dispatch board client for the tmux compatibility campaign.

The board lives in the comments of one GitHub issue. Comments are append-only
and GitHub assigns them monotonically increasing ids, so the comment stream is
a total order every session can replay to the same state. This tool replays it.

State rules the fold enforces:
- an edited comment is void (created_at and updated_at differ);
- the lowest comment id wins any conflicting claim;
- a lease expires by wall clock and frees the front and its zones;
- INTEGRATED, REPAIR, and REJECTED are valid only from the current MAIN holder.

Environment:
  ZZ_BOARD_REPO    repository, default demfabris/zz
  ZZ_BOARD_ISSUE   issue number, default 7
  ZZ_BOARD_HOLDER  holder identity, e.g. mbp/session-name (required to post)

Requires the `gh` CLI authenticated for the repository.
"""

import argparse
import json
import os
import re
import subprocess
import sys
import time
from datetime import datetime, timedelta, timezone

REPO = os.environ.get("ZZ_BOARD_REPO", "demfabris/zz")
ISSUE = int(os.environ.get("ZZ_BOARD_ISSUE", "7"))

MAX_LEASE = timedelta(hours=24)
DEFAULT_LEASE = "6h"
EDIT_GRACE = timedelta(seconds=1)

ZONES = {
    "mux-command": ["crates/zz-mux/src/command.rs"],
    "mux-model": [
        "crates/zz-mux/src/model.rs",
        "crates/zz-mux/src/layout.rs",
        "crates/zz-mux/src/sort.rs",
    ],
    "mux-formats": [
        "crates/zz-mux/src/formats.rs",
        "crates/zz-mux/src/status.rs",
    ],
    "mux-options": [
        "crates/zz-mux/src/tmux_options.rs",
        "crates/zz-mux/src/honest_knobs.rs",
    ],
    "config-parser": ["crates/zz-mux/src/parser.rs"],
    "daemon-core": ["crates/zz-daemon/src/daemon.rs"],
    "daemon-status": ["crates/zz-daemon/src/status.rs"],
    "control-client": ["crates/zz/src/control_mode.rs"],
    "client-core": ["crates/zz-client/src/"],
    "raw-tui": ["crates/zz-tui/src/"],
    "terminal-engine": ["crates/zz-terminal/src/"],
    "protocol-message": [
        "crates/zz-protocol/src/message.rs",
        "crates/zz-protocol/src/snapshot.rs",
        "crates/zz-protocol/src/lib.rs",
    ],
    "protocol-key": ["crates/zz-protocol/src/key.rs"],
    "protocol-catalog": ["crates/zz-protocol/src/catalog.rs"],
    "desktop-gpui": ["crates/zz/src/ (control_mode.rs belongs to control-client)"],
    "main-lock": [],
    "triage-lock": [],
}

VERBS = {
    "FRONT",
    "WITHDRAW",
    "CLAIM",
    "RENEW",
    "RELEASE",
    "CANDIDATE",
    "INTEGRATED",
    "REPAIR",
    "REJECTED",
    "RESIDUAL",
    "NOTE",
}

LIST_FIELDS = {"paths", "proof", "residuals", "gate", "changed-paths"}


def parse_time(s):
    return datetime.strptime(s, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc)


def parse_lease(s):
    m = re.fullmatch(r"(\d+)([hm]?)", s.strip())
    if not m:
        return None
    n, unit = int(m.group(1)), m.group(2) or "h"
    delta = timedelta(hours=n) if unit == "h" else timedelta(minutes=n)
    return min(delta, MAX_LEASE)


def parse_comment(body):
    """Return (verb, arg, fields) or None when the comment is not board input."""
    lines = body.replace("\r\n", "\n").split("\n")
    lines = [l for l in lines if not l.strip().startswith("```")]
    while lines and not lines[0].strip():
        lines.pop(0)
    if not lines:
        return None
    head = lines[0].strip().split()
    if not head or head[0] not in VERBS:
        return None
    verb = head[0]
    arg = head[1] if len(head) > 1 else None
    fields = {}
    current_list = None
    for line in lines[1:]:
        raw = line.rstrip()
        if not raw.strip():
            current_list = None
            continue
        m = re.match(r"^([a-z][a-z-]*):\s*(.*)$", raw)
        if m:
            key, value = m.group(1), m.group(2).strip()
            if key in LIST_FIELDS and value == "":
                fields[key] = []
                current_list = fields[key]
            else:
                fields[key] = value
                current_list = None
            continue
        if raw.startswith("- ") and current_list is not None:
            current_list.append(raw[2:].strip())
            continue
        break
    return verb, arg, fields


class Front:
    def __init__(self, fid, fields, comment_id):
        self.id = fid
        self.kind = fields.get("kind", "work")
        self.priority = int(fields.get("priority", "5"))
        self.contract = fields.get("contract", "")
        self.zones = [z.strip() for z in fields.get("zones", "").split(",") if z.strip()]
        self.paths = fields.get("paths", []) if isinstance(fields.get("paths"), list) else []
        self.deps = [d.strip() for d in fields.get("deps", "").split(",") if d.strip() and d.strip() != "none"]
        self.notes = fields.get("notes", "")
        self.defined_in = comment_id
        self.state = "READY"
        self.holder = None
        self.branch = None
        self.base = None
        self.claim_id = None
        self.expiry = None
        self.candidate = None
        self.merge = None

    def free(self):
        self.state = "STALE-CANDIDATE" if self.candidate else "READY"
        self.holder = None
        self.branch = None
        self.base = None
        self.claim_id = None
        self.expiry = None

    def active(self, asof):
        return (
            self.holder is not None
            and self.expiry is not None
            and self.expiry > asof
        )


class Board:
    def __init__(self):
        self.fronts = {}
        self.residuals = []
        self.notes_log = []
        self.records = []
        self.warnings = []

    def expire(self, asof):
        for f in self.fronts.values():
            if f.holder is not None and not f.active(asof):
                self.warnings.append(f"lease expired on {f.id} (held by {f.holder})")
                f.free()

    def zones_busy(self, front, asof):
        """Return conflicts between `front` and every actively claimed front."""
        conflicts = []
        for other in self.fronts.values():
            if other.id == front.id or not other.active(asof):
                continue
            shared = set(front.zones) & set(other.zones)
            shared |= set(front.paths) & set(other.paths)
            if shared:
                conflicts.append((other.id, other.holder, sorted(shared)))
        return conflicts

    def main_holder(self, asof):
        main = self.fronts.get("MAIN")
        if main is not None and main.active(asof):
            return main.holder
        return None

    def deps_met(self, front):
        return all(
            self.fronts.get(d) is not None and self.fronts[d].state == "INTEGRATED"
            for d in front.deps
        )

    def deps_broken(self, front):
        return [
            d
            for d in front.deps
            if self.fronts.get(d) is None or self.fronts[d].state == "WITHDRAWN"
        ]

    def apply(self, comment):
        cid = comment["id"]
        created = parse_time(comment["created_at"])
        updated = parse_time(comment["updated_at"])
        if updated - created > EDIT_GRACE:
            self.warnings.append(f"comment {cid} is edited and void")
            return
        parsed = parse_comment(comment["body"])
        if parsed is None:
            return
        verb, arg, fields = parsed
        self.expire(created)

        if verb == "NOTE":
            self.notes_log.append(
                {
                    "comment": cid,
                    "front": arg or "none",
                    "holder": fields.get("holder", "unattributed"),
                    "note": fields.get("note", ""),
                }
            )
            return
        if verb == "RESIDUAL":
            self.residuals.append(
                {
                    "comment": cid,
                    "front": fields.get("front", arg or "none"),
                    "holder": fields.get("holder", "unattributed"),
                    "note": fields.get("note", ""),
                }
            )
            return

        if verb == "FRONT":
            if not arg:
                self.warnings.append(f"comment {cid}: FRONT without an id")
            elif arg in self.fronts:
                self.warnings.append(f"comment {cid}: duplicate front {arg} ignored")
            else:
                front = Front(arg, fields, cid)
                unknown = [z for z in front.zones if z not in ZONES]
                if unknown:
                    self.warnings.append(f"comment {cid}: front {arg} names unknown zones {unknown}")
                self.fronts[arg] = front
            return

        front = self.fronts.get(arg) if arg else None
        if front is None:
            self.warnings.append(f"comment {cid}: {verb} for unknown front {arg}")
            return
        holder = fields.get("holder", "")

        if verb == "WITHDRAW":
            triage = self.fronts.get("TRIAGE")
            triage_holder = triage.holder if triage is not None and triage.active(created) else None
            if not holder or holder != triage_holder:
                self.warnings.append(f"comment {cid}: WITHDRAW on {arg} ignored (poster does not hold TRIAGE)")
                return
            if front.state in ("READY", "STALE-CANDIDATE"):
                front.state = "WITHDRAWN"
            else:
                self.warnings.append(f"comment {cid}: WITHDRAW on {arg} in state {front.state} ignored")
            return

        if verb == "CLAIM":
            if front.state not in ("READY", "STALE-CANDIDATE"):
                self.warnings.append(f"comment {cid}: claim on {arg} lost (state {front.state})")
                return
            if front.kind == "work" and not self.deps_met(front):
                self.warnings.append(f"comment {cid}: claim on {arg} invalid (deps not integrated)")
                return
            conflicts = self.zones_busy(front, created)
            if conflicts:
                self.warnings.append(f"comment {cid}: claim on {arg} lost (zones held: {conflicts})")
                return
            lease = parse_lease(fields.get("lease", DEFAULT_LEASE))
            if lease is None:
                self.warnings.append(f"comment {cid}: claim on {arg} has a bad lease and is void")
                return
            front.state = "CLAIMED" if front.candidate is None else "CANDIDATE"
            front.holder = holder
            front.branch = fields.get("branch")
            front.base = fields.get("base")
            front.claim_id = cid
            front.expiry = created + lease
            return

        if verb == "RENEW":
            if not front.active(created) or front.holder != holder:
                self.warnings.append(f"comment {cid}: renew on {arg} invalid (not the active holder)")
                return
            lease = parse_lease(fields.get("lease", DEFAULT_LEASE))
            if lease is None:
                self.warnings.append(f"comment {cid}: renew on {arg} has a bad lease and is ignored")
                return
            added = [z.strip() for z in fields.get("zones", "").split(",") if z.strip()]
            if added:
                probe = Front(front.id, {}, cid)
                probe.zones = added
                probe.paths = []
                conflicts = self.zones_busy(probe, created)
                if conflicts:
                    self.warnings.append(f"comment {cid}: zone expansion on {arg} refused: {conflicts}")
                else:
                    front.zones = sorted(set(front.zones) | set(added))
            front.expiry = created + lease
            return

        if verb == "RELEASE":
            if front.holder != holder:
                self.warnings.append(f"comment {cid}: release on {arg} from non-holder ignored")
                return
            front.free()
            return

        if verb == "CANDIDATE":
            if not front.active(created) or front.holder != holder:
                self.warnings.append(f"comment {cid}: candidate on {arg} invalid (claim not active)")
                return
            front.state = "CANDIDATE"
            front.candidate = {
                "comment": cid,
                "holder": holder,
                "base": fields.get("base"),
                "commit": fields.get("commit"),
                "branch": fields.get("branch", front.branch),
                "proof": fields.get("proof", []),
                "residuals": fields.get("residuals", []),
            }
            return

        if verb in ("INTEGRATED", "REPAIR", "REJECTED"):
            main_holder = self.main_holder(created)
            if main_holder is None or holder != main_holder:
                self.warnings.append(f"comment {cid}: {verb} on {arg} invalid (poster does not hold MAIN)")
                return
            if arg == "MAIN":
                if verb == "INTEGRATED":
                    self.records.append(
                        {
                            "comment": cid,
                            "holder": holder,
                            "merge": fields.get("merge"),
                            "gate": fields.get("gate", []),
                        }
                    )
                else:
                    self.warnings.append(f"comment {cid}: {verb} cannot target MAIN")
                return
            if verb == "INTEGRATED":
                front.state = "INTEGRATED"
                front.merge = fields.get("merge")
                front.holder = None
                front.expiry = None
                return
            if verb == "REPAIR":
                front.candidate = None
                if front.active(created):
                    front.state = "CLAIMED"
                else:
                    front.free()
                return
            if verb == "REJECTED":
                front.candidate = None
                front.free()
                return


def gh(*args, capture=True):
    result = subprocess.run(["gh", *args], capture_output=capture, text=True)
    if result.returncode != 0:
        sys.exit(f"gh {' '.join(args)} failed:\n{result.stderr if capture else ''}")
    return result.stdout if capture else ""


def fetch_comments(args):
    if args.comments_json:
        with open(args.comments_json) as fh:
            return json.load(fh)
    out = gh(
        "api",
        "--paginate",
        f"repos/{REPO}/issues/{args.issue}/comments?per_page=100",
        "--jq",
        '.[] | {id, body, created_at, updated_at, author: .user.login}',
    )
    return [json.loads(line) for line in out.splitlines() if line.strip()]


def fold(args):
    board = Board()
    for comment in sorted(fetch_comments(args), key=lambda c: c["id"]):
        board.apply(comment)
    board.expire(datetime.now(timezone.utc))
    return board


def post(args, body):
    out = gh(
        "api",
        f"repos/{REPO}/issues/{args.issue}/comments",
        "-f",
        f"body={body}",
        "--jq",
        ".id",
    )
    return int(out.strip())


def holder_or_die(args):
    holder = args.holder or os.environ.get("ZZ_BOARD_HOLDER")
    if not holder:
        sys.exit("set ZZ_BOARD_HOLDER or pass --holder (e.g. mbp/blue-heron)")
    return holder


def origin_main_sha():
    result = subprocess.run(
        ["git", "ls-remote", "origin", "refs/heads/main"],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0 or not result.stdout.strip():
        return None
    return result.stdout.split()[0]


def block(verb, arg, pairs, lists=None):
    lines = [f"{verb} {arg}" if arg else verb]
    for key, value in pairs:
        if value is not None and value != "":
            lines.append(f"{key}: {value}")
    for key, items in (lists or []):
        if items:
            lines.append(f"{key}:")
            lines.extend(f"- {item}" for item in items)
    return "\n".join(lines)


def front_row(board, front, asof):
    zones = ",".join(front.zones)
    if front.holder:
        left = front.expiry - asof
        mins = int(left.total_seconds() // 60)
        who = f"{front.holder} ({mins}m left)"
    else:
        who = "-"
    busy = board.zones_busy(front, asof) if front.state in ("READY", "STALE-CANDIDATE") else []
    deps = ""
    if not board.deps_met(front):
        broken = board.deps_broken(front)
        waiting = [d for d in front.deps if d not in broken]
        if broken:
            deps += f" deps-broken:{','.join(broken)}"
        if waiting:
            deps += f" deps-wait:{','.join(waiting)}"
    blocked = f" zones-busy:{','.join(c[0] for c in busy)}" if busy else ""
    return f"{front.state:<16} p{front.priority} {front.id:<28} {who:<40} [{zones}]{deps}{blocked}"


def pickable(board, asof):
    out = []
    for front in board.fronts.values():
        if front.kind != "work" or front.state not in ("READY", "STALE-CANDIDATE"):
            continue
        if not board.deps_met(front):
            continue
        if board.zones_busy(front, asof):
            continue
        out.append(front)
    out.sort(key=lambda f: (f.priority, f.defined_in))
    return out


def cmd_status(args):
    board = fold(args)
    now = datetime.now(timezone.utc)
    if args.json:
        payload = {
            "fronts": {
                f.id: {
                    "state": f.state,
                    "kind": f.kind,
                    "priority": f.priority,
                    "contract": f.contract,
                    "zones": f.zones,
                    "paths": f.paths,
                    "deps": f.deps,
                    "holder": f.holder,
                    "branch": f.branch,
                    "base": f.base,
                    "expiry": f.expiry.isoformat() if f.expiry else None,
                    "candidate": f.candidate,
                    "merge": f.merge,
                    "notes": f.notes,
                }
                for f in board.fronts.values()
            },
            "residuals": board.residuals,
            "notes": board.notes_log,
            "records": board.records,
            "warnings": board.warnings,
        }
        print(json.dumps(payload, indent=2))
        return
    order = {"CANDIDATE": 0, "STALE-CANDIDATE": 1, "CLAIMED": 2, "READY": 3, "INTEGRATED": 4, "WITHDRAWN": 5}
    fronts = sorted(board.fronts.values(), key=lambda f: (f.kind != "lock", order.get(f.state, 9), f.priority, f.defined_in))
    for front in fronts:
        print(front_row(board, front, now))
    if board.residuals:
        print("\nresiduals:")
        for r in board.residuals:
            print(f"- [{r['front']}] {r['note']} ({r.get('holder', 'unattributed')}, comment {r['comment']})")
    if board.notes_log:
        print("\nnotes:")
        for n in board.notes_log:
            print(f"- [{n['front']}] {n['note']} ({n['holder']}, comment {n['comment']})")
    if board.records:
        print("\nrecords-only merges:")
        for r in board.records:
            print(f"- {r['merge'] or 'unknown'} by {r['holder']} (comment {r['comment']})")
    if args.verbose and board.warnings:
        print("\nfold notes:")
        for w in board.warnings:
            print(f"- {w}")
    ready = pickable(board, now)
    print(f"\nclaimable now: {', '.join(f.id for f in ready) if ready else 'none'}")


def cmd_pick(args):
    board = fold(args)
    ready = pickable(board, datetime.now(timezone.utc))
    if not ready:
        print("NOTHING-CLAIMABLE")
        return
    front = ready[0]
    print(front.id)
    print(f"contract: {front.contract}")
    print(f"zones: {','.join(front.zones)}")
    for p in front.paths:
        print(f"path: {p}")
    if front.notes:
        print(f"notes: {front.notes}")
    if front.candidate:
        print(f"stale-candidate: branch {front.candidate['branch']} commit {front.candidate['commit']} (adopt and finish it)")


def adjudicate(args, front_id, holder, comment_id):
    time.sleep(3)
    board = fold(args)
    front = board.fronts.get(front_id)
    if front and front.holder == holder and front.claim_id == comment_id:
        print(f"WON {front_id} (comment {comment_id}, lease until {front.expiry.isoformat()})")
        return 0
    who = front.holder if front else "unknown"
    print(f"LOST {front_id} (held by {who}); pick another front")
    return 1


def cmd_claim(args):
    holder = holder_or_die(args)
    if parse_lease(args.lease) is None:
        sys.exit(f"bad lease {args.lease!r}: use forms like 90m or 2h")
    board = fold(args)
    front = board.fronts.get(args.front)
    if front is None:
        sys.exit(f"unknown front {args.front}")
    now = datetime.now(timezone.utc)
    if front.state not in ("READY", "STALE-CANDIDATE"):
        sys.exit(f"{args.front} is {front.state}, not claimable")
    if front.kind == "work" and not board.deps_met(front):
        sys.exit(f"{args.front} has unintegrated deps: {front.deps}")
    conflicts = board.zones_busy(front, now)
    if conflicts:
        sys.exit(f"zones busy: {conflicts}")
    base = args.base or origin_main_sha()
    if base is None and front.kind == "work":
        sys.exit("cannot resolve origin/main; pass --base")
    branch = args.branch or (f"campaign/{args.front.lower()}" if front.kind == "work" else None)
    body = block(
        "CLAIM",
        args.front,
        [("holder", holder), ("base", base), ("branch", branch), ("lease", args.lease)],
    )
    cid = post(args, body)
    sys.exit(adjudicate(args, args.front, holder, cid))


def cmd_simple_post(verb):
    def run(args):
        holder = holder_or_die(args)
        if verb == "RENEW" and parse_lease(args.lease) is None:
            sys.exit(f"bad lease {args.lease!r}: use forms like 90m or 2h")
        pairs = [("holder", holder)]
        lists = []
        if verb == "RENEW":
            pairs.append(("lease", args.lease))
            if args.zones:
                pairs.append(("zones", args.zones))
        if verb == "RELEASE":
            pairs.append(("reason", args.reason))
        if verb == "CANDIDATE":
            pairs += [("base", args.base), ("commit", args.commit), ("branch", args.branch)]
            lists = [("proof", args.proof or []), ("residuals", args.residual or ["none"])]
        if verb == "INTEGRATED":
            pairs.append(("merge", args.merge))
            lists = [("gate", args.gate or [])]
        if verb in ("REPAIR", "REJECTED"):
            pairs.append(("reason", args.reason))
        cid = post(args, block(verb, args.front, pairs, lists))
        print(f"posted {verb} {args.front} (comment {cid})")

    return run


def cmd_front(args):
    pairs = [
        ("kind", args.kind if args.kind != "work" else None),
        ("priority", str(args.priority)),
        ("contract", args.contract),
        ("zones", args.zones),
        ("deps", args.deps),
        ("notes", args.notes),
    ]
    lists = [("paths", args.path or [])]
    cid = post(args, block("FRONT", args.front, pairs, lists))
    print(f"posted FRONT {args.front} (comment {cid})")


def cmd_residual(args):
    holder = args.holder or os.environ.get("ZZ_BOARD_HOLDER")
    cid = post(
        args,
        block("RESIDUAL", None, [("front", args.front or "none"), ("holder", holder), ("note", args.note)]),
    )
    print(f"posted RESIDUAL (comment {cid})")


def cmd_note(args):
    holder = holder_or_die(args)
    cid = post(args, block("NOTE", args.front, [("holder", holder), ("note", args.note)]))
    print(f"posted NOTE {args.front} (comment {cid})")


def cmd_withdraw(args):
    holder = holder_or_die(args)
    board = fold(args)
    front = board.fronts.get(args.front)
    if front is None:
        sys.exit(f"unknown front {args.front}")
    if front.state not in ("READY", "STALE-CANDIDATE"):
        sys.exit(
            f"{args.front} is {front.state}; only READY or STALE-CANDIDATE fronts can be "
            "withdrawn (release the claim or wait for expiry, then re-post)"
        )
    now = datetime.now(timezone.utc)
    triage = board.fronts.get("TRIAGE")
    if triage is None or not triage.active(now) or triage.holder != holder:
        sys.exit("withdraw requires holding TRIAGE")
    cid = post(args, block("WITHDRAW", args.front, [("holder", holder), ("reason", args.reason)]))
    time.sleep(3)
    verify = fold(args).fronts.get(args.front)
    if verify is not None and verify.state == "WITHDRAWN":
        print(f"WITHDRAWN {args.front} (comment {cid})")
    else:
        state = verify.state if verify else "unknown"
        sys.exit(
            f"withdraw posted (comment {cid}) but did not land: {args.front} is {state}; "
            "re-check the fold and re-post once withdrawable"
        )


def cmd_zones(args):
    for zone, paths in ZONES.items():
        print(f"{zone}: {', '.join(paths) if paths else '(virtual)'}")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--issue", type=int, default=ISSUE)
    parser.add_argument("--holder", default=None)
    parser.add_argument("--comments-json", default=None, help="offline fixture instead of gh")
    sub = parser.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("status", help="replay the board and print every front")
    p.add_argument("--json", action="store_true")
    p.add_argument("--verbose", action="store_true")
    p.set_defaults(fn=cmd_status)

    p = sub.add_parser("pick", help="print the front this session should claim")
    p.set_defaults(fn=cmd_pick)

    p = sub.add_parser("claim", help="post a claim and adjudicate the race")
    p.add_argument("front")
    p.add_argument("--lease", default=DEFAULT_LEASE)
    p.add_argument("--branch", default=None)
    p.add_argument("--base", default=None)
    p.set_defaults(fn=cmd_claim)

    p = sub.add_parser("renew", help="extend an active lease, optionally adding zones")
    p.add_argument("front")
    p.add_argument("--lease", default=DEFAULT_LEASE)
    p.add_argument("--zones", default=None, help="comma-separated zones to add")
    p.set_defaults(fn=cmd_simple_post("RENEW"))

    p = sub.add_parser("release", help="give a front back")
    p.add_argument("front")
    p.add_argument("--reason", required=True)
    p.set_defaults(fn=cmd_simple_post("RELEASE"))

    p = sub.add_parser("candidate", help="hand off a finished commit for integration")
    p.add_argument("front")
    p.add_argument("--commit", required=True)
    p.add_argument("--branch", required=True)
    p.add_argument("--base", required=True)
    p.add_argument("--proof", action="append")
    p.add_argument("--residual", action="append")
    p.set_defaults(fn=cmd_simple_post("CANDIDATE"))

    p = sub.add_parser("integrated", help="record a merge to main (requires holding MAIN)")
    p.add_argument("front")
    p.add_argument("--merge", required=True)
    p.add_argument("--gate", action="append")
    p.set_defaults(fn=cmd_simple_post("INTEGRATED"))

    p = sub.add_parser("repair", help="send a candidate back to its holder (requires MAIN)")
    p.add_argument("front")
    p.add_argument("--reason", required=True)
    p.set_defaults(fn=cmd_simple_post("REPAIR"))

    p = sub.add_parser("rejected", help="dissolve a candidate back to READY (requires MAIN)")
    p.add_argument("front")
    p.add_argument("--reason", required=True)
    p.set_defaults(fn=cmd_simple_post("REJECTED"))

    p = sub.add_parser("front", help="define a new front (hold TRIAGE first)")
    p.add_argument("front")
    p.add_argument("--contract", required=True)
    p.add_argument("--zones", required=True)
    p.add_argument("--priority", type=int, default=5)
    p.add_argument("--kind", choices=["work", "lock"], default="work")
    p.add_argument("--deps", default=None)
    p.add_argument("--path", action="append", help="extra reserved path, repeatable")
    p.add_argument("--notes", default=None)
    p.set_defaults(fn=cmd_front)

    p = sub.add_parser("residual", help="report a discovered gap without expanding your front")
    p.add_argument("--front", default=None)
    p.add_argument("--note", required=True)
    p.set_defaults(fn=cmd_residual)

    p = sub.add_parser("note", help="annotate a front, e.g. a candidate review verdict")
    p.add_argument("front")
    p.add_argument("--note", required=True)
    p.set_defaults(fn=cmd_note)

    p = sub.add_parser("withdraw", help="dissolve a READY front that stopped making sense (hold TRIAGE first)")
    p.add_argument("front")
    p.add_argument("--reason", required=True)
    p.set_defaults(fn=cmd_withdraw)

    p = sub.add_parser("zones", help="print the zone to path map")
    p.set_defaults(fn=cmd_zones)

    args = parser.parse_args()
    args.fn(args)


if __name__ == "__main__":
    main()
