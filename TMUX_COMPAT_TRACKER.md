# tmux compatibility campaign tracker

> Campaign state: **SLICE 10P COMMITTED; RERANKING THE NEXT SLICE**
>
> Tracker resolution progress: **63.8% (118 of 185 known groups)**
>
> Committed milestone base: **2026-08-28** at `587ce5487dfafabe8d6b1357c31bd2ae032f0b8b`

This is the resume point for the entire `alias tmux=zz` campaign. An agent asked to continue the
campaign should read this file, run the preflight below, and resume from the current checkpoint
without reconstructing the history from chat.

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
| Repository | `$HOME/dev/zz` |
| Published branch | `origin/main` |
| Committed milestone base | `587ce5487dfafabe8d6b1357c31bd2ae032f0b8b` |
| Delivery | Local `main` contains 10p; `origin/main` remains at `7cad19e` until an explicit push |
| Dedicated campaign worktree | Removed after delivery on 2026-08-28 |
| Pinned tmux oracle | `d77c9dc6aa021e4bc61f0da128c591af695e6466` (`next-3.8`) |
| GitHub tracker | [Issue #7](https://github.com/demfabris/zz/issues/7), open |
| Campaign point | Slice 10p is committed; the live rerank has not frozen its successor |
| Live registry | 88 active groups, 594 active items, 97 closed records |
| Active status | 47 open, 20 blocked, 21 accepted |
| Known differentials | 2 registered geometry cases |

The 10p milestone commit descends from the verified `origin/main` base. Resolve the commit
containing the latest tracker update with `git log -1 --format=%H -- TMUX_COMPAT_TRACKER.md`, and
resolve live remote `main` with
`git ls-remote https://github.com/demfabris/zz.git refs/heads/main`. Always inspect the live worktree
before acting because other agents may share it.

### Progress calculation

Progress counts a group as resolved when it is either in closed history or has an accepted
`native` or `never` disposition. Open and blocked groups remain unresolved.

```text
(closed records + accepted active groups) / (closed records + all active groups)
(97 + 21) / (97 + 88) = 118 / 185 = 63.8%
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
cd "$HOME/dev/zz"
git status --short --branch
git worktree list
git fetch https://github.com/demfabris/zz.git main:refs/remotes/origin/main
git rev-parse HEAD origin/main
```

Preserve unrelated changes in the standard checkout. If it is dirty or shared, create a fresh
dedicated worktree from the verified `origin/main` instead of changing or committing those edits:

```sh
git worktree add -b codex/tmux-compat-next "$HOME/dev/zz-tmux-compat" origin/main
cd "$HOME/dev/zz-tmux-compat"
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

Fabrico granted standing authority to commit each reviewed campaign milestone and continue into the
next reranked slice. Pushes still require an explicit request.

## Accepted evidence at the 10p checkpoint

The fresh strict-plus-attached 10p artifact is the current accepted checkpoint:

| Evidence | Result |
| --- | --- |
| Differential scenarios | 98 |
| Differential steps | 1,517 |
| Ordinary rows | All clean |
| Registered differences | One GEO cell in each of two named known rows; every other channel clean |
| Attached-client fixture | `PASS` |
| Summary SHA-256 | `9c147eb5caa78ca51e068275b28836ab2647d3d959d047c5fafbcb5c0bf86832` |
| Stored-artifact check | `compat/run.sh --check-summary` passes |

`just compat --strict-geometry --attached-client` completed on the final 10p code and fixture tree.
All 98 scenarios and 1,517 steps ran, the attached-client fixture passed, and the stored summary
check confirmed the digest above. Focused package tests, workspace clippy, formatting, tracker and
OKF validation, shell syntax, and diff checks also passed.

This run supersedes the earlier replacement run that stopped during `lane2-store`.

The fresh workspace test sweep stopped in `zz-daemon` after 706 of 708 tests passed. The two failing
mode-key tests passed immediately when rerun alone, matching the parallel-load classification in
`AGENTS.md`. Cargo therefore did not continue through every later workspace package; affected
package tests and the full compatibility checkpoint remain green.

## Shipped history

The 97 entries under the generated report's
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
| 10p | Raw-TUI popup state, rendering, input ownership, and attached proof | `587ce54` |

`10j/10k` is one deliberate milestone because both commands use the same callback implementation
and attached proof. Slice 10l records source ownership without changing runtime behavior. The count
correction is documentation maintenance, not a compatibility slice. Slices 10n through 10p expand
the attached fixture without adding differential scenario rows.

## Completed slice: 10p raw-TUI popup

The tracker moved `semantic:tui-display-popup-overlay` from
`clients.tui-overlay-consumption` into closed history as
`clients.tui-display-popup-overlay`. No protocol or snapshot change was needed.

Raw zz-tui now:

- seeds, updates, reconnects, replaces, closes, and resets popup state from `ClientCore`;
- centers and clamps one shared floating layout, with borderless content using the full frame and
  bordered content inset beneath the title;
- renders the popup terminal, styles, border, title, and cursor above the workspace and below menu
  and confirmation overlays;
- routes popup keys, paste, pointer, tracked wheel, and focus before global shortcuts, prefix
  handling, prompts, choosers, browsers, or underlay terminal input;
- tracks held keys only when the outer terminal advertises Kitty release events, while the popup
  application's own Kitty mode controls whether release events reach that application;
- removes synthetic frames and renderer caches on close or replacement, then repaints the latest
  underlay.

The daemon now evaluates popup mouse acceptance against the owning popup terminal's per-client
viewport. Tracked popup mouse therefore works while the global mouse option is off. One physical
tracked wheel notch emits one application report; local and Shift scrolling keep the three-line
step.

The attached fixture adds three popup cases. They cover bordered rendering and update-in-place,
bracketed paste, an exact click press and release, one tracked wheel report, a retained dead popup,
close-on-key behavior, external focus ownership, and a final one-byte underlay sentinel. Coordinates
derive from the outer terminal cursor so centering and Unicode width cannot turn the mouse proof
into a false positive. Pinned tmux emits three internal underlay focus-out/focus-in pairs during the
fixture; zz emits none. Explicit external focus still stays popup-owned on both.

Focused proof passed:

- `cargo test -p zz-tui`: 154 tests;
- popup-focused `zz-client` and `zz-daemon` tests, including global-mouse-off popup tracking;
- `cargo clippy -p zz-tui --all-targets --all-features --no-deps -- -D warnings`;
- `cargo build -p zz`, formatting, shell syntax, and `git diff --check`;
- the complete attached-client fixture twice on the final binary and fixture tree.

Review findings were addressed; the final rereview found no remaining actionable issue. The broader
contracts below remain open under `display-popup.behavior-fidelity`:

- `semantic:display-popup-resize-lifecycle`
- `semantic:display-popup-style-refresh`
- `semantic:display-popup-context-menu`
- `semantic:display-popup-border-drag`
- `semantic:display-popup-to-pane`
- `semantic:display-popup-kitty-images`

Real mouse/status format facts remain under `formats.mouse-context`. Control-mode popup
presentation and read-only popup actions also remain outside this slice.

## Candidate queue after 10p

This is a dependency forecast, not a frozen next slice. The post-10p rerank also surfaced
`clients.no-detach-on-destroy` and `mux.chain-parse-abort` as practical contenders. Recheck the live
registry and acceptance cost before choosing among them and the 10q registration slice.

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

The two active groups marked `next` in the generated report are not themselves execution order.
`keys.copy-mode-binding-fidelity` still depends on `copy-mode.command-fidelity` and therefore stays
at slice 15.

Before selecting every later milestone, explicitly recheck attach-dependent and daily-use groups,
including buffer file context, source-file client cwd variants, detach execution, parent-HUP exit,
`display-popup.behavior-fidelity`, menu fidelity, `active-pane`, and `no-detach-on-destroy`.

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
