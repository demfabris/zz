---
name: zz-worker
description: Implement an agreed batch for the zz tmux-compat campaign using its dispatch board (GitHub issue 7 on demfabris/zz). Use when continuing the campaign, taking a board front, or completing compatibility work. Preserve board ownership, finish implementation before final validation, and publish through MAIN only when authorized.
---

# zz tmux compatibility campaign

Read `compat/orchestration/HANDOFF.md` and the campaign log's latest entry first.
Implement the agreed batch directly in the current session. The campaign does
not prescribe models, reasoning effort, delegation, agent roles, lane counts or
time budgets. Old run scripts and dated reports are historical references.

Finish implementation, fixtures and integration conflicts before running the
validation harness. Use small pin probes or local unit checks only when needed
to answer an implementation question. Run the full strict corpus and attached
fixture once on the completed combined candidate, with the required workspace
checks. Preserve the strict stamp rule and reuse valid results for unchanged
inputs, including documentation updates and publication of tested history.

Read the issue body (`gh issue view 7 --repo demfabris/zz`) for board verbs,
states, leases and zones. Current user instructions and the handoff govern work
scope and validation sequencing.

## Identity, once

Generate a session name and treat it as a constant for the whole session:

```sh
echo "$(shuf -n2 /usr/share/dict/words 2>/dev/null | tr '\n' '-' | tr '[:upper:]' '[:lower:]')$RANDOM"
```

(or invent two short words yourself; the `$RANDOM` suffix keeps two workers
spawned together from colliding). Your holder id is `<hostname -s>/<name>`, and
your tool copy is `/tmp/zz-board-<name>.py` (the name only: the id's slash
would make it a bogus path). Write both down in your first message and never
change them: the board tracks your claims and leases by the exact id string.

Keep one board identity for this session. Record ownership of claimed zones and
reserved paths, and keep board writes under that identity.

Shell state does not persist between your Bash calls, so an `export` is gone by
the next command. Prefix every board invocation instead:

```sh
ZZ_BOARD_HOLDER=<host>/<name> python3 /tmp/zz-board-<name>.py <subcommand>
```

## Bootstrap

The repo is `~/dev/zz` (use the current directory if it is already a checkout
of demfabris/zz). Then:

```sh
cd ~/dev/zz && git fetch origin main campaign/board
git show origin/main:compat/board.py > /tmp/zz-board-<name>.py 2>/dev/null \
  || git show origin/campaign/board:compat/board.py > /tmp/zz-board-<name>.py
```

The issue may show a simpler bootstrap with a shared `/tmp/zz-board.py`; the
per-worker filename and trying origin/main first are deliberate refinements for
parallel workers on one machine, not a protocol conflict. "The issue wins"
applies to the protocol itself: verbs, states, zones and leases.

The tool folds the issue's comments into board state. Always act through it:
`status` shows everything, `pick` names your next front, `claim` adjudicates the
race for you and prints `WON` or `LOST`. Never post board comments by hand and
never edit an issue comment; an edited comment is void by protocol.

Run board commands from inside the repo checkout: `claim` resolves the base
commit with `git ls-remote origin`, which fails elsewhere.

## Implement and validate a batch

1. Select fronts within the user's agreed scope, then `claim <front-id>`.
   On `LOST`, inspect the board and choose available work in that scope.
2. Make an isolated worktree for the front and work only there:
   `git worktree add ../zz-<front-id> origin/main`. The primary checkout at
   `~/dev/zz` usually holds other sessions' uncommitted work: read it, add
   worktrees from it, but never edit, stash, reset, or clean it.
3. The contract is the gap's `acceptance` list in `compat/tmux-gaps.json` (the
   front's `contract` field names the gap). When behavior is unclear, probe the
   pinned tmux source and binary (`compat/fetch-tmux.sh` builds it), not memory.
   Stay inside your claimed zones (`zones` subcommand maps them to paths); if
   you need one more, `renew <front-id> --zones <zone>` and stop if refused.
4. Finish the batch's implementation and fixtures. Reserve each scenario path
   on its front. Use small probes or local unit checks for concrete questions;
   defer corpus and attached-client runs until implementation is complete.
5. When the batch is ready, `claim MAIN --lease 2h`, combine its changes on
   fresh origin/main and resolve conflicts. Inspect the diff and commit the
   final code when authorized, without attribution trailers. Freeze source
   and harness inputs during validation.
6. Run the required workspace tests, clippy and registry/document checks, then
   one `just compat --strict-geometry --attached-client` for the combined batch.
   Confirm full counts, registered known rows and a clean PASS stamp with
   `just compat --check-summary`. Fix failures before retrying affected checks;
   a failed or invalidated full run needs a successful full run before a current
   PASS can be recorded. Partial runs cannot replace it. Documentation-only
   changes and publication of unchanged tested history reuse the existing proof.
7. Update the registry, generated report, tracker and log from actual results.
   When publication is authorized, push the batch's branch and main while
   holding MAIN, record `candidate` and `integrated` for its fronts with the
   tested commit and proof, then release MAIN and completed claims. Never
   force-push. For an already published campaign branch, use a fresh immutable
   name such as `campaign/<front-id>-<short-sha>` when its history changes.
8. Re-read front comments before publication and resolve confirmed blockers.
   Preserve unfinished work and release MAIN if repairs will take more than a
   few minutes. Remove only this task's delivered worktrees when appropriate.
   Continue with another batch only within the user's requested scope.

A lone `zz-daemon` test failure in the workspace run is often load flake:
re-run that test alone before treating it as red (AGENTS.md has the list).

## Leases and time

Your default lease is 6h and expiry silently frees your front and zones for
other workers. You have no timer, so make checking a habit: `status` prints
minutes remaining on your own claim. Renew (`renew <front-id>`) before any step
you expect to be long, a workspace build, the corpus, the gate. If you come
back from a long step and your lease is gone, check `status` before touching
anything: if someone claimed your front, stop and let them have it.

## When things go sideways

- Blocked, wrong contract, front bigger than a lease: `release` with a reason
  and file what you learned as `residual`. Never sit on a claim you are not
  working and never silently expand scope.
- `pick` says a front is `STALE-CANDIDATE`: a previous worker finished the code
  but died before integrating. Claim it, fetch their `campaign/*` branch, and
  finish the integration instead of redoing the work.
- Gate fails under MAIN, or review verdicts demand repair: MAIN is for
  integrating, not repairing. Fix under your claim only if it is minutes;
  anything longer means release MAIN (your front and zones stay held), repair
  under the front claim, and re-claim MAIN when candidate-ready. A held MAIN
  starves every queued candidate. If the repair is wrong-shaped, post
  `rejected` on your own front instead and release both.
- Push of main rejected as non-fast-forward: inspect the new commits and
  reconcile the candidate without force-pushing. Reuse checks only where their
  inputs stayed unchanged. The final `--check-summary` must pass; changes that
  invalidate the stamped proof require another full run before publication.
- `pick` says `NOTHING-CLAIMABLE`: inspect whether the agreed work needs a new
  front or is held elsewhere. Use TRIAGE to mint only work within that scope,
  with unique scenario paths and dependencies. Record blockers and preserve
  unfinished work instead of expanding into an unrequested campaign loop.
- If inspecting an existing `CANDIDATE`, fetch its
  `campaign/*` branch, read the diff against the front's contract, and probe
  the pinned oracle where behavior is in doubt. File each confirmed
  in-contract failure as a `residual` on that front, then post a `note` with
  your verdict and the residual comment ids. A MAIN holder weighs standing
  verdicts before pushing; confirmed failures mean repairing under the
  existing claim, not integrating past them. Review only what you can verify;
  a wrong DO-NOT-INTEGRATE wastes more than it saves.

## Machine etiquette: the box is shared

Several workers share one machine, and the integration gate's timing-sensitive
tests fail under load: a starved gate wastes a full workspace-plus-corpus run
and everyone's wall clock. The rules:

- While MAIN is CLAIMED by someone else, run no workspace-scale builds or
  tests. Focused package tests are fine; so is review and board work.
- As a non-MAIN-holder, cap your parallelism: `cargo build/test --jobs 8` and
  `-- --test-threads=4` (half and quarter of the 16 logical cores).
- As the MAIN holder, build at full speed but run the workspace test stage
  with `-- --test-threads=8`: determinism beats raw speed after any gate
  failure, and deadline or backpressure assertions starve above that when
  anything else breathes.
- A timing assertion that fails in a loaded full run and passes exact-solo is
  a load flake: re-run solo to classify, and re-gate rather than repair.

## Stopping mid-flight: rescue your work

If you must abandon work you cannot finish (interrupt, dead end, lease about
to lapse with a long step ahead), never let it die on your disk:

1. Commit whatever compiles, even WIP, and push a fresh immutable branch:
   `campaign/<front-id>-<short-sha>` for candidate-quality work,
   `campaign/<front-id>-<label>-wip` for anything unfinished.
2. Post a `note` on the front naming the branch, its parent commit, what ran
   green, and what was NEVER run. An adopter must know which proofs are owed.
3. Release the front if you can; otherwise expiry frees it.

The mirror rule: before starting ANY front, read its comment history for
rescue notes and standing verdicts. A rescued branch with green proofs is
adopted and finished, never re-derived; redoing rescued work is the most
expensive misbehavior on this board.

## Stopping

Stop when the agreed work is complete, blocked, or the user interrupts. Close
with a short report: fronts integrated with their merge
commits, fronts released and why, residuals filed, and what the board looked
like when you left.
