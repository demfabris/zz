---
name: review
description: Review code — dirty/uncommitted files, a working diff, a branch, a PR, or just some named files — by first running a methodical parallel context hunt with sub-agents, then judging with calibrated severity. Use whenever the user asks to review changes or code, wants a second opinion on a diff/PR/branch, says "review what I've got", "look over my changes", "check these files", "is this safe to merge", "check this before I commit", or wants a sanity check on recent work. Not for general codebase questions (that's research).
---

# Review

You are tasked with reviewing a code change: first build a complete, neutral picture of the change and its blast radius using parallel sub-agents, then — and only then — judge it. Context first, opinions last.

## Why this order matters

Bad reviews fail in exactly two ways, and both are context failures:

1. **Missed real problems** — the reviewer never looked at the callers, tests, or history.
2. **Invented problems** — the reviewer didn't know the surrounding code already handles the case, so a non-issue gets escalated to "blocker" and the author burns time refuting it.

The fix for both is the same: gather evidence before forming judgments, and require evidence for every claim. A false blocker is not a "safe" mistake — it costs trust and often provokes complexity being added to appease the review.

## CRITICAL: Division of labor

- **Sub-agents are documentarians, not critics.** They describe what exists — where code lives, how it works, what patterns and history surround it. They never evaluate, warn, or suggest. If a sub-agent returns opinions ("this looks risky"), discard the opinion and keep only the facts.
- **You, in the main context, are the only judge.** All findings are formed here, after the context hunt, under the calibration rules below.

## Step 1: Establish scope (main context)

**There does not have to be a diff.** "Review my dirty files", "look at what I've been hacking on", "check `parser.rs` and `lexer.rs`" are all valid — a review is a review of *code*, and a diff is just one way to point at it. Work out what the user is pointing at, then read it FULLY yourself before spawning anything.

| The user points at | How you establish scope |
|---|---|
| Dirty / uncommitted work (the default when they just say "review my changes") | `git status --short` first, then `git diff` **and** `git diff --staged`. **Untracked files (`??`) have no diff — Read them whole**, or you'll review half the change and miss the new files entirely. |
| A branch | `git diff <base>...HEAD` plus `git log <base>..HEAD --oneline` |
| A PR | `gh pr diff <number>` and `gh pr view <number>` |
| Named files, or a subset of the dirty ones | Read them entirely (no limit/offset). If the file is also modified, read the diff *and* the current file — the diff shows the delta, the file shows what it now says. |

When it's ambiguous, ask which they mean rather than guessing — reviewing the wrong scope wastes the whole context hunt.

Also capture the *intent*: commit messages, PR description, or just the user's stated goal ("I'm trying to make the retry backoff not hammer the API"). Where there is no commit or PR to read, the user's sentence IS the intent — a review judges the change against its intent, not against a different change you would have made. If there's no stated intent at all and it isn't obvious from the code, ask for one.

## Step 2: Context hunt (parallel sub-agents)

Decompose the change's blast radius and spawn sub-agents in parallel — one per question, not one giant prompt. Sub-agent reference files live in the `agents/` directory that sits alongside `skills/` — `$HOME/.agents/agents/*.md` when installed globally, `.agents/agents/*.md` when vendored into a repo.

Cover these angles methodically (skip an angle only when the diff clearly can't touch it):

| Angle | Agent | Ask it for |
|---|---|---|
| Blast radius | **codebase-locator** | Every caller/importer/consumer of the changed functions, types, endpoints, configs |
| Behavior | **codebase-analyzer** | How the touched code paths actually work now — including upstream validation, error handling, invariants already enforced |
| Conventions | **codebase-pattern-finder** | How the codebase already does the thing the change does (error handling, naming, test shape, module layout) |
| History | **codebase-archaeologist** | Why the old code was the way it was — prior fixes, reverted attempts, load-bearing weirdness (Chesterton's fence) |
| Tests | **codebase-locator** / **codebase-analyzer** | Which tests cover the touched paths, and what they actually assert |

Instruct every sub-agent explicitly: *document, don't evaluate*. You want facts with `file:line` references, because every finding you make later must cite them.

Wait for ALL sub-agents before forming any judgment.

## Step 3: Judgment (main context only)

Form candidate findings, then try to kill each one before it reaches the report.

### The bar for a finding

Every finding must have a **concrete failure scenario**: specific input or state → specific wrong outcome, reachable from real callers found in Step 2. If you cannot write that sentence, it is not a finding — drop it or go read more code until you can.

### Calibration rules — the burden of proof is on the finding

- **Verify before flagging.** "Missing null check" is only a finding if the value can actually be null at that point — check the upstream context the analyzer gathered. Most "missing validation" findings die here.
- **Blocker means demonstrable.** Reserve it for provable incorrectness, data loss, security holes, or breakage of an actual caller — with `file:line` evidence. "Could be a problem", "might not scale", "what if someone later..." are not blockers; usually they are not findings at all.
- **Don't review hypothetical requirements.** No flags for inputs that can't occur, scale the system doesn't have, or flexibility nobody asked for. Robustness against impossible states is complexity, not safety.
- **Match the codebase, not your taste.** If the change follows an existing pattern found in Step 2, the pattern is out of scope. "I'd have done it differently" is not "wrong."
- **Suggested fixes must be net-simplifying.** A fix that adds an abstraction, config knob, or layer must remove more risk than the complexity it adds. Prefer fixes that delete code. If the only fix you can think of adds complexity for a marginal issue, downgrade or drop the finding.
- **No hedged findings.** If you're still unsure after checking the context, either verify it yourself (read the code, trace the path) or drop it. Never ship "might be an issue?" — that outsources your job to the author.
- **A clean review is a successful review.** Zero blockers is a valid, common, correct outcome. You are not graded on finding count; do not manufacture severity to look thorough.

### Severity ladder

Try to argue each finding *down* the ladder before assigning; it stays only where you can defend it.

- **Blocker** — provably breaks correctness, data, security, or a real caller. Evidence attached.
- **Should fix** — real defect or genuine convention violation, but shippable; won't corrupt anything.
- **Nit** — cheap improvement, author's call. One line each, batched. Never dressed up as more.

## Step 4: Report

Present in the conversation (only write a review doc if the user asks; then use `YYYY-MM-DD-review-<topic>.md`):

```markdown
## Review: [change summary]

**Verdict**: Ship it | Ship after fixes | Needs work
[One-sentence justification]

### Blockers
- [Finding] — failure scenario, `file:line` evidence, suggested fix

### Should fix
- ...

### Nits
- ...

### Considered and dismissed
- [Candidate concern] — why it's a non-issue (`file:line` evidence)
```

The **Considered and dismissed** section is mandatory when you dismissed anything. It shows the work without inflating findings, and it pre-empts the same false alarm being raised again by the next reviewer.

## Important notes

- Read everything under review yourself in main context BEFORE spawning sub-agents — you can't decompose a blast radius you haven't seen. Untracked files included; `git diff` won't show them.
- Run sub-agents in parallel; keep the main context for synthesis and judgment, not deep file spelunking.
- Every finding cites `file:line`. Every dismissal of a tempting concern also cites `file:line`.
- Judge the change against its stated intent and the codebase's existing conventions — not against an imagined rewrite.
- If the change reveals a genuinely better larger refactor, mention it once, clearly labeled as out of scope — never as a blocker.
