---
type: Design Plan
title: TUI parity campaign
description: "The terminal-client parity contract: tmux observable behavior by default, zz additions through superset commands, and proof tied to the tested revision."
status: "Contract agreed 2026-09-09; cycles 1, 3, 4, 5, 6, 7 and 8 integrated (8/12 verified: TUI-001/002/003/004/005/007/009/010); wire protocol 102 shipped in zz 0.9.0 and 0.9.1 on 2026-09-14, so the next append opens 103 and compat/wire-version.py enforces it; cycle 9 ran its three lanes on alienware and was stopped before any gate for a machine move, leaving four branches unmerged (see compat/tui/HANDOFF.md): the mouse branch REJECTED on two regressions its own fixture cannot see, the choosers branch unreviewed with TUI-006 at review on its worker's account, the introspection branch approved with fixes; TUI-011 waits on its five children TUI-014 to TUI-018 and TUI-012 holds a complete proof at review behind TUI-008; cycle 10 (run-10.js) is written as stream, modes and capture, and its last gate closes the twelve-item baseline; lane zones are drawn around obligations rather than crates, and a verified claim is re-measured by compat/tui/verify-claims.py; the TUI portion of accepted native-presentation gaps follows this contract (triage 2026-09-10); TUI-013 (the macOS timeout) deferred as added scope"
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

Amendment 2026-09-09 (fabrico): the macOS reproduction half of TUI-001's second clause moved to
the added-scope obligation TUI-013 so the campaign continues on Linux while the macbook is out of
reach; TUI-001 verified under the amended clause (diagnostics retention proven by a driven
failure), the deferral dated in both ledger records and reversible by folding TUI-013 back.

Triage 2026-09-10 (orchestrator, at the cycle-4 close-out): an accepted gap that keeps a native
presentation (`options.native-mode-styles`, `options.native-overlay-styles`,
`choosers.native-presentation`, `presentation.native-status`) does not waive the raw TUI's cells.
Under the commands-and-presentation rule above, the raw TUI renders the pin's surface and the GUI
keeps its native one. A landing that makes the raw TUI honour an item closes that item with a dated
measurement, as cycle 4 closed `options.theme-palette`; the gap's decision stands for the GUI and for
every item still open. Cycle 4's gate held TUI-004 over exactly this question.

Amendment 2026-09-14 (fabrico), under the superset principle that a niche or non-performant tmux
behaviour is reconsidered rather than imitated:

- **Locking is delegated to the OS session.** The pin arms `lock-after-time` and spawns
  `lock-command` onto a client's tty, which works because its server owns that terminal. zz's daemon
  publishes frames and each client draws them, so there is no terminal to spawn onto and a desktop
  window running `lock -np` has no meaning. Building a zz lock surface would mean inventing an idle
  timer, a per-client lock screen, and ownership and cancellation rules for a feature that protects
  nobody who already holds the machine. zz does not build one. `lock-client`, `lock-server` and
  `lock-session` keep the pin's exact stdout, stderr, exit status, target validation and
  `after-lock-server` hook, and `lock-after-time` and `lock-command` stay accepted options that arm
  nothing. TUI-015's acceptance clauses were amended to this decision, the way TUI-001's macOS clause
  was amended on 2026-09-09.
- **The caller stream channel is built, not trimmed.** TUI-018 keeps its full scope: one bounded
  command-stream channel covering stdin, stdout, binary bytes, backpressure, cancellation and
  process lifetime, carrying `source-file -`, `save-buffer -`, `display-message -I` and
  `split-window -I` beside the `load-buffer -` pipe zz already supports. It is milestone 5 of the
  superset roadmap and the last obligation between this campaign and its twelfth item.

Amendment 2026-09-18 (fabrico), under the same principle: **capture returns the spaces a tab left
on screen.** Since tmux 3.4 the pin marks every cell a tab produced (`GRID_FLAG_TAB`) and
`capture-pane` prints a literal tab there whatever its flags (`grid.c:1202`); once an edit removes
the head of such a tab it also drops the padding cells left behind. Cycle 11 imitated this with tab
spans in spare Ghostty cell bits. Review measured the cost at +68% CPU on output that overwrites
tab-bearing rows and +33% with a tab stop on every column, and every later edit (insert, delete,
erase, line insert and delete, scrolling) had to keep the span honest. zz does not imitate the
marker: its capture returns the cells the screen shows, the span tracking left the patch, and
TUI-017 files each tab case under `decided:TUI-017` in `compat/tui-client-commands.sh`. The
indexed-colour class bits stay, because they cost nothing measurable and the pin's `38;5;1` is a
real colour class, not bookkeeping. (Superseded the same day by the ruling below.)

Amendment 2026-09-18 (fabrico), superseding the "keep the cheap indexed-colour bits" half of
the tab ruling: **zz carries no patch on the vendored terminal engine.** The remaining 171
lines went: the indexed-colour class plumbing (`fg_indexed`/`bg_indexed`) and the one ICH hunk
that kept the pin's stale cells after an insert wider than the cells it moves. Two asserted
cases lost their basis and are filed under `decided:TUI-017`:
`capture-low-indexed-colour` (the pin's `capture-pane -e` re-emits the colour class, so
`38;5;1` comes back as indexed 1 while zz returns named red, because libghostty-vt stores both
the same way) and `capture-edited-tab-ich-off-line` (the wide insert clears differently
without the hunk). The safe wrapper vendored to reach those fields went with them:
`libghostty-vt` resolves to upstream again and `build.rs` builds pristine Ghostty. The
divergence is accepted: keeping any patch means carrying a fork of the engine's grid semantics
for two capture spellings, which fabrico ruled not justified.

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
