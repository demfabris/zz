---
type: Playbook
title: Running tmux compatibility cohorts
description: A bounded, parallel workflow for closing the practical alias tmux=zz gap without letting new oracle findings extend one campaign forever.
tags: [tmux, compatibility, campaign, workflow, agents]
timestamp: 2026-08-26T00:00:00-03:00
---

# Outcome

Close the practical `alias tmux=zz` gate through bounded cohorts. Each cohort starts with a fixed
acceptance contract, ends with one reviewed commit, and leaves later discoveries in the
[live tracker](/tmux/gaps.md).

The Alert cohort completed implementation, proof, and documentation on 2026-08-26. Its reviewed
milestone commit is the handoff gate before Client Context begins. The next session records that
committed checkpoint from `HEAD`.

The full Alert checkpoint covers 84 scenarios and 1,475 steps. Every ordinary row is clean.
`known/known-main-preset-two-panes` and `known/known-spread-mixed` each retain exactly one documented
GEO divergence with every other channel clean. The attached-client fixture and
`compat/run.sh --check-summary` both pass. The canonical summary SHA-256 is
`5de67222bc2ebb99c57963be14c865ddfdddc387da34ee32dd86962cef8336c9`.

# Cohorts

| Cohort | Scope | Exit proof |
|---|---|---|
| Alert | Status-message lifecycle for Bell, Activity, and Silence; alert log identity; repeated BEL from one unvisited pane; attached-client timing margins | Focused daemon and terminal tests, pinned alert probes, one full debug attached-client fixture, tracker and knowledge updates |
| Client Context | Attach context, environment refresh, client format values, and client lifecycle hooks | One written oracle contract per behavior family, focused differential coverage, one full debug attached-client fixture |
| Error and Coverage | Error shapes, semantic-coverage blind spots, invalid flag handling, and uncovered asynchronous copy or pipe errors | Every changed claim gets a pinned differential or a focused test with a named tracker item, followed by one full debug attached-client fixture |
| Copy Mode | Default and custom binding fidelity across copy tables, action behavior, and repeat/count handling | Binding manifest reconciliation, attached key-path probes, and one full debug attached-client fixture |

The coordinator may split a cohort before implementation if its oracle contract crosses unrelated
subsystems. Do not merge two listed cohorts to save a commit.

# Four-seat Codex pipeline

Use the four seats as one coordinator and three Codex subagents:

1. The coordinator fixes the cohort boundary, assigns file ownership, integrates changes, and owns
   the commit.
2. The oracle agent probes the pinned tmux commit and writes the acceptance contract plus the
   smallest differential fixture that can disprove it.
3. The implementation agent changes one owned subsystem and runs focused tests. After review starts,
   this seat may scout the next cohort without editing its files.
4. The review agent hunts context, performs an independent code and proof review, then checks
   tracker and knowledge claims against source.

Assign one owner to each path before agents edit. The coordinator resolves overlaps instead of
letting two agents rewrite the same file. Use Codex subagents for this campaign.

# Validation ladder

Run the cheapest proof that can fail the current edit:

1. During implementation, run focused Rust tests and the scenario or attached probe for the changed
   behavior.
2. At cohort close, build the debug binary and run the full attached-client fixture against the
   pinned tmux oracle. Treat a skip or reduced scenario count as a failure.
3. At a campaign checkpoint, run `just compat --strict-geometry --attached-client`, regenerate the
   canonical summary, and run `compat/run.sh --check-summary` as a separate check.

Use campaign checkpoints after the Alert cohort, after two more cohorts, and at the practical exit
gate. Do not run release builds for compatibility work.

# Discovery rule

The oracle agent records new gaps in `compat/tmux-gaps.json`. A discovery joins the active cohort
only when it uses the same production path, needs no protocol or schema change, and fits the cohort's
existing proof. A discovery that invalidates the cohort's claimed behavior blocks closure. All other
findings wait for a later cohort.

Freeze the acceptance contract after the oracle and implementation agents agree on it. Review can
reject the implementation or proof, but it cannot expand the cohort with unrelated cleanup.

# Goal boundary

Create one persistent Codex goal per cohort. Name the tracker groups and exit proof in its objective.
Complete that goal after the milestone commit and handoff, then start the next cohort in a fresh
session. Do not use one goal for the remaining tracker.

# Milestone commits and worktree

Commit each cohort after code review, proof, tracker generation, and OKF validation pass. Stage exact
paths or hunks because the shared checkout may contain unrelated work. Tell Fabrico before invoking
`git commit` so he can touch the YubiKey. Do not push unless he asks.

After the Alert checkpoint lands, create a dedicated `codex/tmux-compat` worktree from that commit.
Resolve the target branch and path with read-only checks first. Leave the current dirty checkout and
its unrelated edits intact.

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

Paste this prompt into the next session after the Alert milestone lands:

```text
Continue the tmux compatibility campaign in /Users/demfabris/dev/zz from the committed Alert
checkpoint at HEAD.

Read AGENTS.md, knowledge/playbooks/tmux-compat-cohorts.md,
knowledge/designs/tmux-superset-roadmap.md, and compat/tmux-gaps.json before editing. Verify that
HEAD closes both alerts.message-lifecycle and alerts.repeated-bell-edge, then capture the checkpoint
with git rev-parse HEAD. Work from a dedicated codex/tmux-compat worktree created from that
checkpoint; preserve every unrelated edit in the shared checkout.

Own the Client Context cohort only: attach context, client environment refresh, client-context
formats, and client lifecycle hooks. Use Codex subagents in the four-seat pipeline from the
playbook. Give each agent disjoint file ownership. Pin behavior to tmux commit
d77c9dc6aa021e4bc61f0da128c591af695e6466, use exact -t =name targets, and treat skips or reduced
scenario counts as failures. New findings go to the tracker unless they use the same path and fit
the existing cohort proof.

Run focused tests while editing. At cohort close, use a debug build and run the full attached-client
fixture. Run the canonical strict plus attached checkpoint only when the playbook calls for it. Do
not run a release build. Update the tracker and OKF docs, get an independent Codex review, and close
the cohort with one milestone commit. Before invoking git commit, stop and tell Fabrico that the
YubiKey touch is next. Do not push.
```
