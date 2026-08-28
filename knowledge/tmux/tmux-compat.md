---
type: Concept
title: tmux compatibility philosophy
description: "The contract for a tmux-compatible zz CLI: tmux spellings keep tmux meaning or fail loudly, native GUI behavior uses zz-only verbs, and compatibility is measured against one pinned upstream commit."
resource: third_party/tmux-reference/UPSTREAM.md
tags: [tmux, compatibility, philosophy, reimplementation, cli]
timestamp: 2026-08-24T00:00:00-03:00
last_updated: 2026-08-28
last_updated_by: Codex
---

# Overview

zz's multiplexer is a Rust reimplementation of tmux behavior. It does not compile, link, or run
tmux, and no tmux C source is copied into the Rust code. Command names, aliases, configuration
grammar, key tables, formats, options, hooks, targets, and layout arithmetic are checked against one
pinned tmux commit recorded in
[`third_party/tmux-reference/UPSTREAM.md`](/references/tmux-upstream.md).

The product target is a **compatible enough CLI plus a native superset**:

1. A tmux command spelling means what it means in tmux.
2. If zz cannot honor that meaning, it returns a loud error. It does not reuse the spelling for a
   different GUI action.
3. Native behavior uses zz-only verbs such as `split-picker`, `split-browser`, `focus-sidebar`,
   `agent-send`, and `capture-browser`.
4. zz's default bindings may call those native verbs. A binding imported from tmux that names
   `split-window` still creates a terminal split.

This boundary lets the GUI be better than a terminal-emulated tmux surface without making copied
tmux config ambiguous.

# Pinned reference

The reference commit is tmux `d77c9dc6aa021e4bc61f0da128c591af695e6466`
(`next-3.8`). Important upstream ownership areas include:

| Behavior | Upstream files consulted |
| --- | --- |
| Tokenization and config loading | `cmd-parse.y`, `arguments.c`, `cfg.c` |
| Command catalog and aliases | `cmd.c`, `cmd-*.c` |
| Root, prefix, copy, and chooser tables | `key-bindings.c`, `window-copy.c`, `mode-tree.c` |
| Targets | `cmd-find.c` |
| Layout and geometry | `layout.c`, `layout-set.c`, `resize.c` |
| Options and environments | `options-table.c`, `options.c`, `environ.c` |
| Formats and status | `format.c`, `format-draw.c`, `status.c` |
| Hooks and jobs | `hooks.c`, `cmd-run-shell.c`, `cmd-wait-for.c`, `window.c` |
| Client and control mode | `server-client.c`, `control.c`, `control-notify.c` |

The pin is an oracle, not a dependency. Updating it is a separate compatibility event.

Oracle schema 4 records 92 commands, 78 aliases, and 572 accepted command-flag shapes: 318
valueless, 246 required-value, and 8 optional-value. Each command also carries positional minimum
and maximum metadata. It parses nine custom `args_parse` callbacks used by 14 commands and reduces
them to six effective rules. The remaining inventories contain 180 options, 198 global format-table
names, 14 source-enumerated names across three selected format contexts, 68 hooks, and 303 default
bindings across five tables. The context selection consists of 1 shared `command-item` name, 3
`list-commands` names, and 10 `list-keys` names. zz implements all 14. The 13 list-specific names
came first, and `formats.command-item-context` closed on 2026-08-24 when the shared `command` name
became a command-queue-item fact that every command the mux engine runs carries.

The same command-item hooks reach the five arguments that tmux expands: both rename names, both
show-option names, and `select-pane -T`. Each handler expands after target resolution in the old
target context. Directional `select-pane -T` reads the original pane and writes the expanded title
to the destination pane.

The selected context rosters do not describe tmux's whole context-format vocabulary. Queue state,
hook arguments, options, user options, and environment variables can contribute names at runtime.
The tracker treats that open-ended surface as semantic work.

The canonical check recaptures the inventory from a `tmux next-3.8` binary at the root of a clean
source checkout at the exact pin. The companion build stamp must also match the commit, version,
fetch recipe, and binary checksum. `ZZ_COMPAT_TMUX` may select another cache produced by that
fetcher; an unstamped checkout or an arbitrary prebuilt that reports the same version fails the
oracle check.

# Status authority

`compat/tmux-gaps.json` is the sole live TODO and status source for tmux compatibility. Schema 3
stores its update date, active gaps, and closed history. The generated
[gap report](/tmux/gaps.md) presents that registry for readers. `status` records an active gap's
product disposition as open, blocked, or accepted. `depends_on` records delivery order and does not
set status.

`just compat-check` calls `compat/check.sh`, validates the clean pinned oracle and registry, then
runs the full `zz-mux` library suite. The Rust gate reconciles upstream command and alias names,
flag arities, positional bounds, custom argument rules, option names, global and selected
context-format names, and hook names. It classifies native commands, native aliases, zz-only flags
on tmux command names, and every zz-only default key. It derives the guarded native-name roster from
the catalog minus the pinned oracle, then checks every pinned canonical prefix against the live
resolver. It pairs every
constant-backed format with a manifest item and tracks every missing default key across `root`,
`prefix`, `copy-mode`, `copy-mode-vi`, and `move`. For each shared default key, it reconciles the
rendered command and repeat bit or requires a named `binding:` divergence.

The gate does not prove that the runtime parser applies each inventoried `args_parse` rule,
open-ended or dynamic context-format names, nonconstant format behavior, hook production, runtime
behavior for shared bindings, or consumer truth for option `BEHAVES`.
`tracker.semantic-coverage` owns those six blind spots. Its command-specific `args-parse:` items name the
implemented callback commands; `choose-client` and `switch-mode` remain covered by their
unimplemented command items. Protocol v84 closes the first two runtime rules. `if-shell` preserves
unquoted typed branches across source-file and Control parsing, rejects typed conditions and option
values before effects, and leaves quoted braces as strings. `run-shell` accepts typed positionals
only when a leading `-C` enables command mode; option values and all positionals without that flag
remain strings. Ten `args-parse:` items remain.
Shared command-flag diagnostics closed on 2026-08-28. One
catalog-driven parser covers all 83 implemented upstream commands and 74 built-in aliases through
mux execution, daemon preflight, and stored commands. Exact native attach shares the leading-option
diagnostics, then stops scanning at its positional-session extension. The focused differential
compares 516 probes against both zz and the pin, including unknown and invalid flags, help usage,
missing values, required-value absorption, and optional-value lookahead. Parser-group atomicity and
the remaining custom `args_parse` behavior stay separate tracker work. Differential scenarios,
attached-client fixtures, unit tests, and manual GUI checks supply behavioral evidence.

The [2026-08-22 CLI compatibility audit](/research/2026-08-22-tmux-cli-compatibility-audit.md)
preserves the measured baseline at commit `202f322`. Its counts describe that audit date. The
[divergence matrix](/tmux/divergences.md) keeps the source rationale and probe evidence behind
accepted differences. Neither document tracks live completion.

# What “compatible enough” means

The alias goal is not a percentage. It is a workload contract:

- Core session, window, pane, buffer, target, layout, and query commands used from a shell work.
- `new-session`, bare attach, reattach, read-only attach, detach, and kill preserve the calling TTY
  contract.
- A user's config and the pinned plugin smoke corpus load without a SKIP. Any SKIP fails the run.
- Script-facing stdout, stderr, exit status, formats, and errors match where the workload observes
  them.
- Ordinary `capture-pane` text follows tmux's `-p` versus named/automatic-buffer routing, clustered
  value flags, inclusive and reversed ranges, target-scoped bound expansion, and invalid-bound
  fallback. Trailing blank rows at a fallback visible end, richer raw and metadata transports, and
  saved-alternate capture remain excluded.
- Bindings explicitly declared in an imported tmux config retain tmux command meaning. Import does
  not synthesize stock bindings absent from the file; zz's own defaults remain free to use native
  verbs.
- Any accepted divergence is named, tested where possible, and excluded from the promise.
- Missing low-value models do not hold the alias hostage.

One loud error-precedence edge sits outside that workload promise. From a nested client,
`new-session -s existing` reports zz's nesting refusal before the mux sees the duplicate name;
pinned tmux reports the duplicate first. Both reject without changing state.

The compatibility gate should name the supported workload and its exclusions. “80 commands” alone
is too weak because a command can still reject flags or produce different output. “All 92 commands”
is too expensive because linked sessions, shared-server ACLs, and tmux floating panes do not fit the
zz model.

# Permanent exclusions

- **Real tmux socket protocol.** `alias tmux=zz` means zz handles the argv. zz never speaks tmux's
  private client/server wire format.
- **Linked windows and session groups.** A zz window belongs to one session. `link-window`,
  `unlink-window`, and `new-session -t` stay loud.

Floating tmux panes, client suspension, and shared-server ACLs are parked, not part of the practical
alias target. They should be revisited only if a real workload needs them and their semantics fit
the product.

# Native presentation

tmux draws status rows, prompts, choosers, copy mode, pane indicators, menus, and popups with terminal
cells. zz publishes daemon-owned state and renders it in its clients:

- `status-format[]` rows render in the TUI and in a top or bottom GUI row when a config customizes
  status. Native sidebar and titlebar presentation remain at defaults.
- Prompts, choosers, menus, popups, copy mode, and pane indicators use native surfaces.
- A native surface may look different. Its command, key, target, exit, and state semantics remain
  part of the compatibility contract.

This is a presentation divergence, not permission to reinterpret a tmux command.

# Config ownership

By default the daemon sources the first existing zz-owned platform candidate for `zz/mux.conf`:
XDG config, the home config directory, macOS Application Support, or Windows AppData in platform
order. One or more top-level startup `-f` files replace that default for the initial load and remain
visible through `#{config_files}`. `reload-config` returns to the first existing platform candidate
and updates the fact to that path, or to empty when no candidate exists. The import flow copies a
donor `.tmux.conf` to the first existing or first constructible candidate. The daemon does not read
`~/.tmux.conf` directly on every boot.

The config parser implements tmux grammar and reports unsupported commands. On Unix, shell and
status jobs spawned by the daemon receive a private `tmux` PATH shim so their subprocess calls
return to the same zz daemon. A user's shell alias does not cover direct process lookup, and
packages do not currently install a global shim.

# Empty boot and attach

A daemon started by a command query begins with no sessions, windows, or panes. The first explicit
`new-session` gets numeric name `0` and ids `$0`, `@0`, and `%0`, matching the pin's allocation.

The installed bare launcher rewrites an empty argv to `new-session -A`. That existing tmux-shaped
verb creates session zero on an empty daemon and attaches the current session on a live daemon.
Explicit targetless `attach` and `attach-session` still preflight the server and return tmux's exact
`no sessions` with exit 1, so `attach || new-session` keeps working. The daemon's lower-level lazy
attach remains serialized: simultaneous first attaches and a command client creating a session at
the same boundary converge instead of creating duplicates or failing. Therefore:

- bare packaged `zz` creates and attaches session zero on an empty daemon;
- bare packaged `zz` attaches the current session when one exists;
- `zz attach` and `zz attach-session` return `no sessions` on an empty daemon;
- `zz new -s NAME` creates and attaches on a TTY;
- `zz attach -t NAME` attaches an existing session;
- direct bundle launch or `zz app` opens the GUI.

Attaching `new-session` also applies the same nested-session refusal as `attach-session` before mux
state changes. The packaged PTY fixture pins detached dash sizing, attached client dimensions,
read-only input rejection and output visibility, requested detach, and `attach -d` peer eviction
through the real spaced-path launcher. Both detach paths require exit zero and the tmux-shaped
`[detached (from session NAME)]` notice after terminal restoration.

# Related

- [live tmux compatibility gaps](/tmux/gaps.md)
- [tmux CLI compatibility audit](/research/2026-08-22-tmux-cli-compatibility-audit.md)
- [tmux divergence matrix](/tmux/divergences.md)
- [tmux drop-in plan](/designs/tmux-drop-in.md)
- [tmux superset roadmap](/designs/tmux-superset-roadmap.md)
- [tmux commands](/tmux/commands.md)
- [key tables](/tmux/key-tables.md)
- [status line and formats](/tmux/status-line.md)
- [configuration parser](/tmux/conf-parser.md)
- [compatibility harness](/playbooks/compat-harness.md)
