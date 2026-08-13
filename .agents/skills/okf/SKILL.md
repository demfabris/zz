---
name: okf
description: >-
  Author and maintain Open Knowledge Format (OKF v0.1) knowledge bundles — a
  directory of markdown files with YAML frontmatter that is both human- and
  agent-readable. Use this whenever the user wants to document a project's
  knowledge for AI agents or teammates: building or updating a knowledge base,
  knowledge catalog, data dictionary, metadata-as-code, an agent-readable
  /knowledge or /docs corpus, concept/table/metric/playbook documentation, or
  when they mention OKF, knowledge bundles, or "Open Knowledge Format". Also use
  to scaffold a new bundle, add concept documents, regenerate index.md listings,
  append to a change log.md, or validate/lint an existing bundle for conformance.
  Prefer this skill over ad-hoc markdown whenever knowledge should outlive a
  single conversation and be portable across tools.
  EQUALLY use this skill on the READING/consumption side: if the project has an
  OKF bundle (e.g. a knowledge/ directory), consult it FIRST to gather background
  and domain context during any task — understanding schemas, tables, metrics,
  pipelines, playbooks, business rules, or conventions before coding, debugging,
  answering, or planning. Use its read commands (list, search, show, context) for
  progressive disclosure and budgeted context-packing instead of blindly grepping
  raw source. Whenever you find yourself needing project knowledge an OKF bundle
  might hold, reach for `okf.py context` before reconstructing it from scratch.
---

# OKF — Open Knowledge Format toolkit

OKF represents knowledge as a directory tree of markdown files with YAML frontmatter. No schema
registry, no SDK, no required runtime: if you can `cat` a file you can read it; if you can `git clone`
you can ship it. It is meant to be authored and consumed by both humans and agents, which is exactly
why a project's durable knowledge belongs here instead of scattered ad-hoc notes.

**The single hard rule:** every non-reserved `.md` file has parseable YAML frontmatter containing a
non-empty `type` field. Everything else is recommended convention. Consumers are *permissive* — they
never reject a bundle for missing optional fields, unknown types, broken links, or missing indexes.
Author generously; the format degrades gracefully.

For the full rules (terminology, every field, citations, versioning), read `references/spec.md`. You
do not need it for routine authoring — the essentials are below.

## Use the bundled toolkit, don't hand-roll

`scripts/okf.py` (next to this SKILL.md) is the deterministic core. It is zero-dependency (uses
PyYAML if present, else a built-in *strict* parser — conformance is identical either way, so it never
depends on `pip list`) and handles the mechanical, error-prone work so you stay consistent across
every invocation. Reach for it first.

**Invocation.** This skill is installed globally but you run inside the user's project, so the script
is *not* in the project tree. Set `OKF` to the absolute path of `scripts/okf.py` inside this skill's
base directory (the harness gives you that absolute base path when the skill loads), then reuse it:

```
OKF="<this skill's base dir>/scripts/okf.py"   # e.g. ~/.claude/skills/okf/scripts/okf.py
python3 "$OKF" validate
```

(If you must derive it in shell and have GNU coreutils, `OKF="$(dirname "$(readlink -f
~/.claude/skills/okf/SKILL.md)")/scripts/okf.py"` works too — the skill may be symlinked.) A
regression suite lives at `tests/test_okf.py` (`python3 tests/test_okf.py`) — run it if you modify the
script.

| Command | What it does |
|---------|--------------|
| `okf.py init [dir] --title T --okf-version 0.1` | Scaffold a bundle (default dir: `knowledge/`) with a root `index.md`. |
| `okf.py new <path> --type "..." [--title --description --resource --tags]` | Create a concept document with correct frontmatter + conventional headings. `--type` is required. Creating into a dated dir (`learnings/`, `research/`, `plans/`, `specs/`) also prints the closest existing concepts — a strong match means update that concept instead. |
| `okf.py index [dir]` | Regenerate every `index.md` listing. Idempotent; rewrites only a sentinel-fenced block, preserving human prose above *and* below it. |
| `okf.py log "message" [--kind Update\|Creation\|Deprecation] [--dir scope]` | Append a dated entry to `log.md`, newest first. |
| `okf.py validate [dir]` | Check conformance. Exit 1 + `ERROR` lines for the hard rule; `WARN` lines for soft conventions (incl. dated-dir compression budgets). |
| `okf.py due [dir]` | List dated dirs over their compression budget with the oldest fold candidates (timestamp, id, backlink count). Exit 1 when folding is due, 0 otherwise. |
| `okf.py render [dir] [-o OUT]` | Pack the whole bundle into one HTML site (default: `<root>/site.html`). ```mermaid fences render as live diagrams via an exact CDN `mermaid.min.js` protected by SHA-384 SRI (they degrade to plain source text when offline or integrity fails). |

Commands discover the bundle root by climbing to the topmost `index.md` (preferring the one that
declares `okf_version`), and will descend into a `knowledge/` child if you run from the project root —
so they work whether you are inside the bundle or one level above it. Pass `--bundle`/`[dir]` to be
explicit when a repo has unrelated `index.md` files.

## Authoring workflow (producer side)

### 1. Starting a new bundle
Default location is `knowledge/` at the project root (keeps knowledge versioned alongside code —
"metadata as code"). Scaffold it, then immediately plan the concept layout from the project itself.

```
python3 "$OKF" init knowledge --title "<Project> Knowledge" --okf-version 0.1
```

Organize subdirectories by what makes the knowledge navigable (`tables/`, `metrics/`, `playbooks/`,
`references/`, `services/`, …). The directory tree is just grouping — real relationships come from
links between concepts, so don't agonize over the hierarchy.

### 2. Adding concepts
One concept = one `.md` file = one unit of knowledge (a table, an API, a metric, a process, an idea).
Generate the skeleton, then fill the body with *structural* markdown — headings, tables, code fences
beat prose for both human reading and agent retrieval.

```
python3 "$OKF" new tables/orders \
  --type "BigQuery Table" \
  --title "Orders" \
  --description "One row per completed customer order." \
  --resource "https://console.cloud.google.com/bigquery?...&t=orders" \
  --tags "sales,orders,revenue"
```

Pick a `type` that is descriptive and self-explanatory (`BigQuery Table`, `API Endpoint`, `Metric`,
`Playbook`, `Reference`). Types are free strings — no central registry — so consistency within a
bundle matters more than matching any external list.

If you author or edit a concept by hand, mirror `assets/concept.template.md` and keep the
spec-strict conventions below.

### 3. Spec-strict authoring conventions (this skill's defaults)
These maximize portability — the whole point of OKF.

- **Fill the recommended frontmatter**, not just `type`: `title`, `description`, `timestamp`.
  `description` is one sentence and shows up in index listings and search snippets, so make it carry
  weight. Use `resource` for any concept backed by a physical asset; omit it for abstract ones.
- **Cross-link with absolute bundle-relative paths**: `[customers](/tables/customers.md)`, not
  `../tables/customers.md`. This is OKF's recommended form — stable when a document moves within its
  subdirectory, and unambiguous for graph consumers. A link asserts a relationship; name the
  relationship in the surrounding prose ("joined with…", "depends on…") since the link itself is
  untyped. *Caveat:* a leading-`/` link resolves against the site/repo root in generic markdown
  renderers (GitHub, IDE preview, Obsidian), so it may render dead there unless the bundle sits at the
  root. `validate` checks these against the real bundle root; if human-rendered preview matters more
  than graph portability for your project, switch to relative links — both conform.
- **Use conventional headings** when applicable: `# Schema` (a column/field table), `# Examples`
  (fenced code), `# Citations` (numbered external sources at the bottom).
- **Anchor code claims to symbols, not line numbers**: cite `path` + a searchable name (function,
  index, constant) — `orders/repo.rs` `claim_next_order` — not `repo.rs:422`. Line anchors in active
  files decay within days and are the single largest staleness class every compaction pass finds;
  reserve them for immutable files (applied migrations, tagged snapshots).
- **Status claims expire — stamp them.** Any "PENDING", "shipped", "awaiting merge", "deployed"
  carries a commit hash, date, or verify-by condition; a bare status reads as current forever and is
  the fastest-rotting content in a bundle. Corollary: claims phrased as observable behavior outlive
  claims phrased as implementation status — contracts drift far slower than status notes.

### 4. Keeping indexes and logs current
`index.md` files give agents *progressive disclosure* — they see what exists before opening files.
After adding, moving, or renaming concepts, regenerate:

```
python3 "$OKF" index
```

It rewrites only the block between the `<!-- okf:listing:start -->` / `<!-- okf:listing:end -->`
sentinels; any prose you write *outside* the fence — title, intro above, notes below — is preserved
across regenerations. Record meaningful changes in a `log.md`:

```
python3 "$OKF" log "Added orders, customers, revenue concepts." --kind Creation
```

### 4b. Rendering a browsable site
When someone wants to *read* the bundle in a browser instead of a terminal — sharing it with
teammates, skimming the link graph, or a quick visual review — render it to a single HTML file:

```
python3 "$OKF" render            # writes <bundle-root>/site.html
python3 "$OKF" render -o /tmp/knowledge.html
```

The output is one HTML file with no sidecar files: a searchable sidebar tree, markdown-rendered
concepts with frontmatter chips and links-to/linked-from footers, hash routing (`#/<concept-id>`),
and a toggleable force-directed graph view. Mermaid rendering is its sole network dependency: the
page loads an exact CDN version protected by SHA-384 SRI; offline or failed-integrity loads leave
the readable diagram source in place. It is a read-only snapshot — re-run `render` after editing
concepts. Prefer it over hand-rolling any HTML export.

Two render-time synthetics exist only in the site, never on disk: a **Glossary** page (`#/glossary`)
aggregating the `- **Term**: definition` bullets (also `**Term:**` / `**Term** —` shapes) from every
`type: Contract` concept's `## Vocabulary` section, each entry linking back to its owning contract;
and reading-order pinning — `systems/start-here` and `systems/workspace-map` sort first in every
list. Both no-op gracefully in bundles without contracts or those concepts.

### 5. Always validate before finishing
Conformance is cheap to check and easy to drift on. Run it and resolve every `ERROR` (the hard rule);
treat `WARN`s as a quality checklist — they flag missing recommended fields, relative cross-links, and
broken links.

```
python3 "$OKF" validate
```

A clean run prints `CONFORMANT (OKF v0.1)` and exits 0. Report the result to the user.

## Compacting a bundle (periodic maintenance)

Dated dirs accumulate; `validate` warns on budget and `okf.py due` lists the fold backlog. When a
compaction pass is due — or before trusting an aging bundle — work in this order (rules calibrated
by a real 220-concept pass):

1. **Pre-filter mechanically before reading anything.** For each concept, extract the repo paths it
   cites and run `git log --oneline --since=<concept timestamp> -- <paths>`. Cited code untouched
   since the concept's timestamp ⇒ near-certainly current, skip it. In a full-read audit, over half
   the effort went to confirming docs that were already fine.
2. **Verify before rewriting.** Trust order: live code > bundle. Every rewritten claim carries
   evidence re-derived at write time (symbol, commit hash) — never copy a prior doc's citation
   forward unchecked.
3. **Second-check destructive proposals, then allow write-time refusal.** Every fold/supersede/delete
   gets an independent refutation attempt before execution: re-run `show <id> --links` AND a
   repo-wide text sweep for the slug. Even then, whoever writes the change skips any action whose
   cited evidence no longer holds at the file — ~5% of double-checked findings still fail there.
4. **Promote before you drop.** Ask "which concept owns this truth now?" — if none, promote the
   surviving nuggets into the owner first; supersede to a 2-3 line pointer while backlinks exist
   (title suffix "(superseded)", description prefix "SUPERSEDED —"); delete only when the backlink
   check and the text sweep are both empty.
5. **Split work safely.** Compaction split across sessions or parallel workers needs disjoint file
   ownership, whole fold chains (source + target + every backlink-holder) kept in one unit, and a
   `git status` check so in-flight edits are never touched.
6. **Sweep external surfaces.** Retiring an entry that concepts cite (a changelog section, a
   regression checklist item, a tracker ticket) orphans those citations — search the bundle for
   references in the same change.

Prefer small rolling passes (triggered by `due`, or folding at capture time) over rare big-bang
audits: in an actively developed repo roughly a third of concepts drifted within one quarter.
Project-specific scope rules (freeze windows, which dirs are in scope, supersede precedents) belong
in the bundle's own compression playbook and override the defaults here.

## Reading & gathering context (consumer side)
A bundle is only half useful if you only ever *write* to it. When you are working on a task and the
project has an OKF bundle, **pull relevant knowledge from it before reconstructing context from raw
source** — that is the entire point of authoring it. Follow progressive disclosure: survey cheaply,
then narrow, then expand along links. Do not load the whole bundle.

| Command | Use it to |
|---------|-----------|
| `okf.py list [--type T] [--tag X]` | Survey what exists (id, type, title, description from frontmatter only — cheap). |
| `okf.py search "<terms>"` | Find concepts by text; frontmatter matches rank above body. Returns ids + snippets. |
| `okf.py show <id> [--frontmatter] [--links]` | Read one concept; `--links` lists what it links to and what links to it (traverse the graph). |
| `okf.py context "<id or terms>" [--depth N] [--budget C]` | Assemble an injectable, budget-capped pack: seed concept(s) + their linked neighbors. |

The typical flow when you need project knowledge mid-task:

```
python3 "$OKF" search "orders revenue"      # 1. what does the bundle know about this?
python3 "$OKF" show metrics/revenue --links # 2. read the most relevant hit + see its neighbors
python3 "$OKF" context "revenue" --depth 1  # 3. pull a ready-to-use context pack (seed + neighbors)
```

`context` is the workhorse for AI workflows: give it a concept id *or* search terms, and it returns the
seed plus everything within `--depth` link-hops, concatenated under `## <id>` headers, capped at
`--budget` characters (default 20000). If the budget clips concepts it says so on stderr — never a
silent cut. Add `--backlinks` to also pull concepts that reference your seed, and `--json` when you
want structured fields rather than markdown. Because OKF links are relationships, this turns the
cross-links you authored into automatic context expansion at read time.

**Trust, but verify load-bearing facts.** A bundle is a fast, navigable *index* — not infallible
ground truth. It can drift from the code it describes. So for any fact you're about to *act* on where
being wrong is costly — exact limits/quotas, the provider/library in use, security and tenancy rules,
schema column names — open the concept's cited `resource:` (the source file) and confirm before
relying on it. Treat the bundle as the map; the code is the territory. This is cheap (the concept
already names the file) and catches the one stale value that would otherwise propagate into your work.

## When NOT to reach for the script
The script is for structure and conformance, not judgment. *You* decide what knowledge is worth
capturing, how to decompose it into concepts, what the prose says, and which concepts link to which.
That curation is the actual value — the toolkit just keeps the scaffolding correct and consistent.
