# tmux compatibility campaign tracker

> Campaign state: **PAUSED before slice 10p**
>
> Tracker resolution progress: **63.6% (117 of 184 known groups)**
>
> Campaign base verified: **2026-08-28** at `1a0f59e8dd6b3885421afb38dad5e5a2ee824aec`

This is the resume point for the entire `alias tmux=zz` campaign. An agent asked to continue the
campaign should read this file, run the preflight below, and resume the current slice without
reconstructing the history from chat.

This file is the campaign rollup, not a second item-level backlog. Individual gap state lives only
in [`compat/tmux-gaps.json`](compat/tmux-gaps.json). The readable
[`knowledge/tmux/gaps.md`](knowledge/tmux/gaps.md) report is generated from that registry and must
not be edited by hand.

## Mission and completion rule

Make `alias tmux=zz` dependable for daily interactive work, imported configuration, the pinned
plugin corpus, common automation, remote use, and silent mismatch detection. A tmux command name
must keep tmux meaning or fail loudly. Native zz behavior stays on zz-only names.

This is not a quest to reproduce every tmux internal. Linked sessions, the private tmux socket
protocol, and multi-user socket ACLs remain outside the practical goal. The full product boundary
is in [`knowledge/tmux/tmux-compat.md`](knowledge/tmux/tmux-compat.md).

The campaign is complete only when the practical exit gate near the end of this file passes. The
percentage is a ledger health metric, not a compatibility claim.

## Current checkpoint

| Fact | Current value |
| --- | --- |
| Repository | `/Users/demfabris/dev/zz` |
| Published branch | `origin/main` |
| Audited campaign base | `1a0f59e8dd6b3885421afb38dad5e5a2ee824aec` |
| Published delivery | Every milestone listed below and this tracker are on remote `main` |
| Dedicated campaign worktree | Removed after delivery on 2026-08-28 |
| Pinned tmux oracle | `d77c9dc6aa021e4bc61f0da128c591af695e6466` (`next-3.8`) |
| GitHub tracker | [Issue #7](https://github.com/demfabris/zz/issues/7), open |
| Pause point | Slice 10p, raw-TUI popup, has not started |
| Live registry | 88 active groups, 589 active items, 96 closed records |
| Active status | 47 open, 20 blocked, 21 accepted |
| Known differentials | 2 registered geometry cases |

The audited base was clean before this file was created. Resolve the commit containing the latest
tracker update with `git log -1 --format=%H -- TMUX_COMPAT_TRACKER.md`, and resolve live remote
`main` with `git ls-remote https://github.com/demfabris/zz.git refs/heads/main`. Always inspect the
live worktree before acting because other agents may share it.

### Progress calculation

Progress counts a group as resolved when it is either in closed history or has an accepted
`native` or `never` disposition. Open and blocked groups remain unresolved.

```text
(closed records + accepted active groups) / (closed records + all active groups)
(96 + 21) / (96 + 88) = 117 / 184 = 63.6%
```

Recompute it from the registry after every tracker change:

```sh
python3 -c 'import json; d=json.load(open("compat/tmux-gaps.json")); done=len(d["closed"])+sum(g["status"]=="accepted" for g in d["gaps"]); total=len(d["closed"])+len(d["gaps"]); print(f"{100*done/total:.1f}% ({done}/{total} resolved groups)")'
```

Each group has equal weight even though effort and item count differ. Closed records do not retain
their former item lists, so schema 3 cannot support an honest historical item-weighted percentage.
New discoveries can lower this number. Passing the practical exit gate matters more than raising
it.

## Source-of-truth order

When sources disagree, use this order and correct stale documentation in the same slice:

1. Current zz source and tests plus measured behavior from the clean pinned tmux checkout.
2. [`compat/tmux-gaps.json`](compat/tmux-gaps.json) for live gap IDs, decisions, status,
   dependencies, evidence, acceptance, and closed history.
3. [`compat/results/summary.md`](compat/results/summary.md) for the persisted accepted differential
   and attached-client artifact.
4. [`knowledge/playbooks/tmux-compat-cohorts.md`](knowledge/playbooks/tmux-compat-cohorts.md) for
   dependency order, slice rules, validation, and the practical exit gate.
5. [`knowledge/playbooks/compat-harness.md`](knowledge/playbooks/compat-harness.md) for oracle,
   tracker, scenario, result, and fixture operation.
6. [`knowledge/designs/tmux-superset-roadmap.md`](knowledge/designs/tmux-superset-roadmap.md) and
   [`knowledge/log.md`](knowledge/log.md) for current narrative and milestone history.
7. [`knowledge/tmux/divergences.md`](knowledge/tmux/divergences.md),
   [`knowledge/research/2026-08-22-tmux-cli-compatibility-audit.md`](knowledge/research/2026-08-22-tmux-cli-compatibility-audit.md),
   and [`knowledge/designs/tmux-drop-in.md`](knowledge/designs/tmux-drop-in.md) for rationale,
   baseline measurements, and older campaign history.
8. This file for the cross-source checkpoint and resume instructions.
9. [Issue #7](https://github.com/demfabris/zz/issues/7) as the external mirror for humans.

Start unfamiliar subsystem research at [`knowledge/index.md`](knowledge/index.md). Knowledge pages
are maps; verify their cited source before changing behavior. Historical counts in the audit and
old drop-in plan are not live status.

## Resume contract

When Fabrico asks to resume the campaign, begin here:

```sh
cd /Users/demfabris/dev/zz
git status --short --branch
git worktree list
git fetch https://github.com/demfabris/zz.git main:refs/remotes/origin/main
git rev-parse HEAD origin/main
```

Preserve unrelated changes in the standard checkout. If it is dirty or shared, create a fresh
dedicated worktree from the verified `origin/main` instead of changing or committing those edits:

```sh
git worktree add -b codex/tmux-compat-next /Users/demfabris/dev/zz-tmux-compat origin/main
cd /Users/demfabris/dev/zz-tmux-compat
```

If that path or branch already exists, inspect and reuse it safely rather than overwriting it. From
the selected clean campaign worktree, run:

```sh
python3 compat/tmux-tracker.py check
compat/run.sh --check-summary
just compat-check
```

Compare live output with the checkpoint above. If HEAD or tracker counts moved, update this file
before implementing. Regenerate and re-rank the whole active registry before freezing a slice
because a newly exposed daily, script, remote, or silent mismatch may outrank the forecast.

Work one bounded milestone at a time:

1. Read `AGENTS.md`, this file, the live gap record, the cohort playbook, the harness playbook, and
   the relevant knowledge and source owners.
2. Probe the pinned tmux source and binary. Record the smallest acceptance contract that can prove
   the behavior wrong.
3. Freeze the exact tracker items and exclusions. Assign disjoint paths when several agents work in
   parallel.
4. Implement one production path. Put unrelated discoveries into their own tracker group or later
   slice.
5. Run focused tests and the smallest differential or attached-client proof that exercises the
   changed behavior.
6. Move completed adopt work into `closed`, regenerate the report, and update every knowledge page
   whose status or evidence changed.
7. Obtain an independent code and evidence review.
8. Run the slice gates, update this file and issue #7, then create one milestone commit only when the
   current task authorizes commits.
9. Never push unless Fabrico explicitly asks.

The current pause stops implementation. This document does not grant commit or push authority.

## Accepted evidence at the pause point

The persisted 10o artifact is the current accepted checkpoint:

| Evidence | Result |
| --- | --- |
| Differential scenarios | 98 |
| Differential steps | 1,517 |
| Ordinary rows | All clean |
| Registered differences | One GEO cell in each of two named known rows; every other channel clean |
| Attached-client fixture | `PASS` |
| Summary SHA-256 | `9c147eb5caa78ca51e068275b28836ab2647d3d959d047c5fafbcb5c0bf86832` |
| Stored-artifact check | `compat/run.sh --check-summary` passes |

The complete attached-client fixture passed directly after slice 10o. Strict workspace clippy,
affected package tests, both client convergence seeds, formatting, tracker generation, shell
syntax, diff checks, and OKF validation also passed.

A later fresh 98-scenario run was stopped during `lane2-store` when the campaign was wrapped. Every
completed cohort was clean, but that run did not finish and is not acceptance evidence. Do not
describe it as a completed rerun. The persisted summary above remains the accepted artifact.

The latest completed workspace test sweep reported three daemon failures under parallel load. Each
exact test passed alone, matching the timing-sensitive classification in `AGENTS.md`. Preserve that
qualification until a newer complete sweep supersedes it.

## Shipped history

The 96 entries under the generated report's
[`Closed history`](knowledge/tmux/gaps.md#closed-history) section are the complete item-level record.
The table below is the milestone rollup an agent needs for orientation.

| Slice | Delivered result | Commit |
| --- | --- | --- |
| Original phases 0 through 8 | Config/script foundation, harness, layout, command coverage, exec family, Control mode, binary surface, and attach contract | See [`tmux-drop-in.md`](knowledge/designs/tmux-drop-in.md#phases) |
| Alert | Per-client alert lifecycle and attached proof | `2e4ccf3` |
| 1 | Session cwd across attach | `2468bfd` |
| 2 | Requested client flags | `3816544` |
| 3 | Retained-client sizing | `a6cee7f` |
| 4 | Client environment refresh, protocol v82 | `defd38b` |
| 5 | Retained client format facts, protocol v83 | `9fa9b62` |
| 6 | Client lifecycle hook producers | `6517213` |
| 7 | Interactive refresh decision remains parked and blocked | Not closed |
| 8 | Silent asynchronous copy-pipe Control behavior | `3adcdff` |
| 9a | Daemon invalid-flag runtime contract | `f08a81f` |
| 9b | Positional maximums | `73b37f7` |
| 9c | Positional minimums | `3a2f173` |
| Gate repair | Native attach `-E`, published client targeting, and CLI assertions | `1c7dfcb` |
| 9d | Shared command arity errors | `436718c` |
| 9e | Shared flag errors across 83 commands and 74 aliases | `e884abc` |
| 9f | Nested `new-session` validation precedence | `59f754f` |
| 10a | `if-shell` argument blocks, protocol v84 | `397d8b0` |
| 10b | `run-shell` argument blocks | `3dba7df` |
| 10c | `set-option` value blocks | `cf01931` |
| 10d | `bind-key` commands-or-string rule | `fe2bfb9` |
| 10e | `confirm-before` commands-or-string rule | `533f123` |
| 10f | `command-prompt` commands-or-string rule | `60f68a4` |
| 10g | `set-hook` monitor-or-value rule | `ad352ef` |
| 10h | `display-menu` repeated-item rule | `0da3a2e` |
| 10i | `display-panes` template rule | `cc9856f` |
| 10j/10k | Shared `choose-buffer` and `choose-tree` template rule | `af1f97d` |
| 10l | Source-owned hook-producer partition | `dd290e5` |
| 10m | Default-key structure and bare `bind-key` mutation | `ff3347d` |
| Main checkpoint | Campaign through 10m merged into remote `main` | `4c5bd8c` |
| 10n | Raw-TUI confirmation rendering and input ownership | `151abce` |
| Count correction | Unsupported flag inventory corrected to 70 pairs across 20 commands | `aad3923` |
| 10o | Raw-TUI menu rendering, shared resolver, lifecycle, and attached proof | `1a0f59e` |

`10j/10k` is one deliberate milestone because both commands use the same callback implementation
and attached proof. Slice 10l records source ownership without changing runtime behavior. The count
correction is documentation maintenance, not a compatibility slice. Slices 10n and 10o expand the
attached fixture without adding differential scenario rows.

## Current slice: 10p raw-TUI popup

Tracker owner: [`clients.tui-overlay-consumption`](knowledge/tmux/gaps.md#clientstui-overlay-consumption-render-and-consume-popup-overlays-in-zz-tui)

Exact current item: `semantic:tui-display-popup-overlay`

Goal: raw attach must render and own a daemon-published `display-popup` terminal session without
letting keyboard, paste, pointer, scroll, focus, or close lifecycle events escape to the pane under
it.

The protocol, daemon popup session, `ClientCore`, and GPUI reference path already carry the needed
descriptor and viewport state. The raw TUI retains synthetic frames but drops `PopupChanged` and
has no popup state, rendering, or input owner. Keep 10p on the raw-TUI consumption path unless fresh
evidence disproves that boundary.

### Expected owned paths

- `crates/zz-tui/src/layout.rs`
- `crates/zz-tui/src/state.rs`
- `crates/zz-tui/src/app.rs`
- `crates/zz-tui/src/input.rs`
- `crates/zz-tui/src/render.rs`
- `compat/attached-client.sh`
- `compat/tmux-gaps.json` and its generated report
- Campaign knowledge pages changed by the closure

### In-scope acceptance

- Seed, update, reconnect, replace, close, and reset popup state from `ClientCore`.
- Render the popup descriptor, border, title, styles, terminal viewport, cursor, and bounded geometry
  above the workspace and below higher-priority menu or confirmation state.
- Route popup keys, key lifecycle, paste, mouse, scroll, and focus before global shortcuts, prefix
  handling, prompts, choosers, browsers, or pane input.
- Keep all popup input out of the underlying pane. Prove it with a one-byte pane sentinel.
- Remove the synthetic viewport and renderer caches on close or replacement, then repaint the latest
  underlay.
- Add focused raw-TUI tests and one attached fixture that runs against both zz and the pin.

### Explicitly outside 10p

Create or retain a separate `display-popup.behavior-fidelity` group for these six contracts:

- `semantic:display-popup-resize-lifecycle`
- `semantic:display-popup-style-refresh`
- `semantic:display-popup-context-menu`
- `semantic:display-popup-border-drag`
- `semantic:display-popup-to-pane`
- `semantic:display-popup-kitty-images`

Real mouse/status format facts remain under `formats.mouse-context`. Control-mode popup
presentation and read-only popup actions also remain outside this slice.

### Planned tracker movement

Close only `semantic:tui-display-popup-overlay`, moving the completed raw-TUI consumption contract
to closed history as `clients.tui-display-popup-overlay`. Do not close broader popup fidelity from
the bounded raw-TUI proof. Regenerate the report and advance the queue to 10q.

### 10p proof ladder

```sh
cargo test -p zz-tui
cargo test -p zz-client popup
cargo test -p zz-daemon popup
cargo clippy -p zz-tui --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
cargo build -p zz
bash -n compat/attached-client.sh
compat/attached-client.sh target/debug/zz "$(compat/fetch-tmux.sh)"
just compat-check
```

The full checkpoint is due now because 10n and 10o completed after the last full accepted run, and
the attempted replacement run was interrupted. Before committing 10p, complete:

```sh
just compat --strict-geometry --attached-client
compat/run.sh --check-summary
```

## Dependency queue after 10p

This is a forecast, not permission to skip the live rerank.

| Order | Slice | Exact owner | Boundary |
| --- | --- | --- | --- |
| 1 | 10q | `semantic:tracker-nonconstant-format-behavior` | Discover and register nonconstant format behavior |
| 2 | 10r | `semantic:tracker-open-context-format-vocabulary` | Discover and register open context formats |
| 3 | 10s | `semantic:tracker-option-consumer-registration` | Discover and register option consumers |
| 4 | 11 | `semantic:copy-mode-action-vocabulary` | Inventory all 95 pinned copy actions before behavior changes |
| 5 | 12a through 12f | Remaining `copy-mode.action-fidelity` items | Cursor, logical line, goto, selection, jump/prompt, and copy effects as separate slices |
| 6 | 13 | `keys.copy-mode-unsupported-default-actions` | Add seven defaults only after their actions exist |
| 7 | 14 | `copy-mode.command-fidelity` | Resolve or reclassify the interactive-refresh dependency first |
| 8 | 15 | `keys.copy-mode-binding-fidelity` | Match the 15 divergent shared command shapes |
| 9 | 16 | `prompt.command-fidelity` | Resolve or reclassify the interactive-refresh dependency first |
| 10 | 17 | `keys.copy-mode-prompt-defaults` | Add ten defaults only after generic prompt fidelity |

The three active groups marked `next` in the generated report are not themselves execution order.
`keys.copy-mode-binding-fidelity` still depends on `copy-mode.command-fidelity` and therefore stays
at slice 15.

Before selecting every later milestone, explicitly recheck attach-dependent and daily-use groups,
including buffer file context, source-file client cwd variants, detach execution, parent-HUP exit,
raw-TUI overlays, menu fidelity, `active-pane`, and `no-detach-on-destroy`.

## Validation and closure gates

Run the cheapest focused proof first. Before closing a normal slice, require:

```sh
python3 compat/tmux-tracker.py write-report
python3 compat/tmux-tracker.py check
just compat-check
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
bash -n compat/attached-client.sh
python3 .agents/skills/okf/scripts/okf.py validate knowledge
git diff --check
compat/run.sh --check-summary
```

Run the equivalent syntax check for every other shell file changed by the slice.

When a full workspace daemon test fails with the exact headless or parallel-load behavior described
in `AGENTS.md`, rerun that exact test alone before classifying it. Record both results.

A checkpoint that changes or invalidates the accepted corpus must also complete the full strict and
attached run. A partial run, reduced scenario count, skip, interrupted run, or headless-only run
cannot replace the persisted summary.

## Practical exit gate

Do not close the campaign until every statement below is true:

- Daily session, window, pane, config, plugin, Control, and attached-TUI workflows have no known
  silent semantic mismatch.
- The config and pinned plugin corpus runs with no unexpected skip.
- The current strict differential and attached-client fixture pass at full counts.
- The persisted summary describes the current corpus rather than an older scenario set.
- Every remaining gap has an explicit `native`, `park`, or `never` decision, or fails loudly without
  corrupting state.

Long-tail work may remain after this gate. An unclassified daily-use surprise may not.

## Updating this file

Update this tracker in the same milestone whenever the registry, accepted artifact, queue, branch,
or delivery state changes:

1. Replace the base-verification date, audited pre-update base, repository/worktree state, delivery
   state, pause state, and next slice. Do not try to embed the SHA of the commit that contains this
   file; resolve it with `git log` and mirror it to issue #7 after the commit exists.
2. Recompute the percentage from `compat/tmux-gaps.json`; update the numerator, denominator, active
   counts, status split, and known differential count.
3. Add the completed milestone row with its exact tracker owner and commit.
4. Replace the current-slice contract and exclusions with the next frozen slice.
5. Record the newest accepted scenario count, step count, attached result, digest, and any incomplete
   or qualified validation.
6. Update the dependency queue after a full live rerank.
7. Update all affected OKF pages and `knowledge/log.md`, then validate the bundle.
8. Mirror the same checkpoint, caveats, next slice, progress, and push state to issue #7.

Never hand-edit `knowledge/tmux/gaps.md`. Change the JSON registry and run
`python3 compat/tmux-tracker.py write-report`.

## Worktree and delivery safety

- Preserve unrelated edits in the shared checkout.
- Never stash, hard-reset, clean, or discard work you did not author.
- Stage exact paths or hunks, never the entire tree.
- Do not add attribution trailers.
- One reviewed, proven, documented slice maps to one milestone commit.
- Commit only with current authorization. Push only on an explicit request.
- Keep issue #7 open until the practical exit gate passes.
