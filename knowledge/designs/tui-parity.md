---
type: Design Plan
title: TUI parity campaign
description: "The terminal-client parity contract: tmux observable behavior by default, zz additions through superset commands, and proof tied to the tested revision."
status: Contract agreed 2026-09-09; cycle 1 (fixture baseline) integrated 2026-09-09 at 38c22b9e with proof banked and 0/12 verified pending the macOS clause of TUI-001; runtime parity work has not started
resource: compat/tui/campaign.json
tags:
- tui
- tmux
- compatibility
- campaign
timestamp: 2026-09-09T20:25:23Z
---

# Overview

The terminal client must support `alias tmux=zz` with the same observable results as the project's
pinned tmux. Additional zz commands expose sidebar, picker, browser, Agent, and editor behavior.
All commands remain available. There is no compatibility profile, feature activation mode, or
terminal-width threshold that opts a user into extra UI.

Fabrico agreed to this contract on 2026-09-09. It supersedes the automatic-sidebar direction in the
[original TUI design](/designs/tui-client.md) for terminal clients. That page still describes the
implementation we start from. The [campaign report](/tmux/tui-parity.md) records progress and the
[campaign playbook](/playbooks/tui-parity-campaign.md) describes the cycle: one worker lane, one
adversarial reviewer, one gate.

# Observable contract

Use the same tmux revision, effective configuration, initial session state, environment, outer
terminal capabilities, and dimensions on both sides. The reference remains
`d77c9dc6aa021e4bc61f0da128c591af695e6466`, recorded in `compat/tmux-oracle.json` and acquired through
`compat/fetch-tmux.sh`. A reference update is an explicit scope change.

| Surface | Required comparison |
| --- | --- |
| CLI | Exact stdout bytes, stderr bytes, exit status, and resulting session state for the command under test |
| Terminal screen | Every cell's glyph, width, foreground/background, text attributes, and selection/search presentation |
| Cursor | Position, visibility, and shape at each named checkpoint |
| Geometry | Pane and window dimensions, borders, status rows, overlay extent, clipping, and terminal size reports |
| Input | Target and result of keys, repeats/releases where supported, paste, mouse, focus, and configured bindings |
| Lifecycle | Attach, detach, resize, restore, and multiple-client behavior within the recorded test matrix |

Compare decoded terminal state. Equivalent escape-sequence spellings, batching, redraw frequency,
and raw output-stream encoding need not match. Timing checks assert observable ordering and bounded
response under stated conditions, rather than identical scheduling or frame timestamps.

Keep dynamic values deterministic through fixture setup: named sessions, fixed pane titles,
controlled content, and explicit environment. If a value cannot be fixed, list the exact field and
its comparison rule in the fixture contract. A whole row, whitespace, a style, or a cursor cannot
be omitted merely because it differs. Preserve raw captures alongside decoded differences when
they help reproduce a failure.

# Commands and presentation

| Command family | Contract |
| --- | --- |
| tmux commands and default bindings | Preserve the pinned command meaning, input routing, and visible result |
| `split-window` | Create a terminal split, including invocation through an unchanged stock binding |
| `choose-tree` | Present the tmux chooser and its interaction |
| `focus-sidebar` | Access the zz session tree through the explicit command |
| `split-picker`, `split-browser`, other zz verbs | Keep their existing declared superset behavior and preserve surrounding terminal commands |

Users may bind zz commands through the normal key-binding system. Local chrome must not consume
ordinary tmux/application keys outside the input context that owns them. Sidebar visibility belongs
to the attached client. Showing it in one client must not select an extra presentation mode in
another client.

Keep one daemon, shared client core, command catalog, and TUI renderer. Change the existing paths
that violate the contract; extract a component only where the current slice needs the boundary.
Shared daemon or protocol changes must preserve other clients. A GUI may retain its native
presentation, but that presentation must not redefine a tmux command for the TUI.

Default launch, default bindings, and configuration precedence require direct observation. Use
isolated config roots for a stock baseline, then test explicit `-f`, tmux roots, and zz config
layering as named cases. Do not silently remove an existing user configuration policy to make
a baseline pass; record any conflicting policy under its existing tmux gap and resolve the TUI
contract there.

# Milestones

1. **Everyday terminal use.** Establish reliable fixtures and a complete screen comparison, then
   prove stock launch/keys, splits, status, borders, and resizing. Include 80, 100, 109, and 120
   columns so the current automatic sidebar cannot hide behind a narrow-terminal test.
2. **Interactive surfaces.** Prove pane copy/search, choosers and command output, prompts,
   confirmations, menus, popups, display-panes, and mouse/key ownership across those surfaces.
3. **Broader parity and command composition.** Exercise a declared terminal-capability matrix,
   slow output/recovery, the remaining stock client-command inventory, multiple clients, and
   existing zz commands beside terminal panes.

The initial ledger contains twelve work packages. Their count measures completed packages, not
the percentage of all tmux behavior or remaining engineering effort. Split a large package into
new stable IDs when measurement warrants it; retain the original obligation and make its closure
depend on the children. Additions appear separately from the fixed initial baseline.

# Proof and ownership

`compat/tui/campaign.json` owns TUI obligations, dependencies and proof status.
`compat/tmux-gaps.json` continues to own existing behavioral differences and
their disposition. Reference its active or closed IDs from a TUI obligation. An old accepted or
closed gap does not prove the new screen and interaction contract. When a measurement contradicts
a recorded decision, update the TUI portion of that existing gap with its reproduction and retain
the GUI scope where applicable.

A package reaches `verified` only when every acceptance clause has passing evidence and an
independent review at the recorded candidate revision. Keep commands, environment, tmux pin,
captures/results, review, and remaining findings together. New code without proof stays `active`
or `review`. A timeout, missing capability, waived comparison, or skipped required case cannot
count as success.

The tracker validates records, references, dependencies, and report freshness. It cannot decide
whether a test proves the claimed behavior. The reviewer and final verification own that judgment.
Stored proof remains evidence for its recorded revision; after a relevant source change, rerun the
affected proof before claiming it holds for the new revision. Keep the older artifact for history.

Reuse the existing differential fixtures and pinned reference. The first proof work is to diagnose
the recorded geometry-fixture timeout and retain failure diagnostics. Extending full-screen
comparison belongs to the second package; this setup does not claim that comparison already exists.

# Scope boundary

This campaign covers terminal-client compatibility and composition of existing zz commands.
Developing new Agent/editor interfaces, redesigning native GUI chrome, changing the tmux pin,
replacing the terminal engine, and implementing tmux socket interoperability are separate work.
There is no tmux visual counterpart for a browser or the zz sidebar; their tests assert the zz
command contract and the behavior of standard terminal commands around them.

Old campaign percentages, current unit tests, selected status-row matches, and successful command
queries are useful prior evidence. None establishes complete attached-screen parity. Report each
milestone's measured matrix and remaining obligations when it closes.

# Sources

- `compat/tmux-oracle.json`, `compat/tmux-gaps.json`: reference inventory and existing gap ownership.
- `compat/tui/campaign.json`: current TUI obligations; `compat/tui/run-1.js`: the cycle-1 runner.
- `compat/status-row.sh`, `compat/tui-pane-geometry.sh`, `compat/attached-client.sh`: existing proof surfaces.
- `crates/zz-tui/src/render.rs`, `state.rs`, `input.rs`, `tty.rs`: terminal presentation and input paths.
- `crates/zz-client/src/chrome.rs`, `crates/zz-protocol/src/key.rs`: chrome and shared default bindings.
