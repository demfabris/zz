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
