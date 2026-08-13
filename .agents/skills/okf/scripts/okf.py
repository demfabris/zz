#!/usr/bin/env python3
"""okf.py — toolkit for Open Knowledge Format (OKF v0.1) bundles.

A bundle is a directory tree of markdown files with YAML frontmatter.
The ONLY hard conformance rule: every non-reserved .md file has parseable
YAML frontmatter containing a non-empty `type` field. Everything else is
soft guidance that this tool surfaces as warnings, never errors.

Zero hard dependencies. Uses PyYAML when importable; otherwise a built-in
parser for the OKF frontmatter subset (flat keys, scalars, inline/block
lists, folded continuations). The fallback is *strict-reject*: it raises on
anything ambiguous or unparseable, so the "parseable YAML" hard rule is
enforced identically whether or not PyYAML is installed — conformance never
depends on `pip list`. Frontmatter is always emitted through one
parser-independent serializer that quotes scalars only when needed, so the
output round-trips cleanly through both parsers.

Authoring (producer) subcommands:
    init     scaffold a new bundle (root index.md, optional okf_version)
    new      create a concept document with correct frontmatter + headings
    index    (re)generate index.md listings (sentinel-fenced, idempotent)
    log      append a dated entry to a log.md (newest first)
    validate check conformance (errors) and spec-strict conventions (warnings)

Reading (consumer) subcommands — gather context during AI workflows:
    list     enumerate concepts from frontmatter (progressive disclosure)
    search   rank concepts by text match (frontmatter weighted over body)
    show     print a concept by id; --links shows outbound links + backlinks
    context  assemble a budgeted context pack: seeds + linked neighbors
    render   pack the bundle into one HTML site (Mermaid needs network)

Run `okf.py <subcommand> -h` for details.
"""
from __future__ import annotations

import argparse
import datetime as _dt
import json
import re
import sys
from pathlib import Path

RESERVED = {"index.md", "log.md"}
# Repo furniture that is not OKF knowledge; validate skips it instead of erroring.
IGNORED = {"readme.md", "contributing.md", "license.md", "changelog.md",
           "code_of_conduct.md", "security.md", "authors.md", "notice.md"}
FM_RE = re.compile(r"^---\n(.*?)\n---\n?(.*)$", re.DOTALL)
LINK_RE = re.compile(r"\[[^\]]*\]\(([^)]+)\)")
LISTING_START = "<!-- okf:listing:start (managed by okf.py index — edit prose outside this fence) -->"
LISTING_END = "<!-- okf:listing:end -->"
_LISTING_BLOCK_RE = re.compile(
    re.escape("<!-- okf:listing:start") + r".*?" + re.escape(LISTING_END), re.DOTALL
)
# Exact listing headings, used only to migrate pre-sentinel index files once.
LEGACY_HEADING_RE = re.compile(r"^#[ \t]+(Subdirectories|Concepts)[ \t]*$", re.MULTILINE)
_YAML_MAGIC = {"true", "false", "yes", "no", "on", "off", "null", "none", "~", ""}


# ---------------------------------------------------------------------------
# YAML: one safe emitter + a strict parser (PyYAML or built-in fallback)
# ---------------------------------------------------------------------------

def _needs_quote(s: str) -> bool:
    """True when a bare scalar would be misread as YAML structure/bool/number."""
    if s != s.strip() or s.lower() in _YAML_MAGIC:
        return True
    if s[0] in "!&*?|>%@`\"'#,[]{}-":
        return True
    if ": " in s or s.endswith(":") or " #" in s:
        return True
    try:
        float(s)  # bare number would parse as int/float, not str
        return True
    except ValueError:
        return False


def _emit_scalar(v) -> str:
    s = str(v)
    if _needs_quote(s):
        return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'
    return s


try:  # pragma: no cover - which branch runs depends on the environment
    import yaml  # type: ignore

    def load_yaml(block: str) -> dict:
        data = yaml.safe_load(block)
        return data if isinstance(data, dict) else {}
except Exception:  # noqa: BLE001 - any import failure means use the fallback

    def _unquote(v: str):
        if v and v[0] in "\"'":
            q = v[0]
            if len(v) >= 2 and v[-1] == q:
                inner = v[1:-1]
                return inner.replace('\\"', '"').replace("\\\\", "\\") if q == '"' else inner
            raise ValueError(f"unterminated quote: {v!r}")
        return None

    def _scalar(v: str):
        v = v.strip()
        unq = _unquote(v)
        if unq is not None:
            return unq
        hashpos = v.find(" #")  # strip YAML inline comment, like PyYAML does
        if hashpos != -1:
            v = v[:hashpos].rstrip()
        if ": " in v or v.endswith(":"):
            raise ValueError(f"ambiguous unquoted scalar (needs quotes): {v!r}")
        return v

    def load_yaml(block: str) -> dict:
        """Strict subset parser. Raises ValueError on anything it can't trust."""
        data: dict = {}
        cur = None  # last scalar key, for folded continuations
        for raw in block.split("\n"):
            if not raw.strip() or raw.lstrip().startswith("#"):
                continue
            stripped = raw.lstrip()
            if stripped.startswith("- "):  # block list item
                if cur is not None and isinstance(data.get(cur), list):
                    data[cur].append(_scalar(stripped[2:]))
                    continue
                raise ValueError(f"unexpected list item: {raw!r}")
            m = re.match(r"^([A-Za-z0-9_-]+):\s*(.*)$", raw)
            if m:
                key, val = m.group(1), m.group(2).strip()
                cur = key
                if val == "":
                    data[key] = []  # a block list (or empty value) follows
                elif val[0] in "|>":
                    raise ValueError(f"block scalars are unsupported: {raw!r}")
                elif val[0] == "[":
                    if not val.endswith("]"):
                        raise ValueError(f"unclosed inline list: {raw!r}")
                    inner = val[1:-1].strip()
                    data[key] = [_scalar(x) for x in inner.split(",")] if inner else []
                    cur = None
                else:
                    data[key] = _scalar(val)
            elif raw[:1] in (" ", "\t") and cur and isinstance(data.get(cur), str):
                data[cur] = (data[cur] + " " + stripped).strip()  # folded continuation
            else:
                raise ValueError(f"unparseable frontmatter line: {raw!r}")
        return data


# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------

def now_iso() -> str:
    """Current UTC time as a Z-suffixed ISO 8601 string."""
    return _dt.datetime.now(_dt.timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def today_iso() -> str:
    return _dt.date.today().isoformat()


def split_frontmatter(text: str):
    """Return (meta_dict_or_None, body, raw_block_or_None). CRLF-tolerant."""
    text = text.replace("\r\n", "\n")
    m = FM_RE.match(text)
    if not m:
        return None, text, None
    block, body = m.group(1), m.group(2)
    try:
        meta = load_yaml(block)
    except Exception:  # noqa: BLE001 - unparseable frontmatter -> not conformant
        return None, body, block
    return meta, body, block


def _parse_iso(s: str):
    """Parse an ISO 8601 timestamp to a tz-aware datetime (assume UTC if naive)."""
    s = (s or "").strip()
    if not s:
        return None
    try:
        dt = _dt.datetime.fromisoformat(s.replace("Z", "+00:00"))
    except ValueError:
        return None
    return dt if dt.tzinfo else dt.replace(tzinfo=_dt.timezone.utc)


def _declares_okf_version(index_path: Path) -> bool:
    try:
        meta, _, _ = split_frontmatter(index_path.read_text(encoding="utf-8"))
    except Exception:  # noqa: BLE001
        return False
    return bool(meta and meta.get("okf_version"))


def find_bundle_root(start: Path) -> Path:
    """Resolve the bundle root robustly.

    Every directory in a bundle gets an `index.md`, so "nearest index.md"
    finds the wrong root. Instead: climb the *contiguous* chain of dirs that
    have an `index.md` (the topmost is the root), stopping early at any dir
    whose index.md declares `okf_version` — the definitive root marker. If
    `start` is not inside a bundle, descend into the conventional `knowledge/`
    child when present, else fall back to `start`.
    """
    start = start.resolve()
    cur = start if start.is_dir() else start.parent
    candidate = None
    c = cur
    while (c / "index.md").exists():
        candidate = c
        if _declares_okf_version(c / "index.md") or c.parent == c:
            break
        c = c.parent
    if candidate is not None:
        return candidate
    if (cur / "knowledge" / "index.md").exists():
        return (cur / "knowledge").resolve()
    return cur


def dump_frontmatter(meta: dict) -> str:
    """Serialize frontmatter in OKF's recommended field order, always valid YAML."""
    order = ["okf_version", "type", "title", "description", "resource", "tags", "timestamp"]
    keys = [k for k in order if k in meta] + [k for k in meta if k not in order]
    lines = ["---"]
    for k in keys:
        v = meta[k]
        if isinstance(v, list):
            if v:
                lines.append(f"{k}:")
                lines.extend(f"- {_emit_scalar(item)}" for item in v)
            else:
                lines.append(f"{k}: []")
        else:
            lines.append(f"{k}: {_emit_scalar(v)}")
    lines.append("---")
    return "\n".join(lines)


# ---------------------------------------------------------------------------
# subcommands
# ---------------------------------------------------------------------------

def cmd_init(args) -> int:
    root = Path(args.dir).resolve()
    root.mkdir(parents=True, exist_ok=True)
    index = root / "index.md"
    if index.exists() and not args.force:
        print(f"refusing to overwrite existing {index} (use --force)", file=sys.stderr)
        return 1
    header = f"---\nokf_version: {_emit_scalar(args.okf_version)}\n---\n\n" if args.okf_version else ""
    title = args.title or root.name
    index.write_text(
        f"{header}# {title}\n\n"
        "_Knowledge bundle. Run `okf.py index` to (re)generate the listing below._\n",
        encoding="utf-8",
    )
    print(f"initialized OKF bundle at {root}")
    print(f"  created {index.relative_to(root.parent)}")
    return 0


# Dated concept dirs accumulate session outputs; past a budget the bundle
# needs a compression pass (docs-discipline playbook), not more files.
DATED_DIRS = ("learnings", "research", "plans", "specs")


def _dated_dir_counts(root: Path) -> dict:
    counts = {}
    for d in DATED_DIRS:
        p = root / d
        counts[d] = sum(1 for f in p.glob("*.md") if f.name not in RESERVED) if p.is_dir() else 0
    return counts


def _dated_dir_lines(root: Path) -> dict:
    """Content volume per dated dir. A file budget alone is blind: a dir can
    sit at its concept cap while holding the most content in the bundle
    (research/ was 25/25 'fine' at 195 lines/concept, 2026-07-21)."""
    lines = {}
    for d in DATED_DIRS:
        p = root / d
        n = 0
        if p.is_dir():
            for f in p.glob("*.md"):
                if f.name in RESERVED:
                    continue
                try:
                    n += len(f.read_text(encoding="utf-8").splitlines())
                except OSError:
                    pass
        lines[d] = n
    return lines


# Dated dirs hold roughly the last week; older concepts are drained into a
# durable owner or deleted (docs-discipline §The one-week rule). Warn-only,
# like the budgets: the point is to surface the drain backlog, never to block
# unrelated work.
DATED_MAX_AGE_DAYS = 7


def _over_age_dated(root: Path) -> dict:
    """Dated concepts older than DATED_MAX_AGE_DAYS, by dir. Age comes from the
    YYYY-MM-DD filename prefix, which is the dir's naming convention and is
    stable across checkouts (unlike mtime, which is clone time)."""
    today = _dt.date.today()
    out = {}
    for d in DATED_DIRS:
        p = root / d
        if not p.is_dir():
            continue
        stale = []
        for f in sorted(p.glob("*.md")):
            if f.name in RESERVED:
                continue
            m = re.match(r"^(\d{4})-(\d{2})-(\d{2})-", f.name)
            if not m:
                continue  # undated filename: the budgets still cover it
            try:
                when = _dt.date(int(m.group(1)), int(m.group(2)), int(m.group(3)))
            except ValueError:
                continue
            age = (today - when).days
            if age > DATED_MAX_AGE_DAYS:
                stale.append((age, f"{d}/{f.stem}"))
        if stale:
            out[d] = sorted(stale, reverse=True)
    return out


def _over_age_warnings(root: Path) -> list:
    over = _over_age_dated(root)
    return [
        f"{d}/: {len(items)} concept(s) older than {DATED_MAX_AGE_DAYS}d "
        f"(oldest {items[0][0]}d: {items[0][1]}) — drain into the owning concept or "
        f"delete (knowledge/playbooks/docs-discipline.md §The one-week rule)"
        for d, items in over.items()
    ]


# Warn-only: a fat dated dir signals a due compression pass, it must not
# block unrelated bundle edits. Two budgets because they fail differently —
# the count catches "too many notes", the line budget catches "notes that
# grew into essays". Either alone is gameable.
DATED_DIR_BUDGETS = {"learnings": 40, "research": 25, "plans": 15, "specs": 25}
DATED_DIR_LINE_BUDGETS = {"learnings": 3500, "research": 3500, "plans": 2500, "specs": 3500}

_COMPRESS_HINT = ("run a compression pass "
                  "(knowledge/playbooks/docs-discipline.md §Compressing the Bundle)")


def _dated_dir_budget_warnings(counts: dict, lines: dict | None = None) -> list:
    out = [
        f"{d}/: {counts[d]} concepts exceeds the {DATED_DIR_BUDGETS[d]}-concept budget — "
        + _COMPRESS_HINT
        for d in DATED_DIRS
        if counts.get(d, 0) > DATED_DIR_BUDGETS.get(d, float("inf"))
    ]
    if lines:
        out += [
            f"{d}/: {lines[d]} lines exceeds the {DATED_DIR_LINE_BUDGETS[d]}-line budget — "
            + _COMPRESS_HINT
            for d in DATED_DIRS
            if lines.get(d, 0) > DATED_DIR_LINE_BUDGETS.get(d, float("inf"))
        ]
    return out


def _closest_concepts_hint(root: Path, rel: str, title: str, description: str) -> None:
    """Creation-time nudge for dated dirs: surface the closest existing
    concepts so 'update in place' gets considered before a new file exists."""
    top = rel.split("/", 1)[0] if "/" in rel else ""
    if top not in DATED_DIRS:
        return
    stem = re.sub(r"^\d{4}-\d{2}-\d{2}-", "", Path(rel).stem).replace("-", " ")
    query = " ".join(x for x in (title or "", description or "", stem) if x).strip()
    if not query:
        return
    nodes, _ = _load_graph(root)
    own_id = rel[:-3] if rel.endswith(".md") else rel
    hits = [h for h in _search_hits(nodes, query) if h[1] != own_id][:3]
    if not hits:
        return
    print(f"note: closest existing concepts — if one already owns this mechanism,"
          f" update it in place instead of adding to {top}/ (docs-discipline):")
    for _score, cid, m, _snip in hits:
        print(f"  {cid}  [{str(m.get('type') or '').strip()}]  {str(m.get('title') or '').strip()}")


def cmd_new(args) -> int:
    root = find_bundle_root(Path(args.bundle) if args.bundle else Path.cwd())
    rel = args.path if args.path.endswith(".md") else args.path + ".md"
    if Path(rel).name in RESERVED:
        print(f"'{rel}' is a reserved filename; concepts cannot use it", file=sys.stderr)
        return 1
    dest = (root / rel).resolve()
    try:
        dest.relative_to(root.resolve())
    except ValueError:
        print(f"'{args.path}' resolves outside the bundle ({root}); refusing", file=sys.stderr)
        return 1
    if dest.exists() and not args.force:
        print(f"refusing to overwrite existing {dest} (use --force)", file=sys.stderr)
        return 1
    dest.parent.mkdir(parents=True, exist_ok=True)
    meta = {
        "type": args.type,
        "title": args.title or dest.stem.replace("-", " ").replace("_", " ").title(),
        "description": args.description or "TODO one-sentence summary.",
    }
    if args.resource:
        meta["resource"] = args.resource
    if args.tags:
        meta["tags"] = [t.strip() for t in args.tags.split(",") if t.strip()]
    meta["timestamp"] = now_iso()
    body = "\n\n# Overview\n\nTODO describe this concept.\n"
    if args.type.lower().endswith(("table", "dataset", "view", "schema")):
        body += (
            "\n# Schema\n\n"
            "| Column | Type | Description |\n"
            "|--------|------|-------------|\n"
            "| `TODO` | TODO | TODO |\n"
        )
    elif args.type.lower().endswith(("endpoint", "api")):
        body += "\n# Request\n\nTODO method, path, parameters.\n\n# Response\n\nTODO shape and fields.\n"
    dest.write_text(dump_frontmatter(meta) + body, encoding="utf-8")
    relid = dest.relative_to(root)
    print(f"created concept {relid} (id: {relid.with_suffix('')})")
    _closest_concepts_hint(root, relid.as_posix(), args.title or "", args.description or "")
    return 0


def _meta_of(path: Path) -> dict:
    meta, _, _ = split_frontmatter(path.read_text(encoding="utf-8"))
    return meta or {}


def _description_for(path: Path) -> str:
    m = _meta_of(path)
    return str(m.get("description") or m.get("title") or "").strip()


def _title_for(path: Path) -> str:
    m = _meta_of(path)
    if m.get("title"):
        return str(m["title"]).strip()
    return path.stem.replace("-", " ").replace("_", " ").title()


def _listing(directory: Path) -> str:
    """The machine-managed listing (Subdirectories + Concepts), no fence markers."""
    subdirs = sorted(
        p for p in directory.iterdir()
        if p.is_dir() and any(c.name not in RESERVED for c in p.rglob("*.md"))
    )
    concepts = sorted(
        p for p in directory.iterdir() if p.is_file() and p.suffix == ".md" and p.name not in RESERVED
    )
    out: list[str] = []
    if subdirs:
        out.append("# Subdirectories\n")
        out.extend(f"* [{d.name}]({d.name}/index.md)" for d in subdirs)
        out.append("")
    if concepts:
        out.append("# Concepts\n")
        for c in concepts:
            desc = _description_for(c)
            out.append(f"* [{_title_for(c)}]({c.name}){f' - {desc}' if desc else ''}")
        out.append("")
    return "\n".join(out).strip()


def _reassemble_index(existing_body: str, listing_block: str) -> str:
    """Replace the fenced listing, preserving all human prose above AND below it.

    First run (no fence) migrates a pre-sentinel file by dropping a trailing
    auto-listing that began at an *exact* `# Subdirectories`/`# Concepts`
    heading; any other prose is kept.
    """
    if LISTING_START.split(" (")[0] in existing_body and LISTING_END in existing_body:
        return _LISTING_BLOCK_RE.sub(lambda _m: listing_block, existing_body, count=1).strip() + "\n"
    m = LEGACY_HEADING_RE.search(existing_body)
    preamble = (existing_body[: m.start()] if m else existing_body).strip()
    return ("\n\n".join(p for p in (preamble, listing_block) if p)).strip() + "\n"


def cmd_index(args) -> int:
    root = find_bundle_root(Path(args.dir) if args.dir else Path.cwd())
    written = 0
    for directory in sorted({p.parent for p in root.rglob("*.md")} | {root}):
        is_root = directory.resolve() == root.resolve()
        listing = _listing(directory)
        if not listing:
            continue
        index = directory / "index.md"
        fm, body = "", ""
        if index.exists():
            meta, body, _ = split_frontmatter(index.read_text(encoding="utf-8"))
            if is_root and meta and meta.get("okf_version"):
                fm = f"---\nokf_version: {_emit_scalar(meta['okf_version'])}\n---\n\n"
        listing_block = f"{LISTING_START}\n{listing}\n{LISTING_END}"
        index.write_text(fm + _reassemble_index(body, listing_block), encoding="utf-8")
        written += 1
        print(f"  wrote {index.relative_to(root)}")
    print(f"regenerated {written} index.md file(s)")
    return 0


def cmd_log(args) -> int:
    root = find_bundle_root(Path(args.dir) if args.dir else Path.cwd())
    target = (root / args.dir) if args.dir and (root / args.dir).is_dir() else root
    log = target / "log.md"
    date = today_iso()
    entry = f"* **{args.kind or 'Update'}**: {args.message}"
    if not log.exists():
        log.write_text(f"# Update Log\n\n## {date}\n{entry}\n", encoding="utf-8")
        print(f"created {log.relative_to(root)}")
        return 0
    text = log.read_text(encoding="utf-8").replace("\r\n", "\n")
    head_re = re.compile(rf"^##[ \t]+{re.escape(date)}[ \t]*$", re.MULTILINE)
    if head_re.search(text):
        text = head_re.sub(lambda m: m.group(0).rstrip() + "\n" + entry, text, count=1)
    else:
        m = re.search(r"^(#[ \t].*\n)", text)
        head = m.group(1) if m else ""
        rest = text[len(head):].lstrip("\n")
        text = f"{head}\n## {date}\n{entry}\n\n{rest}"
    log.write_text(text, encoding="utf-8")
    print(f"appended to {log.relative_to(root)}")
    return 0


def cmd_validate(args) -> int:
    root = find_bundle_root(Path(args.dir) if args.dir else Path.cwd())
    errors: list[str] = []
    warnings: list[str] = []
    all_md = list(root.rglob("*.md"))

    for p in all_md:
        rel = p.relative_to(root).as_posix()
        meta, body, _ = split_frontmatter(p.read_text(encoding="utf-8"))

        if p.name == "index.md":
            is_root = p.parent.resolve() == root.resolve()
            if meta is not None and not (is_root and set(meta) <= {"okf_version"}):
                warnings.append(f"{rel}: index.md should have no frontmatter (root may declare only okf_version)")
            continue
        if p.name == "log.md":
            for heading in re.findall(r"^##\s+(.*)$", body or "", re.MULTILINE):
                if not re.match(r"^\d{4}-\d{2}-\d{2}$", heading.strip()):
                    warnings.append(f"{rel}: log date heading '{heading.strip()}' is not ISO 8601 YYYY-MM-DD")
            continue
        if p.name.lower() in IGNORED:
            continue  # repo furniture, not a knowledge concept

        # concept document — the hard rule lives here
        if meta is None:
            errors.append(f"{rel}: missing or unparseable YAML frontmatter")
            continue
        if not str(meta.get("type") or "").strip():
            errors.append(f"{rel}: frontmatter is missing a non-empty `type` field (the one hard rule)")
        for field in ("title", "description", "timestamp"):
            if not str(meta.get(field) or "").strip():
                warnings.append(f"{rel}: missing recommended `{field}`")

        # drift detection: does the cited source still exist, and is it newer than us?
        if getattr(args, "check_sources", False):
            res = str(meta.get("resource") or "").strip()
            if res and "://" not in res and not res.startswith("mailto:"):
                rp = Path(res) if res.startswith("/") else (root / res)
                if not rp.exists():
                    warnings.append(f"{rel}: resource not found -> {res} (source moved/deleted?)")
                else:
                    cdt = _parse_iso(str(meta.get("timestamp") or ""))
                    try:
                        src_m = _dt.datetime.fromtimestamp(rp.stat().st_mtime, _dt.timezone.utc)
                    except OSError:
                        src_m = None
                    # Compare at day granularity: concept timestamps are typically
                    # day-accurate (midnight UTC), so a same-day source edit is not
                    # drift — only a strictly later calendar day signals staleness.
                    if cdt and src_m and src_m.date() > cdt.date():
                        warnings.append(
                            f"{rel}: source modified {src_m.date()} after concept timestamp "
                            f"{cdt.date()} — may be stale, re-verify against {res}")

        # spec-strict: cross-links should be absolute and resolve within the bundle
        for target in LINK_RE.findall(body or ""):
            link = target.split("#", 1)[0].strip()
            if not link or link.startswith(("http://", "https://", "mailto:", "//")):
                continue
            if not link.endswith(".md"):
                continue
            if link.startswith("/"):
                tgt = root / link[1:]
            else:
                warnings.append(f"{rel}: relative cross-link '{link}' (spec-strict prefers absolute /...md)")
                tgt = p.parent / link
            if not tgt.exists():
                warnings.append(f"{rel}: broken link -> {link}")

    warnings.extend(_dated_dir_budget_warnings(_dated_dir_counts(root), _dated_dir_lines(root)))
    warnings.extend(_over_age_warnings(root))

    n = sum(1 for p in all_md if p.name not in RESERVED and p.name.lower() not in IGNORED)
    print(f"OKF validate: {n} concept(s), {len(all_md)} markdown file(s) under {root}")
    for w in warnings:
        print(f"  WARN  {w}")
    for e in errors:
        print(f"  ERROR {e}")
    if errors:
        print(f"\nNON-CONFORMANT: {len(errors)} error(s), {len(warnings)} warning(s)")
        return 1
    print(f"\nCONFORMANT (OKF v0.1): 0 errors, {len(warnings)} warning(s)")
    return 0


def cmd_due(args) -> int:
    """List dated dirs over their compression budget, link-isolated fold
    candidates first. Exit 1 when work is due (script-friendly, like a
    failing check).

    Ordering is (backlinks, timestamp): a concept nothing links to never got
    folded into its durable owner, which is exactly the fold this command is
    asking for. Oldest-first was the previous ordering and it aimed at the
    wrong targets — a 2026-07-21 pass found 7 of its top candidates were
    accurate and worth keeping, while genuinely isolated concepts ranked
    below the fold.
    """
    root = find_bundle_root(Path(args.dir) if args.dir else Path.cwd())
    counts = _dated_dir_counts(root)
    lines = _dated_dir_lines(root)
    nodes, backlinks = _load_graph(root)

    def _row(cid, node):
        t = (str(node["meta"].get("timestamp") or "undated") + " " * 10)[:10]
        bl = len(backlinks.get(cid, []))
        flag = "  ISOLATED" if bl == 0 else ""
        return f"  {t}  {cid}  ({bl} backlink{'s' if bl != 1 else ''}){flag}"

    def _sorted_dated(d):
        dated = [(cid, node) for cid, node in nodes.items() if cid.startswith(d + "/")]
        # Isolated first, then oldest. Missing timestamps sort as oldest.
        dated.sort(key=lambda it: (len(backlinks.get(it[0], [])),
                                   str(it[1]["meta"].get("timestamp") or "")))
        return dated

    if getattr(args, "orphans", False):
        total = 0
        for d in DATED_DIRS:
            iso = [(c, n) for c, n in _sorted_dated(d) if not backlinks.get(c)]
            if not iso:
                continue
            total += len(iso)
            print(f"{d}/: {len(iso)} link-isolated of {counts[d]}")
            for cid, node in iso:
                print(_row(cid, node))
        if not total:
            print("no link-isolated dated concepts: every one is reachable from the graph")
            return 0
        print("\nLink-isolated means no concept points here, so nobody navigating the"
              "\nbundle will find it. Either its durable owner should link it, or its"
              "\nclaim was never promoted and belongs in the owner instead."
              "\nZero backlinks is a CANDIDATE signal, not a verdict — check the content.")
        return 1

    over = {d: counts[d] - DATED_DIR_BUDGETS[d] for d in DATED_DIRS
            if counts.get(d, 0) > DATED_DIR_BUDGETS.get(d, float("inf"))}
    over_lines = {d for d in DATED_DIRS
                  if lines.get(d, 0) > DATED_DIR_LINE_BUDGETS.get(d, float("inf"))}
    if not over and not over_lines:
        print("nothing due: every dated dir is within its concept and line budgets")
        return 0

    for d in DATED_DIRS:
        n_over = over.get(d)
        if n_over is None and d not in over_lines:
            continue
        head = f"{d}/: {counts[d]} concepts (budget {DATED_DIR_BUDGETS[d]}), " \
               f"{lines[d]} lines (budget {DATED_DIR_LINE_BUDGETS[d]})"
        if n_over is not None:
            head += f" — fold at least {n_over}"
        else:
            head += " — over the LINE budget only: trim, or fold the fattest concepts"
        print(head)
        for cid, node in _sorted_dated(d)[: (n_over or 0) + 2]:
            print(_row(cid, node))
    print("\nfold workflow: docs-discipline §Compressing the Bundle — promote the durable"
          "\nclaim into the owning concept FIRST, then the dated file is free to delete."
          "\nISOLATED entries are the best candidates; see `okf.py due --orphans`.")
    return 1


# ---------------------------------------------------------------------------
# read path — discovery, retrieval, and context gathering for AI workflows
# ---------------------------------------------------------------------------

def _concept_files(root: Path):
    return sorted(
        p for p in root.rglob("*.md")
        if p.name not in RESERVED and p.name.lower() not in IGNORED
    )


def _concept_id(root: Path, path: Path) -> str:
    return path.relative_to(root).with_suffix("").as_posix()


def resolve_concept(root: Path, ident: str):
    """Map a concept-id (with or without .md, leading / optional) to its file."""
    ident = ident.strip().lstrip("/")
    for cand in (root / ident, root / (ident + ".md")):
        if cand.is_file() and cand.name not in RESERVED:
            return cand.resolve()
    return None


def _link_targets(root: Path, path: Path, body: str):
    """Internal concept-ids this concept links to (absolute or relative .md links)."""
    ids = []
    for raw in LINK_RE.findall(body or ""):
        link = raw.split("#", 1)[0].strip()
        if not link or link.startswith(("http://", "https://", "mailto:", "//")) or not link.endswith(".md"):
            continue
        tgt = (root / link[1:]) if link.startswith("/") else (path.parent / link)
        try:
            rid = tgt.resolve().relative_to(root.resolve()).with_suffix("").as_posix()
        except ValueError:
            continue
        if rid not in ids:
            ids.append(rid)
    return ids


def _load_graph(root: Path):
    """Return (nodes, backlinks). nodes[id] = {path, meta, body, out:[ids]}."""
    nodes: dict = {}
    for p in _concept_files(root):
        try:
            text = p.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            continue  # permissive: skip vanished/undecodable files, never reject the bundle
        meta, body, _ = split_frontmatter(text)
        cid = _concept_id(root, p)
        nodes[cid] = {"path": p, "meta": meta or {}, "body": body or "",
                      "out": _link_targets(root, p, body or "")}
    backlinks: dict = {cid: [] for cid in nodes}
    for cid, n in nodes.items():
        for t in n["out"]:
            if t in nodes and cid not in backlinks[t]:
                backlinks[t].append(cid)
    return nodes, backlinks


def _tags_of(meta: dict):
    t = meta.get("tags") or []
    return [str(x).lower() for x in (t if isinstance(t, list) else [t])]


def _matches_filters(meta: dict, type_filter, tag_filter) -> bool:
    if type_filter and type_filter.lower() not in str(meta.get("type") or "").lower():
        return False
    if tag_filter and tag_filter.lower() not in _tags_of(meta):
        return False
    return True


def _snippet(body: str, terms) -> str:
    flat = " ".join((body or "").split())
    low = flat.lower()
    pos = min((low.find(t) for t in terms if low.find(t) != -1), default=-1)
    if pos == -1:
        return flat[:160]
    start = max(0, pos - 60)
    return ("…" if start else "") + flat[start:start + 160] + ("…" if start + 160 < len(flat) else "")


def _search_hits(nodes: dict, query: str, type_filter=None, tag_filter=None):
    """Rank concepts. OR across terms (keep any concept matching >=1 term), so
    natural-language queries like 'AI usage limits' still hit. Frontmatter matches
    weighted over body, plus a coverage bonus so multi-term matches rank highest."""
    terms = [t for t in query.lower().split() if t]
    hits = []
    for cid in sorted(nodes):
        meta, body = nodes[cid]["meta"], nodes[cid]["body"]
        if not _matches_filters(meta, type_filter, tag_filter):
            continue
        fm = " ".join(str(meta.get(k) or "") for k in ("title", "description", "type"))
        fm = (fm + " " + " ".join(_tags_of(meta))).lower()
        bd = body.lower()
        matched = sum(1 for t in terms if t in fm or t in bd)
        if terms and matched == 0:
            continue
        score = sum(3 for t in terms if t in fm) + sum(1 for t in terms if t in bd) + matched
        hits.append((score, cid, meta, _snippet(body, terms or [""])))
    hits.sort(key=lambda h: (-h[0], h[1]))
    return hits


def cmd_list(args) -> int:
    root = find_bundle_root(Path(args.dir) if args.dir else Path.cwd())
    nodes, _ = _load_graph(root)
    rows = [(cid, nodes[cid]["meta"]) for cid in sorted(nodes)
            if _matches_filters(nodes[cid]["meta"], args.type, args.tag)]
    if args.json:
        print(json.dumps([
            {"id": cid, "type": m.get("type"), "title": m.get("title"),
             "description": m.get("description"), "tags": m.get("tags")}
            for cid, m in rows], indent=2))
        return 0
    for cid, m in rows:
        typ = str(m.get("type") or "").strip()
        title = str(m.get("title") or "").strip()
        desc = str(m.get("description") or "").strip()
        print(f"{cid}  [{typ}]  {title}{f' — {desc}' if desc else ''}")
    print(f"\n{len(rows)} concept(s) under {root}", file=sys.stderr)
    return 0


def cmd_search(args) -> int:
    root = find_bundle_root(Path(args.dir) if args.dir else Path.cwd())
    nodes, _ = _load_graph(root)
    hits = _search_hits(nodes, args.query, args.type, args.tag)[: args.limit]
    if args.json:
        print(json.dumps([
            {"id": cid, "score": score, "type": m.get("type"),
             "title": m.get("title"), "description": m.get("description"), "snippet": snip}
            for score, cid, m, snip in hits], indent=2))
        return 0
    if not hits:
        print(f"no concepts match {args.query!r}", file=sys.stderr)
        return 1
    for score, cid, m, snip in hits:
        print(f"{cid}  [{str(m.get('type') or '').strip()}]  {str(m.get('title') or '').strip()}")
        if m.get("description"):
            print(f"    {str(m['description']).strip()}")
        if snip:
            print(f"    … {snip}")
    return 0


def cmd_show(args) -> int:
    root = find_bundle_root(Path(args.dir) if args.dir else Path.cwd())
    path = resolve_concept(root, args.concept)
    if not path:
        print(f"no concept with id '{args.concept}' in {root}", file=sys.stderr)
        return 1
    text = path.read_text(encoding="utf-8")
    if args.frontmatter:
        meta, _, _ = split_frontmatter(text)
        print(dump_frontmatter(meta or {}))
    else:
        print(text.rstrip())
    if args.links:
        nodes, backlinks = _load_graph(root)
        cid = _concept_id(root, path)
        print("\n# Links")
        for t in nodes.get(cid, {}).get("out", []):
            print(f"-> {t}")
        for s in backlinks.get(cid, []):
            print(f"<- {s}")
    return 0


def cmd_context(args) -> int:
    root = find_bundle_root(Path(args.dir) if args.dir else Path.cwd())
    nodes, backlinks = _load_graph(root)
    seed_path = resolve_concept(root, args.query)
    if seed_path:
        seeds = [_concept_id(root, seed_path)]
    else:
        seeds = [cid for _, cid, _, _ in _search_hits(nodes, args.query, args.type, args.tag)][: args.seeds]
    if not seeds:
        print(f"no concepts match {args.query!r}", file=sys.stderr)
        return 1

    # BFS from seeds out to --depth, following links (and backlinks if asked)
    order, seen, frontier, depth = [], set(), list(seeds), 0
    while frontier and depth <= args.depth:
        nxt = []
        for cid in frontier:
            if cid in seen or cid not in nodes:
                continue
            seen.add(cid)
            order.append(cid)
            neighbors = list(nodes[cid]["out"]) + (backlinks.get(cid, []) if args.backlinks else [])
            nxt.extend(t for t in neighbors if t not in seen)
        frontier, depth = nxt, depth + 1

    used, emitted, truncated = 0, [], False
    for cid in order:
        m = nodes[cid]["meta"]
        header = f"## {cid}  [{str(m.get('type') or '').strip()}] — {str(m.get('title') or '').strip()}"
        chunk = header + "\n" + nodes[cid]["body"].strip() + "\n"
        if used + len(chunk) > args.budget and emitted:
            truncated = True
            break
        emitted.append(chunk)
        used += len(chunk)

    if args.json:
        print(json.dumps({
            "seeds": seeds,
            "concepts": [{"id": cid, "type": nodes[cid]["meta"].get("type"),
                          "title": nodes[cid]["meta"].get("title"),
                          "body": nodes[cid]["body"].strip(),
                          "links": nodes[cid]["out"]} for cid in order[: len(emitted)]],
            "truncated": truncated, "omitted": len(order) - len(emitted)}, indent=2))
        return 0
    print(f"<!-- OKF context: {len(emitted)} concept(s) from seed(s) {', '.join(seeds)}, depth {args.depth} -->\n")
    print("\n\n".join(emitted).rstrip())
    if truncated:
        print(f"\n<!-- truncated at {args.budget} chars; {len(order) - len(emitted)} more concept(s) omitted — "
              f"raise --budget or narrow the query -->", file=sys.stderr)
    return 0


# ---------------------------------------------------------------------------
# render — pack the bundle into one HTML site
# ---------------------------------------------------------------------------

def _tags_display(meta: dict):
    """Original-case tag list (unlike _tags_of, which lowercases for matching)."""
    t = meta.get("tags") or []
    return [str(x) for x in (t if isinstance(t, list) else [t])]


def _bundle_title(root: Path) -> str:
    """The bundle's display title: first `# heading` of the root index.md, else dir name."""
    idx = root / "index.md"
    if idx.exists():
        try:
            _, body, _ = split_frontmatter(idx.read_text(encoding="utf-8"))
        except OSError:
            body = ""
        m = re.search(r"^#\s+(.+)$", body or "", re.MULTILINE)
        if m:
            return m.group(1).strip()
    return root.name


# One HTML file with the app shell and bundle data inline. Mermaid is the sole
# network dependency: a version-pinned CDN script protected by SHA-384 SRI.
# The bundle's data is injected verbatim as a JSON island (__OKF_DATA__); the
# client renders markdown, routes on the URL hash, and draws the link graph.
_SITE_TEMPLATE = r"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>__OKF_TITLE__</title>
<style>
/* rustdoc palette, verbatim from rust-lang/rust rustdoc.css themes:
   light = "light" theme; dark = "ayu" base with kind colors from ayu+dark.
   OKF kinds map to rust item kinds and share colors like rustdoc's long
   tail does: system,plan=mod · playbook=fn · reference,spec=struct ·
   project,research=trait · learning,report=macro/fn-green. */
:root{
  --bg:#ffffff; --fg:#000; --muted:#666; --line:#e0e0e0; --panel:#f5f5f5;
  --accent:#3873ad; --code-bg:#f5f5f5; --chip:#e0e0e0; --focus:#66afe9;
  --k-subsystem:#0d8577; --k-contract:#3873ad; --k-glossary:#068000; --k-system:#5c667a;
  --k-playbook:#ad7c37; --k-reference:#ad378a; --k-project:#6e4fc9;
  --k-learning:#068000; --k-research:#6e4fc9; --k-spec:#ad378a; --k-plan:#3873ad; --k-report:#068000;
}
@media (prefers-color-scheme: dark){
  :root{
    --bg:#0f1419; --fg:#c5c5c5; --muted:#8b949e; --line:#3d434c; --panel:#14191f;
    --accent:#39afd7; --code-bg:#191f26; --chip:#252b33; --focus:#5c6773;
    --k-subsystem:#45d0be; --k-contract:#39afd7; --k-glossary:#2bab63; --k-system:#9aa4b2;
    --k-playbook:#fdd687; --k-reference:#ffa0a5; --k-project:#b78cf2;
    --k-learning:#2bab63; --k-research:#b78cf2; --k-spec:#ffa0a5; --k-plan:#39afd7; --k-report:#2bab63;
  }
}
*{box-sizing:border-box}
html,body{margin:0;height:100%}
body{background:var(--bg);color:var(--fg);font:15px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Helvetica,Arial,sans-serif;-webkit-font-smoothing:antialiased}
a{color:var(--accent);text-decoration:none}
a:hover{text-decoration:underline}
.k-subsystem{color:var(--k-subsystem)}.k-contract{color:var(--k-contract)}.k-glossary{color:var(--k-glossary)}
.k-system{color:var(--k-system)}.k-playbook{color:var(--k-playbook)}.k-reference{color:var(--k-reference)}
.k-project{color:var(--k-project)}.k-learning{color:var(--k-learning)}.k-research{color:var(--k-research)}
.k-spec{color:var(--k-spec)}.k-plan{color:var(--k-plan)}.k-report{color:var(--k-report)}
#app{display:grid;grid-template-columns:240px 1fr;height:100vh}
#sidebar{background:var(--panel);border-right:1px solid var(--line);display:flex;flex-direction:column;min-height:0;font-size:13px}
#brand{padding:10px 16px 8px}
#brand h1{font-size:15px;margin:2px 0 0;font-weight:600;cursor:pointer}
#platformlink{display:block;font-size:11px;color:var(--muted)}
#platformlink:hover{color:var(--accent);text-decoration:none}
.pivot{display:flex;gap:4px;padding:4px 12px 10px;border-bottom:1px solid var(--line)}
.pivot button{flex:1;background:none;border:1px solid var(--line);border-radius:6px;padding:3px 0;font-size:11px;color:var(--muted);cursor:pointer}
.pivot button.on{background:var(--chip);color:var(--fg);font-weight:600}
#tree{overflow:auto;padding:8px 6px 20px;flex:1;min-height:0}
#tree details{margin:2px 0}
#tree summary{cursor:pointer;padding:3px 8px;color:var(--fg);font-weight:600;font-size:12px;text-transform:capitalize;list-style:none;user-select:none}
#tree summary::-webkit-details-marker{display:none}
#tree summary::before{content:"\25B8";display:inline-block;width:1em;opacity:.55;transition:transform .12s}
#tree details[open]>summary::before{transform:rotate(90deg)}
.item{display:block;padding:2px 8px 2px 24px;color:var(--accent);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.item:hover{background:var(--chip)}
.item.active{background:var(--accent);color:#fff !important}
#main{display:flex;flex-direction:column;overflow:hidden;min-height:0}
#topbar{display:flex;gap:10px;align-items:center;padding:10px 24px;border-bottom:1px solid var(--line)}
#menubtn{display:none;background:none;border:1px solid var(--line);border-radius:6px;color:var(--fg);padding:4px 9px;cursor:pointer}
#topbar input{flex:1;max-width:620px;padding:7px 12px;border:1px solid var(--line);border-radius:2px;background:var(--bg);color:var(--fg);font-size:14px}
#topbar input:focus{outline:none;border-color:var(--focus)}
#topbar button{background:var(--chip);color:var(--fg);border:1px solid var(--line);border-radius:2px;padding:6px 12px;font-size:12.5px;cursor:pointer}
#topbar button.on{background:var(--accent);color:#fff;border-color:var(--accent)}
#scroll{flex:1;overflow:auto;min-height:0}
#content{max-width:960px;padding:24px 32px 80px}
#graph{display:none;flex:1;position:relative;min-height:0}
#graph canvas{display:block;width:100%;height:100%;cursor:grab}
h1.title{font-size:24px;margin:0 0 4px;font-weight:600}
.meta{color:var(--muted);font-size:13px;margin:0 0 20px}
.meta b{font-weight:600}
.desc{color:var(--muted);margin:0 0 4px;font-size:15px}
.section h2{font-size:17px;font-weight:600;margin:28px 0 8px;padding-bottom:4px;border-bottom:1px solid var(--line)}
.item-table{display:grid;grid-template-columns:minmax(150px,max-content) 1fr;column-gap:22px;row-gap:1px;margin:0}
.item-table>.nm{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:14px}
.item-table>.ds{color:var(--muted);font-size:13.5px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.item-table>.ds .when{font:11.5px ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;margin-right:8px;opacity:.8}
.md{font:16px/1.62 Georgia,'Iowan Old Style','Times New Roman',serif}
.md h1{font:600 21px/1.3 -apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;margin:26px 0 10px;padding-bottom:5px;border-bottom:1px solid var(--line)}
.md h2{font:600 18px/1.3 -apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;margin:22px 0 8px}
.md h3{font:600 15.5px/1.3 -apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;margin:18px 0 6px}
.md p{margin:10px 0}
.md ul,.md ol{margin:10px 0;padding-left:26px}
.md li{margin:3px 0}
.md code{background:var(--code-bg);padding:1.5px 5px;border-radius:3px;font:13.5px/1.4 ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}
.md pre{background:var(--code-bg);border-radius:4px;padding:13px 15px;overflow-x:auto}
.md pre code{background:none;padding:0}
.md pre.mermaid{background:none;text-align:center}
.md pre.mermaid svg{max-width:100%;height:auto}
.md blockquote{margin:12px 0;padding:2px 14px;border-left:3px solid var(--line);color:var(--muted)}
.md hr{border:none;border-top:1px solid var(--line);margin:20px 0}
.md table{border-collapse:collapse;margin:14px 0;display:block;overflow-x:auto;max-width:100%;font-size:14px;font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif}
.md th,.md td{border:1px solid var(--line);padding:7px 11px;text-align:left;vertical-align:top}
.md th{background:var(--panel);font-weight:650}
.md a[target=_blank]::after{content:"\2197";font-size:.8em;opacity:.6;margin-left:1px}
footer.links{margin-top:34px;border-top:1px solid var(--line);padding-top:16px;display:grid;grid-template-columns:1fr 1fr;gap:20px}
footer.links h4{margin:0 0 8px;font-size:12px;text-transform:uppercase;letter-spacing:.05em;color:var(--muted)}
footer.links ul{list-style:none;margin:0;padding:0}
footer.links li{margin:4px 0;font-size:13px}
.result{padding:10px 0;border-bottom:1px solid var(--line)}
.result a.rt{font-weight:600;font-size:14.5px}
.result .k{font-size:11px;color:var(--muted);margin-left:8px;text-transform:lowercase}
.result p{margin:4px 0 0;font-size:13px;color:var(--muted)}
mark{background:hsl(45,95%,55%,.45);color:inherit;border-radius:2px;padding:0 1px}
.empty{color:var(--muted);padding:40px;text-align:center}
@media (max-width:720px){
  #app{grid-template-columns:1fr}
  #sidebar{position:fixed;z-index:5;width:82%;height:100%;transform:translateX(-100%);transition:transform .2s}
  #sidebar.open{transform:none}
  #menubtn{display:block}
  #content{padding:18px 16px 60px}
}
</style>
</head>
<body>
<div id="app">
  <aside id="sidebar">
    <div id="brand"><a id="platformlink" href="/" title="Back to the internal docs landing page">&#8592; docs home</a><h1 id="brandtitle" title="Bundle home"></h1></div>
    <div class="pivot"><button id="pivot-folders">Folders</button><button id="pivot-domains">Domains</button></div>
    <nav id="tree"></nav>
  </aside>
  <section id="main">
    <div id="topbar">
      <button id="menubtn" title="Menu">&#9776;</button>
      <input id="search" type="search" placeholder="Search everything&hellip; (press S)" autocomplete="off">
      <button id="graphbtn" title="Toggle graph view">Graph</button>
    </div>
    <div id="scroll"><div id="content"></div></div>
    <div id="graph"><canvas id="canvas"></canvas></div>
  </section>
</div>
<script type="application/json" id="okf-data">__OKF_DATA__</script>
<script src="https://cdn.jsdelivr.net/npm/mermaid@11.12.0/dist/mermaid.min.js" integrity="sha384-o+g/BxPwhi0C3RK7oQBxQuNimeafQ3GE/ST4iT2BxVI4Wzt60SH4pq9iXVYujjaS" crossorigin="anonymous"></script>
<script>
"use strict";
var DATA = JSON.parse(document.getElementById("okf-data").textContent);
var TITLE = DATA.title || "Knowledge";
var CONCEPTS = DATA.concepts || [];
var LINKS = DATA.links || {};
var BACKLINKS = DATA.backlinks || {};
var BY_ID = {};
CONCEPTS.forEach(function(c){ BY_ID[c.id] = c; });

// ---- helpers ----
function esc(s){ return String(s).replace(/&/g,"&amp;").replace(/</g,"&lt;").replace(/>/g,"&gt;").replace(/"/g,"&quot;"); }
function hueFor(t){ var h=0,s=String(t||""); for(var i=0;i<s.length;i++){ h=(h*31+s.charCodeAt(i))>>>0; } return h%360; }
// Order = homepage section order: human-readable architecture companions
// (subsystem) lead, contracts next, everything dated last.
var KINDS=["subsystem","contract","glossary","system","playbook","reference","project","learning","research","spec","plan","report"];
function kindClass(t){ t=String(t||"").toLowerCase(); return KINDS.indexOf(t)>-1 ? "k-"+t : ""; }
function titleOf(id){ var c=BY_ID[id]; return c?c.title:id; }

// ---- markdown link resolution -> hash routes ----
var CUR_DIR = "";
function resolveId(path, baseDir){
  path = path.replace(/\.md$/i,"");
  var parts = path[0]==="/" ? path.slice(1).split("/") : (baseDir?baseDir.split("/"):[]).concat(path.split("/"));
  var out=[];
  for(var i=0;i<parts.length;i++){ var p=parts[i]; if(p===""||p===".")continue; if(p===".."){out.pop();}else out.push(p); }
  return out.join("/");
}
function linkHtml(txt, url){
  var u = url.trim();
  if(/^https?:/i.test(u)) return '<a href="'+u+'" target="_blank" rel="noopener">'+txt+'</a>';
  if(/^mailto:/i.test(u)) return '<a href="'+u+'">'+txt+'</a>';
  var path = u.split("#")[0];
  if(/\.md$/i.test(path)) return '<a href="#/'+esc(resolveId(path, CUR_DIR))+'">'+txt+'</a>';
  return '<a href="'+esc(u)+'">'+txt+'</a>';
}

// ---- inline markdown ----
function inline(s){
  var codes=[], NUL=String.fromCharCode(0), SOH=String.fromCharCode(1);
  s = s.replace(/`([^`]+)`/g, function(m,c){ codes.push(c); return NUL+(codes.length-1)+SOH; });
  s = esc(s);
  s = s.replace(/\[([^\]]*)\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g, function(m,txt,url){ return linkHtml(txt, url); });
  s = s.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
  s = s.replace(/__([^_]+)__/g, "<strong>$1</strong>");
  s = s.replace(/\*([^*\s][^*]*?)\*/g, "<em>$1</em>");
  s = s.replace(/(^|[^A-Za-z0-9_])_([^_]+)_(?![A-Za-z0-9_])/g, "$1<em>$2</em>");
  s = s.replace(new RegExp(NUL+"(\\d+)"+SOH,"g"), function(m,i){ return "<code>"+esc(codes[+i])+"</code>"; });
  return s;
}

// ---- block markdown ----
function cells(row){ return row.trim().replace(/^\|/,"").replace(/\|$/,"").split("|").map(function(x){return x.trim();}); }
function buildList(items){
  var idx={i:0};
  function build(minIndent){
    var out="", tag=null;
    while(idx.i<items.length && items[idx.i].indent>=minIndent){
      var it=items[idx.i];
      if(tag===null){ tag=it.ordered?"ol":"ul"; out+="<"+tag+">"; }
      var li="<li>"+inline(it.text);
      idx.i++;
      if(idx.i<items.length && items[idx.i].indent>it.indent){ li+=build(items[idx.i].indent); }
      li+="</li>"; out+=li;
    }
    if(tag) out+="</"+tag+">";
    return out;
  }
  return build(items.length?items[0].indent:0);
}
function renderMd(src, dir){
  CUR_DIR = dir||"";
  var lines=String(src||"").replace(/\r\n/g,"\n").split("\n"), out="", i=0;
  var blockStart=/^(#{1,6}\s|```|\s*>|\s*([-*+]|\d+\.)\s)/;
  while(i<lines.length){
    var line=lines[i];
    if(/^```/.test(line)){
      var lang=line.replace(/^```\s*/,"").trim().toLowerCase();
      i++; var code=[];
      while(i<lines.length && !/^```/.test(lines[i])){ code.push(lines[i]); i++; }
      i++;
      // mermaid fences become live diagrams (mermaid.run picks up pre.mermaid
      // after each route render); when the CDN is unavailable or fails SRI
      // they degrade to the plain source text, which is still readable.
      if(lang==="mermaid"){ out+='<pre class="mermaid">'+esc(code.join("\n"))+"</pre>"; continue; }
      out+="<pre><code>"+esc(code.join("\n"))+"</code></pre>"; continue;
    }
    if(/^\s*$/.test(line)){ i++; continue; }
    var h=line.match(/^(#{1,6})\s+(.*)$/);
    if(h){ var lv=h[1].length; out+="<h"+lv+">"+inline(h[2].trim())+"</h"+lv+">"; i++; continue; }
    if(/^\s*([-*_])(\s*\1){2,}\s*$/.test(line)){ out+="<hr>"; i++; continue; }
    if(/^\s*>/.test(line)){
      var q=[];
      while(i<lines.length && /^\s*>/.test(lines[i])){ q.push(lines[i].replace(/^\s*>\s?/,"")); i++; }
      out+="<blockquote>"+renderMd(q.join("\n"),dir)+"</blockquote>"; continue;
    }
    if(/\|/.test(line) && i+1<lines.length && /^\s*\|?[\s:|-]*-[\s:|-]*\|?\s*$/.test(lines[i+1])){
      var header=line; i+=2; var rows=[];
      while(i<lines.length && /\|/.test(lines[i]) && !/^\s*$/.test(lines[i])){ rows.push(lines[i]); i++; }
      out+="<table><thead><tr>"+cells(header).map(function(c){return "<th>"+inline(c)+"</th>";}).join("")+"</tr></thead><tbody>";
      out+=rows.map(function(r){ return "<tr>"+cells(r).map(function(c){return "<td>"+inline(c)+"</td>";}).join("")+"</tr>"; }).join("");
      out+="</tbody></table>"; continue;
    }
    if(/^\s*([-*+]|\d+\.)\s+/.test(line)){
      var items=[];
      while(i<lines.length && /^\s*([-*+]|\d+\.)\s+/.test(lines[i])){
        var mm=lines[i].match(/^(\s*)([-*+]|\d+\.)\s+(.*)$/);
        items.push({indent:mm[1].length, ordered:/\d/.test(mm[2]), text:mm[3]});
        i++;
      }
      out+=buildList(items); continue;
    }
    var para=[line]; i++;
    while(i<lines.length && !/^\s*$/.test(lines[i]) && !blockStart.test(lines[i])){ para.push(lines[i]); i++; }
    out+="<p>"+inline(para.join(" "))+"</p>";
  }
  return out;
}

// ---- sidebar tree ----
function buildTree(){
  var root={dirs:{},files:[]};
  CONCEPTS.forEach(function(c){
    var parts=c.id.split("/"), node=root;
    for(var k=0;k<parts.length-1;k++){ var d=parts[k]; node.dirs[d]=node.dirs[d]||{dirs:{},files:[]}; node=node.dirs[d]; }
    node.files.push(c);
  });
  return root;
}
var TREE=buildTree();
function matchIds(){
  var terms=document.getElementById("search").value.toLowerCase().split(/\s+/).filter(Boolean);
  if(!terms.length) return null;
  var set={};
  CONCEPTS.forEach(function(c){
    var hay=(c.title+" "+c.description+" "+(c.tags||[]).join(" ")+" "+c.body).toLowerCase();
    if(terms.every(function(t){ return hay.indexOf(t)!==-1; })) set[c.id]=1;
  });
  return set;
}
// NAV: "folders" mirrors the bundle's type directories (authoring taxonomy);
// "domains" pivots on frontmatter tags (how humans actually look things up).
var NAV=(function(){ try{ return localStorage.getItem("okf-nav")||"folders"; }catch(e){ return "folders"; } })();
function setNav(m){ NAV=m; try{ localStorage.setItem("okf-nav",m); }catch(e){} syncPivot(); renderTree(); }
function syncPivot(){
  document.getElementById("pivot-folders").classList.toggle("on",NAV==="folders");
  document.getElementById("pivot-domains").classList.toggle("on",NAV==="domains");
}
function itemHtml(c){ return '<a class="item '+kindClass(c.type)+'" data-id="'+esc(c.id)+'" href="#/'+esc(c.id)+'">'+esc(c.title)+'</a>'; }
// Reading order: the didactic entry points lead every list they appear in;
// everything else stays alphabetical.
var PINNED=["systems/start-here","systems/workspace-map"];
function pinRank(id){ var i=PINNED.indexOf(id); return i===-1?PINNED.length:i; }
var byTitle=function(a,b){return pinRank(a.id)-pinRank(b.id) || a.title.localeCompare(b.title);};
function renderTree(){
  var filter=matchIds(), nav=document.getElementById("tree");
  var body="";
  if(NAV==="domains"){
    var groups={};
    CONCEPTS.forEach(function(c){
      var tags=(c.tags&&c.tags.length)?c.tags:["untagged"];
      tags.forEach(function(t){ (groups[t]=groups[t]||[]).push(c); });
    });
    Object.keys(groups).sort(function(a,b){ return groups[b].length-groups[a].length || a.localeCompare(b); }).forEach(function(t){
      var inner="";
      groups[t].slice().sort(byTitle).forEach(function(c){ if(filter && !filter[c.id]) return; inner+=itemHtml(c); });
      if(inner==="") return;
      var open = filter ? "open" : (groups[t].length>=8?"open":"");
      body+='<details '+open+'><summary>'+esc(t)+' <span style="opacity:.6">('+groups[t].length+')</span></summary>'+inner+'</details>';
    });
  } else {
    // Human-readable dirs first; dated session-output dirs sink to the bottom.
    var DIR_ORDER=["systems","playbooks","references","projects","specs","plans","research","learnings"];
    var dirRank=function(d){ var i=DIR_ORDER.indexOf(d); return i===-1 ? DIR_ORDER.length : i; };
    var node=function(n, path, depth){
      var html="", dirNames=Object.keys(n.dirs).sort(function(a,b){
        return dirRank(a)-dirRank(b) || a.localeCompare(b);
      });
      dirNames.forEach(function(d){
        var inner=node(n.dirs[d], path?path+"/"+d:d, depth+1);
        if(inner==="") return;
        var open = filter ? "open" : (depth<1?"open":"");
        html+='<details '+open+'><summary>'+esc(d)+'</summary>'+inner+'</details>';
      });
      n.files.slice().sort(byTitle).forEach(function(c){
        if(filter && !filter[c.id]) return;
        html+=itemHtml(c);
      });
      return html;
    };
    body=node(TREE,"",0);
  }
  nav.innerHTML = body || '<div class="empty" style="padding:20px;font-size:13px">No matches</div>';
  markActive();
}
function markActive(){
  var cur=location.hash.replace(/^#\//,"");
  document.querySelectorAll("#tree .item").forEach(function(a){
    a.classList.toggle("active", a.getAttribute("data-id")===cur);
  });
}

// ---- views ----
function metaFor(c){
  var bits=['<b class="'+kindClass(c.type)+'">'+esc(c.type||"concept")+'</b>'];
  (c.tags||[]).forEach(function(t){ bits.push("#"+esc(t)); });
  if(c.timestamp) bits.push(esc(c.timestamp));
  return '<p class="meta">'+bits.join(" · ")+'</p>';
}
function footerFor(id){
  var outs=(LINKS[id]||[]).filter(function(t){return BY_ID[t];});
  var ins=(BACKLINKS[id]||[]).filter(function(t){return BY_ID[t];});
  if(!outs.length && !ins.length) return "";
  function list(arr){ return arr.length ? "<ul>"+arr.map(function(t){return '<li><a href="#/'+esc(t)+'">'+esc(titleOf(t))+"</a></li>";}).join("")+"</ul>" : '<p class="desc">none</p>'; }
  return '<footer class="links"><div><h4>Links to</h4>'+list(outs)+'</div><div><h4>Linked from</h4>'+list(ins)+"</div></footer>";
}
function renderConcept(c){
  var el=document.getElementById("content");
  el.innerHTML = '<h1 class="title">'+esc(c.title)+"</h1>"
    + (c.description?'<p class="desc">'+esc(c.description)+"</p>":"")
    + metaFor(c)
    + '<div class="md">'+renderMd(c.body, c.id.split("/").slice(0,-1).join("/"))+"</div>"
    + footerFor(c.id);
  document.getElementById("scroll").scrollTop=0;
  runMermaid();
}

// ---- mermaid ----
// Route renders rebuild #content from source markdown, so every pass sees
// fresh unprocessed pre.mermaid nodes; a scheme flip just re-inits and
// re-routes. No-op when the CDN runtime is unavailable or fails SRI.
var MERMAID_DARK=matchMedia("(prefers-color-scheme: dark)");
var MERMAID_READY=false;
function runMermaid(){
  if(!window.mermaid) return;
  if(!MERMAID_READY){
    mermaid.initialize({startOnLoad:false, securityLevel:"strict",
      theme: MERMAID_DARK.matches ? "dark" : "neutral"});
    MERMAID_READY=true;
  }
  var els=document.querySelectorAll("#content pre.mermaid");
  if(els.length) mermaid.run({nodes:els}).catch(function(e){ console.error("mermaid:", e); });
}
if(MERMAID_DARK.addEventListener) MERMAID_DARK.addEventListener("change", function(){ MERMAID_READY=false; route(); });
function itemRow(c, pre){
  return '<div class="nm"><a class="'+kindClass(c.type)+'" href="#/'+esc(c.id)+'">'+esc(c.title)+'</a></div>'
    + '<div class="ds">'+(pre||"")+esc(c.description||"")+'</div>';
}
// rustdoc-style front page: everything on one page as name+description
// item tables, grouped by kind — systems and playbooks first.
var KIND_LABELS={subsystem:"How it works",contract:"Domain contracts",glossary:"Glossary",system:"Platform",playbook:"Playbooks",reference:"References",project:"Projects",learning:"Learnings",research:"Research",spec:"Specs",plan:"Plans",report:"Reports"};
function renderHome(){
  var lc=function(s){ return String(s||"").toLowerCase(); };
  var groups={};
  CONCEPTS.forEach(function(c){ var k=lc(c.type)||"other"; (groups[k]=groups[k]||[]).push(c); });
  var order=KINDS.filter(function(k){ return groups[k]; })
    .concat(Object.keys(groups).filter(function(k){ return KINDS.indexOf(k)===-1; }).sort());
  var recent=CONCEPTS.filter(function(c){ return c.timestamp; })
    .sort(function(a,b){ return b.timestamp.localeCompare(a.timestamp); }).slice(0,10);
  var html='<h1 class="title">'+esc(TITLE)+"</h1>"
    +'<p class="meta">'+CONCEPTS.length+' concepts · press <b>S</b> to search everything</p>';
  if(recent.length){
    html+='<div class="section"><h2>Recently updated</h2><div class="item-table">'
      +recent.map(function(c){ return itemRow(c,'<span class="when">'+esc(c.timestamp)+'</span>'); }).join("")+"</div></div>";
  }
  order.forEach(function(k){
    html+='<div class="section"><h2>'+esc(KIND_LABELS[k]||k)+'</h2><div class="item-table">'
      +groups[k].slice().sort(byTitle).map(function(c){ return itemRow(c); }).join("")+"</div></div>";
  });
  var el=document.getElementById("content"); el.innerHTML=html;
  document.getElementById("scroll").scrollTop=0;
}
function route(){
  var id=location.hash.replace(/^#\//,"");
  showGraph(false);
  if(id && BY_ID[id]) renderConcept(BY_ID[id]);
  else renderHome();
  markActive();
  if(window.innerWidth<=720) document.getElementById("sidebar").classList.remove("open");
}
// ---- full-text search results (content pane) ----
function hilite(s, terms){
  var out=esc(s);
  terms.forEach(function(t){
    out=out.replace(new RegExp("("+t.replace(/[.*+?^${}()|[\]\\]/g,"\\$&")+")","ig"),"<mark>$1</mark>");
  });
  return out;
}
function snippet(c, terms){
  var body=String(c.body||""), lower=body.toLowerCase(), pos=-1;
  for(var i=0;i<terms.length;i++){ var p=lower.indexOf(terms[i]); if(p>-1&&(pos===-1||p<pos)) pos=p; }
  if(pos===-1) return hilite(c.description||"", terms);
  var start=Math.max(0,pos-100), end=Math.min(body.length,pos+140);
  var frag=(start>0?"…":"")+body.slice(start,end).replace(/\s+/g," ")+(end<body.length?"…":"");
  return hilite(frag, terms);
}
function renderSearch(){
  var q=document.getElementById("search").value.trim();
  if(!q){ route(); return; }
  var terms=q.toLowerCase().split(/\s+/).filter(Boolean);
  var scored=[];
  CONCEPTS.forEach(function(c){
    var title=c.title.toLowerCase(), desc=(c.description||"").toLowerCase(),
        tags=(c.tags||[]).join(" ").toLowerCase(), body=(c.body||"").toLowerCase();
    var ok=terms.every(function(t){ return title.indexOf(t)>-1||desc.indexOf(t)>-1||tags.indexOf(t)>-1||body.indexOf(t)>-1; });
    if(!ok) return;
    var s=0;
    terms.forEach(function(t){
      if(title.indexOf(t)>-1) s+=8;
      if(tags.indexOf(t)>-1) s+=4;
      if(desc.indexOf(t)>-1) s+=3;
      var i=-1,n=0; while((i=body.indexOf(t,i+1))>-1 && n<20){ n++; } s+=n;
    });
    scored.push([s,c]);
  });
  scored.sort(function(a,b){ return b[0]-a[0]; });
  var html='<h1 class="title">Search</h1><p class="desc">'+scored.length+' result'+(scored.length===1?"":"s")+' for “'+esc(q)+'”</p>';
  scored.slice(0,50).forEach(function(pair){
    var c=pair[1];
    html+='<div class="result"><a class="rt '+kindClass(c.type)+'" href="#/'+esc(c.id)+'">'+hilite(c.title,terms)+'</a><span class="k">'+esc(c.type)+"</span><p>"+snippet(c,terms)+"</p></div>";
  });
  if(!scored.length) html+='<div class="empty">Nothing matched.</div>';
  showGraph(false);
  var el=document.getElementById("content"); el.innerHTML=html;
  document.getElementById("scroll").scrollTop=0;
}

// ---- graph view (canvas force-directed) ----
// ALPHA cools the simulation each frame; the loop stops below the floor so
// the layout settles instead of jiggling (and burning CPU) forever.
var GS=null, raf=null, ALPHA=1, dpr=Math.min(window.devicePixelRatio||1,2), canvas, ctx;
function initGraph(){
  var nodes=CONCEPTS.map(function(c,i){
    var a=i/CONCEPTS.length*Math.PI*2;
    return {id:c.id,title:c.title,type:c.type,x:Math.cos(a)*180+ (Math.random()*30-15),y:Math.sin(a)*180+(Math.random()*30-15),vx:0,vy:0};
  });
  var idx={}; nodes.forEach(function(n,i){ idx[n.id]=i; });
  var edges=[];
  CONCEPTS.forEach(function(c){ (LINKS[c.id]||[]).forEach(function(t){ if(idx[t]!==undefined && idx[c.id]!==idx[t]) edges.push([idx[c.id],idx[t]]); }); });
  GS={nodes:nodes, edges:edges, idx:idx};
}
function step(){
  var n=GS.nodes, e=GS.edges, N=n.length;
  var W=canvas.clientWidth, H=canvas.clientHeight, cx=W/2, cy=H/2;
  for(var a=0;a<N;a++){
    var fx=0,fy=0;
    for(var b=0;b<N;b++){ if(a===b)continue; var dx=n[a].x-n[b].x, dy=n[a].y-n[b].y, d2=dx*dx+dy*dy+0.01; var f=1400/d2; var d=Math.sqrt(d2); fx+=dx/d*f; fy+=dy/d*f; }
    fx+=(cx-n[a].x)*0.006; fy+=(cy-n[a].y)*0.006;
    n[a]._fx=fx; n[a]._fy=fy;
  }
  for(var k=0;k<e.length;k++){
    var A=n[e[k][0]], B=n[e[k][1]], dx=B.x-A.x, dy=B.y-A.y, dd=Math.sqrt(dx*dx+dy*dy)+0.01;
    var fo=(dd-90)*0.015, ux=dx/dd, uy=dy/dd;
    A._fx+=ux*fo; A._fy+=uy*fo; B._fx-=ux*fo; B._fy-=uy*fo;
  }
  for(var i2=0;i2<N;i2++){ var p=n[i2]; p.vx=(p.vx+p._fx*ALPHA)*0.86; p.vy=(p.vy+p._fy*ALPHA)*0.86; p.x+=p.vx; p.y+=p.vy; }
  ALPHA*=0.98;
}
function draw(){
  var W=canvas.clientWidth, H=canvas.clientHeight;
  if(canvas.width!==W*dpr||canvas.height!==H*dpr){ canvas.width=W*dpr; canvas.height=H*dpr; }
  ctx.setTransform(dpr,0,0,dpr,0,0);
  ctx.clearRect(0,0,W,H);
  var cs=getComputedStyle(document.body), line=cs.getPropertyValue("--line"), fg=cs.getPropertyValue("--fg");
  var n=GS.nodes, e=GS.edges;
  ctx.strokeStyle=line; ctx.lineWidth=1; ctx.globalAlpha=.7;
  for(var k=0;k<e.length;k++){ var A=n[e[k][0]], B=n[e[k][1]]; ctx.beginPath(); ctx.moveTo(A.x,A.y); ctx.lineTo(B.x,B.y); ctx.stroke(); }
  ctx.globalAlpha=1;
  var cur=location.hash.replace(/^#\//,"");
  for(var i=0;i<n.length;i++){
    var p=n[i], r=p.id===cur?7:5;
    ctx.beginPath(); ctx.arc(p.x,p.y,r,0,Math.PI*2);
    ctx.fillStyle="hsl("+hueFor(p.type)+",55%,50%)"; ctx.fill();
    if(p.id===cur){ ctx.lineWidth=2; ctx.strokeStyle=fg; ctx.stroke(); }
  }
  if(n.length<=120){
    ctx.fillStyle=fg; ctx.font="10px -apple-system,sans-serif"; ctx.globalAlpha=.8;
    for(var j=0;j<n.length;j++){ ctx.fillText(n[j].title.slice(0,22), n[j].x+8, n[j].y+3); }
    ctx.globalAlpha=1;
  }
}
function loop(){ step(); draw(); if(ALPHA>0.02){ raf=requestAnimationFrame(loop); } else { raf=null; } }
function reheat(t){ ALPHA=Math.max(ALPHA,t); if(!raf && GS && document.getElementById("graph").style.display==="block") loop(); }
function showGraph(on){
  var g=document.getElementById("graph"), c=document.getElementById("scroll"), btn=document.getElementById("graphbtn");
  g.style.display=on?"block":"none"; c.style.display=on?"none":"block"; btn.classList.toggle("on",on);
  if(on){
    canvas=document.getElementById("canvas"); ctx=canvas.getContext("2d");
    if(!GS) initGraph();
    reheat(0.4);
    if(!raf) loop();
  } else if(raf){ cancelAnimationFrame(raf); raf=null; }
}
function toggleGraph(){ showGraph(document.getElementById("graph").style.display!=="block"); }

// ---- wiring ----
document.getElementById("brandtitle").textContent=TITLE;
document.getElementById("brandtitle").onclick=function(){ location.hash=""; };
document.getElementById("graphbtn").onclick=toggleGraph;
document.getElementById("search").addEventListener("input", function(){ renderTree(); renderSearch(); });
document.getElementById("pivot-folders").onclick=function(){ setNav("folders"); };
document.getElementById("pivot-domains").onclick=function(){ setNav("domains"); };
document.getElementById("menubtn").onclick=function(){ document.getElementById("sidebar").classList.toggle("open"); };
syncPivot();
// rustdoc keyboard idiom: S focuses search, Escape clears and restores the view.
document.addEventListener("keydown", function(ev){
  var el=document.activeElement, typing=el&&(el.tagName==="INPUT"||el.tagName==="TEXTAREA");
  var box=document.getElementById("search");
  if(!typing && (ev.key==="s"||ev.key==="S"||ev.key==="/")){ ev.preventDefault(); box.focus(); box.select(); }
  else if(typing && el===box && ev.key==="Escape"){ box.value=""; box.blur(); renderTree(); route(); }
});
window.addEventListener("hashchange", route);
window.addEventListener("resize", function(){ reheat(0.3); });
document.getElementById("canvas").addEventListener("click", function(ev){
  if(!GS) return;
  var rect=canvas.getBoundingClientRect(), mx=ev.clientX-rect.left, my=ev.clientY-rect.top, best=null, bd=1e9;
  GS.nodes.forEach(function(p){ var dx=p.x-mx, dy=p.y-my, d=dx*dx+dy*dy; if(d<bd){bd=d;best=p;} });
  if(best && bd<400) location.hash="#/"+best.id;
});
renderTree();
route();
</script>
</body>
</html>
"""


def _vocab_entries(body: str) -> list:
    """Extract (term, definition) bullets from a '## Vocabulary' section.

    Accepts the three separator shapes contracts use — '**Term**: def',
    '**Term:** def', '**Term** — def'; indented follow-up lines are folded
    into the running definition. The section ends at the next h1/h2.
    """
    entries, in_section = [], False
    for line in body.splitlines():
        if re.match(r"^##\s+Vocabulary\s*$", line):
            in_section = True
            continue
        if in_section and re.match(r"^#{1,2}\s", line):
            break
        if not in_section:
            continue
        m = re.match(r"^-\s+\*\*(.+?):?\*\*\s*[:—–-]?\s*(.*)$", line)
        if m:
            entries.append([m.group(1).strip(), m.group(2).strip()])
        elif entries and line.strip() and line[:1] in (" ", "\t"):
            entries[-1][1] = (entries[-1][1] + " " + line.strip()).strip()
    return [(t, d) for t, d in entries]


def _build_glossary(nodes: dict):
    """Synthesize the render-only Glossary concept from contract vocabularies.

    Returns (concept_dict, source_ids) or (None, []) when no contract defines
    a vocabulary. The glossary never exists on disk: terms are owned by the
    contracts and collected fresh on every render, so it cannot go stale.
    """
    rows, sources = [], []
    for cid in sorted(nodes):
        meta = nodes[cid]["meta"]
        if str(meta.get("type") or "").strip().lower() != "contract":
            continue
        entries = _vocab_entries(nodes[cid]["body"])
        if not entries:
            continue
        title = str(meta.get("title") or "").strip() or cid.rsplit("/", 1)[-1]
        sources.append(cid)
        rows.extend((term, dfn, cid, title) for term, dfn in entries)
    if not rows:
        return None, []
    rows.sort(key=lambda r: (r[0].casefold(), r[3].casefold()))
    lines = [
        "Every domain term, defined once, A to Z. Generated at render time",
        "from the `## Vocabulary` sections of the domain contracts — edit a",
        "term in its contract, never here. Each entry links to the contract",
        "that owns it.",
    ]
    letter = ""
    for term, dfn, cid, title in rows:
        first = term[:1].upper()
        if not first.isalpha():
            first = "#"
        if first != letter:
            letter = first
            lines += ["", f"## {letter}", ""]
        lines.append(f"- **{term}** — {dfn} *([{title}](/{cid}.md))*")
    concept = {
        "id": "glossary",
        "type": "Glossary",
        "title": "Glossary",
        "description": (f"All {len(rows)} domain terms in one place, A-Z — "
                        "generated from the contracts' Vocabulary sections."),
        "tags": ["glossary", "vocabulary"],
        "timestamp": "",  # keep it out of "Recently updated"
        "resource": "",
        "body": "\n".join(lines) + "\n",
    }
    return concept, sources


def _render_html(data: dict) -> str:
    payload = json.dumps(data, ensure_ascii=False)
    # Neutralize any sequence that could break out of the JSON <script> island.
    payload = payload.replace("<", "\\u003c").replace(">", "\\u003e").replace("&", "\\u0026")
    title = str(data.get("title") or "Knowledge")
    esc_title = title.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
    return _SITE_TEMPLATE.replace("__OKF_TITLE__", esc_title).replace("__OKF_DATA__", payload)


def cmd_render(args) -> int:
    root = find_bundle_root(Path(args.dir) if args.dir else Path.cwd())
    nodes, backlinks = _load_graph(root)  # permissive: skips unreadable concepts
    concepts = []
    for cid in sorted(nodes):
        m = nodes[cid]["meta"]
        concepts.append({
            "id": cid,
            "type": str(m.get("type") or "").strip(),
            "title": str(m.get("title") or "").strip() or cid.rsplit("/", 1)[-1],
            "description": str(m.get("description") or "").strip(),
            "tags": _tags_display(m),
            "timestamp": str(m.get("timestamp") or "").strip(),
            "resource": str(m.get("resource") or "").strip(),
            "body": nodes[cid]["body"],
        })
    links = {cid: nodes[cid]["out"] for cid in nodes}
    blinks = {cid: backlinks.get(cid, []) for cid in nodes}
    glossary, sources = _build_glossary(nodes)
    if glossary:
        concepts.append(glossary)
        links["glossary"] = sources
        for cid in sources:
            blinks[cid] = blinks.get(cid, []) + ["glossary"]
    data = {
        "title": _bundle_title(root),
        "concepts": concepts,
        "links": links,
        "backlinks": blinks,
    }
    out = Path(args.output).resolve() if args.output else (root / "site.html")
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(_render_html(data), encoding="utf-8")
    print(f"rendered {len(concepts)} concept(s) -> {out}")
    return 0


# ---------------------------------------------------------------------------
# cli
# ---------------------------------------------------------------------------

def build_parser() -> argparse.ArgumentParser:
    ap = argparse.ArgumentParser(prog="okf.py", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("init", help="scaffold a new bundle")
    p.add_argument("dir", nargs="?", default="knowledge", help="bundle directory (default: knowledge)")
    p.add_argument("--title", help="bundle title (default: dir name)")
    p.add_argument("--okf-version", dest="okf_version", help="declare okf_version in root index.md, e.g. 0.1")
    p.add_argument("--force", action="store_true")
    p.set_defaults(func=cmd_init)

    p = sub.add_parser("new", help="create a concept document")
    p.add_argument("path", help="concept path within the bundle, e.g. tables/orders")
    p.add_argument("--type", required=True, help="REQUIRED concept type, e.g. 'BigQuery Table', 'Metric', 'Playbook'")
    p.add_argument("--title")
    p.add_argument("--description")
    p.add_argument("--resource", help="canonical URI of the underlying asset")
    p.add_argument("--tags", help="comma-separated tags")
    p.add_argument("--bundle", help="bundle root (default: discover from cwd)")
    p.add_argument("--force", action="store_true")
    p.set_defaults(func=cmd_new)

    p = sub.add_parser("index", help="(re)generate index.md listings")
    p.add_argument("dir", nargs="?", default="", help="bundle root (default: discover from cwd)")
    p.set_defaults(func=cmd_index)

    p = sub.add_parser("log", help="append a dated log entry")
    p.add_argument("message", help="entry text")
    p.add_argument("--kind", help="leading bold word: Update | Creation | Deprecation (default: Update)")
    p.add_argument("--dir", default="", help="scope subdirectory or bundle root (default: bundle root)")
    p.set_defaults(func=cmd_log)

    p = sub.add_parser("validate", help="check conformance and conventions")
    p.add_argument("dir", nargs="?", default="", help="bundle root (default: discover from cwd)")
    p.add_argument("--check-sources", action="store_true",
                   help="drift check: warn if a concept's resource file is missing or newer than the concept")
    p.set_defaults(func=cmd_validate)

    p = sub.add_parser("due", help="list dated dirs over their compression budget (link-isolated fold candidates first)")
    p.add_argument("dir", nargs="?", default="", help="bundle root (default: discover from cwd)")
    p.add_argument("--orphans", action="store_true",
                   help="list link-isolated dated concepts regardless of budget state")
    p.set_defaults(func=cmd_due)

    # --- read path (consumption) ---
    p = sub.add_parser("list", help="enumerate concepts (progressive disclosure)")
    p.add_argument("dir", nargs="?", default="", help="bundle root (default: discover from cwd)")
    p.add_argument("--type", help="filter by type (case-insensitive substring)")
    p.add_argument("--tag", help="filter by tag")
    p.add_argument("--json", action="store_true")
    p.set_defaults(func=cmd_list)

    p = sub.add_parser("search", help="rank concepts by text match")
    p.add_argument("query", help="space-separated terms (AND); frontmatter weighted over body")
    p.add_argument("dir", nargs="?", default="", help="bundle root (default: discover from cwd)")
    p.add_argument("--type", help="filter by type")
    p.add_argument("--tag", help="filter by tag")
    p.add_argument("--limit", type=int, default=20)
    p.add_argument("--json", action="store_true")
    p.set_defaults(func=cmd_search)

    p = sub.add_parser("show", help="print a concept by id")
    p.add_argument("concept", help="concept id, e.g. tables/orders (with or without .md)")
    p.add_argument("dir", nargs="?", default="", help="bundle root (default: discover from cwd)")
    p.add_argument("--frontmatter", action="store_true", help="print only the frontmatter")
    p.add_argument("--links", action="store_true", help="also list outbound links and backlinks")
    p.set_defaults(func=cmd_show)

    p = sub.add_parser("context", help="gather a budgeted context pack for an AI workflow")
    p.add_argument("query", help="a concept id, or search terms to seed from")
    p.add_argument("dir", nargs="?", default="", help="bundle root (default: discover from cwd)")
    p.add_argument("--type", help="filter search seeds by type")
    p.add_argument("--tag", help="filter search seeds by tag")
    p.add_argument("--depth", type=int, default=1, help="link-expansion hops from seeds (default: 1)")
    p.add_argument("--seeds", type=int, default=3, help="max search-seed concepts when query is not an id")
    p.add_argument("--budget", type=int, default=20000, help="max characters to emit (default: 20000)")
    p.add_argument("--backlinks", action="store_true", help="also expand to concepts that link IN")
    p.add_argument("--json", action="store_true")
    p.set_defaults(func=cmd_context)

    p = sub.add_parser("render", help="render the whole bundle to one HTML file")
    p.add_argument("dir", nargs="?", default="", help="bundle root (default: discover from cwd)")
    p.add_argument("-o", "--output", help="output HTML file (default: <bundle-root>/site.html)")
    p.set_defaults(func=cmd_render)
    return ap


def main(argv=None) -> int:
    args = build_parser().parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
