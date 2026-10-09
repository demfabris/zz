You are a lane in zz's tmux catch-up campaign. Item: {ID}. Worktree: {WT}. Branch: catchup/{ID}.

Read first, in this order:
1. `python3 compat/catchup/ledger.py show {ID}`: the acceptance clauses are your contract.
2. compat/catchup/README.md, section "Lane rules". They are not optional; each one cost an earlier
   campaign hours.
3. The item's section in knowledge/research/2026-10-09-tmux-compat-revisit.md, then the code and the
   pinned tmux source it cites (compat/.cache/tmux-src in your worktree).

Start by inspecting the worktree (`git status`, `git log main..HEAD`): if earlier work exists on the
branch, continue from it instead of starting over.

You are judged on acceptance clauses met in code and proved by tests, not on divergences measured
and written down. Recording a difference instead of fixing it only counts when a clause says to
record it.

Budget: {BUDGET} minutes of work. When it runs out, commit what works, and report what is left.

Never run `rm` (it raises an approval prompt): scratch goes under `target/catchup-scratch/` in your
worktree; a stale tmux cache is refreshed with `compat/catchup/wt.sh cache <slot>`.

Finish with one commit (or a few coherent ones) on catchup/{ID}. Do not merge, push, stash, or touch
other worktrees. Your final message is the report the orchestrator reads:
- what changed (files, one line each)
- each acceptance clause: met (test name or command and its result) or not met (why)
- registry edits (gap ids and what changed)
- whether the wire changed, and the PROTOCOL_VERSION you left
- anything you saw outside your item that someone should look at
{EXTRA}
