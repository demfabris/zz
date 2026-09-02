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
