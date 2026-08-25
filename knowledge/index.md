---
okf_version: "0.1"
---

# zz Knowledge

Agent- and human-readable knowledge for **zz**, a cross-platform GPUI workspace that multiplexes
native terminal panes (libghostty-vt + daemon-owned PTYs), Chromium browser panes (CEF Alloy
off-screen rendering), and native ACP Agent panes over a persistent local daemon with a tmux-style
model.

**Start here:** [System overview](architecture/overview.md) → [Process model](architecture/process-model.md)
→ [Data flow](architecture/data-flow.md). Then dig into a [crate](crates/index.md) or a subsystem below.

Each directory below has an `index.md` listing its documents; read that first, then the document.
Load-bearing facts (exact versions, hashes, wire constants) cite a `resource:` source file . verify
there before acting, because source is the ground truth and these pages are a map of it.

| Section | What lives here |
|---------|-----------------|
| [architecture](architecture/index.md) | System overview, process/threading model, end-to-end data flow |
| [crates](crates/index.md) | Crate-level concepts for `zz` and its major `zz-*` support crates |
| [protocol](protocol/index.md) | Wire protocol (v76), stable IDs, packed terminal lanes, snapshots |
| [tmux](tmux/index.md) | tmux-compat philosophy, config parser, key tables, commands, copy-mode, choosers |
| [terminal](terminal/index.md) | libghostty embedding, Zed rendering parity, interaction, appearance |
| [browser](browser/index.md) | CEF runtime, OSR rendering + DPI/blur fix, lifecycle, profile, input, element picker |
| [concepts](concepts/index.md) | Cross-cutting: Agent pane, PTY worker, terminal frame, split-pane layout, session persistence, command palette |
| [configuration](configuration/index.md) | Application configuration and [UI design conventions](configuration/ui-conventions.md) |
| [designs](designs/index.md) | Feature plans and decision records, each carrying a `status:` field |
| [playbooks](playbooks/index.md) | Prerequisites, running zz, building/updating the CEF bundle, updating the tmux reference |
| [research](research/index.md) | Dated investigations that preserve reproduction evidence, source analysis, and unresolved proof gaps |
| [references](references/index.md) | External pins: CEF artifacts, GPUI revision, tmux upstream, Ghostty colors |

<!-- okf:listing:start (managed by okf.py index — edit prose outside this fence) -->
# Subdirectories

* [architecture](architecture/index.md)
* [browser](browser/index.md)
* [concepts](concepts/index.md)
* [configuration](configuration/index.md)
* [crates](crates/index.md)
* [designs](designs/index.md)
* [playbooks](playbooks/index.md)
* [protocol](protocol/index.md)
* [references](references/index.md)
* [research](research/index.md)
* [terminal](terminal/index.md)
* [tmux](tmux/index.md)
<!-- okf:listing:end -->
