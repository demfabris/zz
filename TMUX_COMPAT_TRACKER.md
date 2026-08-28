# tmux compatibility campaign tracker

> Campaign state: **SLICE 10S DELIVERED: LIVE RERANK REQUIRED**
>
> Tracker resolution progress: **64.7% (121 of 187 known groups)**
>
> Audited pre-close base: **2026-08-28** at `6e076028e867e67bbbc0a988224ec0c5cf42f1aa`

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
| Audited pre-close base | `6e076028e867e67bbbc0a988224ec0c5cf42f1aa` |
| Delivery | The current local milestone closes 10s on top of its committed plan; `origin/main` remains at `7cad19e` until an explicit push |
| Dedicated campaign worktree | Removed after delivery on 2026-08-28 |
| Pinned tmux oracle | `d77c9dc6aa021e4bc61f0da128c591af695e6466` (`next-3.8`) |
| GitHub tracker | [Issue #7](https://github.com/demfabris/zz/issues/7), open |
| Campaign point | Slice 10s is complete; run a live full-registry rerank before freezing its successor |
| Live registry | 87 active groups, 593 active items, 100 closed records |
| Active status | 46 open, 20 blocked, 21 accepted |
| Known differentials | 2 registered geometry cases |

The 10s closure descends from the committed plan above. Resolve the commit
containing the latest tracker update with `git log -1 --format=%H -- TMUX_COMPAT_TRACKER.md`, and
resolve live remote `main` with
`git ls-remote https://github.com/demfabris/zz.git refs/heads/main`. Always inspect the live worktree
before acting because other agents may share it.

### Progress calculation

Progress counts a group as resolved when it is either in closed history or has an accepted
`native` or `never` disposition. Open and blocked groups remain unresolved.

```text
(closed records + accepted active groups) / (closed records + all active groups)
(100 + 21) / (100 + 87) = 121 / 187 = 64.7%
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

## Accepted evidence after 10s

The strict-plus-attached artifact remains the current accepted checkpoint. Slice 10s adds no
differential row or step and does not change its digest:

| Evidence | Result |
| --- | --- |
| Differential scenarios | 98 |
| Differential steps | 1,517 |
| Ordinary rows | All clean |
| Registered differences | One GEO cell in each of two named known rows; every other channel clean |
| Attached-client fixture | `PASS` |
| Summary SHA-256 | `9c147eb5caa78ca51e068275b28836ab2647d3d959d047c5fafbcb5c0bf86832` |
| Stored-artifact check | `compat/run.sh --check-summary` passes |

`just compat --strict-geometry --attached-client` completed on the final 10q code and fixture tree.
All 98 scenarios and 1,517 steps ran, the attached-client fixture passed, and the stored summary
check confirmed the digest above. The fixture now includes the 10q mixed-client destruction case:
the flagged client survives on the newest remaining session, while its unflagged peer exits, on zz
and pinned tmux alike. The complete fixture also passed independently under `LC_ALL=C`, retaining
the repaired 10p full-frame and focus proof. Slice 10q adds no differential scenario or step and
does not change the digest. Focused package tests, affected clippy, formatting, tracker and OKF
validation, shell syntax, and diff checks also passed.

The 10r fixture adds 11 cold-socket probes per engine. They cover implemented and parked built-in
syntax through canonical names, aliases, and prefixes; exact native `attach` and `attach-session`
tails; and `-N` routes. Every invalid vector exits before spawn or mutation. The accepted artifact
therefore stays at 98 scenarios, 1,517 steps, attached-client `PASS`, and SHA-256
`9c147eb5caa78ca51e068275b28836ab2647d3d959d047c5fafbcb5c0bf86832`.

Slice 10s adds only source-registration invariants. The required mux manifest test partitions all
198 pinned global format names, and the required exact daemon test resolves every delegated name
through the production consumer. It changes no differential row, fixture step, runtime value, or
accepted digest.

This run supersedes the earlier replacement run that stopped during `lane2-store`.

The fresh monolithic workspace run reached the unrelated
`pipe_pane_has_no_gap_when_control_attaches_during_a_flood` test after every changed suite passed,
then wedged under full-workspace load. Its exact solo rerun passed in 0.06 seconds. The complete
daemon package passes 711 tests, and `cargo test --workspace --all-features --exclude zz-daemon`
passes every remaining workspace test, including 643 all-feature app-library tests and all 111 CLI
tests. Workspace strict clippy also passes. This is the load-induced daemon-test classification
documented in `AGENTS.md`, not a 10r regression; the checkpoint does not claim one uninterrupted
monolithic workspace test process.

The fresh 10s daemon-package sweep again reached that flood test after the changed status-test
cluster passed, then wedged. Its exact solo rerun also wedged and was stopped. The focused delegated
consumer test, both exact compatibility daemon tests, all 422 mux tests, the all-feature workspace
suite excluding `zz-daemon`, and strict workspace clippy pass. Slice 10s therefore claims the
source-registration closure above, not a fresh uninterrupted 712-test daemon-package pass.

## Shipped history

The 100 entries under the generated report's
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
| 10p proof repair | Live-popup focus suppression, dead `-k` focus-close, and C-locale frame proof | `c909406` |
| 10q | Per-client no-detach-on-destroy fallback and attached proof | `8310fb7` |
| 10r | Local cold-start CLI parse abort under `mux.local-cli-autospawn-parse-abort` | `02bb4a1` |
| 10s | Nonconstant global-format behavior partition under `tracker.nonconstant-format-behavior` | Current milestone; resolve after commit |

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
- routes popup keys, paste, pointer, and tracked wheel before global shortcuts, prefix handling,
  prompts, choosers, browsers, or underlay terminal input;
- updates client focus state without forwarding focus reports into a live popup terminal, while a
  dead close-on-any-key popup closes on FocusOut;
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
close-on-key behavior, live-popup focus suppression, dead `-k` FocusOut closure, and a final
one-byte underlay sentinel. Coordinates derive from the outer terminal cursor so centering and
Unicode width cannot turn the mouse proof into a false positive. The frame assertion accepts both
Unicode and ACS borders, so the complete fixture also proves the contract under `LC_ALL=C`.

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

## Completed slice: 10q per-client no-detach-on-destroy fallback

The tracker moved `clients.no-detach-on-destroy` and its only item,
`semantic:no-detach-on-destroy-fallback`, into closed history. The requested flag was already
retained and reported, so the slice needed no protocol or snapshot change.

The daemon now computes the configured primary and bounded newest-session fallback once before
moving any client. The primary still applies to every client. Under `on`, and under `no-detached`
when no detached primary exists, only a flagged client uses the fallback. This preserves mixed
flagged and unflagged behavior without letting the first reattach change the next client's choice.
No remaining session still exits every client, and the flag cannot alter `off`, `previous`, or
`next`.

Focused daemon tests cover mixed clients, both `no-detached` branches, no remaining session,
unchanged direct policies, exact `Attached` and `SessionDestroyed` delivery, and retained flag
state. The attached fixture creates two real raw clients on one victim session, flags one by its
exact tty, and proves that client survives on the newest fallback while its peer process exits on
both zz and pinned tmux.

Active-pane routing, detach execution, parent-HUP exit, resize-hook ordering, buffer and source-file
cwd, and popup or menu residue remain separate.

## Completed slice: 10r local cold-start CLI parse abort

The tracker moved `semantic:local-cli-autospawn-parse-abort` into closed history as
`mux.local-cli-autospawn-parse-abort`. The old chain-abort item remains split: warm unaliased generic
command groups and config or source-file replay keep separate active owners.

Before 10r, a missing local daemon made whole-vector preparation fail open. Routing then inspected
the first command, so a later syntax error could arrive after spawn and mutation. The CLI now checks
the complete raw vector before routing, stdin, native TUI handoff, spawn, startup config, or effects.
The static catalog covers all 83 implemented upstream commands plus nine parked commands. Canonical
names, built-in aliases, and prefixes receive flag, arity, and callback-type validation without
expanding arbitrary user aliases.

Exact native `attach` and `attach-session` routes validate their command tail before handoff. The
same gate blocks `-N new-session`, `-N attach`, and `-N attach-session` from reaching a TUI or spawn
after a later syntax error. Canonical startup spellings still admit config shadowing; an arbitrary
startup alias name cannot trigger autospawn.

After the raw gate passes, the spawned daemon prepares the complete vector under one post-config
alias snapshot. It constructs nested callbacks and checks every result before the first command
runs. The CLI tags that daemon generation, and a one-shot lease assigns the first external client
as owner while excluding startup reentry. Sticky contention prevents a disconnected competitor
from restoring exclusivity. A successful prepare or pipelined command commits the lease. On a
prepare failure, the daemon sends the error before the owner's disconnect requests shutdown, and
the stopping state rejects later registrations.

Focused tests cover startup shadowing, arbitrary and nested aliases, exact native routes, lease
ownership, contention, pipelining, and shutdown ordering. The strict fixture runs 11 cold probes on
each engine. Runtime target and effect errors retain queue semantics: earlier effects stay visible,
and later commands do not run. Slice 10r changes neither protocol nor snapshot schema.

## Completed slice: 10s nonconstant format behavior partition

The post-10r rerank selected `semantic:tracker-nonconstant-format-behavior` as the largest bounded
silent-discovery surface. The pinned oracle and zz both name 198 global format-table variables, but
the compatibility gate currently classifies only the 74 variables whose zz backing is a constant
placeholder. The other 124 names are implicitly trusted even though their behavior is split
between 92 mux-resolved backings and 32 daemon-provided status-hook values.

Slice 10s closes the blind spot with one source-derived partition:

- the production format table classifies exactly 124 unique nonconstant format names as 92 direct
  mux values plus 32 values delegated to the daemon;
- an exact daemon consumer test proves that the complete 32-name delegated roster is handled by the
  production format hook rather than merely named by the mux;
- the 124 behavior registrations and 74 active `format:` items are disjoint and their union equals
  all 198 unique names in the pinned oracle and zz format table;
- every registered or tracked name remains live, and duplicate, stale, missing, or newly
  unclassified names fail the compatibility gate;
- the invariant extends the existing required manifest test and adds a required exact daemon test,
  so `just compat-check` cannot silently stop enforcing either half.

This is a registration milestone, not a claim that all 124 values match tmux in every context. It
changes no runtime value, fixes no active `format:` item, expands no open-ended command-specific
context vocabulary, registers no option consumer, and changes neither the oracle nor protocol.
Existing format owners retain responsibility for value and context parity.

## Non-frozen forecast after 10s

No successor is frozen. Run a full live rerank first; the table records the previous rerank's
strongest successors, not permission to skip that audit.

| Order | Exact owner | Boundary |
| --- | --- | --- |
| 1 | `semantic:source-file-startup-initial-client-cwd` | Recheck the newly bounded cold-bootstrap seam for startup-relative source paths |
| 2 | `semantic:config-source-group-parse-abort` | Recheck config and source replay atomicity using the 10r preparation machinery |
| 3 | `semantic:tracker-open-context-format-vocabulary` | Discover and register open context formats |
| 4 | `semantic:tracker-option-consumer-registration` | Discover and register option consumers |
| 5 | `semantic:copy-mode-action-vocabulary` | Inventory all 95 pinned copy actions before behavior changes |
| 6 | `semantic:source-file-sourced-hook-client-cwd` | Preserve the invoking client when a sourced hook re-enters source replay |
| 7 | Remaining `copy-mode.action-fidelity` items | Cursor, logical line, goto, selection, jump/prompt, and copy effects as separate slices |
| 8 | `keys.copy-mode-unsupported-default-actions` | Add seven defaults only after their actions exist |
| 9 | `copy-mode.command-fidelity` and dependent binding or prompt work | Resolve or reclassify the interactive-refresh dependency first |

The active groups marked `next` in the generated report are not themselves execution order.
`keys.copy-mode-binding-fidelity` still depends on `copy-mode.command-fidelity`; forecast labels are
assigned only when a slice is frozen.

Before selecting every later milestone, explicitly recheck attach-dependent and daily-use groups,
including buffer file context, source-file client cwd variants, detach execution, parent-HUP exit,
`display-popup.behavior-fidelity`, menu fidelity, and `active-pane`.

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
