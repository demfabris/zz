---
type: Playbook
title: Running tmux compatibility cohorts
description: A bounded, parallel workflow for closing the practical alias tmux=zz gap without letting new oracle findings extend one campaign forever.
tags: [tmux, compatibility, campaign, workflow, agents]
timestamp: 2026-08-27T00:00:00-03:00
last_updated: 2026-08-27
last_updated_by: Codex
---

# Outcome

Close the practical `alias tmux=zz` gate through bounded slices. Each slice starts with a fixed
acceptance contract, ends with one reviewed commit, and leaves later discoveries in the
[live tracker](/tmux/gaps.md).

The Alert cohort completed in commit `2e4ccf3b9b6706e44215d74ca147643e6baa3d2e`. The dedicated
campaign branch then closed session cwd in
`2468bfd8f1a11430a73b7066b022101b4048d981`, requested client flags in the next milestone, and
retained-client sizing in the third milestone. The fourth milestone closed client-environment
seeding and refresh with protocol v82. The fifth closed retained client formats with protocol v83,
the sixth closed the six client lifecycle hook producers, and the seventh proved silent Control
delivery for asynchronous copy-pipe failures. The eighth registered and executed the 24-command
daemon invalid-flag runtime roster. The ninth closed all eight pinned positional-maximum mismatches
with catalog-owned metadata and the pin's first-positional flag boundary. The last four needed no
protocol change.

`clients.attach-context` closed as three bounded contracts. Sessions keep one internal cwd, and
attached source loading prefers it. Clients keep requested flags through attach, switch, detach,
and TUI reconnect. `resize-window -A` and `-a` now aggregate retained client geometry once and
freeze the result as manual sizing. None of the three slices changed the wire or snapshot schema.
Clients now add one bounded environment snapshot to the handshake. Fresh sessions, existing
attach, native attach, Control attach, and targeted switch apply the pinned `update-environment`,
wildcard, missing, empty, hidden, `-A`, `-e`, `-E`, and `-T` rules. Session values survive client
disconnect, future panes read updates, and existing processes keep their startup environment.
One shared retained client-fact record now serves list rows, ordinary and inserted commands, status
recipients, and `display-message`; the attached fixture covers Interactive and Control empty
behavior against the pin.
Per-window latest-client retention now drives `client-active` only on ownership changes, while
focus, theme, and positive Interactive size reports retain the pin's duplicate behavior. Hook
replay stays clientless, `hook_client` names the reporter, and the reporter's copied session,
window, and shared active pane supply ordinary target formats. Control clients originate none of
those report hooks but remain eligible for promotion after the latest client leaves.
Changed-resize post-geometry format timing remains a separate protocol-owned slice so the producer
milestone does not grow across the TUI message boundary.
`active-pane` and `no-detach-on-destroy` are retained and reported, but their consumers remain
explicit later gaps.

The fresh 2026-08-27 checkpoint covers 86 scenarios and 1,481 steps. Every ordinary row is clean.
`known/known-main-preset-two-panes` and `known/known-spread-mixed` each retain exactly one documented
GEO divergence with every other channel clean. The sizing milestone's expanded multi-client
attached fixture passes, and `compat/run.sh --check-summary` confirms the canonical summary SHA-256
is
`40358b89c67686bc10ddfa1384f17ecfd71a2edf97f444974a1b1855c54eb18d`.
The full strict suite was rerun after the positional-maximum closure, not carried forward from an
older artifact. The new three-step scenario proves canonical and alias diagnostics, the
first-positional flag boundary, and side-effect suppression across all eight commands.

# Cohorts

| Phase | Tracker scope | Dependency | Exit proof |
|---|---|---|---|
| Alert | Closed alert groups | Complete | Focused daemon and terminal tests, pinned alert probes, one full debug attached-client fixture, tracker and knowledge updates |
| Client foundation | Session cwd, requested flags, sizing, environment, formats, and `clients.event-hooks` closed | Complete | One written oracle contract per slice, focused differential coverage, and one full debug attached-client fixture per milestone |
| Error contracts | Async copy-pipe delivery closed; `mux.error-shapes` and `tracker.semantic-coverage` remain | Independent of Client foundation except where a proof names client context | Every changed claim gets a pinned differential or a focused test with a named tracker item, followed by one full debug attached-client fixture |
| Copy behavior | `copy-mode.action-fidelity`, `copy-mode.command-fidelity`, `keys.copy-mode-binding-fidelity`, `keys.copy-mode-unsupported-default-actions`, `keys.copy-mode-prompt-defaults` | Command fidelity requires `clients.interactive-refresh`; prompt-backed defaults also require `prompt.command-fidelity` | Source-owned action and binding inventories, attached key-path probes, and one full debug attached-client fixture |

These phases are navigation, not commit boundaries. One persistent goal and one milestone commit own
one bounded slice. Split a tracker group before implementation when its acceptance contract crosses
unrelated production paths. Do not merge slices to save a commit.

# Selected dependency-ordered tranche

The queue separates execution order from apparent ease. A blocked medium item does not jump ahead of
the hard state contract that makes its proof meaningful. A range such as `10a-10f` means one
milestone per letter, never one combined commit.

| Order | Slice | Current tracker ownership | Relative effort | Why it is bounded |
|---|---|---|---|---|
| 1 | Session cwd and attached `source-file` cwd | Closed under `clients.attach-session-cwd` on 2026-08-26 | Complete | One internal session-state path; no client-environment or format vocabulary |
| 2 | Requested client flags | Closed under `clients.attach-flags` on 2026-08-27 | Complete | One attach-state contract; establishes `ignore-size` |
| 3 | Largest and smallest client sizing | Closed under `clients.attach-sizing` on 2026-08-27 | Complete | One-shot component-wise aggregation, manual freeze, global `ignore-size` fallback, Control ceilings, and default fallback; no wire change |
| 4 | Client environment seeding and refresh | Closed under `clients.attach-environment` on 2026-08-27 | Complete | Protocol v82 plus one per-connection snapshot; exact and wildcard refresh semantics remain session scoped |
| 5 | Client format facts | Closed under `clients.context-formats` on 2026-08-27 | Complete | Protocol v83 plus one retained-fact contract across list, status, Control, inserted, and targeted contexts |
| 6 | Client lifecycle hook producers | Closed under `clients.event-hooks` on 2026-08-27 | Complete | Per-window latest ownership plus five report boundaries; no protocol change |
| 7 | Interactive refresh decision | `clients.interactive-refresh` | Hard decision gate | Either justify and adopt the cross-client mode contract or keep it parked and reclassify dependent copy claims |
| 8 | Async copy or pipe error delivery | Closed under `control-mode.async-copy-pipe-errors` on 2026-08-27 | Complete | Pinned source plus a delayed exit-7 Control probe prove silent delivery and copy-mode cancellation |
| 9a | Daemon invalid-flag runtime contract | Closed under `tracker.daemon-invalid-flag-runtime` on 2026-08-27 | Complete | Source-owned 24-command roster plus production-dispatch proof; later error-shape items remain open |
| 9b | Positional maximums | Closed under `mux.positional-maximums` on 2026-08-27 | Complete | One catalog validator across eight commands, with mux and daemon production-boundary proof |
| 9c | Required positional minima | The 14 `positional-min:*` items in `mux.error-shapes` | Medium | One catalog sidecar and validator after flags but before targets and effects |
| 9d | Shared arity and flag errors | `semantic:command-arity-errors` and `semantic:command-flag-errors` | Medium | Exact shared families without absorbing callback-body grammar |
| 9e | Nested `new-session` error precedence | `semantic:nested-new-session-error-precedence` | Medium | Separate client-lifecycle path with its own oracle proof |
| 10a-10f | `args_parse` runtime rules | Corresponding `args-parse:*` items in `tracker.semantic-coverage`, one measured rule per slice | Medium | Six effective source rules, never all callback commands at once |
| 10g-10k | Source-owned tracker registrations | Hook producers, key bindings, nonconstant formats, open context formats, and option consumers, one semantic item per slice | Small to medium | Five unrelated owners remain five independent milestones |
| 11 | Copy action vocabulary inventory | `semantic:copy-mode-action-vocabulary` in `copy-mode.action-fidelity` | Small research | Record and classify all 95 pinned actions before behavior changes |
| 12a-12f | Copy action behavior | The other six `copy-mode.action-fidelity` semantics, one category per slice | Hard | Cursor, logical-line, goto, selection, jump/prompt, and copy effects stay independently provable |
| 13 | Unsupported stock action bindings | `keys.copy-mode-unsupported-default-actions` | Medium after slice 12 | Seven keys become honest only after their five actions exist |
| 14 | Copy command fidelity | `copy-mode.command-fidelity` | Hard | Requires the interactive-refresh decision |
| 15 | Shared copy binding fidelity | `keys.copy-mode-binding-fidelity` | Hard | Follows command fidelity; owns exactly 15 divergent command shapes |
| 16 | Generic prompt command fidelity | `prompt.command-fidelity` | Hard | Requires the interactive-refresh decision and remains broader than copy mode |
| 17 | Prompt-backed copy defaults | `keys.copy-mode-prompt-defaults` | Medium after slice 16 | Ten defaults land only after their generic prompt contract |

Slice 9b is closed and slice 9c is next. Before choosing each later milestone, regenerate the report
and re-rank every active daily, script, remote, or silent-mismatch group. That audit must include
attach-dependent work such as `buffers.client-file-context`, the three open `source-file.*-client-cwd` groups,
`clients.detach-exec`, and `clients.parent-hup-exit`. Rows 4 and later are a dependency forecast,
not permission to skip a newly unblocked practical gate. Keep formats, hooks,
`active-pane`, and `no-detach-on-destroy` as separate slices.

# Four-seat Codex pipeline

Use the four seats as one coordinator and three Codex subagents:

1. The coordinator fixes the slice boundary, assigns file ownership, integrates changes, and owns
   the commit.
2. The oracle agent probes the pinned tmux commit and writes the acceptance contract plus the
   smallest differential fixture that can disprove it.
3. The implementation agent changes one owned subsystem and runs focused tests. After review starts,
   this seat may scout the next slice without editing its files.
4. The review agent hunts context, performs an independent code and proof review, then checks
   tracker and knowledge claims against source.

Assign one owner to each path before agents edit. The coordinator resolves overlaps instead of
letting two agents rewrite the same file. Use Codex subagents for this campaign.

# Validation ladder

Run the cheapest proof that can fail the current edit:

1. During implementation, run focused Rust tests and the scenario or attached probe for the changed
   behavior.
2. At slice close, build the debug binary and run the full attached-client fixture against the
   pinned tmux oracle. Treat a skip or reduced scenario count as a failure.
3. At a campaign checkpoint, run `just compat --strict-geometry --attached-client`, regenerate the
   canonical summary, and run `compat/run.sh --check-summary` as a separate check.

Use campaign checkpoints after the Alert cohort, after two more completed slices, and at the
practical exit gate. Run one earlier if a change invalidates the stored checkpoint. Do not run
release builds for compatibility work.

# Discovery rule

The oracle agent records new gaps in `compat/tmux-gaps.json`. A discovery joins the active slice
only when it uses the same production path, needs no protocol or schema change, and fits the slice's
existing proof. A discovery that invalidates the slice's claimed behavior blocks closure. All other
findings wait for a later cohort.

Freeze the acceptance contract after the oracle and implementation agents agree on it. Review can
reject the implementation or proof, but it cannot expand the slice with unrelated cleanup.

# Goal boundary

By default, create one persistent Codex goal per slice and name its exact tracker items and exit
proof. When Fabrico explicitly asks for the whole campaign to continue unattended, one campaign
goal may span the practical exit gate. The slice boundary does not change: freeze, prove, review,
document, and commit one milestone before starting the next one.

# Milestone commits and worktree

Commit each slice after code review, proof, tracker generation, and OKF validation pass. Stage exact
paths or hunks because the shared checkout may contain unrelated work. Ask before the first commit
unless Fabrico has already authorized continuous campaign commits. Do not push unless he asks.

Resolve the `codex/tmux-compat` branch and `/Users/demfabris/dev/zz-tmux-compat` path with read-only
checks first. If both are absent, create the dedicated worktree from the current clean campaign base
after verifying that the Alert commit is its ancestor. If either exists, inspect and reuse it safely;
never overwrite it. Leave unrelated shared-checkout edits intact.

# Practical exit gate

The campaign reaches compatible-enough status when all of these hold:

- Daily session, window, pane, config, plugin, Control, and attached-TUI workflows have no known
  silent semantic mismatch.
- The config and plugin corpus runs with no unexpected skip.
- The current strict differential and attached-client fixture pass at their full scenario counts.
- The canonical summary describes the current corpus rather than an older scenario set.
- Every remaining gap has an explicit `native`, `park`, or `never` decision, or produces a loud error
  that cannot corrupt state.

The tracker can retain long-tail work after this gate. It cannot retain an unclassified daily-use
surprise.

# Bootstrap prompt for the next session

Paste this prompt into the next session:

```text
Continue the tmux compatibility campaign in /Users/demfabris/dev/zz-tmux-compat on
codex/tmux-compat. Preserve unrelated work and do not push.

Verify that the session-cwd, requested-client-flags, retained-client-sizing, client-environment,
client-formats, client-hooks, asynchronous copy-pipe, daemon-invalid-flag, and positional-maximum
milestones are committed and their tracker entries are closed. Confirm the current checkpoint still
reports 86 scenarios, 1,481 steps, attached-client PASS, and only the two documented GEO rows.

Regenerate and re-rank the entire active tracker before selecting the next bounded slice. Include
daily, script, remote, and silent mismatches plus newly unblocked attach-dependent work. Freeze one
acceptance contract after that audit. Do not combine context formats, event hooks, exit actions,
`active-pane`, or `no-detach-on-destroy` behavior merely because they share
client state.

Read AGENTS.md, this playbook, the live tracker, the roadmap, the relevant OKF pages, and cited
source before editing. Use one coordinator and three Codex subagents to probe the selected
pinned-tmux behavior, trace its current owners, and design the minimum differential proof. Freeze
the contract before implementation and assign disjoint file ownership.

Run focused tests, build a fresh debug binary, and run the full attached-client fixture when the
slice touches attached clients. Rerun the canonical strict differential at the next campaign
checkpoint, when a change invalidates the artifact, or when a new canonical scenario joins the
corpus. Update the tracker
and OKF documents, validate them, get an independent review, and commit one milestone. Continue the
campaign goal into the next slice without pushing.
```
