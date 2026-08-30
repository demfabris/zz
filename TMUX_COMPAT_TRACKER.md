# tmux compatibility campaign tracker

> Campaign state: **SLICE 10AH CLOSED LOCALLY; THREE-FRONT TRIAL ACTIVE**
>
> Tracker resolution progress: **68.5% (139 of 203 known groups)**
>
> Integrated campaign base: **2026-08-29** at `01d540758909ab33eb22bdb074f5bf3db18d755f`

This is the resume point for the entire `alias tmux=zz` campaign. An agent asked to continue the
campaign should read this file, run the preflight below, and resume from the current checkpoint
without reconstructing the history from chat.

Fabrico resumed the campaign after slice 10v. Slice 10w closes the `R` repeat format modifier
already consumed by zz's default second and third status rows. Slice 10x closes the two retained-cwd
edges for `new-session -A -c` and fresh explicit-empty `new-session -c ''`. Slice 10y closes the
config and source replay alias snapshot. Slice 10z constructs each config or source file before any
of its commands run. The same rerank also closed `hooks.queue`: pinned tmux registers
`after-queue`, but ordinary queues never produce it automatically. Slice 10aa closes
`format:session_active` with explicit no-client, unattached-client, and attached-session states
while preserving separate raw invoking and selected target format clients. Slice 10ab closes
`format:window_activity` with a Unix-second timestamp distinct from zz's logical window-order
counter. Slice 10ac closes clean command and status job environments for shell-form `run-shell`,
shell-form `if-shell`, and status `#()`. Slice 10ad moves the unchanged 105-name option-consumer
roster beside command behavior and guards its exact partition against all 180 pinned options and
75 live option gaps. Slice 10ae closes complete option-name format coverage across mux and daemon
format producers. Slice
10af closes positive-delay shell-form `run-shell` environment timing. Scheduling
retains command, target, expanded argument, and cwd context; child launch reads current global,
original-session, terminal, and startup state. Focused daemon coverage proves foreground timing,
and the three-step background differential now completes twelve checks per engine with no differing
channel. Slice 10ag closes startup initial-client cwd. A cold auto-spawned daemon receives the
launcher's bounded UTF-8 cwd through a private argument, uses it only for startup config replay,
then clears it before ordinary client commands. The isolated startup-client-cwd differential passes
exactly on both engines. The full eight-case startup diagnostic reaches a separate Control
exit difference: zz may drain queued shell-prompt `%output` after the flags-0 guard, while pinned
tmux discards it before `%exit`. A rerank then exposed a higher-priority shutdown race: `kill-server`
can close a client mailbox before its successful response is admitted. That response-order item is
the only `next` group; pane-output discard is frozen as slice 10ai. Slice 10ah now closes that race
without a wire change: shutdown freezes admissions, waits for active responses, drains all client
writers while retaining the listener, then removes the endpoint. Pane-output discard becomes the
sole `next` group. The persisted accepted slice
10ag artifact covers 103 scenarios and 1,630 steps, with attached-client `PASS`, exactly two approved
GEO rows, every other channel clean, and SHA-256
`46fdd592366fe2b500fd2031fe82b87df3e4f3fda17f9a6d1a98595ad5da5313`. Commit `562b950c`
contains slices 10w through 10ag.

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
| Integrated campaign base | `01d540758909ab33eb22bdb074f5bf3db18d755f` |
| Delivery | Local `main` contains the trial workflow plus slice 10ah; remote `main` remains through slice 10ag |
| Campaign worktrees | Three short-lived trial worktrees branch from the integrated base; the table below owns their paths and file zones |
| Pinned tmux oracle | `d77c9dc6aa021e4bc61f0da128c591af695e6466` (`next-3.8`) |
| GitHub tracker | [Issue #7](https://github.com/demfabris/zz/issues/7), open |
| Campaign point | Slice 10ah is closed; config parser edges are accepted for the next integration, strict key grammar remains under final review, and 10ai is the sole `next` group |
| Live registry | 86 active groups, 593 active items, 117 closed records |
| Active status | 44 open, 20 blocked, 22 accepted |
| Known differentials | 2 registered geometry cases |

Commit `562b950c` is the campaign code base for this trial. Resolve the commit containing the latest
tracker update with `git log -1 --format=%H -- TMUX_COMPAT_TRACKER.md`, and
resolve live remote `main` with
`git ls-remote https://github.com/demfabris/zz.git refs/heads/main`. Always inspect the live worktree
before acting because other agents may share it.

### Progress calculation

Progress counts a group as resolved when it is either in closed history or has an accepted
`native` or `never` disposition. Open and blocked groups remain unresolved.

```text
(closed records + accepted active groups) / (closed records + all active groups)
(117 + 22) / (117 + 86) = 139 / 203 = 68.5%
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

The 2026-08-29 request resumed the campaign through slices 10w, 10x, 10y, 10z, 10aa, 10ab, 10ac,
10ad, 10ae, 10af, and 10ag, and closed the false `after-queue` producer gap. Slice 10af closes
positive-delay `run-shell` environment timing. Slice 10ag closes cold startup initial-client cwd.
Commit `562b950c` contains slices 10w through 10ag.

## Persisted acceptance evidence for 10ag

Slice 10z adds the two-step `smoke/config-chain-parse-abort` scenario. Slice 10aa extends
`formats-values` from 26 to 28 steps. Slice 10ab extends that row to 45 steps. Slice 10ac adds the
three-step `smoke/jobs-command-environment` scenario. Slice 10ad adds no differential work. Slice
10ae adds the 60-step `option-name-formats` scenario. Slices 10af and 10ag keep the scenario and step
counts unchanged while refreshing the full differential and attached checkpoint:

| Evidence | Result |
| --- | --- |
| Differential scenarios | 103 |
| Differential steps | 1,630 |
| Ordinary rows | All clean |
| Registered differences | One GEO cell in each of two named known rows; every other channel clean |
| Attached-client fixture | `PASS` |
| Summary SHA-256 | `46fdd592366fe2b500fd2031fe82b87df3e4f3fda17f9a6d1a98595ad5da5313` |
| Stored-artifact check | `compat/run.sh --check-summary` passes |

`compat/run.sh --attached-client` completed on the final 10ag code and fixture
tree. All 103 scenarios and 1,630 steps ran, the two registered geometry rows matched their exact
tuples, the attached-client fixture passed, and the stored summary check confirmed the digest
above. The fixture retains the 10q mixed-client destruction case:
the flagged client survives on the newest remaining session, while its unflagged peer exits, on zz
and pinned tmux alike. The complete fixture also passed independently under `LC_ALL=C`, retaining
the repaired 10p full-frame and focus proof. Slice 10q adds no differential scenario or step and
does not change the digest. Slice 10ag validation passes the complete zz package at 653 unit tests
plus 113 CLI binary tests, and the serialized daemon package at 736 unit tests plus two active agent
integrations; one long soak remains ignored. The all-feature workspace run excluding the daemon,
full workspace clippy, and `cargo fmt --check` pass. The artifact and digest above describe commit
`562b950c` through slice 10ag.
Slice 10ad changes no runtime path, protocol, snapshot, scenario, step, attached fixture, or digest.
Its compatibility gate passes 445 mux tests plus the three required daemon inventory tests. The
complete workspace tests and clippy, formatting, tracker, summary, and diff checks also pass.

Slice 10ae adds the focused 60-step `option-name-formats` differential. It reports zero topology,
geometry, format, output, or warning differences. The attached-client fixture passes its live
status option probe. Focused mux and daemon tests cover all 105 names, all four option scopes,
arrays, target selection, missing targets, loops, direct daemon producers, and detached status
refresh. `just compat-check` passes 452 mux tests plus the three required daemon inventory tests.
The slice changes no protocol, wire snapshot, or native GUI styling. The accepted 103-scenario,
1,630-step artifact includes the focused 10ae row and carries the digest above.

Slice 10af keeps `smoke/jobs-command-environment` at three harness steps and expands its background
fixture from eight to twelve checks per engine. It covers a live target, a destroyed target followed
by same-name recreation, a missing target followed by creation, and a timer that crosses startup
completion. Those cases prove frozen formats, numeric arguments, target identity, and cwd together
with launch-time global state, original-session state, `default-terminal`, and the startup TERM
gate. The focused row reports zero topology, geometry, format, output, or warning differences.
Daemon coverage proves the foreground path by waiting for `active_shell_jobs` before it mutates the
environment, then checking the delayed child. Mux coverage proves that retained session handles
follow the original session after destruction and observe later writes when the session started
without an overlay. Focused validation, `cargo build -p zz`, strict workspace clippy, formatting,
and `just compat-check` pass. The full 103-scenario, 1,630-step differential and attached-client run
also pass on the final 10af runtime. The two registered GEO rows retain their exact tuples, every
other channel is clean, and the summary keeps the digest above.

The ten new `new-session-cwd` steps prove that an existing `new-session -A -c` target receives one
format expansion in its resolved session, window, pane, and invoking-client context. An escaped
hash remains literal, and the source session keeps its cwd. Fresh creation and an `-A` miss retain
an empty session path when the command supplies `-c ''`. Focused mux and daemon tests add
clientless inert behavior, permitted Control attach, a nonnested headless terminal-open failure
that retains the cwd mutation, and nested Interactive, Control, and `-A -d` refusals before
expansion, retargeting, or mutation. The slice changes no protocol or snapshot.

The five new differential steps create two detached sessions with `/tmp` and lexical `/tmp/..`,
then prove two targeted displays and one deterministic filtered list return the selected session's
exact stored path on zz and pinned tmux. Focused mux tests separately prove missing retained state,
target isolation, valid UTF-8 with spaces and glob metacharacters, and visibility after the real
`attach-session -c` command updates one session without changing another.

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

Slice 10u passes all 112 CLI integration tests, its focused daemon and Control regressions, strict
workspace clippy, formatting, the all-feature workspace suite excluding `zz-daemon`, and the
six-probe three-step differential. A parallel daemon run passed 710 of 712 tests; both unrelated
failures passed immediately alone. A sequential daemon run passed 711 of 712 tests; its unrelated
viewport-queue assertion also passed immediately alone. Slice 10u therefore does not claim one
uninterrupted 712-test daemon-package process.

## Shipped history

The 115 entries under the generated report's
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
| 10s | Nonconstant global-format behavior partition under `tracker.nonconstant-format-behavior` | `8e0ef67` |
| 10t | Target-session path format under `formats.session-path` | `0da518e` |
| 10u | Warm command-group argument preflight under `mux.command-group-argument-parse-abort` | `a91128c` |
| 10v | Source-owned format producer and modifier registration under `tracker.format-vocabulary-registration` | `bbea66a` |
| Registry correction | Already-shipped whole-file parser abort recorded under `config.parser-abort` | `562b950c` |
| 10w | `R` repeat format modifier under `formats.repeat-modifier` | `562b950c` |
| 10x | Existing-attach and explicit-empty cwd behavior under `sessions.new-session-attach-cwd` | `562b950c` |
| 10y | Config and source replay alias snapshot under `aliases.config-parse-unit` | `562b950c` |
| 10z | Config and source file-unit construction under `mux.chain-parse-abort` | `562b950c` |
| Registry correction | Explicit-only `after-queue` recorded under `hooks.queue` | `562b950c` |
| 10aa | Three-state session-active client context under `formats.session-runtime` | `562b950c` |
| 10ab | Window activity timestamp under `formats.window-activity-time` | `562b950c` |
| 10ac | Clean command and status job environment under `jobs.command-status-environment` | `562b950c` |
| 10ad | Option-consumer source registration under `tracker.semantic-coverage` | `562b950c`; runtime-neutral |
| 10ae | Complete option-name format coverage under `options.option-name-format-coverage` | `562b950c` |
| 10af | Positive-delay run-shell environment timing under `jobs.run-shell-positive-delay-environment` | `562b950c` |
| 10ag | Startup initial-client cwd under `source-file.startup-client-cwd` | `562b950c`; isolated differential exact |

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

## Completed slice: 10t target session path

The full post-10s rerank first surfaced `formats.session-runtime`, but independent source and oracle
audits disproved its shared-client premise. The live registry now separates `formats.session-path`
from the residual `formats.session-runtime`. Pinned `session_path` reads only the selected session's
stored cwd at expansion time. Pinned `session_active` is empty without a target session or format
client; otherwise it is `1` exactly when that client is attached to the target and `0` for an
unattached client or one attached elsewhere. zz does not yet represent that three-state context
across all format producers. Combining the two would turn a direct value repair into a hidden
caller policy refactor.

Slice 10t closes only `formats.session-path/format:session_path`:

- `#{session_path}` resolves from the selected session's retained working directory, independent of
  the invoking client's selected session;
- targeted display and list-session rows return the matching session's path, while a missing
  session or missing retained path expands to empty;
- lexical UTF-8 paths remain unchanged, including `..`, spaces, and glob metacharacters; the format
  layer does not canonicalize or expand the stored value a second time;
- an existing `attach-session -c` update is visible on the next expansion without changing cwd
  mutation semantics;
- the required 198-name manifest partition moves from 92 direct, 32 delegated, and 74 active gaps
  to exactly 93 direct, 32 delegated, and 73 active gaps;
- the five-step pinned differential proves two targeted rows, one filtered list, and lexical `..`
  retention; focused mux tests prove missing session or retained state and production
  `attach-session -c` update visibility.

The slice changes no protocol, snapshot, daemon client selection, session-cwd mutation, startup
source provenance, non-UTF-8 path policy, format vocabulary, or other format value. In particular,
`format:session_active` stays live under its own later three-state producer audit.

Review also exposed two separate cwd-mutation mismatches. Pinned `new-session -A -c` delegates an
existing target to attach handling and replaces that session's cwd, while an explicitly empty `-c`
on fresh creation remains an empty session cwd. zz returns from its existing-session branch before
applying `-c` and collapses an empty fresh value to omitted. At the 10t checkpoint,
`sessions.new-session-attach-cwd` owned both silent mismatches; 10t changed
neither. Slice 10x closes both paths below.

## Completed slice: 10u warm command-group argument preflight

The full post-10t registry rerank overturned the forecast queue. Two independent audits ranked
warm unaliased command-group preflight ahead of the newly exposed cwd pair because it protects
every ordinary chained local script from partial effects and reuses a complete source-owned
validator. The cwd audit also disproved its `easy` label: a direct store in the existing `-A` hit
branch would leak through nested Control, attached Interactive, and `-A -d` refusals that zz
currently classifies after mux execution.

The registry split the old `mux.chain-parse-abort` container. Slice 10u closes only
`mux.command-group-argument-parse-abort/semantic:command-group-argument-parse-abort`:

- against an already-running compatible daemon on the local default or an explicit socket, the
  daemon validates every ordinary tmux command in one prepared vector before the CLI captures
  stdin, selects attach or TUI routing, or executes the first effect;
- canonical names, built-in aliases, unique prefixes, recognized parked commands, and user-alias
  expansions receive the same generic flag, arity, required-value, and nested-block validation;
- a later preparation error returns the pinned diagnostic and leaves earlier state untouched,
  while runtime target or effect errors retain sequential queue ordering and prune only later
  commands;
- exact unaliased `attach` and `attach-session` keep zz's dedicated local parser and positional
  session extension only at vector index zero; later exact spellings and every alias to attach use
  ordinary catalog grammar;
- callback commands keep their existing typed construction path, and valid native zz grammar is
  not pulled into the tmux validator.

The implementation enables ordinary unaliased static validation only for a registered
`ClientKind::Command`. The daemon still prepares one immutable vector, callback commands retain
their typed construction path, user aliases retain their existing validation, and native zz names
remain runtime-owned. The CLI scans every returned result before preprocessing or execution.
Exact native attach bypasses generic validation only at index zero, matching the only position the
private parser can route. Independent review caught and repaired an earlier over-broad exemption
that had allowed a later positional attach to fail after an earlier mutation.

Focused daemon coverage proves canonical, built-in alias, unique-prefix, parked, native, callback,
user-alias, and position-sensitive attach branches. The 112 CLI integration tests prove invalid
flag, excessive arity, missing required value, both later exact attach spellings, exact native
position-zero routing, and runtime target-error ordering; the existing Control regression proves
its framed preparation behavior is unchanged. The strict
three-step `cli-chain-parse-abort` differential now runs six warm probes plus its cold matrix with
zero topology, geometry, format, output, or warning differences. The accepted full artifact stays
at 98 scenarios and 1,522 steps with attached-client `PASS`, two registered geometry rows, and
SHA-256 `810a4adc857b27b42e81fd1bc0c3574e589fcd8d403cb386c5300dfea6276432`.

Slice 10u changes no protocol or snapshot and excludes config or source-file replay construction,
config alias snapshots, parser first-error policy, remote `--host`, Control input, runtime rollback,
multi-command alias bodies, and native zz command grammar. The residual
`mux.chain-parse-abort/semantic:config-source-group-parse-abort` remains active and later.

## Completed slice: 10v format vocabulary registration

The post-10u oracle audit found that schema 4 and its Rust invariant confirmed the same
hand-selected three source scopes and 14 context names. Pinned tmux exposes 31 literal producer
scopes keyed by source path and function, 153 scoped literal registrations, and 108 unique literal
names. The pinned modifier parser accepts 36 modifier tokens.

Slice 10v closes `tracker.format-vocabulary-registration`. Oracle schema 5 now records:

- all 31 literal producer scopes as 153 `(path, function, name)` pairs with 108 unique names;
- 10 queue-added or derived families, including `current_file`, hook arguments and flags,
  run-shell positions, and window-neighbour facts;
- 5 format propagation records;
- all 36 outer modifier tokens.

The Rust and daemon gates keep each source set disjoint and exhaustive. The 153 literal pairs split
into 58 implemented producers, 54 accepted native producers, and 41 active gaps. The 10 derived
families split into 8 implemented families and 2 active gaps. The parser owns 30 modifier tokens;
`w`, `I`, `L`, `O`, `V`, and `R` remain active gaps. The exact daemon test proves the 32
daemon-owned literal pairs and the run-shell positional family against their production consumers.
The mux gate rejects duplicate, stale, missing, or unclassified registrations.

The post-10v rerank split `R` into `formats.repeat-modifier`, which slice 10w later closed. The
remaining `w`, `I`, `L`, `O`, and `V` tokens stay under `formats.modifier-fidelity`.
`formats.context-producer-fidelity` owns the 41 missing literal pairs plus `current_file` and
`next_@*` or `prev_@*` user-option families. The accepted native partition stays under
`formats.native-typed-context-producers`.

This registration changes no format value, protocol, snapshot, scenario, or accepted artifact. It
does not register option consumers or touch cwd, `session_active`, config replay, or startup
provenance. The accepted checkpoint remains 98 scenarios, 1,522 steps, attached-client `PASS`, and
SHA-256 `810a4adc857b27b42e81fd1bc0c3574e589fcd8d403cb386c5300dfea6276432`.

## Registry correction before 10w

The rerank found that `semantic:config-parse-abort` described behavior already implemented and
tested. The parser clears the file's command list on its first diagnostic, stops scanning, keeps
only assignments reduced before that error, and suppresses later diagnostics. The item moved from
`config.parser-edge-cases` into closed history as `config.parser-abort`. The remaining group covers
post-closing-quote tilde expansion plus passwd-backed bare and named-user lookup. This is a ledger
correction with no runtime or accepted-artifact change.

## Completed slice: 10w repeat format modifier

Slice 10w closed `formats.repeat-modifier/semantic:format-modifier-repeat`:

- `R` splits its body at the first top-level comma, then recursively expands the repeated value and
  count;
- counts from 1 through 10,000 repeat the value, while a missing separator, invalid count, zero,
  negative count, or oversized count follows the pinned empty or failure result;
- nested modifiers, an escaped comma, and post-repeat transformations preserve pinned evaluation
  order;
- zz's shipped `P:` and `S:` status rows use the modifier to indent by the byte length reported by
  `n` for the session name without exposing literal `R` syntax;
- `R` moved into the implemented modifier roster, taking it from 30 of 36 to 31 of 36, while exact
  items for `w`, `I`, `L`, `O`, and `V` remain active;
- nested repeat amplification over 40,960,000 intermediate bytes fails before allocation, replacing
  the pin's time budget with a deterministic safety bound.

The slice excludes display-cell width, client interrogation, client loops, option loops,
environment loops, format-context producers, protocol changes, and daemon state modeling.
Focused mux tests, a direct status-row proof, and the clean 16-step pinned `formats` differential
cover the contract. The full 98-scenario, 1,526-step strict run and attached-client fixture pass
with summary SHA-256 `f2aa32e0935e8a839c0abcd43da85e0f474d6c191421776847f7a464cc7257ff`.

## Completed slice: 10x new-session cwd edges

Slice 10x closed `sessions.new-session-attach-cwd` and its two items,
`semantic:new-session-attach-existing-cwd` and `semantic:new-session-explicit-empty-cwd`:

- an existing `new-session -A` target now uses the same retarget and cwd path as
  `attach-session`;
- `-c` expands once after the engine resolves the target session, window, and pane, while the
  invoking client supplies its client context;
- the engine stores the target cwd before a nonnested terminal-open preflight, so a headless
  failure retains the mutation;
- clientless calls stay inert, and a permitted Control client attaches and updates the target;
- nested Interactive, Control, and `-A -d` calls refuse before window or pane selection, format
  expansion, retargeting, or mutation;
- fresh creation and an `-A` miss retain an empty session cwd when `-c` expands to empty, while the
  initial pane still uses the existing donor or caller fallback;
- omitted `-c` keeps its prior inheritance behavior.

The ten-step `new-session-cwd` scenario covers one-pass target-context expansion, escaped hashes,
source-session isolation, fresh explicit-empty creation, and an explicit-empty `-A` miss. Focused
mux and daemon tests cover the remaining client and failure-order branches. The full 99-scenario,
1,536-step strict run and attached-client fixture pass with summary SHA-256
`ed1422d318298b2fee9c31c160393cc2709b9d9137705e96c2632cc700cdcd01`.

## Completed slice: 10y config replay alias snapshot

Slice 10y closed `aliases.config-parse-unit`:

- config construction stores each original invocation beside its alias-expanded command or
  preparation error before replay begins;
- the daemon parses one file, applies that file's environment assignments, and prepares every
  command under one engine lock, so a replayed alias mutation cannot change a later same-line or
  later-line invocation from that file;
- startup roots finish construction before startup replay, and one top-level `source-file`
  invocation constructs all matched files before replaying the batch;
- a nested source receives a fresh snapshot when its parent source command runs, so the child sees
  alias changes replayed before that nested invocation;
- stored preparation errors stay deferred to their original replay positions and retain their
  source and physical-group metadata;
- Control warning-versus-guard classification is frozen with the stored error, preventing an
  earlier replayed alias mutation from changing how the later diagnostic is published;
- `source-file -n` keeps its existing no-effect behavior, including suppression of stored alias
  preparation errors.

Four focused daemon tests cover startup roots, same-file mutation, file environment timing,
multi-file batches, nested refresh, parse-only behavior, deferred errors, and Control diagnostic
classification. The two-step `smoke/config-alias-parse-unit` differential matches the pin in every
comparison channel. The full 100-scenario, 1,538-step strict run and attached-client fixture pass
with summary SHA-256
`8d53288c8050e5c8cf7f19e6c81687f91544877d32ea4de9f7d40ea2934736b7`.

The slice changes no protocol. At the 10y checkpoint it did not close empty or multi-command alias
bodies, generic alias recursion, or eager name, flag, arity, callback, and nested-child
construction. Slice 10z below closes that last construction boundary, including validation under
`source-file -n`.

## Completed slice: 10z config and source construction

Slice 10z closed `mux.chain-parse-abort`:

- each config file now parses, applies permitted bare environment assignments, expands aliases,
  and validates every command group before any command from that file runs;
- the first construction failure preserves earlier bare assignments and discards every command
  effect from that file;
- `source-file -n` performs the same validation against the environment from before that file and
  applies neither assignments nor commands;
- startup roots and files matched by one source invocation remain independent construction units,
  processed in path order before replay, so one invalid file loses only its own commands and later
  files continue;
- a nested child gets a fresh construction snapshot; failure drops that child while the parent
  continues with later physical groups;
- runtime target and effect errors keep sequential behavior and prune only their physical group;
- Control emits one located `%config-error` without a failed-command guard, and delays construction
  warnings until the complete sibling batch has replayed;
- verbose output retains completed physical groups and successful alias-subparse traces before the
  first failure, including parse-only input.

Parser, command, and daemon regressions cover assignment timing, alias snapshots, parse-only input,
startup roots, sibling files, nested children, Control ordering, verbose traces, and the runtime
contrast. The two-step `smoke/config-chain-parse-abort` differential matches the pin in every
comparison channel. The full 101-scenario, 1,540-step strict run and attached fixture pass with
summary SHA-256
`afd1fdf9a79e06f449e8c43abd63b14a2a4968338110223750d4171889c34aaf`.

The slice changes no protocol. Recognized unsupported `choose-client` and `switch-mode` typed
positions, multi-command aliases, non-UTF-8 config bytes, and source stdin retain their existing
owners.

The same evidence pass closed `hooks.queue`. Pinned tmux stores `after-queue` but has no automatic
queue-completion producer. Ordinary single-command and multi-command queues leave it untouched;
`set-hook -R after-queue` runs the stored hook once. The daemon inventory now divides all 68 pinned
names into 64 automatic producers, explicit-only `after-queue`, and three active pane-event gaps.
The existing three-step `smoke/args-parse-set-hook` differential proves those rules without a
runtime or protocol change.

## Completed slice: 10aa session-active client context

Slice 10aa closes `formats.session-runtime/format:session_active`. The format table now has 94
direct mux values, 32 daemon-delegated values, and 72 active constant-backed gaps across the 198
pinned names.

`FormatClient` represents the three pinned states as `NoClient`, `Unattached`, and
`Attached(SessionId)`. `ExecutionContext::format_client()` derives the raw invoking client from its
terminal and attachment state. `ExecutionContext::target_format_client()` overlays the current or
explicitly selected target client when a producer expands against that client. The daemon resolves
that target before mux execution without replacing the raw client, because one command can use the
invoker for a name or cwd expansion and the selected client for `session_active`.

Clientless list and filter rows, chooser rows, and `list-commands` keep `NoClient`. Target-aware
command formats, deferred pane output, shell callbacks, buffer paths, capture boundaries, popup and
menu text, `list-keys`, status rows, Control subscriptions, and display-panes labels receive the
selected client state. `MuxEffect::PaneFormatOutput` carries that exact state through terminal
identity waits. Fresh `new-session -c` expands its stored session cwd and initial pane cwd as two
independent uses of the original format. A non-detached `new-session -P` expands after attachment;
detached creation retains the invoker's raw state.

Focused mux and daemon tests cover the reachable empty, false, and true branches. The final facts
audit proves that `client_*` values and `session_active` use the same selected client across unit,
source-file, `run-shell`, `if-shell`, per-client snapshot, and attached-client fixture paths. The
28-step `formats-values` row passes, and the complete 101-scenario, 1,550-step checkpoint passes
with attached-client `PASS` and SHA-256
`bc0f6ad0fb52d35b6e2e20869d896174ac06b6cb12243e03bcf13e7536134119`. The implementation changes
no protocol or snapshot field.

## Completed slice: 10ab window activity timestamp

Slice 10ab closes `formats.window-activity-time/format:window_activity`. Each window now stores an
optional Unix-second `activity_time` beside the existing logical `Window.activity` counter. Window
creation, parsed nonempty pane output, and the pinned current-window transition paths refresh both
values from the injected engine clock. Same-window selection, pane selection, pane creation,
splits, and layout-only changes without output leave the timestamp alone. Move and swap retain
their pinned transition details.

The independent audit found one direct daemon path that changed a current window without first
refreshing the injected clock. `switch-client` now updates the engine clock before its selection.
The direct Time backing expands empty without a window and preserves the stored seconds through
plain, boolean, comparison, list-row, and time-modified forms. The 198-name partition now contains
95 direct mux values, 32 daemon-delegated values, and 71 active gaps. The implementation changes no
protocol or snapshot field.

The 45-step `formats-values` row proves deterministic creation values, target isolation, actual
selection changes, same-window and output-free no-op paths, and parsed nonempty output. The complete
101-scenario, 1,567-step checkpoint passes with attached-client `PASS` and SHA-256
`309aed0df108abd93e50f2073af7df5991d266c25e55dd266f0c8fc7f412ad72`.

## Completed slice: 10ac command and status job environment

Slice 10ac closes
`jobs.command-status-environment/semantic:shell-job-clean-environment`. Shell-form `run-shell` and
shell-form `if-shell` start children from an empty process environment, apply the modeled global
overlay, then apply the selected session overlay when a session exists. Status `#()` starts from
the same clean base and applies the global overlay only. An explicit missing `-t` target leaves a
command job sessionless, so it receives only the global overlay.

Hidden entries and unset markers do not enter the child. A visible modeled `TMUX_PANE` value
survives, but zz does not invent or delete one. After startup, all three paths set `TERM` from
`default-terminal`, `TERM_PROGRAM=tmux`, `TERM_PROGRAM_VERSION=3.8-zz`, and
`COLORTERM=truecolor`. Startup command jobs preserve the modeled TERM family instead of adding
post-startup values. `TMUX` uses `socket,pid,session` for session jobs and `socket,pid,-1` for
sessionless and status jobs. The private tmux executable remains first on PATH using the modeled
PATH value, and stale private startup variables are removed before the current invocation adds its
own values.

The three-step `smoke/jobs-command-environment` row runs eight internal assertions for inherited
canary removal, overlay order, hidden and unset handling, target loss, terminal identity, modeled
`TMUX_PANE`, and cold versus completed startup. The attached fixture proves the global-only status
path. The complete 102-scenario, 1,570-step checkpoint passes with attached-client `PASS`, exactly
the two registered GEO rows, and SHA-256
`542f7187cb0600c1e28df592c0497aaa90aa8c71c9f07ae3bf76030e54964016`.

Delayed `run-shell` environment timing, `copy-pipe` job environments, popup job environments, and
status `#()` cwd remain active. Pinned status jobs use the attached session cwd; zz still uses
`pane_current_path`. Command-form `run-shell -C` and format-condition `if-shell -F` do not spawn
shell jobs and remain outside this environment closure.

## Completed slice: 10ad option-consumer source registration

Slice 10ad closes
`tracker.semantic-coverage/semantic:tracker-option-consumer-registration`. The unchanged 105-name
option behavior roster now lives in `command::TMUX_OPTION_CONSUMERS` beside mux command and
accessor behavior instead of beside the 180 storage definitions in `tmux_options`. The public
`BEHAVES` alias remains available.

The required exact manifest test proves that the pin and live catalog contain the same 180 unique
option names. It also proves that 105 unique catalogued names belong to the consumer roster, 75
names retain active `option:` gaps, the two sets do not overlap, and their union exhausts the
catalog. The test requires the active discovery item and group to be absent and the closed record
to exist. `copy-mode-mark-style` belongs to the roster only because status option-variable
expansion consumes it; this closure makes no visual mark-rendering claim.

The slice changes no runtime behavior, oracle, protocol, snapshot, scenario, attached proof, or
accepted artifact. `just compat-check` passes 445 mux tests plus the three required daemon
inventory tests. The full workspace tests and clippy, formatting, tracker, summary, and diff checks
pass. The registry now has 85 active groups, 592 active items, and 113 closed records: 43 open, 20
blocked, and 22 accepted, resolving 135 of 198 groups (68.2%).

## Completed slice: 10ae option-name format coverage

Slice 10ae closes
`options.option-name-format-coverage/semantic:option-name-format-coverage`. The format expander now
checks the exact 105-name option roster before format-table names, command-item facts, and
environment values. Exact option names and legacy aliases resolve; command prefixes do not.

The roster contains 13 server, 42 session, 40 window, and 10 pane consumers. Server, session,
window, and pane lookups use the selected target and pinned inheritance chain. Active child state,
attached-client fallback, explicit missing targets, and `S`, `W`, and `P` loop retargeting use the
same resolver. Flags render as `0` or `1`; other types keep their tmux spelling.

`command-alias`, `status-format`, and `update-environment` support whole-array and indexed lookup.
Whole arrays emit numeric entries before named entries. Numeric indices normalize leading zeroes;
malformed, missing, and overflowing indices expand empty. A local array shadows the inherited array
as one unit. Live mux formats read current option state. Every direct daemon format producer calls
the same live resolver. Detached status builds one all-scope option snapshot per refresh batch and
shares it across the client rows in that batch.

Missing-target shell-form `run-shell` and `if-shell` read global option values for `-C` insertion
and `-F` branch selection. Their inserted command or selected branch keeps the caller execution
context. The focused 60-step `option-name-formats` row has zero differential channels, and the
attached status probe passes. Exhaustive mux and daemon tests cover the roster, scope counts,
arrays, targets, loops, producer inventory, and detached refresh sharing. The implementation changes
no protocol, wire snapshot, or native GUI styling.

## Completed slice: 10af positive-delay run-shell environment timing

Slice 10af closes
`jobs.run-shell-positive-delay-environment/semantic:run-shell-positive-delay-environment-timing`.
The scope covers shell-form `run-shell` with an explicit numeric `-d` greater than zero in
foreground and background forms. The scheduler retains command text, target identity and numeric
session id, expanded text and numeric arguments, and the cwd string. When the timer expires, the
child reads current global state, the original session environment, `default-terminal`, and the
startup TERM gate, then checks whether the retained cwd still exists.

A live original session contributes writes made after scheduling. Destroying that session does not
discard its retained environment, and a new session with the same name cannot replace it. A target
missing at scheduling remains global-only with a `TMUX` suffix of `-1` if a matching session appears
before launch. Session handles also cover sessions that had no overlay when the job started, so a
later session write reaches the delayed child.

The foreground daemon regression waits until `active_shell_jobs` reports the scheduled job, mutates
the modeled state, and checks the delayed child after completion. A second daemon regression covers
destroyed and recreated targets plus missing and later-created targets. The mux regressions prove
retained original-session identity and writes into an initially empty overlay. The background
three-step differential completes twelve checks per engine across live, destroyed and recreated,
missing and later-created, and startup-crossing cases. It reports no differing channel. The slice
changes no wire protocol or snapshot field.

`run-shell -C`, `if-shell`, absent `-d`, `-d 0`, immediate background ordering, cwd producer
selection, `copy-pipe`, and popup jobs retain their separate owners. The full 103-scenario,
1,630-step corpus and attached-client fixture pass on the final 10af runtime. The two registered GEO
rows remain bounded, every other channel is clean, and the summary digest remains
`46fdd592366fe2b500fd2031fe82b87df3e4f3fda17f9a6d1a98595ad5da5313`.

## Completed slice: 10ag startup initial-client cwd

Slice 10ag closes
`source-file.startup-client-cwd/semantic:source-file-startup-initial-client-cwd`. The launcher
captures an absolute UTF-8 cwd only when it can represent it within the existing 16 KiB bound and
passes it through private `--bootstrap-client-cwd` only when auto-spawning a daemon. A direct daemon
launch has no bootstrap cwd. Startup replay temporarily installs the value, prefers it over session,
registered reentry-client, command-context, HOME, and root fallbacks, then clears it at the replay
boundary on success or error.

Relative top-level and nested startup sources use the same initial base. A path containing spaces
and glob metacharacters stays literal. After startup, a runtime `source-file` uses the registered
client's current cwd, proving the bootstrap value has expired. Focused launcher, CLI, and daemon
regressions cover capture bounds, priority, nesting, literal paths, expiration, and cleanup. The
isolated startup-client-cwd differential passes exactly on both engines. No public protocol
field or version changes.

The full eight-case startup diagnostic now gets past the cwd assertions and exposes a separate
Control exit mismatch. After a cold `-C new-session`, zz can render three to five queued shell-prompt
`%output %0` rows after the flags-0 `%end` and before `%exit`; ten equivalent pinned-tmux probes
rendered none. `control-mode.exit-pane-output/semantic:control-mode-exit-pane-output-discard` owns
that behavior. Event-hook cwd, sourced Control-hook cwd, non-UTF-8 path transport, and top-level
config discovery retain their existing owners.

## Candidate queue after 10ag

The pre-10x forecast treated `semantic:format-modifier-width` as a small isolated change. The
pinned implementation requires more than a call to zz's current `unicode-width` dependency.
`format_width` parses leading hashes and `#[...]` style spans, returns zero for malformed style
markup, skips control bytes, and reads widths produced by the live `codepoint-widths[]` overrides
and tmux's 162-entry default cache. The pinned harness build passes `--disable-utf8proc`, so its
fallback uses the host `wcwidth` policy. zz uses `unicode-width` 0.2.2. A bounded `w` slice must pin
the style, malformed-input, control, override, cache, platform, and Unicode cases before changing
runtime behavior. The tracker now rates the group later and hard.

The live registry now has 86 active groups, 593 active items, and 117 closed records: 44 open, 20
blocked, and 22 accepted. Closed history plus accepted groups resolve 139 of 203 groups (68.5%).
Priority has one `next`, 63 `later`, and 22 `none` groups.

Slice 10ah closes
`control-mode.kill-server-response-order/semantic:control-mode-kill-server-response-order`.
Response admission now freezes atomically before `ServerStopping`, every registered writer joins the
bounded drain, and the foreground thread keeps the listener until responses and writers finish.
It removes the endpoint before dropping that listener. Controlled tests cover the former admission
race, late Control and Command requests, stalled and disconnected writers, and replacement binding
during cleanup. CLI tests prove the empty successful response, one final Control `%exit`, and an
immediate fresh-daemon launch.

Slice 10ai is frozen under
`control-mode.exit-pane-output/semantic:control-mode-exit-pane-output-discard`. On EOF or a blank
Return, it must discard pending and later `%output` and `%extended-output` pane bytes while retaining
pane output already written before the return. Config diagnostics, command output, guards,
pause/continue notifications, retained return status, and exactly one final `%exit` keep their
pinned order. The cold startup probe must emit diagnostics, one flags-0 success guard, no pane-output
frame, and `%exit`; a long-lived Control client must still receive ordinary pane output before exit.
Hard-disconnect queue cancellation, detach targeting, async command output, no-output, pause, wait,
and transport pressure stay outside 10ai.

The rerank places kill-server response order first because a successful administrative command can
currently lose its response on both Control and ordinary Command paths, and a deterministic daemon
test can close the race without waiting on shell timing. Exit pane-output discard remains the next
independently closable Control slice and is frozen as 10ai. `jobs.shell-job-cwd` follows as a
separate process-launch producer fix. Immediate background `run-shell` ordering stays later and hard
because it must prove absent-delay and `-d 0` queue order without timer races.

`jobs.run-shell-immediate-background-environment` no longer depends on 10af, but it remains later
and hard. It owns absent-delay and `-d 0` foreground blocking plus same-group background ordering
without timing races. `jobs.shell-job-cwd` owns command and status cwd. `jobs.environment` owns
`copy-pipe` and popup jobs.

The active groups marked `next` in the generated report are not themselves execution order.
`keys.copy-mode-binding-fidelity` still depends on `copy-mode.command-fidelity`; forecast labels are
assigned only when a slice is frozen.

Before selecting every later milestone, explicitly recheck attach-dependent and daily-use groups,
including buffer file context, source-file client cwd variants, detach execution, parent-HUP exit,
`display-popup.behavior-fidelity`, menu fidelity, and `active-pane`.

The rerank also corrected several high-confidence registry premises. Floating `break-pane -W` geometry
now depends on `pane.floating-model`; marked display-message target aliases depend on
`pane.selection-state`; option-name format coverage follows source-owned option-consumer
registration; and startup client cwd is a daemon bootstrap-provenance gap rather than an ordinary
protocol/source-target repair. `formats.window-runtime` and `terminal.key-control` remain discovery
containers that must split by producer before implementation.

Two source-file premises changed after direct pin checks. Bare tilde expansion reads a nonempty
server-global `HOME`, falls back to the current user's passwd entry when that value is empty or
unset, resolves named users through `getpwnam`, and reports a located syntax error only when the
required account lookup fails. A tilde after either closing quote also expands. The sourced-hook
cwd mismatch applies to Control replay only; Command replay already retains the caller cwd.

## Three-front trial after 10ag

The trial starts from commit `562b950c`. The registry has 45 open groups; 36 have no declared
prerequisite. A source audit found three bounded chunks with separate production paths:

| Front | Worktree and branch | Tracker contract | Exclusive production and test zone |
| --- | --- | --- | --- |
| Control response | `$HOME/dev/zz-tmux-control`, `codex/tmux-control-10ah` | Slice 10ah: `control-mode.kill-server-response-order/semantic:control-mode-kill-server-response-order` | `crates/zz-daemon/src/daemon.rs`, `crates/zz/src/control_mode.rs`, and Control sections of `crates/zz/tests/cli_binary.rs` |
| Config parser | `$HOME/dev/zz-tmux-config`, `codex/tmux-config-edges` | `config.parser-edge-cases` and its three tilde-expansion items | `crates/zz-mux/src/parser.rs` plus the config-grammar scenario and fixture |
| Key grammar | `$HOME/dev/zz-tmux-keys`, `codex/tmux-key-validation` | `keys.strict-validation` | `crates/zz-protocol/src/key.rs` plus a dedicated key-validation scenario and fixture |

The Control response front enters integration first because 10ah is the sole `next` group. The
other two fronts may finish candidate commits while 10ah runs. The coordinator reranks before each
integration. Slice 10ai stays on the Control file zone and starts after 10ah; its read-only oracle
work may run during 10ah.

The integration coordinator owns `compat/tmux-gaps.json`, generated `knowledge/tmux/gaps.md`, this
tracker, shared OKF status pages, `compat/results/summary.md`, `compat/attached-client.sh`, and
`compat/startup-diagnostics.sh`. A front may add a scenario with a path unique to its chunk. If a
front needs another front's zone or a coordinator-owned path, it stops and reports the overlap.

Each front probes the pinned source and binary, freezes its acceptance contract, edits its owned
paths, runs focused proof, and creates a candidate commit on its branch. The coordinator reviews
the complete candidate diff, applies accepted work without preserving the branch commit as a main
milestone, updates the registry and knowledge pages, runs the closure gates, and creates one
milestone commit on `main`. Fabrico has authorized campaign commits. Pushes still require a new
request.

Focused Cargo tests and read checks may run at the same time in separate worktrees. Each worktree
uses its default `<worktree>/target`; setting a shared `CARGO_TARGET_DIR` breaks the compatibility
runner's binary lookup. Cache repair, oracle writes, tracker report generation, full corpus runs,
attached-client runs, full workspace tests, and strict clippy run through the integration
coordinator. Full corpus runs also touch `/tmp/zz-c1-history`, so two worktrees must not run them at
the same time.

After all three chunks reach a candidate result, review the trial itself. Record merge conflicts,
cross-front file requests, test interference, review fixes, abandoned work, and the number of
closures that reached `main`. Keep the model when it reduces idle time without weakening the
per-slice proof or creating cleanup work.

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
4. Replace the current-slice contract and exclusions with the next frozen slice, or record an
   explicit pause without selecting one.
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
