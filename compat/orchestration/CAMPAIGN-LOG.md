# Orchestration log

Per-cycle notes exported from the orchestrator's session memory. `HANDOFF.md` is the current
state; this file is the history and the gotchas each cycle produced.

## 2026-08-31 night: cycle 1, the batching fix works

Two Opus workers in parallel worktrees plus one serial gate agent (about 978k subagent tokens,
2h19m) landed 14 commits: daemon basket merge `a5b924ec` (run-shell background environment
ordering, copy-pipe job environment, pane-set-clipboard hook, both source-hook cwd contracts) and
copy-mode merge `6624f042` (95-action inventory in `crates/zz-mux/src/copy_actions.rs`, vocabulary
66 to 81 mapped, selection lifecycle, scroll-exit, cursor-view, recentre, previous-bracket). Meter
1.6% to 4.6%. Batching quirk: the board refuses two claims in one zone even for the same holder,
so claim one front per zone cluster as the lock and ledger the rest at integration. Follow-ups done
by hand: the v88 protocol bump `616c12e4` (the worker shipped 15 wire-reachable CopyModeAction tail
tags without a bump, citing bogus precedent; the gate flagged it; three pinned fixtures including a
hex hello frame the greps missed) and a tracker refresh `71b3ed3b`.

## 2026-09-01: cycle 2, three lanes, 18 fronts, meter 4.6% to 44.4%

Three Opus workers (mux formats and geometry, daemon display and buffers, park dispositions
records-only) plus a serial gate; about 1.6M subagent tokens. The machine slept mid-gate and the
gate agent died; `Workflow({scriptPath, resumeFromRunId})` replayed the workers from cache and
re-ran only the gate (35 min). Merges: mux `9b4867ab` (config-byte engine adapters, `w` and `O`
modifiers, option-loop context producers, both geometry residues), daemon `9a4129c3` (v89 bump done
correctly by the worker: ClientFileRequest/Response for client-side load and save-buffer IO; menu
shortcut grammar; menu and popup resize lifecycle; popup job environment), park `6904747f` (14 of
15 park groups settled: 12 native, 2 never; `display-panes.command-template` deliberately left
blocked because it is closable work with a wiring recipe), ledger `30d90f3a`. Lessons: put the
protocol-bump recipe in the worker prompt; the old checkout's rerere cache holds a wrong
resolution for `knowledge/tmux/gaps.md`, always regenerate with `write-report`; the gate can shard
the delta corpus eight ways over disjoint scenario sets (3h to 30 min); withdrawn-as-merged
dependencies read as deps-broken, so remint dependants as V(n+1).

## 2026-09-01 evening: cycle 3, Fable reviewers earn their keep, meter 46.1%

Same lanes plus one Fable reviewer per lane and a Fable gate; about 1.9M tokens, 2h06m. Merges:
formats `a3562a34` (L/V loop modifiers, client and environment context producers, `current_file`,
`next_@*`/`prev_@*`), daemon `1d4ab6b8` (config non-UTF-8 file bytes group closed via byte
loaders; menu cell layout with the v90 bump, `MenuItem.annotation` inserted mid-struct while the
changelog said appended), hygiene `039c47b7` (four new divergence gaps registered, check-summary
repaired), ledger `4bac509d`. The review catch that justified the stage: the menu width rule
ignored the title seed (the pin's `menu_create` seeds `menu->width` from the title); the gate
applied the fix with a regression test. The daemon worker honestly skipped four groups: tilde-home
(blocked by an accepted non-UTF-8 limit), key-reset (`-R` is RIS-to-pane, larger than promoted),
GUI pane width (GUI-only residue), control disconnect (needs a per-connection worker; third
bounce, needs a design front). New ops lore: preset `ZZ_COMPAT_CORPUS` for sharded gates; `wait_exit`
can hang under load; use `--no-fail-fast` on gate workspace runs; `smoke/source-replay-diagnostics`
is load-fragile on the pin side.

## 2026-09-01 night: cycle 4 landed, campaign paused for a machine move, meter 54.9%

Merges: formats `747acb39` (expansion output clamp moved daemon-side; the pin's FORMAT_TIME_LIMIT
deliberately refused as accepted-native after it broke option and argument expansion under load;
window cell metrics closed, `window_bigger` and the offsets stay open on the no-client-row
context), copy-mode `ad539f4c` (five stock action keys, emacs `M-1`..`M-9` and `g`, goto-line
rebuilt as the pin's scrollback offset with the strtonum grammar; ten slugs relocated into
accepted-native groups; 14 of 16 binding-fidelity items are blocked by the accepted
`command-prompt -P` decision), daemon `21ef482c` (load and set-buffer `-w` OSC 52, typed Control
config diagnostics, detach-client `-E`/`-P`, attach `-x`, new-session `-X` exit actions; v91 bump:
`ControlConfigError` tag 51 and `Detached.action`), ledger `8dd47505`. Reviewer catches: the daemon
tip had four tests orphaned by its last commit (the worker's proofs predated it; "proofs at tip"
is now a prompt rule), Control victims of detach-exec must stay attached like the pin, a
misattributed pin-derivation doc comment, a wrong count in a group reason. Restart lore: a Claude
Code restart killed the gate mid-corpus; the journal cache kept all six worker and review results
and the gate had already pushed two lanes, so the third lane was finished by hand (its fix commit
already existed; only the sharded delta corpus, push, ledger, and board were left).
`compat/.cache/corpus` does not exist; the plugin corpus is `compat/.cache/plugins`.

## 2026-09-01 late: cycle 5 on the ubuntu box, meter 54.9% to 65.8%

First cycle after the machine move. The handoff recipe held: script paths, 8-core etiquette (workers
`--jobs 4`, gate `--jobs 8`, four shards), SSH origin, holder `ubuntu/orchestrator`; 3h55m wall,
2.04M subagent tokens, seven agents. Merges: terminal `8c1da05` (options.terminal-behavior settled
as 7 native, 8 parked engine knobs, 2 parked client options, `editor` onto the floating stance;
`send-keys -R` closed as a scrollback-preserving reset composed from VT bytes; options.pane-chrome
settled as 4 native scrollbars plus 7 parked border-chrome items), panes `887a372` (server-global
marked pane with its four formats, select-pane input and style controls, special pane target aliases,
exact-match slot classification, split-window `-k`/`-m`, break-pane placement flags onto the native
floating stance; catalog 452/51), daemon `9cab1fa` (Control tildes parsed against the daemon
environment through a batched HomeDirectory round trip, ClientHello cwd as bytes, v92; the pane
focus hooks close reverted at the gate), ledger `bbc09f5`. Every review was approve-with-fixes and
every fix was pin-side: DECSTR is a no-op in the pinned libghostty (the reset now clears each mode
explicitly), the mark must die on a cross-session `move-window`, and the pin gates pane-focus hooks
behind `focus-events` and queues them ahead of the change notifications. Gate lore: a mux-only lane
broke two daemon tests nobody had run (the downstream rule is now in the prompts); `--delta --list`
came back stale once; three new load-only flakes joined the list. Rebased campaign branches stay at
their old tips on origin because forcing is forbidden.

## 2026-09-02: cycle 6 launched, paused after 80 minutes for a machine move

Lanes from a fresh census: mux keys/copy/formats (`send-keys -c`/`-K`/`-H`, copy-mode `-k`/`-s`,
`display-message -a`, per-client window formats), daemon prompt/hooks (the p2 alias-forgery defect
first, then command-prompt fidelity, pane-focus hooks, the re-scoped resize context), client
choosers/overlays (chooser flags, pane-colours, menu and popup behavior). Warm worktrees made the
lanes roughly twice as fast as cycle 5: the keys lane pushed and was reviewed in 75 minutes, the
daemon lane pushed five commits in 80. The run was stopped cleanly at that boundary: the client
lane's uncommitted work became a snapshot commit on `campaign/batch-choosers-overlays-opus-wip`,
the three cached reports were exported into `opus-compat-run-6-continue.js` (machine strings via
workflow `args`), and the lock fronts were released so another holder can claim them. Lesson: the
Workflow journal cache is same-session only, so a machine move means exporting reports into a
continuation script, not copying the session.

## 2026-09-02: cycle 6 finished on the macbook, meter 65.8% to 71.7%

The continuation script ran as written on the new machine: 16-core etiquette (workers `--jobs 8`,
gate `--jobs 16`, eight shards), SSH origin, holder `macbook/orchestrator`, four agents (daemon
review, client worker, client review, gate), 2h00m wall, 1.07M subagent tokens, every build cold
because the warm worktrees stayed on the Ubuntu box. Merges: keys `b1c80f66` (`send-keys -c` with
the target client on the three send-keys effects, `-H` high hex through `KeyToken::Raw` on protocol
93, copy-mode command errors; gate fix: bound-key chains preflight the selected client's read-only
bit, not the invoker's), daemon `0201a5e9` (unforgeable alias-group provenance, status-keys derived
from the editor, `command-prompt -l`, chains, labels and pass order; the clients.event-resize-context
close was reverted at the gate because the pin resolves hook items through `cmd_find_best_client` by
activity time, not the notified client, so the group is re-scoped to a two-client differential; the
status-keys probe now takes one socket per probe after four orphaned daemons per run), client
`0d727e36` (chooser `-F`/`-h`/`-k` and inert choose-buffer `-y` with the row text on the wire,
pane-colours palette, menu action context and queue ordering, menu and popup style refresh; gate
fixes: the menu cursor survives a restyle, `-k` kills before the template runs, the pane-colours
default slot is a recorded residue), ledger `1d99c77b`. Board: three lanes ledgered, seven mooted
fronts withdrawn, four residuals posted (F-KEY-CONTROL-V3, F-DISPLAY-MESSAGE-PANE-TARGETS-V3,
F-PANE-BORDER-LINES-TILED, and the front-less display-menu waiter-wake race). New lore: `/bin/bash`
3.2 has no `mapfile`, so a shard runner under it ran the full corpus eight times at once; APFS
refuses the non-UTF-8 cwd fixture's mkdir on both binaries, so that scenario is environmental on
macOS; the display-menu resize test raced the chosen row's command (it polls now) and two more
load-only flakes joined the list. The campaign branches stay at their old tips on origin.

Cycle 7 was written from the fresh census (35 unresolved groups: 26 open, 9 blocked) as
`opus-compat-run-7.js`: a daemon lane (command prompt `-t` as the routing itself plus vi editing in
the daemon, the pane focus hooks with a PANE_FOCUSED set, the per-command format client that closes
the resize context and `window_bigger` together, display-panes templates and waits, the
display-message client aliases), a copy lane (the copy-mode format family produced from the daemon's
copy sessions so the count items become observable, then `-k`/`-s`, the refresh re-sync for the `r`
keys, the copy-line family, `send-keys -F` for the vi `#`/`*` bindings, the null-aware format
enumeration, terminfo-backed `I/c`), and a client lane (the two client-consumed options, the pin's
pane border chrome in order, menu mouse and paste rules, the mode-tree key vocabulary, popup kitty
images and the nested pointer items). Lock fronts F-DAEMON-PROMPT-FOCUS, F-COPY-MODE-DAEMON-VIEW,
and F-CLIENT-CHROME-OVERLAYS are minted and READY; the script is not launched.

## 2026-09-02: cycle 7 on the macbook, meter 71.7% to 77.6%

Seven agents, 3h53m wall, 2.64M subagent tokens, all three lanes merged. Merges: daemon `327d036f`
(command-prompt `-t` as the routing itself with the issuing command client parked on a waiter until
the prompt closes, `-F`, display-panes templates and waits, the display-message client aliases with
the pin's diagnostic and status, the APFS fixture guard; gate fix: a hook-raised prompt parked the
connection reader and left a phantom client, so the waits are gated on a real client, and a second
display-panes on a busy client answers at once), copy `dee45667` (the copy-mode format family
answered from the daemon's copy sessions, which made the `-N` count items observable and closed
them, `copy-mode -k`, the refresh re-sync and the `r` keys, the copy-line family, `pane_in_mode` and
`pane_mode`; gate fix: a failed `-k` left its kill armed for the next entry), client `03f61a41`
(`default-client-command` through the launcher, `focus-follows-mouse`, menu mouse policy and paste
ordering re-measured and re-scoped, the renderer residue written out; gate: the popup kitty close was
reopened because its proof was a headless renderer test, and the focus-follows-mouse close records
that zz fires `after-select-pane` where the pin fires only `window-pane-changed`), ledger
`e910a732`. Protocol 93 to 94 from two lanes, reconciled at the gate. Board: three lanes ledgered,
`F-DISPLAY-MESSAGE-PANE-TARGETS-V3` withdrawn, keep-notes on the three fronts whose slugs stayed
open. Lore: the SSH security keys became unavailable mid-gate ("device not found"), so `origin` was
switched to HTTPS through gh's credential helper and the gate finished over it;
`client_focus_closes_display_panes_and_preserves_chooser_modes` also fails about one run in three
exact-solo (an async status/snapshot refresh race), so it is a known flake even solo; the `rm -rf
$HOME` shape in a worker's probe trips Claude Code's critical-path guard and now meets a local hook
that denies it with the rewrite. Skips worth reading: the pane focus hooks were skipped a third time
for lack of budget (the recipe is complete), the per-command format client was skipped twice for
sharing `status.rs` with another lane, the I modifier's blocker sharpened to the pin's own
`tty_term_codes` and `tty_features` tables, the vi `#`/`*` bindings now wait on the search action
family rather than the format, the border chrome has an exact recipe for the status row, and the
three popup pointer items wait on a menu over a popup.

## 2026-09-02: cycle 8 overnight on the macbook, meter 77.6% to 84.2%

Seven agents, 4h14m wall, 2.78M subagent tokens, two of three lanes merged. Merges: daemon
`7f212120` (the pane focus hooks at the pin's own call sites with a PANE_FOCUSED set and the
transitions spliced by anchor, the item three lanes had skipped; status-keys with the prompt's vi
table, the three key spellings, the message-covers-prompt shape; gate fix: a false `-d 0`
measurement rewritten to the proved pair), formats `37c91bf2` (the per-command format client by
activity time with null list rows, closing the resize context and all three `window_*` runtime
formats; the null-aware `display-message -a` listing against the measured name sets; `pane_pipe`,
`pane_pipe_pid` and `pane_unseen_changes` with the unseen-changes premise corrected to the pin's
mode-gated flag; the bare `=` mouse target; four settlements: tilde home into the byte-streams
stance, per-client active pane, remote alias preflight, shutdown unlink order as never; reviewed
approve with zero defects), ledger `6e0bdd48`. The terminal lane (all eight engine knobs, `copy-mode
-s`, `clear-history -H`, `resize-pane -T`, `remain-on-exit-format`; reviewed approve-with-fixes
and all five fixes applied) was SKIPPED at the gate: drawing the pin's default dead-pane notice
scrolls the child's last line into history, and zz's retained-pane capture has no scrollback, so two
pre-existing corpus scenarios lost a line. The gate's rebased tip with the fixes was recovered from
unreachable commits and pushed as `campaign/batch-terminal-knobs-opus-gated` (`c328ebd3`); the
front was withdrawn and reminted as `F-TERMINAL-KNOBS-RELAND` with the capture fix first. Skips
worth reading: the I modifier now decomposes into `tty_term_codes`, `tty_features`, the
terminal-features fnmatch pass and `infocmp -x` (zz's `client_termfeatures` is the renderer roster,
not the pin's detection), the `-v` trace needs modifier arguments expanded at parse time, the
environment bytes need a byte-clean session environment end to end, `send-keys -K` is mapped onto a
SendClientKeys effect, the mode-keys branches are blocked behind cursor geometry (zz's cursor-right
never wraps), and `pane_pb_progress` is the ConEmu OSC 9;4 progress bar, not paste progress. New
lore: `attached_client_extents_clamp` fails two to four runs in six even solo on this box on
pristine main (the 128 MiB revision limit under the huge-client resize), so it is a known race.

## 2026-09-02: cycle 9 on the macbook, meter 84.2% to 90.5%, then PAUSED

Seven agents, 3h52m wall, 2.58M subagent tokens, all three lanes merged. Merges: daemon
`5e6de65a` (`send-keys -K` handed to the target client's key handler; the I modifier answered from
the client's own terminfo through a ported `infocmp -x` reader with `tty_term_codes`,
`tty_features`, `terminal-overrides` via `tty_term_apply` and the ignorefkeys cancels; the
environment-bytes loss and the `set-hook -B` monitor subsystem written down as records; gate fix:
`attached_client_extents_clamp` was never a load flake but a fixture race, the output-view pane
exited and the watcher killed its window on the first resize, so the test now retains that pane),
terminal `4cb913dc` (the cycle-8 re-land with the retained-pane history readable on a dead pane,
which cleared both regressed corpus scenarios; the eight engine knobs, `copy-mode -s`,
`clear-history -H`, `resize-pane -T`, `remain-on-exit-format`; the ConEmu OSC 9;4 progress bar for
`pane_pb_progress`; the copy cursor per-line limit and the mode-keys branches that read it, with
the blocker for the rest recorded; gate fix from the reviewer's probe: an OSC now commits on CAN and
SUB and drops the control bytes inside it), client `6f307120` (the pin's pane border chrome, closing
`options.pane-border-chrome`; a popup's Kitty images handed to the drawing client; the raw TUI's
mouse routed through the bottom-status content box and `join-pane -b` z-order, both reviewer
must-fixes; the chooser vocabulary and popup pointer needs written down; protocol 94 to 95 with six
appended fields; the popup kitty close was REOPENED at review because placements arrive only after a
client resize, and the reason now carries the per-view viewport recipe; gate fix: a cross-lane test
collision, the terminal lane promoted `remain-on-exit-format` into the option-consumer roster while
the client lane's test used it as the unconsumed example), ledger `3726adf8` (199 scenarios / 2,527
steps / PASS, settlement 95.3%). Newly closed groups: `terminal.key-control`,
`formats.modifier-fidelity`, `options.pane-engine-knobs`, `options.remain-on-exit-format`,
`formats.pane-process`, `history.hyperlink-reset`, `copy-mode.command-fidelity`,
`options.pane-border-chrome`; `terminal.resize-pane-trim` settled never,
`options.terminal-engine-limits` added accepted-native. Board: three lanes ledgered,
`F-KEY-CONTROL-V3`, `F-PANE-BORDER-LINES-TILED` and `F-PANE-BORDER-ZORDER` withdrawn as mooted, two
residuals on no front (the retained pane's `history()` after `ActorStopped`, the popup Kitty
placements). Lore: `event_hooks_fire_after_mutation_with_captured_formats` is a new timing flake
(automatic-rename race); the registry needed a three-way merge by group id at the client rebase
(kept in the gate's scratchpad, worth landing under `compat/` if the campaign resumes);
`compat/run.sh` hard-codes `RESULTS_DIR`, so shards only stay apart by disjoint scenario names.
Fabrico asked for a pause after this cycle: nothing minted, claimed, or launched; the census and the
shape of a possible cycle 10 are in `HANDOFF.md`.

## 2026-09-02: cycle 10 launched on the ubuntu box, the goal switched on

Fabrico resumed on the ubuntu box (8 cores, 30 GB, bash 5.3, btrfs, SSH origin) with one
instruction: go all the way. Cycle 10 was written from the pause census as `opus-compat-run-10.js`:
a queue lane (the per-connection worker for Control clients that three cycles bounced, then the
EOF drain and hard-loss cancel, `split-window -W`, the display-panes template routing residue and
the environment bytes behind it), a copy lane (the copy-mode search family into the engine and the
mode-keys tail, `send-keys -F` with the vi cursor-word bindings, the `-P` stance, the `set-hook -B`
monitors, the `-v` trace), and a client lane (the popup Kitty per-view viewport, the chooser key
vocabulary with `-y`, the popup pointer trio, the GUI pane width). Product decision taken this
cycle by the orchestrator under that instruction, recorded in the registry as reversible:
`command-prompt -P` moves from loudly unsupported to an accepted flag whose prompt stays on the
client surface, so the fourteen stock copy-mode bindings can carry the pin's exact strings. The
script now carries the box traps through `args` defaults (`boxNote`, `gitNote`) instead of
hard-coded macbook text, and the gate uses one shared `zz-gate-target` build directory. `gh` had no
usable token on this box at launch, so board claims and ledger entries are replayed from
`board-replay-10.sh` once it does; the workers never touch the board.

## 2026-09-02/03: cycle 10 on the ubuntu box, meter 90.5% to 96.1%, the client lane in a follow-up gate

Seven agents in the main run, 5h54m wall, 2.43M subagent tokens; the reviewer of the client lane
ended its turn waiting on a background monitor instead of reporting, which the pipeline counts as a
failed stage, so the main gate integrated two lanes and the third went through
`opus-compat-run-10b.js` (its worker report embedded, a review-only stage first so it ran beside
the queue worker, then the gate resumed from the same run). Merges: queue `fd19cce` (Control
clients get the per-connection command worker that three cycles bounced; hard loss frees the
not-yet-started queue while the in-flight item finishes, graceful EOF runs every queued line
through the first yielding one and drops the rest, replacing the client-side truncation;
`split-window -W` parks the invoker on the pane it made and hands back the child's exit or signal
status; the display-panes template residue stays recorded; protocol 95 to 96 appends
`ProtocolMessage::CommandQueueParked`; gate fixes from the review: an instant-exit `-W` answered 0
one run in ten because the pane-removal arm raced the waiter's removal, and a non-zero `-W` status
skipped the after-hook), copy `cd03bb8` (copy-mode text search moved from a client surface into
the engine as `CopyModeAction::Search`, with the search formats; the mode-keys tail; `send-keys -F`
as the pin's search-string expansion, not a general key expansion; the sixteen stock binding
strings under the `-P` decision, with `prompt.pane-rendered` closed rather than kept accepted
because the manifest refuses a flag item on a promoted flag; the environment tail in format
expansion, a divergence with no slug; gate fixes: the reviewer measured three more mode-keys reads
the eleven-place enumeration missed, so `copy-mode.action-fidelity` is open again with the fix
shape instead of a revert that would have taken the search engine with it, and the new prompt
bindings dropped the armed count prefix, now kept for the answer the way `window-copy.c` does),
ledger `2af51ff` (207 scenarios / 2,586 steps / PASS). Skips worth reading: the environment bytes
are priced at four channels and 99 read sites behind 65 signatures in the mux alone, the monitors
and the `-v` trace ran out of budget, the chooser vocabulary found that the pin's kill and `:`
prompts belong to the mode overlay (an overlay-owned nested overlay, the shape the popup context
menu took this cycle, is the prerequisite), and the GUI pane width needs a decision about which
extent the window formats report for a client that draws chrome (the gpui test harness already
exists). Box lore: three compat rows are red on Linux before any lane (`smoke/remain-on-exit-format`
wants `term` where Linux answers signal 15, `smoke/format-modifier-interrogate` sees `smxx` in the
harness's outer TERM, `smoke/pane-engine-knobs-input` flakes pin-side under load) and pass
`--check-summary` only because `summary.md` was recorded on the macbook; `zz-gate-target` (47G)
holds a reflinked ghostty source for `GHOSTTY_SOURCE_DIR`; the queue lane ran 4h15m because its
foundation group was given an open budget, so cycle 11 caps every group. Process lore: subagents
must run everything in the foreground and finish with the structured report (the `FOREGROUND`
rule in 10b), and `board.py renew` on a front the holder does not hold posts a harmless RENEW.

Pause, 2026-09-03 ~00:30Z: fabrico needed the box for a machine move while the client gate was
twenty minutes in (rebased onto `2af51ff`, no fixes applied, no tests run), so the gate was
stopped. Its rebased tip is `campaign/batch-client-choosers-popups-opus-gated` (`affcfc9`), the
review (approve-with-fixes, six must-fixes: an extra menu separator with no paste buffer, a
popup-owned menu outliving its popup, drag re-entry arming a move the pin refuses, Fill Space
resizing the job, Centre and Fill Space rewriting the preferred placement, one stale schema row)
is embedded in `opus-compat-run-10b.js`, and `args: {stage: "gate"}` runs only the gate from that
tip on any machine. The client lock was released with that note; MAIN was used for the records
push and released. `HANDOFF.md` has the census and the first task.

Cycle 10b, 2026-09-03 on the ubuntu box (74 minutes, 0.26M subagent tokens, one Opus 5 gate at
xhigh): the paused client lane landed. `origin/main` `383fdcb`, meter 96.1% to 97.4% (296/304),
`display-popup.behavior-fidelity` closed, 207 scenarios / 2,586 steps / PASS. The gate rebased
`affcfc9` over the 22 non-campaign commits main took during the pause (the iOS and iPad client, the
C ABI paste/reply/output-cancel verbs, `import-tmux-config`) and hit exactly one conflict, twice,
both in the generated `knowledge/tmux/gaps.md`; a `git merge-tree --write-tree` probe run before
launch had predicted exactly that, and it is worth running before any gate whose branch has sat.
All six review must-fixes landed as `9ddeae0` before the gate, each proved by reverting it. Two
things to carry: a review's probes must live in the repo or on a branch, because this one's lived
in a session scratchpad under `/tmp` and the machine move erased them, costing the gate its first
hour; and the lane worker's reported item count was wrong (it claimed 457 to 453 where the registry
says 438 to 435), so a ledger recomputes from the registry, never from a worker report.

Cycle 11 launch, 2026-09-03: fabrico's instruction is now TWO lanes per cycle and Opus 5 at xhigh
for every agent, workers, reviewers and gate alike; the Fable reviewers and gates of cycles 6 to 10
are history. Fronts `F-FORMATS-MONITORS-TRACE` (the `set-hook -B` monitors plus the `-v` trace) and
`F-CHOOSERS-VOCAB-COPY-TAIL` (the copy-mode mode-keys tail plus the chooser vocabulary on an
overlay-owned prompt), zones pairwise disjoint, script `opus-compat-run-11.js`. Every group carries
a HARD budget in minutes, the `FOREGROUND` rule is in all three prompt kinds, and exactly one lane
(choosers) is told it owns the 96 to 97 bump while the other is told it owns none.
`clients.path-encoding` is deliberately not scheduled: its reason prices it across four channels
and half-landing has been refused twice, so it gets a lane to itself in cycle 12.
`rendering.geometry-residue` is the orchestrator's decision to take, not a lane's.

Cycle 11, 2026-09-03/04 on the ubuntu box (two Opus 5 lanes at xhigh, workers about 1h50m, the
whole cycle spread across a network outage): meter 97.4% to 99.0% (301/304), 62/65 groups, three
items left. Formats lane `89f36ac` closed `formats.context-producer-fidelity` (the `set-hook -B`
monitor subsystem: the name:what:format grammar, the one-second tick, baseline-then-fire, and the
nine `notify_monitor_cb` names produced on the existing `hook_format_variables` path rather than as
table entries) and `display-message.verbose-trace` (the pin's expansion trace, which required moving
modifier-argument expansion into parsing, an observable behaviour change the pin's own
`#{=#{@three}:pane_title}` literal answer pins). Choosers lane `3eda6ed` closed
`copy-mode.action-fidelity` (the per-action search-mark clear class, the incremental-origin re-latch,
the emacs selection trim, `previous_word`'s `stop_at_eol`) and `flag:choose-tree:-y`, bumped the
protocol to 97, and landed `t`, `T`, `C-t`, `x` and `X` on the overlay-owned prompt that cycle 10
identified as the prerequisite. Both lanes finished inside their hard budgets, against cycle 10's
open-budget lane that ran 4h15m alone, so the budgets stay.

The reviews were worth more than the tests this cycle. The choosers reviewer found that the new `x`
binding made the GPUI desktop chooser perform a SILENT DESTRUCTIVE KILL: the desktop client forwards
every key to the daemon with no local filtering, so the daemon raised its confirm prompt and the
client never drew it. No test could have caught it, because the corpus proves the raw TUI. The
formats reviewer found `expand_client_loop` building its sub-expander with `trace: None` while the
four sibling loop expanders share the sink, which the closed record and `divergences.md` both
asserted was not the case; it needed a unit test rather than a scenario, because every corpus server
is detached and the client roster is empty.

The gate agent died on a network timeout at 20:17Z while sharding the choosers corpus, after
merging formats, rebasing choosers, applying both must-fixes and clearing the workspace suite and
clippy. Nothing was lost and nothing was rebuilt: its worktree survived, the rebased tip went to
`campaign/batch-choosers-copy-tail-opus-gated` as insurance before anything else, both must-fixes
were verified present at the tip rather than trusted from a report that never existed, clippy was
re-run because two fix commits landed after the gate's own clippy, and the remaining stages ran by
hand. The recovery order is written into `HANDOFF.md` under "If the gate dies mid-cycle". One thing
that has now bitten twice: a lane touching `crates/zz-ui` must also fix `examples/ui-showcase`,
which is workspace-excluded, so `cargo test --workspace` cannot see it; the choosers lane got it
right and it was confirmed with a `cargo check` in that crate.

Cycle 12, 2026-09-04 on the ubuntu box (two Opus 5 lanes at xhigh, about 4h end to end): meter
99.0% to 99.3% (302/304), protocol 97 to 98. Bytes lane `595616b` closed `clients.path-encoding`
in full, the item priced across four channels and refused twice for half-landing: the ClientHello
entry, `CommandInvocation.args` as `RawText`, the byte-clean environment store, and
`CommandResponse::Success` output as bytes. Tail lane `12b4776` closed the chooser vocabulary and
MEASURED `rendering.geometry-residue` instead of closing it: the two pane numbers diverge by
exactly one cell, both halves committed as tests. That measurement refuted the orchestrator's own
narrowing from the day before, which had argued the format should follow the PTY; `pane_width` is
one of a tiled family (`pane_left`, `pane_right`, `pane_at_*`, `window_layout`), so the coherent
direction is the PTY following the settled layout, the pin's own order. Reviews: six defects on
bytes, two on tail, all applied at the gate. The gate then did something worth keeping: it moved
`semantic:config-tilde-home-non-utf8` OUT of an accepted group into open work because v98 had
falsified its acceptance clause (zz no longer refuses a byte-only path, it opens a mangled one),
which cost 0.4 meter points it could have kept, and it opened `clients.byte-clean-consumers` to
carry two divergences the bytes reviewer found living only as prose inside a closed record. Same
pattern the reviewer caught on `protocol.binary-streams`, whose premise the branch also falsified.

Cycle 13, 2026-09-04 on the ubuntu box (two Opus 5 lanes at xhigh, about 4h30m): meter 99.3% to
100.0% (304/304), 65/65 groups, ZERO open, 174 closed records, 220 scenarios / 2,648 steps. The
registry is empty; the campaign's implementation phase is done. Geometry lane `fd2e790` closed the
last frozen-scope item the way cycle 12's probe said to (an attached pane's PTY now follows the cell
the layout settled on, with the one-cell guard kept for split windows because `implied_window_extent`
amplifies drift). Consumers lane `37e8df0` closed all three byte consumers: format expansion, the
client-encoding sanitizer zz never had (gated on the pin's `CLIENT_UTF8` inputs, and only on the
output the pin routes through `server_client_print`), and byte-only path arguments. Reviews: two
and one defects, applied at the gate. The first geometry reviewer was interrupted mid-run and the
pipeline replaced it cleanly from the same branch tip. The gate's own tracker headline says the
part that matters: the persisted attached-client row still reads PASS but the fixture does not
complete on this box, so the PRACTICAL exit gate is not met even though the meter is.

Retrospective, 2026-09-04, while the cycle-13 gate ran: eight Fable reviewers at xhigh, one per
lens (oracle coverage, the 42 accepted groups, the ecosystem beyond the eight-plugin corpus, harness
blind spots, proof quality over 57 sampled closes, the entrypoint, control-mode tools, prose with no
slug), then one critic to dedupe and rank by who breaks. 79 raw findings became 15 ranked ones and
a plan; `CAMPAIGN-REVIEW.md` beside this file is the report. The short version: the list was drawn
after argv parsing and below the screen, and that is where a switcher breaks. The desktop app
discards the tmux status line (`client.rs:3841`, `CoreEvent::StatusChanged` in a no-op arm) so every
theme plugin does nothing in the app; `~/.tmux.conf` is never read at boot and the harness injected
every config with `-C source-file` so discovery was never measured; pane processes have no `tmux` on
PATH so the plugin corpus was proven under a wrapper the product does not ship; the default prefix
table kills without `confirm-before`. Two accepted reasons name a reopen condition the corpus itself
meets (resurrect reads `#{history_size}`, oh-my-tmux runs `save-buffer -`). The biggest gaps are not
lane-shaped: an instrument pass first (fix the attached fixture and make `--check-summary` refuse a
stale footer, a launcher mode that runs the installed layout, a row-level TUI differential), then
three two-lane cycles. What held up is named in the report so the next campaign keeps it.

Instrument pass, 2026-09-04 on the ubuntu box, stopped mid-way for a machine move (fabrico's call;
the state below is exact). Three instruments from the retrospective, two landed, one measured:

- `compat/run.sh --check-summary` now REFUSES a `PASS` footer that proves nothing about the tree
  it is checked on. A full run stamps `Recorded at: <commit>` under the status (`-dirty` when the
  fixture or `crates/` had uncommitted changes), and the check dies unless the stamp is a commit
  this checkout has, an ancestor of HEAD, not dirty, and nothing under `compat/attached-client.sh`
  or `crates/` changed since. CONSEQUENCE: the stored footer has no stamp, so `--check-summary` is
  RED on main from this commit until someone records a full run with the fixture passing
  (`compat/run.sh --attached-client`), and every gate that merges code must re-record it. That is
  the point: the review found the fixture red on this box under a stored PASS. `ZZ_COMPAT_ZZ=path`
  lets `run.sh` use a prebuilt zz instead of building.
- `compat/diff-scenario.sh` gained a `launcher:` scenario header: the zz side runs the `zz_cli`
  launcher from PATH with no `--socket`, no `ZZ_SOCKET`, the default socket under a scratch
  `XDG_RUNTIME_DIR`, and no harness `tmux` wrapper in front of the panes. Proved by
  `smoke/launcher-installed-layout` (7 steps clean) and by a probe reading `#{socket_path}` on
  the zz side as `<scratch>/runtime/zz/default.sock`. Retrospective findings 2 and 3 are provable
  now; they were not before.
- `compat/status-row.sh` is the row-level TUI differential: both binaries attached inside an
  outer pinned tmux at 79x24 (below the sidebar's 80-column auto-hide), the same status options
  applied to both, the last row's bytes diffed with escapes after each step. First measurement,
  9 of 9 rows differ, and the differences are the findings: the zz TUI emits truecolor SGR
  (`38;2;r;g;b`) where the pin emits the named colour (`37`, `44`) for `bg=blue,fg=white`; the
  default status colours differ (pin `154,205,50` on `13,13,13`, zz `0,205,0` on `0,0,0`); the
  default `status-right` shows the pane title, which is the hostname on the pin and the shell name
  on zz; `#{window_height}` answers 23 on the pin and 22 on zz at the same client size; and zz's
  default row carries a ` Ctrl-\ detach` hint the pin never draws. Recorded, not fixed.

The attached-client fixture is still red here and the reasons are now measured rather than
guessed. Eight runs at `0092abf`+, each ending on a different probe: (1) `probe_command_output_navigation`
waited for a bare `/` prompt on zz where the TUI now draws the pin's `(search down)`; FIXED, the
one fixture change kept, both sides match the same string. (2) The command-output view is a
CLIENT-LOCAL view: `#{pane_in_mode}` stays 0 and `#{client_key_table}` answers `copy-mode-vi`
while it is open, and a probe shows Escape under vi keeps it open, which is what `bda6e90` says;
yet one run saw the table drop to `root` after Escape. Unresolved; do not replace the
`client_key_table` observable in that probe with `pane_in_mode`, it measures nothing there.
(3) `probe_command_prompt` on the TMUX side answered `mainprompted`: the four BSpace keys raced
the prompt opening, a probe with a settle between `C-b ,` and the keys passes. (4) `probe_side`
once never saw `ATTACHED_ROOT_OK`: the first keys reached the zz client before it was ready.
(3) and (4) are races the fixture must wait out, not behaviour. Every run leaves zz daemons on
`/tmp/zza-*.sock` when it dies; the cleanup trap does not reach them. Reap by pid from the socket
name, never by `pkill -f`.


## 2026-09-05: cycle 14 on the ubuntu box, the first Codex cycle (13:07 to 19:52)

Fabrico's instruction: run cycle 14 on Codex gpt-6-astra instead of Opus 5, first at high
reasoning on the fast tier, switched at 14:45 to medium reasoning on the default tier.
`codex-compat-run-14.py` replaced the Workflow script: the same prompts and rules, driven through
`codex exec` with `--output-schema` enforcing each JSON report. Timings: keys worker 24 min,
buffers worker 58 min, reviewers 10 to 20 min each, the instrument worker 2h05m (it ran a full
sweep of its own), the gate 3h04m including the 90-minute stamped sweep.

Both code lanes were REJECTED on first review and fixed by resuming the worker's Codex session
with the blockers, then resuming the reviewer at the new tip. Keys: 17 of the 20 adopted prefix
bindings had no attached proof and none carried the pin's `-N` note (fixed; `f`, `M-n` and `M-p`
were reopened with measured reasons, and the reviewer then caught a tautological `prefix i` proof
whose patch the gate applied). Buffers: the rewritten 24-name terminal-runtime clause called six
zero placeholders "pin defaults" (fixed as declared known differences with the pin's values).
Both rejections also read the `client_focus_closes_display_panes_and_preserves_chooser_modes` flake
as a red tip; the keys lane's `93f90bea` repaired the racy assertion and the gate carried it under
buffers by gating keys first.

A third, compat-only lane was added after the pre-cycle full run failed here: the fixture died on
the vi command-output view (`copy-mode-vi` dropping to `root` during settle) and the three
environment rows fail a full run outright, so no stamp was possible on this box. The instrument
lane made the probes wait for observable readiness, fixed the rows, and registered two zz defects
it uncovered (stock vi search discards command output when the prompt opens; the dead-pane notice
spells the signal `term` where the Linux pin prints 15). It was gated first so the stamped sweep
ran on the repaired fixture: 231 scenarios, 2,742 steps, PASS at `c282741787c9`, the first stamp
this box has produced.

The stamp rule was relaxed at fabrico's instruction while the sweep ran ("we don't need to RERUN
just because I committed some UI changes"): `75d9ca24` keeps the stamp, the ancestor and dirty
checks and the fixture-changed refusal, and turns the crates/-changed refusal into a drift
warning. The gate re-records after every code merge; ordinary pushes to main never force a sweep.

Tooling lessons: `codex exec` needs stdin closed or the prompt on `-`, or it hangs on "Reading
additional input from stdin"; `codex exec resume <id>` takes no `-C`, `-s` or `--color` (run it
from the worktree, pass `-c 'sandbox_mode="danger-full-access"'`); the Claude Code harness kills
background shell tasks when free memory dips during concurrent links, while Monitor tasks survive;
a `pkill -f` whose pattern appears in the calling shell's command line kills that shell; the
orchestrator holds lane results in memory, so a mid-cycle re-review means killing the Python
process (its `codex exec` child keeps running and still writes its report) and rerunning the stage
from the JSON files.

### 2026-09-05 cycle-14 integration

One Codex gpt-6-astra gate ran alone on Ubuntu at high reasoning. Instrument landed first at
`f6348f19`, keys second at `533d253c`, buffers third at `c2827417`. The instrument-only lane
skipped workspace tests and clippy. Both code lanes ran the full workspace gate and clippy;
keys had no failures, buffers had one loaded history-request failure that passed exact-solo.
Delta gates covered 184, 216, and 169 scenarios. Instrument chooser-tree/client-format and buffers
own-conf failures passed solo; source-replay diagnostics passed solo in each gate.

Every reviewer fix landed before its gate. Instrument's stock-array correction records smxx and
strikethrough at 1/1 on both binaries without the fixture override. Keys' new prefix-i proof passes
stock and rejects a no-op binding on both binaries. Buffers inherits original `93f90bea` as
`ce7be406`; the corrected focus assertion passes solo and in the full daemon suite. The combined
registry needed sorting, and the generated gap report was regenerated. Protocol stays v98.

The full corpus plus attached-client run passed on clean `c282741787c9`: 231 scenarios, 2,742
steps, four registered known rows. The harness wrote the PASS and commit stamp itself and printed
`summary current`. Summary SHA-256:
`eb015c8382850aac3a8d2355fab28296667088c7c67592e8cd6e2c36639a8c2b`.
The owned attached daemon exited and the fixture removed its scratch directory. No fixture retry
or hand-written PASS was needed.

Frozen delivery remains 304/304 items and 65/65 groups. Live state is 4 open, 0 blocked, 42
accepted groups and 180 closed records: 222/226 settled (98.2%). Six post-freeze items remain
open: mixed CLI queues, stock command-output search, Linux signal spelling, and prefix f/M-n/M-p.
Interactive refresh and the 24-name terminal-runtime placeholder contract remain accepted, not
open. All lane locks were ledgered and released; the final records commit owns MAIN settlement,
and TRIAGE records the residuals without changing the F-SPLIT-MUX-*-V5 chain.

Owner commits `e9d23ec9`, `5dad18fe`, `b75bc348`, `4cee170c`, and `75d9ca24` landed during the full run. The records rebased conflict-free onto `75d9ca24`; the attached fixture is unchanged. The owner's `75d9ca24` makes post-stamp crate changes a warning, so the full run remains recorded at `c282741787c9`, with three owner crate commits afterward.
The first records push was refused non-fast-forward. A premature local-SHA MAIN ledger entry
was corrected by an append-only note; the pushed rebased SHA is the final records entry.
The bounded rebase check passed zz package tests and a fresh build, then six strict
buffers/terminal scenarios. `--check-summary` prints summary current with three post-stamp
crate commits warned under the owner's revised rule.

## 2026-09-05/06: cycle 15 on the ubuntu box, the second Codex cycle (22:40 to 04:34)

Launched from the layout commit with two fresh worktrees and cold targets; both workers compiled
first and were productive inside 35 minutes. Lanes at medium reasoning on the default tier, the
orchestrator `codex-compat-run-15.py`, run dir `~/dev/zz-run-15`.

- 22:40 launch. 23:36 config worker done in 55 min with all three groups closed. 23:48 its review
  REJECTED in 12 minutes (the reviewer shared the worker's target, so it skipped the cold build):
  reload-config rebuilt the key tables from mux.conf alone and erased every binding discovered from
  the tmux files, and Settings runs reload-config after each save. Resumed worker session 23:49 to
  00:08 fixed it (startup and reload share one root selection, F11 regression test). Re-review
  00:08 to 00:28: approve-with-fixes (two registry records contradicted each other about the pane
  wrapper).
- 23:58 status worker done in 78 min: four groups closed with recorded residues, the stock
  command-output prompt lifecycle left open as a daemon lifecycle change outside its zones. 00:20
  review REJECTED: `DATE[#(date +%s%N)]` never rendered on zz (completion polled once a second and
  cached under a stale key) and a pre-existing scenario, control-eof-drain, regressed with an extra
  `%layout-change` while an EOF'd control client drained. Resumed session 00:20 to 01:11 fixed both
  and measured the premise wrong: the pin strftime-expands `%s` inside `#()` before the shell runs,
  so that job's key changes every second. Re-review 01:11 to 01:35: approve-with-fixes (unnamed
  Linux signals still spelled by description, stale registry references).
- 01:36 to 04:34 gate, alone: config `fc4f5ded` at 02:20 (fast-forward), status `d59236fc` at
  03:03 (rebased), stamped full run without retries, ledger `43ccc5e4` at 04:32. The gate's own
  follow-ups were the two must-fixes.

Four rejects or must-fixes, all real; two were regressions no lane test covered (reload, EOF drain)
and one a wrong close (signal spelling). Each fix loop cost about an hour and fifty minutes of
worker plus re-review time. Total wall clock 5h54, of which the gate was three hours.

Tooling lessons, in addition to cycle 14's:

- Board fronts must carry disjoint zones; an overlap makes the second claim fail with "zones busy".
  The status front was withdrawn and re-fronted as `-V2` with the daemon.rs, lib.rs and command.rs
  hunks declared as excursions in its notes.
- Reviewers get a worktree without a build and `CARGO_TARGET_DIR` pointing at the worker's target
  (the worker is finished by then). Reviews took 12 to 24 minutes instead of a cold build each, and
  review worktrees are free to delete.
- The orchestrator's renew loop shortens the 10h launch leases to 4h and renews MAIN unheld (a void
  comment). When stages are driven by hand after killing the orchestrator, run a renew loop per lock
  front.
- The restart pattern from cycle 14 held: kill the python orchestrator, resume the worker's Codex
  session with a fix prompt via `codex exec resume … -o worker-<lane>.json`, merge the fix report
  with the first pass so the gate sees the whole lane, `--stage work --lane <lane>` for the
  re-review with the old review JSON moved aside, `--stage gate` once both lanes hold an approve.
- A worker's early probe ran a bare `tmux set-environment -g` against the default socket; no server
  was running there, and only its own report disclosed it. Prompts should forbid bare `tmux` without
  `-L` outright.
- Deleting the eight closed worktrees and the gate target's incremental cache freed 234G before
  launch; the cold rebuild cost about 30 minutes per lane.

### 2026-09-05 cycle-15 integration

Codex gpt-6-astra lanes ran at medium reasoning on the default tier on Ubuntu. One integration
gate ran alone: config landed at `fc4f5ded`, then status at `d59236fc`, each after its own full
workspace, clippy, delta and records gate. The gate kept protocol 98 and regenerated gaps.md at
each rebase conflict; the disjoint source hunks merged without conflicts. Original campaign
branches remain at their reviewed tips on origin; no force push occurred.

Both reviews were approve-with-fixes. Config's follow-up quotes the old no-wrapper product clause
and supersedes it in both durable records. The reviewer's instrumented launcher probe confirms
the daemon wrapper is first on pane PATH and shared with jobs. Status's follow-up maps Linux
PWR/power failure to 30 and STKFLT/stack fault to 16, adds unit and drawn-notice fixture coverage,
and corrects stale direct-mixed-queue and signal closure references. The original reviewer probe
now matches the pin for TERM, PWR, STKFLT, their drawn notices, and mixed stdout bytes.

Config's first workspace command inherited host configuration and failed four assertions; the
complete scratch-HOME rerun passed. All subsequent gate commands that could start servers used
scratch configuration. Config's 221-scenario delta had chooser-tree-vocabulary and client-loop
load failures; status's 236-scenario delta had popup-menu-policy, copy-mode-prompt-bindings, and
background-status-job load failures. Each passed unchanged alone. Source replay passed alone in
both gates, and the full status attached-client fixture passed before its push.

The final full run passed without retries at clean `d59236fc92d2`: 238 scenarios, 2,795 steps,
four registered known rows, and attached-client PASS. The harness wrote the stamp and printed
summary current. Summary SHA-256: `f0ac2b7bda5c2bb3c2835aee89e7666ac367429be9cfd0c69c112b3a048f2b56`.

Frozen delivery remains 304/304 items and 65/65 groups. The live registry holds 46 active groups
and 440 items: 4 open, 0 blocked, 42 accepted, plus 185 closed records; 227/231 settled (98.3%).
Thirteen post-freeze items remain: sourced stdout ownership, stock command-output prompt lifecycle,
ten Control notification residues, and status loop-tag job identity. These need daemon/source
replay or mux hook ownership beyond the lanes' zones. macOS signal runtime and a one-hour real
iTerm2 session remain maintainer validations. Both lane locks were integrated and released;
MAIN owns the records settlement, and TRIAGE preserves the F-SPLIT-MUX-*-V5 chain.

### 2026-09-07 cycle-17 integration

Claude Code Opus 5 lanes ran at xhigh on Ubuntu and were frozen mid-flight before the gate; one
integration gate resumed them alone on the macbook. Wire control claims landed at `eee64c47`, pane
runtime facts at `50e785de`, the TUI status and menu work at `8ad2e633`, then the census close at
`6fa9ef58`, each after its own gate. The wire lane bumped the protocol 98 to 99 with three pure
appends - `stdout_claim` on `CommandResponse::Success`, `producer` on `EventPayload::Clipboard`, and
the `EnvironmentRequest`/`EnvironmentResponse` pair - and no other lane touched the wire, so the
three later rebases landed on a changed tree without a single compile break. `knowledge/tmux/gaps.md`
conflicted on every rebase and was regenerated with `tmux-tracker.py write-report` each time;
`compat/tmux-gaps.json` conflicted only on the proof lane and merged by record id with
`gaps-merge.py`. Original campaign branches remain at their reviewed tips on origin, and no force
push occurred.

All four reviews were approve-with-fixes and every must-fix landed with the reviewer's probe re-run.
The wire lane's own protocol page still carried the v92 sentence its change refutes, that `$NAME`
stays literal in direct Control input. The client lane had a duplicate `#[test]` attribute that
would have made CI's workspace clippy a hard error on main and ran the test twice. The proof lane
had buried two measured divergences in closed-record prose with no slug and no owner, where neither
the tracker nor the meter can ever surface them again; both are now open groups,
`tui.client-input-backpressure` and `buffers.target-error-message`.

The panes lane needed the most gate work. Its differential guarded `pane_tty` with `/dev/pts/`,
which no macOS pty satisfies, so the whole fixture aborted on the gating machine and proved three of
its four items nowhere; relaxing that guard to `/dev/` turned the scenario green end to end. The
reviewer had also found an undisclosed regression, a dead pane answering its start directory where
the pin answers empty, and proposed guarding `synchronize_pane_runtime`. That does not reach it: the
runtime facts are a stored snapshot and nothing recomputes them once the pane is dead, so the guard
still measured `[<DIR>]` against the pin's `[]`. The fix went where the pin's own read is, at
expansion in `crates/zz-mux/src/formats.rs`, since `format_cb_current_path` reads
`osdep_get_cwd(ft->wp->fd)` and `wp->fd` is -1 after the child exits. A committed dead-pane row now
proves it.

The gate found one divergence of its own. The proof lane's new oh-my-tmux differential was red on
macOS, and not for anything the closed item covers: the config picks its clipboard tool with six
ordered `if -b` branches, the fixture stages a fake `xsel` to pin the choice, and on macOS
`/usr/bin/pbcopy` makes a second branch match too. Reproduced on throwaway sockets outside the
harness, with both branches matching the pin's prefix `y` ends as the first branch's `xsel` bind and
zz's ends as the last branch's `pbcopy` bind, while `command -v pbcopy` answers `/usr/bin/pbcopy` on
both - so the binaries agree on what exists and disagree only on which inserted bind survives. It
reproduces on `origin/main` and is nobody's regression; it is invisible on Linux, which is why nine
cycles of corpus runs never saw it. Registered as `config.background-if-shell-order`, and the
fixture now stages whichever single tool its platform lets match.

The stamped full run is the one thing this cycle did not deliver. It covers 251 scenarios and 3,080
steps, and the attached-client fixture PASSED at the merged tip `6fa9ef581e06`, but eleven corpus
rows diverge on the macbook - the keys fixtures that press keys on an attached client, `census-hooks`,
`smoke/pane-tmux-path`, `smoke/source-file-byte-name` on an APFS volume that refuses non-UTF-8 names,
and three plugin runtimes - and every one of them diverges identically in a baseline worktree built
at the pre-merge `origin/main`. `check_summary` therefore refused the write and the canonical summary
keeps the Ubuntu stamp `6edc5c7ec425`. No PASS was hand-edited. The first attempt also died at
scenario 79 when the machine ran out of disk with 1.0 GiB free; reclaiming the shared target's
12 GiB incremental cache and restarting from zero was enough.

One of those eleven rows deserves its own note, because it is the same behaviour the proof lane
measured and the gate registered: `compat/tui-pane-geometry.sh` never finishes on this box. It sends
`tput cols; tput lines` into an attached zz pane and waits ten seconds, and the wait expires at both
the baseline and the merged tip - yet the answer file is complete and correct once the script tears
the client down. So the keystroke is delivered late rather than lost, and the campaign's only
drawn-geometry differential cannot assert here. The gate read its values by hand instead: the
baseline hands the pane 80x22 and the merged tip 80x23 against the pin's 80x23, which is exactly the
row the client lane claimed to fix.

### 2026-09-06 cycle-16 integration

Claude Code Opus 5 lanes ran at xhigh on Ubuntu. One integration gate ran alone: daemon open
groups landed at `b547b29e`, the desktop tmux client at `b6732f05`, then proof debt at
`6edc5c7e`, each after its own gate. Protocol stays 98 and no lane touched the wire. Only
`knowledge/tmux/gaps.md` conflicted on the rebases, regenerated with `tmux-tracker.py
write-report` each time; the registry records merged by id without a manual merge. Original
campaign branches remain at their reviewed tips on origin, and no force push occurred.

All three reviews were approve-with-fixes, and every must-fix landed with the reviewer's probe
re-run. The daemon's closed record claimed a command client's stdout is claimed exactly once
while the claim lived on the innermost source-file frame, so a nested `source-file` re-claimed a
stream its parent owned; `note_stdout` now reads the claimed and written state across the whole
transcript stack, and both measured cases (a print, a nested raw write and a print; two nested raw
writes) are now byte, stderr and status identical on the two binaries. The desktop's NOMOUSE menu
came from the press half of `menu_key_cb` only: a live probe on the pin, an inner mouse-on client
inside an outer throwaway server with SGR bytes injected through `send-keys -H`, shows the
button-1 press keeping the menu drawn with nothing chosen and the release closing it with nothing
chosen, so the desktop now answers the release with `Cancel` and the test asserts the pin instead
of the divergence. The proof lane's new prose-citation rule tripped on its own bare
`compat/.cache` token and failed the tracker, `compat/check.sh` and `compat/run.sh` on any tree
without a generated cache; its census `buffer-limit` step asserted parity of a non-effect, since
`paste_set` reaches `paste_add` only on the unnamed path, and now evicts through three unnamed
`set-buffer` calls; and two reasons cited `cmd-display-popup.c`, a file the pinned tree does not
have, one of them inverting the popup's real pointer policy, which `popup.c:549 popup_key_cb`
owns through `popup_handle_drag`, a right-click menu and `input_key_get_mouse` into the popup's
own job.

The gate found two reds the lanes could not have seen, because neither code lane ran
`cargo test -p zz`. The daemon's guarded `%pause`/`%continue` placement contradicted
`control_mode`'s blank-and-EOF test and `cli_binary`'s live control test, both of which asserted
the pre-lane placement outside the guard. The desktop's sidebar threshold broke three `cli_binary`
status tests that read the status block at column 30, and two of them read `status-left`, which
the raw TUI paints only inside the sidebar: measured at 80 and 100 columns the merged binary draws
the window list and `status-right` but never `status-left`, and at 109 and 120 it draws all three.
Those tests now attach at 120 columns and the hole is registered as
`presentation:tui-status-left-needs-the-sidebar` rather than hidden.

Both code lanes passed the full workspace suite and clippy with warnings denied. The daemon run
needed the known-flake rule twice (`daemon_native_split_resize_commits_exactly` and
`history_request_is_guarded`), both green exact-solo. The proof branch has zero files under
`crates/`, so its code stages were skipped by rule. Deltas were 219 scenarios for daemon, 185 for
desktop and the proof lane's own 15 plus its deliberately red negative control; every list was
taken twice and reconciled. Only the four registered `known/` rows diverge, each at its documented
tuple. `smoke/source-replay-diagnostics` ran solo in each gate, and `smoke/pane-border-lines` hung
twice under four-way shard load and passed solo in 34 seconds. `compat/status-row.sh` still
reports 9 of 9 rows differing, with verdicts identical to the pre-merge main binary.

The final full run passed without retries at clean `6edc5c7ec425`: 249 scenarios, 3,073 steps,
four registered known rows, and attached-client PASS. The harness wrote the stamp and printed
summary current. Summary SHA-256:
`d1af67b9adcb23a29fcbad655b6282ea68102cae696fa17c8258baaf57f2baa0`.

Frozen delivery remains 304/304 items and 65/65 groups. The live registry holds 50 active groups
and 454 items: 8 open, 0 blocked, 42 accepted, plus 188 closed records; 230/238 settled (96.6%).
Fourteen post-freeze items remain across eight open groups: the sourced raw-newline claim, three
Control notification residues, the TUI copy-selection OSC 52 field, the rest of the menu mouse
policy, the status block's two rows and its missing `status-left`, four plugin runtime
measurements awaiting owners, the format census remainder, and harness theme steering. A one-hour
real iTerm2 session remains a maintainer validation. All three lane locks were integrated and
released; MAIN owns the records settlement, and TRIAGE preserves the F-SPLIT-MUX-*-V5 chain.

## 2026-09-06: cycle 16 on the ubuntu box, back on Opus 5 through the Workflow tool (08:42 to 17:41)

Fabrico set two things before launch: the desktop keeps its native status bar (recorded on
`presentation.native-status` as `presentation:gui-status-row-native`, commit `5235ef46`), and the
cycle runs on Opus 5 at xhigh rather than Codex. The runner went back to the workflow-script shape of
cycles 5 to 13, `opus-compat-run-16.js`, with one addition: a fix stage by a fresh agent between a
rejected review and a re-review, inside the pipeline. It never fired; all three reviews were
approve-with-fixes and the gate applied the fixes.

Timeline (local): desktop worker 08:42 to 09:39, review approve-with-fixes at 10:05 (the NOMOUSE
menu close); proof worker to 10:31, review at 10:57 (four must-fixes, the citation rule among
them); daemon worker to 12:02, review at 12:40 (the once-only stdout claim). Gate 12:40 to 17:41:
daemon integrated by 13:40 with two fixes of its own, desktop at 14:08 after the menu fix and three
red `cli_binary` status tests, proof at 14:53, then the stamped full run. The first full run died at
scenario 137 on the gate's own 48-minute inner timeout and was restarted with two hours; the second
passed first time (249 scenarios, 3,073 steps, PASS at `6edc5c7ec425`), records `62aa597c` pushed.
Fabrico pushed `2678806e` to main mid-cycle; the daemon rebase took it cleanly.

Lessons for the runner. Workers and reviewers never ran `cargo test -p zz`, so the integration
tests under crates/zz/tests were the gate's to discover red twice; that command joins the worker
and reviewer rules. Three lanes with declared, disjoint registry records merged with nothing but
generated `gaps.md` conflicts, so the ownership lists in the batch prompts are worth their length.
The proof lane building nothing and testing the gate's warm binary worked and freed the cores for
two cold builds. A shared gate target means `target/debug/zz` is whichever worktree built last;
rebuild before spawning. A journal watch armed by the orchestrator had a shell-quoting bug and
never fired; it did not matter because the gate handled the fixture-changed summary failure on its
own, but a monitor's script belongs in a file, not an inline `python -c`. `compat/__pycache__`
was tracked and rewritten by every board call; untracked in the close-out commit.

## 2026-09-06: cycle 17 launched at 21:09Z on the ubuntu box and frozen at 23:44Z for a machine shutdown

Four lanes this time, all Opus 5 at xhigh through `opus-compat-run-17.js`: wire (the cycle's only
protocol bump, 98 to 99, carrying the sourced stdout claim kind, the OSC 52 producer and control
environment expansion, plus the percent-word parse error and the hook-before-rename order), panes
(the new `pane.runtime-facts` group the orchestrator split out of `plugins.runtime-paths` before
launch in records commit `e0987ace`, plus the zz-tui clippy dead code), client (the raw TUI rows and
status-left, a new `tui.status-row` group from the status-row tool's classes, the desktop menu
mouse arm) and proof (oh-my-tmux prefix y, the census close by citation, the theme stance). The
panes lane got a fresh worktree with a btrfs reflink copy of the gate's 70 GB target directory,
which took 3.5 seconds and gave it a warm dependency cache.

Two and a half hours in, fabrico had to shut the box down. State at the stop: the proof, client and
panes workers had reported and pushed (`fe917342`, `e8bf4d1b`, `c3a66301`); the proof and client
reviews had come back approve-with-fixes with one must-fix each (two disclosed divergences without
slugs; a duplicated `#[test]` attribute); the panes review was 26 tool calls in; the wire worker had
all three groups committed on a clean tree and was re-running its proofs at tip, minutes from its
push. The orchestrator pushed the wire tip as `campaign/batch-wire-control-claims-frozen`
(`fb0dc67f`) as insurance, and after the stop found the worker had amended that commit during its
proofs, so the clean worktree's tip `711040e6` went up under the real name
`campaign/batch-wire-control-claims`; the orchestrator stopped the workflow, reaped the agents' servers by pid, released the four
locks with the state in their reasons, exported the journal's cached reports and reviews to
`cycle-17-reports.json`, and generated `opus-compat-run-17b.js` from the cycle script with those
reports inlined, a resume pass for the wire lane, and a shared review target for a machine with no
warm builds. HANDOFF.md's top section is the resume recipe. Nothing merged; `origin/main` stayed at
fabrico's `a7ca9482`.

Lessons. A freeze costs one insurance push per unreported lane and a journal export; the Workflow
journal is the only place a finished worker's report lives, so export it before the machine goes.
A worker's own push is the last thing it does, after every proof at tip, so a lane that is "done"
in git is still an hour from reporting when its proofs include the attached fixture and the zz
suite twice; the next runner could push a `-wip` tip before the proofs and the real name after.

## 2026-09-07: cycle 18 written and minted on the macbook, not launched

After cycle 17's close-out, fabrico asked for the docs to carry the next machine through the last
five items and for the macbook to be cleaned. The census registered two prose-only divergences as
items first: `semantic:explicit-config-keeps-mux-conf-layer` (accepted under fabrico's 2026-09-05
config.discovery stance; an explicit `-f` keeps the zz/mux.conf layer, which is how the macbook's
real mux.conf reached `cli_binary` tests that do not pin HOME) and
`presentation:tui-status-row-theme-colours-per-client` (open; the acceptance clause had said "OPEN,
and it needs the wire" with no slug). Six open items across five groups; the meter stays 304/304;
`cargo test -p zz-mux` and `tmux-tracker.py check` green on the records commit.

`opus-compat-run-18.js` is `opus-compat-run-17.js` with two lanes and the cycle-17 lore folded in:
client (the stdin stall, the detach hint decision, HOME pinning and the producer arm, the theme
colours with the cycle's only possible bump 99 to 100, last and budgeted) and mux (buffer error
wording, background if-shell insertion order after `cmdq_insert_after`). The desktop menu's mouse
modality is a design front, `F-GUI-MENU-MOUSE-MODALITY`, not a lane. Board: `F-TUI-CLIENT-CLOSE`
and `F-MUX-BUFFER-IFSHELL` minted as locks under TRIAGE, released.

The macbook: the `zz-lane-daemon`, `zz-review-daemon` and `zz-review-panes` worktrees and the
`zz-review-target` and `zz-gate-target` build directories were removed (about 60 GB). The Codex-era
`codex/attached-client-instrument` worktree carried five unpushed commits and an uncommitted docs
diff from 2026-09-04/05, so it was committed as WIP and pushed to origin at `ad29bc84` before
removal (`git cherry` says none of its commits is on main by patch; the cycle-14 instrument lane
landed the same instruments separately). HANDOFF.md's head is the launch recipe.
