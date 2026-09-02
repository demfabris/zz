# Cycle-8 handoff for the tmux-compat campaign

Written 2026-09-02 ~04:30Z on the macbook, mid-overnight autonomous run (fabrico authorized driving
the campaign end to end without asking). Cycle 7 is DONE; cycle 8 is written, minted on the board,
and launched right after this commit. Everything a machine needs is in this repository and in issue
#7.

## State now

| Fact | Value |
| --- | --- |
| `origin/main` | `e910a732` (cycle-7 ledger; lanes `327d036f` daemon, `dee45667` copy, `03f61a41` client) plus this records commit |
| Agreed-scope meter | 77.6% (236/304 items), 36/65 groups done; `python3 compat/progress.py` |
| Live registry | 29 unresolved groups (22 open, 7 blocked); ledger 86.6% (188/217) |
| Corpus | 178 scenarios / 2,435 steps, attached-client PASS |
| `PROTOCOL_VERSION` | 94 (hex hello frame 0x5E, test `..._ninety_four`); cycle 8 bumps to 95 (0x5F) |
| Remote | `origin` is HTTPS (`https://github.com/demfabris/zz.git`, gh credential helper) since the SSH security keys became unavailable mid-cycle-7; switch back with `git remote set-url origin git@github.com:demfabris/zz.git` once the YubiKey is back |
| Board (issue #7) | MAIN and TRIAGE free. Cycle-8 lock fronts READY: `F-DAEMON-FOCUS-PROMPT-V2` (p2, contract 5506005142), `F-FORMATS-REGISTRY-SETTLE` (5506005284), `F-TERMINAL-KNOBS-COPY` (5506005480). Also READY and untouched: `F-KEY-CONTROL-V3` (mooted if cycle 8 closes `-K`), `F-PANE-BORDER-ZORDER` + `-LINES-TILED` and `F-PANE-COMMAND-COMPLETION` (next client cycle / design front), the `F-SPLIT-MUX-*-V5` chain |

## Cycle 8, launching

`opus-compat-run-8.js` beside this file is the full cycle: three Opus implementor lanes in parallel
worktrees, one Fable adversarial reviewer pipelined behind each, one serialized Fable gate (daemon,
formats, terminal) that merges to main, ledgers the board, recomputes `TMUX_COMPAT_TRACKER.md`, and
runs TRIAGE. Machine facts come from workflow `args`; the defaults are this macbook.

| Lane | Lock front | Batch |
| --- | --- | --- |
| daemon | `F-DAEMON-FOCUS-PROMPT-V2` | the pane focus hooks first with a three-hour budget (PANE_FOCUSED set at the pin's trigger points, transitions spliced by anchor); then status-keys plus vi prompt editing, the three key spellings, the message-covers-prompt shape; client environment bytes (94 to 95); `send-keys -K` last |
| formats | `F-FORMATS-REGISTRY-SETTLE` | the per-command format client (activity-time best client, null list rows) closing the resize context and `window_bigger`; the null-aware `display-message -a` listing against the measured 142/28/28 name sets; the four pane process formats; the bare `=` mouse target; the I modifier through ported `tty_term_codes` and `tty_features`; four registry settlements (tilde home, per-client active pane, remote alias preflight, shutdown unlink order); `-v` last |
| terminal | `F-TERMINAL-KNOBS-COPY` | the knob path widened and scroll-on-clear, alternate-screen, allow-rename, backspace implemented, the four libghostty-impossible knobs settled; `copy-mode -s` as a source-pane revision; the three terminal-owned blocked items measured and implemented or settled; mode-keys plumbing last |

What cycle 8 leaves for cycle 9: the client basket (`options.pane-border-chrome` with its exact
status-row recipe, `choosers.command-flags`, the three popup pointer items behind a menu-over-popup),
`keys.copy-mode-binding-fidelity` (14 on the accepted `-P` decision, 2 on the search action family),
`formats.context-producer-fidelity` (the `set-hook -B` monitor subsystem), and the two design fronts
(`control-mode.disconnect-cancels-command-queue`, `split-window -W`).

Launch:

```js
Workflow({
  scriptPath: "<checkout>/compat/orchestration/opus-compat-run-8.js",
  args: {
    root: "/Users/demfabris/dev/zz", dev: "/Users/demfabris/dev", holder: "macbook/orchestrator",
    machine: "16-core, 48 GB macOS box (macbook)", cores: 16,
    workerJobs: 8, workerThreads: 4, gateJobs: 16, gateThreads: 8, shards: 8,
    protected: "the user's default tmux server (sessions clairvo, home, zz) and the /Applications/zz.app daemon on the default socket",
    bash: "/opt/homebrew/bin/bash"
  }
})
```

Omit `args` on this macbook. Before launching: claim the three lock fronts under your holder
identity (`ZZ_BOARD_HOLDER=<holder> python3 compat/board.py claim <FRONT> --lease 6h`), check
`origin/main`, and start a lease renewer (a loop renewing the three fronts and MAIN with
`--lease 6h` every ~100 minutes). Cycle 7 took 3h53m and 2.64M subagent tokens for seven agents.
After the gate finishes, verify `origin/main`, the board records, and `compat/progress.py`, then
write cycle 9 from a fresh registry census as described under the loop below. `CAMPAIGN-LOG.md` has
the per-cycle history.

## The cycle, in general

If a run dies mid-way in the SAME session, `Workflow({scriptPath, resumeFromRunId})` replays
finished agents from the journal cache (`subagents/workflows/<runId>/journal.jsonl`, result key
`result`); across machines or sessions, export the cached reports into a continuation script the
way `opus-compat-run-6-continue.js` did (it embeds worker and review reports as constants and skips
the agents they replace). If the gate died after pushing some lanes, finishing the rest by hand is
often cheaper: the gate's fix commits sit in its `zz-gate-*` worktrees.

Orchestrator loop per cycle: claim the three lock fronts under your holder identity (6h leases,
renew them and MAIN while the gate runs), launch, verify `origin/main`, the board records, and
`compat/progress.py` when the gate finishes, then write the next script from a fresh registry census
(protocol version, lock names, group lists, the mooted fronts for TRIAGE; the census is
`compat/tmux-gaps.json` gaps with status open or blocked, read every reason), mint the next lock
fronts under TRIAGE, commit the orchestration records under MAIN (a records-only push is ledgered as
`integrated MAIN --merge <sha>`), and repeat.

## This machine (`macbook`)

16 cores, 48 GB, macOS 27. `origin` is HTTPS (gh's credential helper) since the SSH security keys
went away mid-cycle-7 ("device not found" on the ED25519-SK identities); SSH worked non-interactively
before that and will again with the YubiKey. Unattended runs need two Claude Code settings that are
in place: a global `Bash(rm:*)` allow rule, and the PreToolUse hook `~/.claude/scripts/guard-rm-home.py`
that denies `rm -rf $HOME` (the built-in critical-path guard prompts on that shape and no allow rule
bypasses it). `caffeinate -is -w <claude pid>` keeps the Mac awake for the session; the lid must stay
open. `gh` is logged in with repo scope. Holder identity
`macbook/orchestrator`. Caches are populated (`compat/.cache/tmux-src/tmux` at `d77c9dc6`,
`compat/.cache/plugins`, eight plugins); the `formats` scenario ran clean here. Worktrees: `~/dev/zz-opus-dint`,
`~/dev/zz-opus-panes` and `~/dev/zz-opus-termopts` exist, clean and warm at the cycle-7 lane tips; the
reviewers reuse `zz-review-*` when present. Machine traps the prompts already
carry: `/bin/bash` is 3.2 (no `mapfile`; shard runners use `/opt/homebrew/bin/bash` or python3),
APFS refuses non-UTF-8 file names (`smoke/client-non-utf8-cwd` guards for it since cycle 7), and the user has a live tmux server (sessions `clairvo`, `home`, `zz`)
and a live `/Applications/zz.app` daemon that no worker may kill. The Ubuntu box still holds the
cycle-6 worktrees `zz-opus-panes`, `zz-opus-dint`, `zz-review-dint` with 15-29 GB `target/` dirs.

## Before launching on a new machine

- Pass the machine facts as `args` (see above): paths, holder, core etiquette, shard count, the
  protected-server sentence, and the bash to use for helpers. Nothing in the script needs editing.
- The git transport lines assume SSH works non-interactively; the prompts carry the HTTPS fallback.
- Populate the caches once: the pinned tmux builds through `compat/fetch-tmux.sh`, the plugin corpus
  through `compat/fetch-corpus.sh`; running any scenario through `compat/run.sh` triggers both.
  `ZZ_COMPAT_TMUX` and `ZZ_COMPAT_CORPUS` are preset in every prompt so sharded runs never race the
  clone.
- `gh auth login` with repo scope for `compat/board.py`; pick a holder identity like
  `<host>/orchestrator` and claim the three lock fronts under it before launching.

## Board tool quirks

- `--holder` is a global flag (before the subcommand); `ZZ_BOARD_HOLDER` does the same job.
- `release` and `withdraw` require `--reason`; `note` takes `--note`; `candidate` takes `--commit
  --branch --base` plus repeatable `--proof`; `integrated` takes `--merge` and optional `--gate`;
  `front` takes `--contract --zones` plus `--priority --kind {work,lock} --deps --path --notes`.
- `renew <FRONT> --lease 2h`: a bare number is silently ignored, always give a unit; the new expiry
  is the comment time plus the lease, so a short renew can shorten a long lease.
- One zone, one claim, even for the same holder: claim one lock front per lane and let the gate
  ledger the fronts it moots. READY fronts whose zones overlap a claimed lock read `zones-busy`
  until the release; that is expected.
- `withdraw` and `front` need TRIAGE held; `integrated`, `repair`, and `rejected` need MAIN held.
  A records-only push (ledger, docs) is ledgered as `integrated MAIN --merge <sha>`.
- A withdrawn front that other fronts depend on reads as `deps-broken` for them: withdraw the
  dependent first (`F-PANE-BORDER-LINES-TILED` before `F-PANE-BORDER-ZORDER`), or remint as V(n+1).
- `board.py` stores a front's contract as a free string: when a contract group closes but its slugs
  move to another group, a RESIDUAL redirecting the claim is enough, no remint needed.
- Unknown zone names only warn; `python3 compat/board.py zones` lists the real ones.
- `python3 compat/board_test.py` is part of every gate.

## Moving the Claude Code session itself

The repository and the board are the durable state. If you also want the old session's transcript
and memory, Claude Code keeps them under `~/.claude/projects/<checkout path with slashes as
dashes>/`: the session's `.jsonl`, a same-named directory with subagent transcripts and workflow
journals, and `memory/` with the project's auto-memory. Copy them into the matching project
directory on the new machine (clone at the same absolute path or rename the directory to match),
then `claude --resume <session-id>` from that checkout. Workflow journal replay depends on the old
scratchpad paths, so launch the next cycle's script fresh rather than resuming an old run.

## Lore the prompts already encode

Flaky-under-load list (all pass exact-solo): `client_focus_closes_display_panes_and_preserves_chooser_modes`,
`attached_client_extents_clamp_retained_and_default_dimensions`,
`history_request_is_guarded_clamped_and_returns_self_contained_rows`, copy-mode reconcile tests,
`daemon_native_split_resize_commits_exactly_and_rejects_stale_contexts`,
`nested_alias_queue_bubbles_shutdown_and_yield_to_its_parent`,
`control_sourced_run_shell_closes_before_raw_output_and_same_line_continues`,
`request_full_enqueues_only_the_requested_visible_pane`,
`display_menu_resize_lifecycle::a_resize_moves_the_menu_and_keeps_everything_else` (waiter wakes
before the chosen row's command runs; the test polls now; residual 5503619053),
zz-terminal `pty_output_drains_while_the_input_writer_is_backpressured`,
`wait_exit_holds_the_control_process_until_a_second_blank_line` (hangs under load; timeout-guard
cli_binary runs), `concurrent_default_interactive_attaches_share_session_zero` (headless "not a
terminal", may be misattributed), `smoke/source-replay-diagnostics` (pin-side crash under
concurrent scenario load; run it solo after sharded gates), and `smoke/client-non-utf8-cwd` on APFS.

Registry grammar: closing = removing the slug from the group's items (an emptied group moves to
`closed[]`); native decisions = relocate the slug into an accepted-native group with the measured
stance (precedent `1f24a1f1`); park = relocate into a `park`/`blocked` group with the recipe; flag
promotions move `catalog.rs` counters and `compat_manifest_tests.rs` partition counts together
(precedent `c6ce82c4`); a wrong close is reverted and the reason records the refuting measurement
(precedents `0fec342` + `9cab1fa`, `be0052bb` + `6ca61b1f`). `cargo test -p zz-mux` is mandatory
after any registry edit, and `cargo test -p zz-daemon --lib` whenever mux target resolution, effect
shapes, or layout change.

Reviewer catches worth remembering: proofs gathered before the final commit are worthless (cycle 4);
durable registry resolutions must carry every divergence the worker discloses; doc comments must
stay attached to their fn; a menu width rule that ignored the title seed (cycle 3); a mode reset
built on DECSTR, which the pinned libghostty ignores entirely (cycle 5); a fixture that configured
away the pin's gating (`focus-events on`) and so could not see that the pin emits two focus hooks
where zz emitted ten (cycle 5); a resolution claiming the mark survives cross-session `move-window`
when the pin never retargets `marked_pane.s` (cycle 5); a hook close built on a mechanism the pin
does not have (the notified client as format client, where the pin uses the activity-time best
client; a single-client differential could not see it) (cycle 6); bound-key chains preflighting the
invoker's read-only bit instead of the `-c` client's (cycle 6); a fixture step that landed in the
wrong snapshot on both binaries and so looked like a divergence (cycle 6). Gate lore:
`compat/run.sh --delta --list` once returned a stale selection, so list twice and reconcile against
`git diff --name-only`; grep the tree for conflict markers after every rebase (one got committed
into `catalog.rs`); a shard runner under bash 3.2 ran the whole corpus eight times at once.
