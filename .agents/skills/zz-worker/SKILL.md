---
name: zz-worker
description: Autonomous worker for the zz tmux-compat campaign dispatch board (GitHub issue #7 on demfabris/zz). Use whenever the user says to grab work from the board or issue 7, work a front, act as a campaign worker, "grab a front and do it all the way", or spawn a compat worker. Turns this session into a self-contained worker that claims a front, proves it in an isolated worktree, integrates to main through the MAIN lock, and loops until nothing is claimable.
---

# zz campaign worker

You are one worker among several running in parallel on this machine. The board
(issue #7 on demfabris/zz) hands out bounded fronts; you take one all the way
through integration, then take another. No human reviews your work: the
integration gate is the reviewer, so run it honestly or the next worker inherits
your mess.

The issue body is the protocol's source of truth and may have evolved since this
skill was written. Read it first (`gh issue view 7 --repo demfabris/zz`) and
follow it where it disagrees with anything below.

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

If you fan work out to subagents (parallel oracle probes, test shards, code
mining), they inherit your holder id and never speak on the board: no claims,
no comments, no work outside your claimed zones and reserved paths. One
holder, one voice.

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
applies to the protocol itself: verbs, states, zones, the gate.

The tool folds the issue's comments into board state. Always act through it:
`status` shows everything, `pick` names your next front, `claim` adjudicates the
race for you and prints `WON` or `LOST`. Never post board comments by hand and
never edit an issue comment; an edited comment is void by protocol.

Run board commands from inside the repo checkout: `claim` resolves the base
commit with `git ls-remote origin`, which fails elsewhere.

## The loop

1. `pick`, then `claim <front-id>`. On `LOST`, pick again; losing a race costs
   nothing.
2. Make an isolated worktree for the front and work only there:
   `git worktree add ../zz-<front-id> origin/main`. The primary checkout at
   `~/dev/zz` usually holds other sessions' uncommitted work: read it, add
   worktrees from it, but never edit, stash, reset, or clean it.
3. The contract is the gap's `acceptance` list in `compat/tmux-gaps.json` (the
   front's `contract` field names the gap). When behavior is unclear, probe the
   pinned tmux source and binary (`compat/fetch-tmux.sh` builds it), not memory.
   Stay inside your claimed zones (`zones` subcommand maps them to paths); if
   you need one more, `renew <front-id> --zones <zone>` and stop if refused.
4. Prove it focused: package tests plus a differential scenario at the path your
   front reserves. `compat/run.sh <scenario-name>` runs one scenario against
   both engines. The first corpus run builds the pinned tmux and is slow; that
   is normal.
5. One commit, no attribution trailers, then
   `git push origin HEAD:campaign/<front-id>` and post `candidate` with your
   proof lines.
6. Integrate it yourself: `claim MAIN --lease 2h`. Holding MAIN, rebase onto
   fresh origin/main, run the full gate from the issue (workspace tests, clippy,
   strict corpus with the attached fixture, registry close + tracker
   check/write-report, rollup counts), `git push origin HEAD:main`, post
   `integrated`, `release MAIN`. Only ever push `campaign/*` branches, plus
   main while holding MAIN, and never force-push anything.
   If `compat/board.py` is not on origin/main yet, merge
   `origin/campaign/board` as part of your first integration.
7. Remove the worktree (`git worktree remove ../zz-<front-id>`) and go to 1.

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
- Push of main rejected as non-fast-forward: the lock was violated somewhere.
  Never force; re-rebase, re-gate, push again.
- `pick` says `NOTHING-CLAIMABLE`: claim TRIAGE, mint fronts from open registry
  groups per the issue's triage section (bounded to one 6h lease each, unique
  scenario path, deps where needed), release, and continue. Mint for the
  future, not just for now: a front blocked behind an active claim still
  counts, because it becomes claimable the moment that claim integrates. Never
  release TRIAGE with nothing minted while open registry groups remain, and
  pair every withdrawal of a real contract with a corrected re-mint, or a
  residual saying exactly what must change before re-minting. If the registry
  itself has nothing left to mint, you are done.
- `NOTHING-CLAIMABLE` while another worker holds TRIAGE: do not stop and do
  not camp the lock. Re-check `status` every few minutes; their mints or an
  integration will free work. Spend the wait on review (below).
- A front is in `CANDIDATE` and you are idle or blocked: review it. Fetch its
  `campaign/*` branch, read the diff against the front's contract, and probe
  the pinned oracle where behavior is in doubt. File each confirmed
  in-contract failure as a `residual` on that front, then post a `note` with
  your verdict and the residual comment ids. A MAIN holder weighs standing
  verdicts before pushing; confirmed failures mean repairing under the
  existing claim, not integrating past them. Review only what you can verify;
  a wrong DO-NOT-INTEGRATE wastes more than it saves.

## Stopping

Stop only when nothing is claimable and triage has nothing to mint, or the user
interrupts. Close with a short report: fronts integrated with their merge
commits, fronts released and why, residuals filed, and what the board looked
like when you left.
