---
type: Design Plan
title: Input-reliability audit and hardening plan
description: The 2026-08-03 four-sweep audit of the bug classes behind the dead-chord field reports . stale-context panics, silent command failures, focus-path listener gaps, strandable pairing state, and stale memo digests . with every confirmed site and the agreed fix order.
status: Shipped (residuals listed at the end)
tags:
- mux
- input
- daemon
- focus
- reliability
timestamp: 2026-08-03T00:00:00Z
---

# Why this document exists

A day of dogfooding with the incident toolkit (markers, stall sampler, chord trace, focus
sensors) turned three vague field reports into five *bug classes*, each fixed once where it
bit. This audit sweeps the whole app for the remaining members of those classes. Four
parallel read-only sweeps produced the findings below; nothing here is speculative . every
site was verified against the code, with file:line. Line numbers are as of commit `a4b3c9a`.

The five classes, named after their type specimen:

1. **Dead-reference trust** (the `resolve_pane` bug): a per-connection `ExecutionContext`
   id used after the thing it names died on another thread; `repair_context` runs only
   after a *successful* execution, so the divergence is self-sustaining.
2. **Silent failure** (the chord-error bug): a command with no requester whose `Err`
   reaches neither a log nor the user.
3. **Focus-path listener gaps** (the `C-a s` bug): gpui dispatches key events along the
   focus path only; a listener mounted on a subtree misses events while a sibling
   (shell-hosted sidebar, dialog layer, second window) holds focus.
4. **Strandable pairing state** (the `PrefixClaim` bug): hand-tracked begin/end state
   whose end event can be missed, leaving "held/dragging/armed" latched forever.
5. **Stale memo digests** (the `SynchronizeSignature` bug): a render-skip digest that
   stores entry-time state or omits an input the guarded pass reads, masking later
   divergence.

# Class 1+2, daemon side . stale context ids

## Eleven daemon-panic sites (stale id → direct map index)

A dead session/window in a connection's context reaches `BTreeMap` indexing and panics the
daemon, killing every session. Four sit behind **default prefix bindings**.

| Site | Command / trigger | Path |
| --- | --- | --- |
| `zz-mux/src/command.rs:673` | `next-window` / `previous-window` (`prefix n`/`p`) | `step_window` → `sessions[&session]` |
| `zz-mux/src/command.rs:1633` | `prefix $` (rename-session prompt, `#S` expansion) | `expand_prompt_input` → `sessions[&session]` |
| `zz-mux/src/command.rs:1641` | `prefix ,` (rename-window prompt, `#W`) | `expand_prompt_input` → `windows[&window]` |
| `zz-mux/src/command.rs:1402` | `next-layout` (`prefix Space`), `select-layout` | `windows[&window].active_pane` |
| `zz-mux/src/model.rs:1049` | any `resolve_window(None, …)` caller with stale session | `sessions[&session].active_window` |
| `zz-mux/src/model.rs:1065` | `select-window -t <index>` with stale context session | `sessions[&session]` |
| `zz-mux/src/model.rs:1074` | `select-window -t <name>` ditto | `sessions[&session]` |
| `zz-mux/src/command.rs:548` | bare `attach-session` with dead context session | `sessions[&session].active_window` |
| `zz-mux/src/command.rs:607` | `list-windows` | `sessions[&session]` |
| `zz-mux/src/command.rs:659` | bare `select-window` with stale context window | `windows[&window].session` |
| `zz-mux/src/command.rs:1296` | `list-panes` | `windows[&window]` |

Root causes, two resolver twins of the fixed `resolve_pane` defect:

- `resolve_session` (`model.rs:1014`) returns `current` without `sessions.contains_key`;
  its any-session fallback only fires on `None`, never on `Some(dead)` . the recovery path
  exists but is unreachable in the failure case.
- `resolve_window` (`model.rs:1044`) returns `current_window` unchecked.
- `step_window` (`command.rs:673-674`) additionally reads `context.window` raw, bypassing
  resolvers; a stale id silently teleports to window index 0 rather than erroring.

Non-panicking members of the same class (silent `MissingTarget` loops until an
explicit-target command heals the context): `rename-window`, `kill-window`, `last-pane`
(`prefix ;`), `rotate-window` (`prefix C-o`), layout cycling, `set-window-option`,
`rename-session`, `kill-session`, and . nastiest . `new-window` (`prefix c` silently stops
creating windows).

## Adjacent findings

- **Seven pre-empt verbs escape `repair_context` even on success** (`daemon.rs:1313-1340`):
  `capture-pane`, `agent-send`, `send-last-output`, `capture-browser`, `debug-marker`,
  `tools`, `set-buffer`/`show-buffer` return before `MuxEngine::execute`'s repair line, so
  a broken context survives unbounded successful calls.
- **`resolve_pane`'s new fallback has a blast-radius flaw** (`model.rs:1097-1104`): with
  pane and window both dead it falls back to `default_context()` . the *first session by
  id*, not the client's . so `send-keys`/`split-window` can silently retarget into an
  unrelated session. Prefer the client's attached session.
- `default_context` (`model.rs:1003`) gives up if the first session's `active_window`
  dangles instead of trying the next session . the last-resort recovery path should not
  have a single point of failure.
- Load-bearing unwritten invariants worth comments/asserts: `command.rs:683` divides by
  `windows.len()` (zero windows in a session = panic); `command.rs:975` is safe only via
  `join_pane`'s cannot-move-last-window guard.

# Class 2, client side . every GUI command failure is invisible

`MuxClient::execute_on_host` (`zz/src/mux/client.rs:1789-1820`) **drops the request id**
returned by `InteractiveClient::execute`, so error responses (which carry a nonzero id)
land in the catch-all (`client.rs:2696`) and set `self.error` . which is only ever
*rendered* when no session is attached, as a full-screen connection banner. In normal use a
failed GUI command produces **no toast, no log line at any level, and no UI change**; the
next successful command (e.g. the `select-pane` sent by any mouse-down) wipes the state.

Notable consequences, ranked:

- **Drag-drop lies** (`view.rs:1497-1505`): `swap-pane`/`join-pane` sets an *optimistic*
  `pane_layout_override` before sending; a rejection produces no snapshot bump, so the UI
  keeps showing a layout the daemon never applied.
- **Fleet hosts are worse** (`client.rs:2208-2217`): responses from non-attached hosts are
  discarded before the catch-all. The sidebar's delete/`+`/rename buttons all use
  `execute_on_host` against arbitrary rows' hosts.
- **Sticky banner**: a second `close_active_pane` on an already-exited last pane returns
  `PaneExited` with a *nonzero* id, missing the teardown-race arm → full-screen
  "pane has exited" where the empty workspace belongs, uncleanable.
- The full fire-and-forget inventory (pane-type picker, unzoom chip, window pills,
  browser/agent/editor metadata like `set-agent-session` . whose failure silently breaks
  agent restore-after-restart . `reload-config`, overlay confirms that close before
  executing) is in the audit transcript; the daemon-side twins are
  `select_display_pane` (`daemon.rs:3416`) and `activate_choose_tree_target`
  (`daemon.rs:3488`), the two gesture paths that did not get the `key_command_failed`
  treatment.

The model to copy is the command-prompt path (`daemon.rs:3807`): on `Err`, publish
`EventPayload::ClientMessage { kind: Error }` → GUI toast.

# Classes 3+4 . listener scope and strandable pairing state

gpui facts verified against the pinned source: key-ups have no keystroke and dispatch only
along the focus path; **all** mouse-up listeners . `capture_any_mouse_up` included . gate
on hitbox hover recomputed at the up's position and truncated by `.occlude()`; and the
**modality trap**: `Hitbox::is_hovered` returns false whenever the last input was a
keypress, and mouse-up does not reset the modality . so *any key pressed between a
mouse-down and its mouse-up disables every hitbox-gated mouse-up handler in the app*.
Only `on_mouse_down_out`/`on_mouse_up_out` escape hitbox scope.
The opposite direction . a motion or release arriving with a button whose press the pane never took .
is now gated on the press-origin bitmask in both `terminal/view.rs` and `browser/view.rs`, so a chrome
window drag cannot leak a drag into the panes its lagging positions sweep over.

- **Terminal selection/scrollbar drag strands → runaway autoscroll**
  (`terminal/view.rs:1895`, state `:389/:396/:382`): release over the sidebar, off-window,
  or after a mid-drag keypress and `selection_dragging` never clears; the autoscroll task
  loops forever with no button held, later bare hovers scroll-to-cursor, and the PTY app
  never sees the release. This is the leading suspect for the original "weird TUI
  mouse-selection interactions" field report.
- **Browser pointer-up dropped outside the content bounds** (`browser/view.rs:2088`,
  `:2192-2201`): CEF never gets `PointerPhase::Up` . selections keep extending, HTML5
  drags hang. Moves are hitbox-gated too, freezing drags that leave the page.
- **`split_drag` hard-strands** (`view.rs:1366-1382`): unlike `reconcile_pane_drag` there
  is no `cx.has_active_drag()` self-heal; a missed up leaves the divider preview and
  `resizing` highlight latched forever with no daemon resize sent.
- **`pane_drag` missed-up path skips `dismiss_armed_prefix`** (`view.rs:1406-1412`): the
  prefix stays armed and the next keystroke is eaten as a binding.
- **Window drag handle** (`window/drag.rs:17-80`): stranded `armed` makes a buttonless
  titlebar hover start a window move.
- **Terminal `swallowed_overlay_key`** (`terminal/view.rs:376`): the same press/release
  pairing bug one level down; focus leaving the terminal between press and release makes
  the next Escape in that terminal vanish (never reaching vim/less).
- **Kitty press/release imbalance** (`terminal/view.rs:1541-1758`): several key-down paths
  swallow the press but key-up forwards a release for any key . phantom releases in
  kitty-keyboard apps; a mode flip mid-key drops a release.
- **Daemon `swallowed_keys` pairs modifier-sensitively** (`daemon.rs:3905-3936`) while the
  client pairs by bare key (rolling a chord lifts ctrl first): press `"C-a"` meets release
  `"a"`, the entry leaks, and the unmatched release is dispatched to the PTY as a phantom.
- **Daemon `suppressed_text` counts leak** (`daemon.rs:5562`, `:3110`, `:3939`): IME
  composition and Caps Lock both break the repayment match; a leaked count later eats one
  real keystroke of that character, anywhere in the session, forever (cleanup is
  disconnect-only).
- **Residual `PrefixClaim` gaps after the shell fix**: (a) the app-global keystroke
  interceptor also fires for the *Settings window*, whose focus path has no AppShell .
  chording there strands keys (guard on window identity, or clear on activation); (b) a
  keypress in the same frame a focused element is torn down dispatches key-ups to
  `[root]` only.
- Confirmed good: the shell-level `capture_key_up` does cover the dialog layer, slideover,
  and notification layer (deferred draws keep their dispatch parent).

# Class 5 . memo digests

- **HIGH . empty-workspace focus latch** (`view.rs:865-867` vs `:1240`):
  `empty_workspace_became_visible` is latched unconditionally at the top of the pass but
  consumed in branch 7 of the priority chain; if the 0-session snapshot arrives while any
  overlay is up, the latch is consumed unseen and the new-session view renders **without
  keyboard focus**, permanently (typing does nothing until a click).
- `AppRevision` omits the per-client `focused_window` stamp (`view.rs:455` vs
  `daemon.rs:7067`) . the focus chain's primary input is outside the digest, currently
  safe only because every write happens to coincide with a generation bump; the sibling
  stamped field (`viewers`) already violates that pattern and is covered explicitly by
  `SidebarRevision`.
- `agent_config` is a digest input with no invalidation edge (nothing observes the global;
  hot-reloading agent config waits for an unrelated repaint).
- The five overlay `*_created` focus flags are pass-local and dropped if a higher-priority
  overlay wins . held together solely by the daemon's `dismiss_overlays` ordering
  invariant; make overlay focus idempotent (`focused_overlay: Option<OverlayKind>`).
- Sidebar `render_attention` reads the live snapshot while `SidebarRevision` keys off the
  pending-aware one . stale attention click-targets during a host switch.
- Cross-session UI commands are never drained (`view.rs:1112/:1123`): queued commands for
  a non-attached session replay on re-attach, arbitrarily later.
- Confirmed correct: `SynchronizeSignature` post-fix, `MuxTreeModel` reconciliation,
  `SidebarRevision` coverage (modulo attention), one-shot revision self-healing, agent
  timeline digest, terminal observe filter, `RowRenderCache`, browser frame ordering.

# Outcome (2026-08-03, same day)

All six steps shipped in one three-way parallel pass (mux/daemon, client, GUI), full
workspace suite green. The chaos test lives at `crates/zz-mux/tests/chaos.rs` and was
proven against a temporarily reverted de-index (panicked at exactly the audited site).
Deliberate residuals, each documented in code where it lives: `resolve_pane`'s both-dead
fallback can still land in another session (MuxState holds no client attachments);
browser mouse-moves that leave the pane hitbox entirely are undispatchable by gpui (the
clamped Up covers the reported failure); `suppressed_text` deliberately survives the
focus-sidebar reset (clearing there would type the chord's own character into the shell);
digest findings F3 (agent-config invalidation edge), F5 (sidebar attention snapshot
asymmetry), and F6 (cross-session UI command queues) remain open as lower-severity.

One same-day correction the first field test forced: the split-drag self-heal initially
*discarded* an uncommitted drag whose gpui drag had ended . which made a long-standing
silent failure visible as an instant snap-back. The divider hit target `.occlude()`s and
tracks the pointer, and browser panes occlude too, so the release routinely lands on an
occluding surface and the hitbox-gated committing listener never fires . meaning many
divider releases had *never* reached the daemon (the stranded preview merely looked
saved). Since gpui clears the active drag on any mouse-up regardless of hitboxes, the
drag ending is itself the release signal: `reconcile_split_drag` now **commits** the
previewed ratio as the path of last resort instead of discarding it, which fixes both
the snap-back and the older never-saved half of the bug.

# Fix order

1. **Resolver hardening + de-indexing** (neutralizes 10 of 11 panics): liveness checks in
   `resolve_session`/`resolve_window` mirroring `resolve_pane`, fix `step_window`'s raw
   reads, convert the eleven panic indexes to `.get()` + `MissingTarget`, prefer the
   client's session in `resolve_pane`'s fallback, run `repair_context` for the pre-empt
   verbs (or repair at execute entry).
2. **The chaos property test**: zz-mux is pure . randomized command interleavings across
   several contexts with external kills, asserting no panic, no silent success, and
   context self-heal within one failure. This is the regression net for class 1 forever.
3. **Error surfacing**: track request ids in `MuxClient` (small ring of id→command name),
   toast + `warn!` on error responses; stop routing command failures into `self.error`;
   log discarded non-attached-host errors; `key_command_failed`-style logging or
   `ClientMessage` toasts for the two daemon gesture paths; drop the id-0 gate on the
   teardown-race arm.
4. **Mouse-up stranding**: shell-level `capture_any_mouse_up` teardown for terminal
   selection/scrollbar and browser pointer state (with `_out` companions), `split_drag`
   self-heal, `pane_drag` prefix dismissal, window-drag `pressed_button` validation.
5. **Focus-latch and digest fixes**: empty-workspace latch commit-on-consume,
   `focused_window` into `AppRevision`, overlay focus idempotence, `swallowed_overlay_key`
   cleared on focus-out.
6. **Key bookkeeping symmetry**: bare-key pairing for daemon `swallowed_keys`, repay
   `suppressed_text` against produced text + expire with overlay resets, kitty
   forwarded-press set, Settings-window guard on the keystroke interceptor.
