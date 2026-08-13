# OKF v0.1 — condensed reference

Authoritative source: <https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md>
This file is the working subset the skill relies on. When in doubt, the upstream SPEC wins.

## Table of contents
1. What OKF is
2. Terminology
3. Conformance (the rules that actually bind)
4. Frontmatter fields
5. Body conventions
6. Cross-linking
7. Reserved files: index.md
8. Reserved files: log.md
9. Citations
10. Permissive consumption

## 1. What OKF is
An open, human- and agent-friendly format for representing *knowledge* — the metadata, context,
and curated insight around data and systems. Physically: a directory tree of UTF-8 markdown files
with YAML frontmatter. No schema registry, no central authority, no required tooling. Authored by
people or agents, exchanged across orgs, consumed by both.

Design priorities: **readable** by humans without tooling, **parseable** by agents without bespoke
SDKs, **diffable** in version control, **portable** across tools/orgs/time.

## 2. Terminology
| Term | Meaning |
|------|---------|
| Knowledge Bundle | Self-contained directory tree of knowledge docs. Unit of distribution (git repo, tarball, or subdir). |
| Concept | One markdown document = one unit of knowledge (a table, API, metric, process, idea). |
| Concept ID | The file path within the bundle minus `.md`. `tables/users.md` → `tables/users`. |
| Frontmatter | YAML block delimited by `---` at the top of a file. |
| Body | Everything after the frontmatter. |
| Link | A standard markdown link from one concept to another; asserts a relationship. |
| Citation | A link to an external source backing a claim. |

The directory structure is **independent of the domain** — organize however suits the knowledge.
Hierarchy expresses grouping; *links* express the real semantic graph.

## 3. Conformance — the only hard rules
A bundle is conformant with OKF v0.1 iff:
1. Every non-reserved `.md` file contains a **parseable YAML frontmatter block**.
2. Every such frontmatter contains a **non-empty `type` field**.
3. Reserved files (`index.md`, `log.md`) follow their structure (§7, §8) when present.

Everything else in this document is *recommended*, not required.

## 4. Frontmatter fields
````yaml
---
type: <Type name>                  # REQUIRED — the one hard rule
title: <display name>              # recommended
description: <one-sentence summary># recommended — used in indexes, search snippets, previews
resource: <canonical URI>          # recommended when the concept describes a physical asset
tags: [<tag>, <tag>]               # optional — cross-cutting categorization
timestamp: <ISO 8601 datetime>     # recommended — last meaningful change
# …any producer-defined keys allowed; consumers preserve unknown keys
---
````
- `type` is a free string. Pick descriptive, self-explanatory values (`BigQuery Table`,
  `BigQuery Dataset`, `API Endpoint`, `Metric`, `Playbook`, `Reference`). NOT registered centrally;
  consumers must tolerate unknown types as generic concepts.
- Omit `resource` for abstract concepts (a metric, a process) that have no physical asset URI.
- Spec-strict default for this skill: always fill `type`, `title`, `description`, `timestamp`.

## 5. Body conventions
Standard markdown. Favor **structural** markdown (headings, lists, tables, fenced code blocks) over
freeform prose — structure aids both human reading and agent retrieval. No body section is required.
Conventional headings, used when applicable:
| Heading | Purpose |
|---------|---------|
| `# Schema` | Structured description of an asset's columns/fields (usually a table). |
| `# Examples` | Concrete usage examples, often fenced code blocks. |
| `# Citations` | External sources backing claims (§9). |

## 6. Cross-linking
Concepts link via standard markdown links. Two forms:
- **Absolute (bundle-relative)** — begins with `/`, resolved from the bundle root:
  `[customers](/tables/customers.md)`. **Recommended / this skill's default** — stable when a
  document moves within its subdirectory.
- **Relative** — standard relative paths: `[neighbor](./other.md)`.

A link from A to B asserts an (untyped) *relationship*; the kind (joins-with, depends-on, parent…)
lives in the surrounding prose, not the link. Consumers building a graph treat links as directed
edges. **Broken links are tolerated** — a missing target may just be not-yet-written knowledge.

## 7. Reserved file: index.md
A directory listing supporting **progressive disclosure** — lets a reader/agent see what's available
before opening documents. MAY appear in any directory.
- Contains **no frontmatter**, with one exception: the bundle-root `index.md` MAY declare
  `okf_version: "0.1"` in a frontmatter block (the only place frontmatter is allowed in an index).
- Body is one or more sections grouping links under headings. Entries SHOULD carry the linked
  concept's `description`. Index links to direct children are conventionally relative
  (`tables/index.md`, `orders.md`).
````markdown
# Subdirectories

* [tables](tables/index.md) - transactional tables

# Concepts

* [Orders](orders.md) - one row per completed order
````

## 8. Reserved file: log.md
Records the history of changes to its scope. MAY appear at any level. Flat list of date-grouped
entries, **newest first**. Date headings MUST be ISO 8601 `YYYY-MM-DD`. The leading bold word
(`**Update**`, `**Creation**`, `**Deprecation**`) is convention, not requirement.
````markdown
# Update Log

## 2026-05-22
* **Update**: Added [Customer Metrics](/tables/customer-metrics.md).
* **Creation**: Established the [Dataplex Playbook](/playbooks/dataplex.md).
````

## 9. Citations
When the body makes externally-sourced claims, list them under a trailing `# Citations` heading,
numbered. Links MAY be external URLs, bundle-relative paths, or paths into a `references/` subtree
that mirrors external material as first-class concepts.
````markdown
# Citations

[1] [BigQuery public dataset announcement](https://cloud.google.com/blog/...)
[2] [Internal data quality runbook](https://wiki.acme.internal/data/quality)
````

## 10. Permissive consumption (why the format survives churn)
Consumers MUST NOT reject a bundle for: missing optional frontmatter fields, unknown `type` values,
unknown extra frontmatter keys, broken cross-links, or missing `index.md` files. This is intentional:
OKF must stay useful as bundles grow, get refactored, and are partially generated by agents.

## Versioning
`<major>.<minor>`. Minor = backward-compatible additions (new optional fields, new conventional
headings). Major = breaking changes (renaming required fields, changing reserved filenames). A bundle
MAY declare its target via `okf_version: "0.1"` in the root `index.md` frontmatter.
