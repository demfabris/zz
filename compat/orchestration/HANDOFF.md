# Cycle-7 handoff for the tmux-compat campaign

Written 2026-09-02 ~03:20Z on the macbook. Cycle 6 is DONE and cycle 7 is written, minted on the
board, and NOT launched. Everything a machine needs is in this repository and in issue #7.

## State now

| Fact | Value |
| --- | --- |
| `origin/main` | `1d99c77b` (cycle-6 ledger; lanes `b1c80f66` keys, `0201a5e9` daemon, `0d727e36` client) |
| Agreed-scope meter | 71.7% (218/304 items), 31/65 groups done; `python3 compat/progress.py` |
| Live registry | 35 unresolved groups (26 open, 9 blocked); ledger 83.9% (182/217) |
| Corpus | 167 scenarios / 2,402 steps, attached-client PASS, summary digest `878c2299…` |
| `PROTOCOL_VERSION` | 93 (hex hello frame 0x5D, test `..._ninety_three`); cycle 7 bumps to 94 (0x5E) |
| Board (issue #7) | MAIN and TRIAGE free. Cycle-7 lock fronts READY: `F-DAEMON-PROMPT-FOCUS` (p2, contract 5503736750), `F-COPY-MODE-DAEMON-VIEW` (5503736866), `F-CLIENT-CHROME-OVERLAYS` (5503736968). Also READY and untouched: `F-PANE-COMMAND-COMPLETION` (needs a design front), the `F-SPLIT-MUX-*-V5` chain, and the three fronts cycle 7 moots (`F-DISPLAY-MESSAGE-PANE-TARGETS-V3`, `F-KEY-CONTROL-V3`, `F-PANE-BORDER-ZORDER` + `-LINES-TILED`) |

## Cycle 7, ready to launch

`opus-compat-run-7.js` beside this file is the full cycle: three Opus implementor lanes in parallel
worktrees, one Fable adversarial reviewer pipelined behind each, one serialized Fable gate (daemon,
copy, client) that merges to main, ledgers the board, recomputes `TMUX_COMPAT_TRACKER.md`, and runs
TRIAGE. Machine facts come from workflow `args`; the defaults are this macbook.

| Lane | Lock front | Batch |
| --- | --- | --- |
| daemon | `F-DAEMON-PROMPT-FOCUS` | fixture guard for APFS first; command-prompt `-t` as the routing (a command client is held until the prompt closes) plus vi editing in the daemon, `-F`, key spelling, message freeze; pane focus hooks with a PANE_FOCUSED set at the pin's trigger points; the per-command format client (activity-time best client, null for list rows) closing the resize context and `window_bigger`; display-panes templates and waits; the display-message client aliases; client environment bytes last |
| copy | `F-COPY-MODE-DAEMON-VIEW` | the copy-mode format family produced from the daemon's copy sessions (makes the `-N` count items observable), `copy-mode -k`/`-s`, a daemon-owned refresh re-sync for the `r` keys, the copy-line family, `send-keys -F` plus `copy_cursor_word` for the vi `#`/`*` bindings, null-aware `display-message -a`, terminfo-backed `I/c`; mode-keys plumbing, `-K`, `-v` last |
| client | `F-CLIENT-CHROME-OVERLAYS` | `default-client-command` through the launcher and `focus-follows-mouse`; the pin's pane border chrome in order (tiled z-order on the snapshot with the 94 bump, border lines with junctions, indicators, the border-status row as mux layout geometry with the format published per pane, the renderer residue named); menu mouse policy and paste swallowing; the mode-tree key vocabulary with `x`/`X` prompts and `-y`; popup kitty images then the nested menu-over-popup pointer items |

Launch:

```js
Workflow({
  scriptPath: "<checkout>/compat/orchestration/opus-compat-run-7.js",
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
identity (`ZZ_BOARD_HOLDER=<holder> python3 compat/board.py claim <FRONT> --lease 6h`), check that
`origin/main` is still `1d99c77b` (a moved main only means a longer rebase for the gate), and start
a lease renewer (a loop that renews the three fronts and MAIN with `--lease 6h` every ~100 minutes;
`renew` sets expiry to now plus the lease). Cycle 6's continuation on this box took 2h00m and 1.07M
subagent tokens for four agents with cold builds; a full seven-agent cycle is 3-4h. After the gate
finishes, verify `origin/main`, the board records, and `compat/progress.py`, then write cycle 8 from
a fresh registry census as described under the loop below. `CAMPAIGN-LOG.md` has the per-cycle
history.

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

16 cores, 48 GB, macOS 27. `origin` is SSH and works non-interactively (Secretive plus the brew
ssh; a dry-run push succeeded without a prompt); HTTPS `https://github.com/demfabris/zz.git` is the
hang fallback (gh's credential helper). `gh` is logged in with repo scope. Holder identity
`macbook/orchestrator`. Caches are populated (`compat/.cache/tmux-src/tmux` at `d77c9dc6`,
`compat/.cache/plugins`, eight plugins); the `formats` scenario ran clean here. Worktrees: only
`~/dev/zz-opus-termopts` exists (clean, warm zz-tui/zz build at the cycle-6 client tip); the other
lanes and the reviewers build cold, which is fine on 16 cores. Machine traps the prompts already
carry: `/bin/bash` is 3.2 (no `mapfile`; shard runners use `/opt/homebrew/bin/bash` or python3),
APFS refuses non-UTF-8 file names (so `smoke/client-non-utf8-cwd` is environmental until the daemon
lane's fixture guard lands), and the user has a live tmux server (sessions `clairvo`, `home`, `zz`)
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
