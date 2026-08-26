---
type: Playbook
title: Running tmux compatibility cohorts
description: A bounded, parallel workflow for closing the practical alias tmux=zz gap without letting new oracle findings extend one campaign forever.
tags: [tmux, compatibility, campaign, workflow, agents]
timestamp: 2026-08-26T00:00:00-03:00
last_updated: 2026-08-26
last_updated_by: Codex
---

# Outcome

Close the practical `alias tmux=zz` gate through bounded slices. Each slice starts with a fixed
acceptance contract, ends with one reviewed commit, and leaves later discoveries in the
[live tracker](/tmux/gaps.md).

The Alert cohort completed implementation, proof, and documentation in commit
`2e4ccf3b9b6706e44215d74ca147643e6baa3d2e`. The latest implementation commit before this handoff
plan is `32bbd2f0e02292e112a98001cdc16753ad6f45ea`; verify that both commits remain ancestors of the
current clean `main` before starting a new worktree.

The persisted Alert checkpoint covers 84 scenarios and 1,475 steps. Every ordinary row is clean.
`known/known-main-preset-two-panes` and `known/known-spread-mixed` each retain exactly one documented
GEO divergence with every other channel clean. The attached-client fixture and
`compat/run.sh --check-summary` both pass. The canonical summary SHA-256 is
`5de67222bc2ebb99c57963be14c865ddfdddc387da34ee32dd86962cef8336c9`.
These artifacts were produced at the Alert commit; their presence is not proof that the full suite
was freshly rerun on the later handoff base.

# Cohorts

| Phase | Tracker scope | Dependency | Exit proof |
|---|---|---|---|
| Alert | Closed alert groups | Complete | Focused daemon and terminal tests, pinned alert probes, one full debug attached-client fixture, tracker and knowledge updates |
| Client foundation | `clients.attach-context`, `clients.attach-environment`, `clients.context-formats`, `clients.event-hooks` | Formats and hooks follow attach state; environment is independent but shares protocol files | One written oracle contract per slice, focused differential coverage, and one full debug attached-client fixture per milestone |
| Error contracts | `control-mode.async-copy-pipe-errors`, `mux.error-shapes`, `tracker.semantic-coverage` | Independent of Client foundation except where a proof names client context | Every changed claim gets a pinned differential or a focused test with a named tracker item, followed by one full debug attached-client fixture |
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
| 1 | Session cwd and attached `source-file` cwd | `clients.attach-context`: `attach-session -c` and `semantic:source-file-attached-session-cwd` | Medium | One session-state path; no client-environment or format vocabulary |
| 2 | Requested client flags | `clients.attach-context`: `attach-session -f`, `new-session -f`, and `protocol:client-attach-context` | Hard | One attach-state contract; establishes `ignore-size` |
| 3 | Largest and smallest client sizing | `clients.attach-context`: `resize-window -A`, `resize-window -a`, and `semantic:resize-window-client-sizes` | Hard | Depends on retained sizes and `ignore-size`, but not environment or hooks |
| 4 | Client environment seeding and refresh | `clients.attach-environment` | Hard | Independent behavior, serialized because it changes the same handshake files |
| 5 | Client format facts | `clients.context-formats` | Hard | Define one coherent retained-fact contract across list, status, Control, and targeted contexts |
| 6 | Client lifecycle hook producers | `clients.event-hooks` | Hard | Prove target-client context and six transition boundaries without prescribing the storage shape |
| 7 | Interactive refresh decision | `clients.interactive-refresh` | Hard decision gate | Either justify and adopt the cross-client mode contract or keep it parked and reclassify dependent copy claims |
| 8 | Async copy or pipe error delivery | `control-mode.async-copy-pipe-errors` | Medium | Probe first, then prove the measured daemon-to-Control delivery contract |
| 9a | Daemon invalid-flag runtime contract | `semantic:tracker-daemon-invalid-flag-runtime` in `tracker.semantic-coverage` | Medium | Establish shared validation scaffolding without claiming later error-shape items |
| 9b | Arity and flag error shapes | The positional and shared error items in `mux.error-shapes` | Medium | One central validation path across the affected commands |
| 9c | Nested `new-session` error precedence | `semantic:nested-new-session-error-precedence` | Medium | Separate client-lifecycle path with its own oracle proof |
| 10a-10f | `args_parse` runtime rules | Corresponding `args-parse:*` items in `tracker.semantic-coverage`, one measured rule per slice | Medium | Six effective source rules, never all callback commands at once |
| 10g-10k | Source-owned tracker registrations | Hook producers, key bindings, nonconstant formats, open context formats, and option consumers, one semantic item per slice | Small to medium | Five unrelated owners remain five independent milestones |
| 11 | Copy action vocabulary inventory | `semantic:copy-mode-action-vocabulary` in `copy-mode.action-fidelity` | Small research | Record and classify all 95 pinned actions before behavior changes |
| 12a-12f | Copy action behavior | The other six `copy-mode.action-fidelity` semantics, one category per slice | Hard | Cursor, logical-line, goto, selection, jump/prompt, and copy effects stay independently provable |
| 13 | Unsupported stock action bindings | `keys.copy-mode-unsupported-default-actions` | Medium after slice 12 | Seven keys become honest only after their five actions exist |
| 14 | Copy command fidelity | `copy-mode.command-fidelity` | Hard | Requires the interactive-refresh decision |
| 15 | Shared copy binding fidelity | `keys.copy-mode-binding-fidelity` | Hard | Follows command fidelity; owns exactly 15 divergent command shapes |
| 16 | Generic prompt command fidelity | `prompt.command-fidelity` | Hard | Requires the interactive-refresh decision and remains broader than copy mode |
| 17 | Prompt-backed copy defaults | `keys.copy-mode-prompt-defaults` | Medium after slice 16 | Ten defaults land only after their generic prompt contract |

The next session owns slice 1 only. Before editing, split `clients.attach-context` in the live
tracker so cwd, flags, and sizing can close independently without falsifying group status.

This tranche is not the whole active tracker. After slice 3 closes, regenerate the report and
re-rank every active daily, script, remote, or silent-mismatch group before choosing slice 4. That
audit must include attach-dependent work such as `buffers.client-file-context`, the three open
`source-file.*-client-cwd` groups, `clients.detach-exec`, and `clients.parent-hup-exit`. Rows 4 and
later are the current dependency forecast, not permission to skip a newly unblocked practical gate.

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

Create one persistent Codex goal per slice. Name the exact tracker items and exit proof in its
objective. Complete that goal after the milestone commit and handoff, then start the next slice in a
fresh session. Do not use one goal for a phase or the remaining tracker.

# Milestone commits and worktree

Commit each slice after code review, proof, tracker generation, and OKF validation pass. Stage exact
paths or hunks because the shared checkout may contain unrelated work. Tell Fabrico before invoking
`git commit` so he can touch the YubiKey. Do not push unless he asks.

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
Start the next bounded tmux compatibility slice for /Users/demfabris/dev/zz.

First verify without editing that the checkout is clean and record both main and origin/main. If
they differ, confirm that main contains origin/main; stop on a true divergence. Verify that both
32bbd2f0e02292e112a98001cdc16753ad6f45ea and
2e4ccf3b9b6706e44215d74ca147643e6baa3d2e are ancestors of main. Record the current main SHA as the
handoff base. Verify that the two Alert groups are closed and that compat/run.sh --check-summary
reports 84 scenarios, 1,475 steps, and attached-client PASS. Treat this as the persisted Alert
checkpoint, not proof that the full suite was freshly rerun on the handoff base.

Resolve the codex/tmux-compat branch and /Users/demfabris/dev/zz-tmux-compat path read-only. If both
are absent, create that dedicated worktree and branch from the recorded main handoff base. If either
exists, inspect and reuse it safely; do not overwrite it. Perform all remaining work in the dedicated
worktree.

Read AGENTS.md, knowledge/playbooks/tmux-compat-cohorts.md,
knowledge/designs/tmux-superset-roadmap.md, compat/tmux-gaps.json, and the relevant cited source
before editing.

Create one persistent goal for the session-cwd slice only. It owns attach-session -c and attached
source-file cwd behavior from clients.attach-context. Before implementation, split that tracker
group so session cwd, requested client flags, and retained-client sizing can close independently.
Do not implement client flags, resize-window -A/-a, client environments, client formats, hooks, or
interactive refresh in this goal.

Use one coordinator and three Codex subagents. Before implementation, have agents independently:
1. probe pinned tmux commit d77c9dc6aa021e4bc61f0da128c591af695e6466 for the exact cwd contract;
2. trace the mux and daemon state path and propose the smallest state change;
3. audit existing tests and design the minimum non-vacuous differential proof where client cwd and
   session cwd deliberately differ.

Freeze the acceptance contract after synthesis and assign disjoint file ownership. Use exact
-t =name targets. Treat skips or reduced scenario counts as failures. Run focused tests while
editing, then a fresh debug build and the full attached-client fixture at slice close. Do not run a
release build. The next canonical strict checkpoint is after this and the following completed slice,
unless a change invalidates the stored checkpoint earlier.

Update compat/tmux-gaps.json, regenerate knowledge/tmux/gaps.md, update the relevant OKF documents,
validate OKF, and get an independent Codex review. Close the slice with one milestone commit. Stop
immediately before git commit and tell Fabrico the YubiKey touch is next. Do not push.
```
