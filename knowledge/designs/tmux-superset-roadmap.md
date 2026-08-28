---
type: Design Plan
title: tmux-compatible CLI and native superset roadmap
description: The dependency plan and delivery history for making alias tmux=zz practical while keeping picker, browser, agent, editor, and fleet behavior on explicit zz-only commands.
status: In Progress
tags:
- tmux
- compatibility
- roadmap
- cli
- fleet
- native-superset
timestamp: 2026-08-27T00:00:00-03:00
last_updated: 2026-08-28
last_updated_by: Codex
---

# Outcome

Build a zz CLI that is compatible with the tmux workloads people actually carry, then add native
commands that make the GUI better than tmux:

```text
tmux names        -> tmux semantics or a loud error
zz-only names     -> picker, browser, agent, editor, GUI, and fleet semantics
zz default keys   -> may call zz-only names
imported bindings -> preserve the command they name
```

The goal is not all tmux internals. It is a reliable `alias tmux=zz` for daily shell use, imported
config, the pinned plugin corpus, and common automation. Linked sessions and a real tmux socket stay
out permanently; multi-user ACLs are parked outside the practical alias gate.

The schema 3 [tmux compatibility gap report](/tmux/gaps.md) owns live status, ordering, priorities,
and closed history. The
[2026-08-22 tmux CLI compatibility audit](/research/2026-08-22-tmux-cli-compatibility-audit.md)
records the source-anchored baseline used to build this plan.

# Current checkpoint, 2026-08-28

The live tracker has 85 active groups, 588 classified active items, 84 closed groups, and two known
differentials. The Alert cohort closed without a protocol bump. Bell, Activity, and Silence
messages now share the daemon-owned status-message identity, timer, replacement, dismissal,
terminal-publication freeze, and full-viewport thaw. Repair requests, resync, and popup viewports
obey the same gate. Each eligible attached Interactive client
appends an exact `<client> message: <text>` entry to the bounded server log, using its registered
name or `device-<id>` fallback. Control clients receive no alert status message or alert log entry.
`TerminalSession` emits one reliable Bell event per occurrence while the mux owns the visible flag,
so repeated BELs from one unvisited monitored pane still notify while that flag remains set.
The attached PTY fixture replaces a 1,500 ms sticky message with a 5,000 ms alert, proves 1.8
seconds of freeze, repeats the same-pane Bell, drains the pin's old timer for 5.2 seconds, and proves
zero-duration persistence and input dismissal on zz and pinned tmux. Ordinary publication, repair
requests, resync, and popup viewports remain frozen until the message clears. The pin's stale-timer
bug remains a deliberate correctness divergence because zz cancels and identity-checks old timers.

The canonical checkpoint freshly rerun after the third callback rule closed covers 91 scenarios
and 1,496 steps.
Every ordinary row is clean. `known/known-main-preset-two-panes` and
`known/known-spread-mixed` each retain their one documented GEO divergence with every other channel
clean. The expanded attached-client fixture and `compat/run.sh --check-summary` both pass. The
persisted summary SHA-256 is
`50b5eddb77da336747557d66928289dec366e6699917d3495c115f360caa5102`. Requested flags, attached
sizing, and client environments extend the attached fixture, while the daemon invalid-flag closure
and both positional-bound closures each add one fail-closed three-step canonical scenario. The
three-step shared flag scenario passes 516 focused probes on zz and the pin inside that full run.
The attached-client fixture now also compares nested validation status, stderr, session roster,
client count, aliases, and command-list stop behavior on both servers.

Protocol v82 appends one bounded UTF-8 client-environment snapshot to `ClientHello`. Local and
SSH-forwarded clients now seed fresh sessions and refresh existing sessions through the effective
`update-environment` patterns. `-E`, `-A`, Control, native attach, and targeted `switch-client`
follow the pinned selection and ordering rules. Existing processes keep their startup environment;
future panes read the refreshed session map. Non-UTF-8 Unix entries remain under
`clients.path-encoding` rather than being substituted.

Protocol v83 appends `ClientHello.process_id` and closes `clients.context-formats`. The daemon builds
one retained client-fact record for list rows, ordinary commands, foreground inserted commands,
status recipients, and `display-message`. The attached fixture covers Interactive, status,
bound-command, implicit target-client, and Control contexts against the pin.

Protocol v84 appends zero-based lexical command-block positions to `CommandInvocation` and now
closes `tracker.args-parse-if-shell`, `tracker.args-parse-run-shell`, and
`tracker.args-parse-set-option`. Source-file and Control
parsing preserve unquoted typed arguments through wire transport, aliases, bindings, and hooks;
quoted braces remain strings. `if-shell` accepts typed branch positions while rejecting typed
conditions, option values, and extra positionals. `run-shell` accepts typed positionals only when a
leading `-C` enables command mode, keeps option values string-only, and stops scanning flags at the
first positional or `--`. Its strict three-step scenario runs 21 internal checks on both servers
and finishes with `ARGS_PARSE_RUN_SHELL=clean:21`. `set-option` and `set-window-option` share the
`SetOptionValue` rule: only positional 1 accepts a typed block; option names, flag values, and extra
positionals remain strings. Recursive command printing preserves same-line `;` and physical-line
`;;`, empty blocks become empty values, quoted braces stay literal, and `-F` expands after typed
normalization. Their strict three-step scenario runs 21 internal checks on both servers and finishes
with `ARGS_PARSE_SET_OPTION=clean:21`.

Protocol v81 closes `control-mode.async-command-output`. Targetless and
invalid-target foreground shell output reaches the exact originating Control client raw after its
empty flags-1 guard; direct and sourced same-line continuation keeps separate guards. Embedded LF
and percent-prefixed lines stay literal, a missing trailing LF is supplied, and a shell's nonzero
status does not change Control retval. Foreground `run-shell -C` remains synchronous. Resolved `-t`
and ordinary `run-shell -b` open zz's native per-Interactive command-output view for attached pane
viewers without raw Control text or `%pane-mode-changed`, preserving deliberate GUI ownership.

The later asynchronous copy-pipe slice needs no protocol or runtime change. Pinned tmux starts the
worker without a completion callback, and a delayed exit-7 Control probe observes successful copy
mode cancellation with no message, error frame, or extra command guard. zz keeps the same silent
Control contract while retaining its native Interactive error notification.

Protocol v80 closed `config.startup-diagnostic-delivery`. Startup parses all roots
before replay, retains normalized root and nested read failures, parser diagnostics, unsupported and
runtime failures, and successful `display-message -p` output, and discards list-style output. Root
causes precede replay causes; replay stays root-ordered and nested depth-first. Successful physical
multiline commands use their completion line.

A detached Command launch stays rc 0 with empty stdout and stderr and cannot drain the causes. Only
the post-spawn Control advertises `startup-config-owner-v1`; it receives the raw bounded vector after
`ServerHello` and before its first `%begin`. A late Control receives it after `Attached` inside the
attach frame. An attached Interactive winner opens a PTY-free `configuration errors` view with an
ordered, control-sanitized, UTF-8-safe 64 KiB preview. The explicit truncation line directs full
recovery through a Control-mode restart. This preview is a deliberate product boundary: an
Interactive client cannot recover the exact retained 1 MiB vector.

Eligible delivery is linearized globally. The daemon commits the one-shot only after the complete
attach sequence remains admitted; failure retains the set and retires only the startup actor by
exact ID. A restart builds a fresh set. The checksum-attested seven-case differential against pinned
tmux `d77c9dc6` passes with no skips and leaves the canonical scenario summary untouched.

# Baseline captured 2026-08-22

These counts preserve the audit baseline. Use the generated gap report for current totals.

- 83 of 92 tmux commands execute; 9 are recognized but unimplemented.
- 85 tmux-valid flags are rejected across 23 implemented commands.
- All 180 named options stored at the audit baseline; 104 behaved and 76 were storage-only. The
  2026-08-24 separator slice moved the current split to 105 behaving and 75 storage-only.
- The audit used a flat format ledger. Schema 3 supersedes that snapshot with 198 global
  format-table names and source-enumerated rosters for selected contexts; use the generated report
  for current limitations. `config_files` retains the active top-level config selection and
  `pane_dead_time` tracks retained exits.
- All 68 hooks store; 10 lack an automatic producer.
- 71 differential scenarios and 1,058 executable steps were represented by the audit inventory.
- Bare packaged `zz` now creates session zero on an empty daemon and attaches it; simultaneous first
  attaches and a racing command-side session creator converge on the existing session.
- The default zz prefix table intentionally favors picker/sidebar behavior over complete stock tmux
  parity.

# Doctrine

## Keep tmux syntax frozen

Every tmux line a user pastes must mean what tmux meant or fail. Do not use spare-looking tmux flags
for pane kinds, hosts, browser profiles, agent providers, or GUI layout.

- Pane kind belongs in the verb: `split-window` creates a terminal, while `split-picker` and
  `split-browser` are native commands.
- Host belongs before the command: `zz --host NAME ...`. It never enters tmux's `-t` target grammar.
- A native default binding can call a native command. Config that binds `%` to `split-window -h`
  keeps terminal-split behavior.

## One native command catalog

The shared catalog is now the source for command discovery, prefix resolution, `list-commands`,
stored-command rendering, and palette completion. Its 102 canonical verbs comprise 83 tmux verbs
and 19 zz-native verbs. Exact names and aliases resolve first. Non-exact lookup searches tmux names
before native names, so new GUI verbs cannot change a pinned tmux abbreviation. `tools`,
`agent-send`, `send-last-output`, `capture-browser`, and `debug-marker` joined the catalog on
2026-08-22. The remaining duplicate is the long-form prose in `zz tools`; it is a documentation
cleanup, not a discovery gap.

Native command families should remain small and composable:

- Pane creation: `split-picker`, `split-browser`, and a direct agent form if automation needs it.
- Pane materialization: `select-pane-kind terminal|browser|agent|editor`.
- Browser control: `set-browser-url`, `set-browser-tabs`, `set-browser-profile`, `capture-browser`.
- Agent control: `agent-send`, `send-last-output`, `set-agent-session`, `set-agent-provider`,
  `restart-agent-pane`.
- Editor control: `set-editor-path`.
- Workspace navigation and operations: `focus-sidebar`, `tools`, `debug-marker`, and fleet verbs.

Do not add both `new-X` and `split-X` for every pane kind without evidence. The picker plus one
script-friendly direct form is the smaller primitive.

# Definition of compatible enough

The alias milestone is met when all of these are true:

1. Bare packaged `zz` creates and attaches on an empty daemon, attaches on a live daemon, and
   `new-session`/`attach-session` preserve TTY, nested-session, read-only, and detach semantics.
2. The current pinned config/plugin corpus runs without a SKIP, and the checked-in report proves
   every current scenario. Any SKIP exits nonzero.
3. A short published workload covering create, attach, list, target, split, resize, move, capture,
   buffer, option, environment, hook, source, and kill operations is differential-clean for exit
   status, stdout, stderr, topology, and geometry.
4. An attached-client harness covers copy mode, choosers, prompts, key bindings, and launcher paths
   that the headless corpus cannot see.
5. Every remaining accepted divergence is explicit. No unsupported tmux syntax silently changes
   GUI state.
6. Migration documents the one-time config import and the limit of a shell alias. Unix shell and
   status jobs spawned by the daemon receive zz's private shim; arbitrary programs that require an
   executable named `tmux` use a separate opt-in shim.

This gate does not require closing every registered gap. Pull work from the supported workload and
real config or plugin hits.

# Implementation-ease assessment captured 2026-08-22

This table records the audit's initial complexity assessment. The registry owns current rank,
status, `depends_on` ordering, and acceptance evidence. The delivery plan below groups related work
by dependency rather than raw ease.

| Ease rank | Gap | Why it falls here | Target |
| ---: | --- | --- | --- |
| 1 | Empty-daemon bare launch | The daemon and `new-session -A` already had the needed create-or-attach behavior; the launcher needed to select it without changing explicit attach semantics. | **Shipped 2026-08-22.** Bare `zz` creates-or-attaches, explicit attach preserves `no sessions`, nested `new-session` is guarded, and both first-session races are covered. |
| 2 | Prompt history commands | The existing separate rings and file policy needed command handlers plus serialized persistence. | **Shipped 2026-08-22.** Both commands, aliases, `-T`, output, errors, clears, and persistence are covered. |
| 3 | Native command catalog cleanup | Five daemon-only verbs needed shared specs and consumer convergence. | **Shipped 2026-08-22.** All 19 native verbs are discoverable; no direct agent split was justified. |
| 4 | Local parser and no-model flags | `unbind-key -a/-q`, `new-window -b`, and `kill-*-a -f` use state and formats the mux already owns. | **Two slices shipped 2026-08-22.** The 22-step local-flag fixture and 17-step kill-filter fixture are clean; pull further flags by corpus hit. |
| 5 | Small state and format facts | Bare `list-keys` padding, `pane_dead_time`, `config_files`, client timestamps, missing hook producers with an existing event seam, and straightforward output formatting. | **Three pulls shipped.** The 2026-08-22 pull covered bare `list-keys`, explicit-startup `config_files`, and retained `pane_dead_time`. The 2026-08-24 pull added pin-ordered `show-options -H` hook rows and item-scoped `window-status-separator` expansion. The 2026-08-25 pull exposed retained session activity and corrected logical MRU ordering. |
| 6 | Manual geometry | `resize-window` and `window-size manual` need a durable manual size plus clear precedence against per-client measurements. The command is small; the policy is not. | **Shipped 2026-08-22.** Absolute and relative practical forms, target/error precedence, manual formats, per-client precedence, and daemon PTY resize behavior are pinned. The later 2026-08-27 `clients.attach-sizing` slice closed client-derived `-A`/`-a`. |
| 7 | Capture, chooser, prompt, and list fidelity | `capture-pane` routing/ranges, chooser formats, command-prompt chains, and exact `list-keys` rendering need attached-client and output fixtures. | **List and chooser presentation fidelity completed 2026-08-24.** The list selectors, positional key filter, stock repeat metadata, canonical Space spelling, and `-1` attached-client status route are pinned by a 46-step differential plus the attached fixture. Chooser static-filter fallback state now survives deltas, both clients show `filter: no matches`, and fully keyless lists omit the shortcut gutter; the attached fixture proves tree and buffer fallback on zz and tmux. Ordinary capture was extended 2026-08-23; trailing blank viewport rows and richer capture transports remain. |
| 8 | Spawn and attach context | Attached cwd, client flags, sizes, environment refresh, client targeting, and exit actions cross different state owners. | **Twelve bounded slices have shipped.** Protocol v72 carries caller cwd; later slices closed client targeting, nested intent, supported tty selectors, local Control identity, session cwd, requested flags, retained sizing, and protocol v82 environment refresh. Protocol v83 closed `clients.context-formats`: one retained client-fact record covers list rows, ordinary and inserted commands, status recipients, and `display-message`, with pinned Interactive and Control empty behavior. The client lifecycle slice now produces all six report hooks with pinned duplicate, ordering, client-kind, and target-context rules. The fresh attached-client differential passes against tmux `d77c9dc6`. `detach-client -E`, active-pane consumption, changed-resize post-geometry hook context, no-detach-on-destroy fallback, parent-HUP exit actions, non-UTF-8 path bytes, read-only/focus policy, and interactive refresh remain in separate groups. |
| 9 | Interactive client behavior | Full `refresh-client`, `switch-mode`, mouse-targeted forms, pane marking, mode state, focus hooks, and client fanout cross daemon, protocol, TUI, and GUI ownership. | Implement only for named workloads. |
| 10 | Binary streams and process control | `display-message -I`, `split-window -I`, buffer/source `-`, and lock execution require bounded transport, backpressure, cancellation, and process lifetime rules. | Separate design approval. |
| 11 | tmux floating panes | `new-pane` and the parked `move-pane`/placement flags need a new mux-state model that is distinct from current native floating UI. | Park. |
| 12 | Linked sessions, ACLs, and tmux socket interop | These require changing core ownership or implementing unrelated security/wire protocols. | Linked sessions and socket interop are permanent non-goals; park ACLs. |

The 21 theme/palette options and four tree-mode options are easy to store but not necessarily easy
to make meaningful across native clients. They remain demand-driven rather than inflating an option
percentage.

# Implementation progress: 2026-08-25

The first eight ease ranks have shipped at least one evidence-driven slice:

- Bare installed `zz` routes through `new-session -A`, preserving product-friendly create-or-attach
  without weakening explicit `attach`/`attach-session`: those verbs return tmux's `no sessions` on
  an empty daemon, and a literal `attach || new-session` fallback is PTY-tested. Empty-daemon
  materialization remains atomic, including two simultaneous lower-level attaches and an ordinary
  command client creating the first session at the same boundary. Attaching `new-session` uses the
  same nested-session refusal as `attach-session` before changing mux state.
- `show-prompt-history`/`showphist` and `clear-prompt-history`/`clearphist` match the pin's two rings,
  `-T command|search`, output shape, invalid-type error, selective/all clear, and persistence. Save
  ordering is serialized so a racing record or clear cannot restore stale disk state.
- The shared catalog contains all 19 native verbs. A review-caught long-option rendering bug is
  pinned: storing `agent-send --submit --context=...` no longer turns the long flag into a short
  cluster when rendered by `list-keys` or hooks.
- `unbind-key -a/-q` and `new-window -b` match the pin in a 22-step differential scenario.
  `kill-session -a -f`, `kill-window -a -f`, and `kill-pane -a -f` share the existing contextual
  format engine and match in another 17 steps. The unsupported ledger fell from 113 pairs across
  29 commands to 107 across 26 before manual geometry made `resize-window -A`/`-a` explicit,
  bringing the then-current ledger to 109 across 27. Later `display-panes`, `join-pane`, and pane-spawn
  slices brought the ledger to 102 pairs across 24 commands. `last-pane -d/-e` then brought it to
  100 pairs across 23 commands. Four micro flags, three `list-keys` selectors, and creation-time
  `new-session -e/-E` brought the ledger to 91 pairs across 23 commands; the following
  `set-buffer -n`, `source-file -F`, `split-window -Z`, and `break-pane -a/-b` slices left the
  ledger at 85 pairs across the same 23 commands after `move-pane -l` joined the supported surface.
- `list-keys` now shares the pin's global padding facts, optional key filter, `-1`, `-O`, `-r`,
  error precedence, and stock copy-table repeat metadata. Its `-1` result is stdout for Command and
  Control clients and a frozen timed status for Interactive clients. `config_files` reaches command,
  status, list, label, and renderer-style contexts and changes from startup selection to the file
  selected by native reload; retained panes expose `pane_dead_time` and clear it on revive/respawn.
- `session_activity` now exposes retained Unix seconds initialized from session creation and refreshed
  by the shared attach and terminal-input funnels. `S/t` and `list-sessions -O activity` use a
  separate logical counter, preserving deterministic same-second MRU order. Sessions now retain an
  internal cwd, while the client-derived `session_active` and public `session_path` format facts
  remain under `formats.session-runtime`. Every attach now
  advances latest geometry independently of `focus-events`; enabled FocusIn uses the same owner
  seam. Read-only rejected native input updates activity and latest geometry without clearing bells,
  while writable chooser input counts once, advances latest geometry, and preserves bells. Chooser
  routing stays client-scoped. Read-only-safe local view actions bypass retained chooser and
  display-panes surfaces without dismissing them. Display-panes valid selection and bare hover
  consumption, unmatched key/Escape/non-hover mouse fallthrough, and timeout accounting are
  explicit. Typed `send-keys -X` authorization, all-or-nothing binding preflight, and pane-focus
  blocking are closed under `clients.read-only-local-view-actions`. Committed text now uses one
  bounded ordered queue per client whose entries record pane and input lane: a matching Key-plus-Text
  pair takes the Key result and contributes at most one activity/latest update, standalone read-only
  terminal text accounts without PTY input or a bell clear, and writable modal consumption can
  contribute zero. Cleanup and synchronous switch behavior are closed under
  `formats.session-activity-text-input`; tmux's inapplicable suspended-client wake path is accepted
  under `formats.session-activity-wake-lifecycle`.
- `resize-window`/`resizew` now resize the durable layout extent, select a window-local manual
  sizing policy, expose the two manual-size formats, and outrank later client measurements. The
  16-step strict-geometry scenario covers absolute and relative sizes, option transitions, output,
  bounds, and missing-target error precedence against the pin. The later `clients.attach-sizing`
  slice closed client-derived `-A`/`-a` aggregation.
- `new-window` and `split-window` apply repeated `-e NAME=VALUE` entries only to the new pane,
  ignore malformed entries, and let the last value win without changing `show-environment`.
  Their `-E` forms create live panes with no child process, reject nonempty commands after target
  resolution, and match the pin throughout a 25-step strict-geometry scenario.
- Creation-time `new-session -e` overlays the normal `update-environment` seed, persists on the
  session, and reaches its first pane; later entries win and malformed entries are ignored.
  Creation-time `-E` skips that normal seed while retaining explicit overlays, and `-A` ignores
  `-e` when the session already exists. An 18-step differential fixture pins the behavior. Protocol
  v82 later closed bounded UTF-8 client-sourced values and attach-time reseeding.

Focused protocol, mux, daemon, TUI, completion, strict-Clippy, formatting, and differential tests
cover the tranche. Gate 0's mechanism and current canonical summary are complete. The 2026-08-26
checkpoint covers 84 scenarios and 1,475 steps; every ordinary row is clean, and the two registered
known rows each retain exactly one documented GEO divergence with every other channel clean. The
attached-client fixture is part of the strict Linux CI run and drives real zz and pinned-tmux
attaches through outer PTYs. The packaged CLI fixture clones
a verified macOS bundle through a path containing spaces and passes bare/new/attach against empty
and existing daemons. It now also pins detached `-x`/`-y`, attached client dimensions, read-only
input rejection with visible pane output, requested detach notices, and `attach -d` eviction notices.
The attached-client fixture also targets the live zz and tmux clients by their real outer PTYs,
requires their attached-client counts to reach zero, and thereby proves normal local TUI tty publication.
It now also refuses attach and `new-session -A` with inherited `$TMUX`, then repeats both through
`env -u TMUX` on the same retained tty and requires them to attach.
The current attached fixture also runs local Control from each outer PTY. It requires terminal-backed
`attach-session` and `new-session -A` refusal against existing sessions, permits a fresh `-A` miss,
and proves piped stdin does not acquire a tty identity. The daemon unit matrix covers
`new-session -Ad`; the attached fixture does not. The complete attached differential passed for zz
and pinned tmux. The 2026-08-26 canonical run persists that result as `PASS` below the scenario rows.
The attached proof also exposed and closed a copy-mode ordering race where a queued yank could be
canceled before the terminal processed it.

The next slices are evidence-ordered rather than alphabetical:

1. Make bare `list-keys` compute the pin's global repeat, key, and table widths. **Complete:** the
   pinned `tmux-sensible` runtime path calls this form, and the expanded 19-step scenario also pins
   `-N`, `-a`, and `-P` with deterministic prefix/root ordering. **Extended 2026-08-24:** the
   46-step scenario covers the remaining selectors, positional filtering, reverse orders,
   canonical Space spellings, and post-`-1` aggregate facts; the attached fixture covers the timed
   status route.
2. Add a shell-level attached-client driver. **Complete and integrated:** a pinned tmux outer pane
   supplies the PTY for an inner
   zz or tmux attach; the fixture compares semantic queries and small mode markers, not native
   presentation pixels. It covers readiness, root/prefix/prefix2 bindings, copy mode, command
   prompt rename, tree row-key switching, buffer paste/deletion, and nested attach.
3. Back `config_files` from the active top-level selection and `pane_dead_time` from the
   retained-pane exit seam. **Complete:** harness startup is symmetric for explicit `/dev/null`,
   native reload replaces the retained config selection, style conditionals receive the same fact,
   retained death stamps the timestamp, and revive/respawn clear it.
4. Implement manual window geometry without letting a later client measurement overwrite it.
   **Complete:** `resize-window` uses the existing durable layout extent, `window-size manual`
   freezes that extent, and the supported forms are strict-geometry clean.

# Recommended delivery order

This section records sequencing decisions and dated completion evidence. It does not serve as the
work queue. Select exact gap IDs from the generated report before starting a slice.

## Gate 0: make the evidence current

1. Revalidate `smoke/config-grammar`: the current tmux-only warning expectation is correct; the
   nested zz control client still does not emit `%config-error`.
2. Run all 84 differential scenarios and persist the 1,475-step summary. **Complete 2026-08-26:**
   every ordinary row is clean. `known/known-main-preset-two-panes` and
   `known/known-spread-mixed` each retain exactly one documented GEO divergence and no other
   difference. The attached-client fixture is `PASS`, and `compat/run.sh --check-summary` passes.
   The summary SHA-256 is
   `5de67222bc2ebb99c57963be14c865ddfdddc387da34ee32dd86962cef8336c9`. CI checks scenario paths,
   step counts, every stored result cell, and the attached `PASS` before the run, then diffs every
   result column after a complete strict run.
3. Add a drift check that fails when scenario files and checked-in result rows differ. **Complete:**
   `compat/run.sh --check-summary` compares exact scenario paths, step counts, and all seven stored
   row cells against the ordinary clean tuple or the tracker's registered known tuple. It also
   requires a persisted attached-client `PASS`. Partial and headless-only runs cannot overwrite the
   combined report, and only the summary is versionable under `compat/results/`.
4. Add an attached TUI fixture for copy mode, choose-tree, choose-buffer, command prompt, prefix
   tables, and nested attach. **Complete as `compat/attached-client.sh`:** both sides run through
   real 80x24 PTYs with bounded semantic polling, diagnostic screen dumps, and deterministic
   cleanup. `compat/run.sh --attached-client` includes it in overall success without mixing its
   result into the headless scenario counts; strict Linux CI runs that combined contract.
5. Add packaged-launcher smoke tests for bare, `new`, `attach`, empty daemon, existing daemon, and a
   path containing spaces. **Complete:** the Cargo launcher matrix covers the cheap seam, while
   `compat/packaged-cli.sh` verifies a freshly built CEF bundle and development signature, clones
   the whole app under a spaced path, then passes all six command/server cases through the real
   `Contents/MacOS/cli`. Four PTY cases additionally pin detached and attached sizing, read-only
   input/output, requested detach, and peer eviction. The macOS CI leg repeats that smoke after
   bundle creation. Release notarization and `/Applications` installation remain packaging checks,
   not tmux compatibility gates.
6. Put every compatibility TODO and accepted difference in one repo-owned registry. **Schema 3
   review complete 2026-08-23:** `compat/tmux-gaps.json` owns stable IDs, product status,
   `depends_on` ordering, priorities, evidence, `updated_on`, and closed history. Oracle schema 4
   captures 92 commands, 78 aliases, 572 flag shapes split into 318 valueless, 246 required-value,
   and 8 optional-value shapes, plus positional minimum and maximum metadata. It parses nine custom
   `args_parse` callbacks used by 14 commands and reduces them to six effective rules. It also
   captures 180 options, 198 global formats, 14 source-enumerated names across the selected
   `command-item`, `list-commands`, and `list-keys` contexts, 68 hooks, and 303 default bindings
   across five tables from an attested clean build at the exact pin. `just compat-check` runs the
   oracle and registry checks plus the full `zz-mux` library suite.

   `mux.resize-pane-optional-values` closed on 2026-08-25 as a catalog-only reconciliation.
   Runtime already accepted bare direction flags with amount 1 and attached or separated integer
   amounts. The four direction entries now expose optional values to the manifest gate. Nine focused
   resize tests, 175 protocol unit tests, 14 protocol framing tests, and the strict 16-step
   `resize-directions` differential pass. No runtime path or wire version changed. `resize-pane -M`
   and `-T` remain open under their existing owners.

   The Rust gate reconciles names, flag arities, positional bounds, native extensions, the guarded
   native-name roster, every pinned canonical prefix, zz-only defaults, every constant-backed
   format gap, every missing default key, and rendered command plus repeat metadata for every
   shared default binding. It also reconciles the three selected context rosters. zz implements all
   14 selected names:
   `formats.command-item-context` closed on 2026-08-24 once the mux dispatch chokepoint started
   carrying the canonical entry name into every command it runs.
   `formats.daemon-command-item-context` closed the same day by carrying that resolved name only
   through daemon-owned item expansion, including the post-spawn `new-window`/`split-window -P -F`
   pass that adds live pane facts. Typed blocks and delayed formats retain their own or empty item
   context instead of inheriting a parent command.

   `formats.command-argument-expansion` closed five paths on 2026-08-24. Both rename names, both
   optional show-option names, and `select-pane -T` expand in their resolved target contexts. The
   differential fixture covers canonical names, tmux aliases, permitted unique prefixes, old
   session/window facts, and directional pane-title application. `formats.new-session-name-expansion`
   closed on 2026-08-25: `new-session -s` now expands once before attach-or-create lookup, carries
   only a genuinely attached client's target facts, preserves explicit command-item precedence,
   and refuses a nested formatted `-A` attach before applying its effect while leaving a formatted
   detached miss intact. `formats.name-validation-cleaning` then closed the adjacent pinned name
   pipeline: `new-session -n` expands, validates, and vis-cleans before `-s`; `new-window -n`
   expands once in its destination session context with session format type before the same helper;
   both rename commands expand through their resolved active pane with pane format type and then use
   that helper; and `break-pane -n` deliberately stays literal before validation and cleaning. Empty
   names and valid Unicode survive, ASCII controls fail before
   mutation, cleaned backslashes determine identity and collision or reuse behavior, and a detached
   formatted `-A` miss is no longer refused merely because its raw format text names another
   session. `formats.creation-name-edges` closed the last two edges on 2026-08-25. An unindexed
   `new-window -S` performs the pin's second format pass over the cleaned first-pass value for lookup
   while creation keeps the first-pass name. An explicit `break-pane -n` pins window-local
   `automatic-rename off` on both placement paths. The full mux suite passes 379 tests, and the
   125-step `command-item-format` plus 30-step `break-pane` differentials report zero differences and
   no skips.
   `formats.buffer-path-expansion` closed on 2026-08-25: `load-buffer` and `save-buffer` now expand
   paths once through the shared daemon command hooks before home-directory handling and file I/O.
   Client selection supplies the target session, focused window, and active pane; aliases and unique
   prefixes retain the canonical command name, explicit item state wins, and replacement text is not
   expanded again. `protocol.binary-streams`, `buffers.clipboard-write`, and
   `buffers.client-file-context` retain the stream, clipboard, relative-path, attached-session,
   and remote transport work. The separate 29-step
   `native-prefix-isolation` fixture closes all 25 unique prefixes that native names had changed
   and checks ambiguous `list-commands` exit parity. The daemon authorizes one expanded alias
   invocation and dispatches that same value. Writable stored bindings resolve each command just
   before dispatch, so an earlier command can change a later alias; read-only clients resolve and
   authorize the whole chain before any effect. A typed alias result now keeps an exact empty,
   multi-command, or unparsable match from falling through to the canonical or catalog-alias command
   it shadows. Actual empty and multi-command execution remains under `aliases.command-bodies`.
   Protocol v74 closes Control's static unknown-name precheck by preparing each complete input unit
   under one daemon lock before framing. Prepared execution freezes only that alias lookup and still
   reauthorizes normally. Local ordinary CLI commands now prepare the complete vector against an
   existing compatible daemon. Canonical identity and alias-match state drive exact attach, TUI,
   stdin, and kill recovery routing, and the TUI carries the immutable vector across its reconnect.
   A typed name or alias-body error anywhere in the local prepared vector aborts before
   preprocessing or execution; runtime failures retain tmux queue ordering. Remote `--host` routing
   remains under `aliases.remote-client-preflight`; config replay alias snapshots stay under
   `aliases.config-parse-unit`, while local argument validation and replay-group parse abort stay
   under `mux.chain-parse-abort`.

   `tracker.args-parse-inventory` closed callback discovery on 2026-08-25. The oracle rejects an
   unknown callback body, the Rust catalog carries typed rules for all 12 implemented callback
   commands, and the third manifest test requires one `args-parse:` item for each implemented
   callback command absent from `COMMAND_ARGS_PARSE_BEHAVES`. The behaving roster now contains `if-shell`, `run-shell`,
   `set-option`, and `set-window-option`; three effective rules and eight command-specific items
   remain. The unimplemented
   `choose-client` and `switch-mode` callbacks stay covered by their command items.

   `tracker.semantic-coverage` tracks runtime adoption of the three remaining argument rules, open-ended or
   dynamic context formats, nonconstant formats, hook production, shared binding runtime behavior,
   and option `BEHAVES` consumer truth. Daemon invalid-flag coverage first closed on 2026-08-27 with
   a 24-command production-dispatch roster. The shared flag closure on 2026-08-28 removed that
   partial roster and routed daemon preflight through the catalog parser used by mux execution.
   The first eight positional maximum mismatches closed later that day, followed by all fourteen
   required minima. The full shared arity closure then removed the partial maximum roster: all 72
   implemented finite upstream commands now validate their catalog maximum after flags and minima
   but before targets or effects. Stored binding and hook children use the same two bounds before
   replacing state. The three-step `positional-maximums` fixture checks 71 generic-CLI-routed
   command-drivable canonical names and 62 aliases with exact stderr and unchanged pane, buffer, and
   file state; an exhaustive daemon test covers all 72 engine paths and aliases. The minimum fixture
   retains its exact canonical and alias proof. The shared option parser now covers all 83
   implemented upstream commands and 74 aliases, including stored commands and exact native attach.
   Its three-step differential compares 516 probes against zz and the pin with exact diagnostics,
   required-value absorption, optional-value lookahead, and unchanged sentinels. The final
   `mux.error-shapes` item closed on 2026-08-28 when nested `new-session` adopted the pin's exact
   validation order ahead of its mutation-free nesting refusal.
   `knowledge/tmux/gaps.md` remains generated from the registry.

Without this gate, easy compatibility fixes can land while the persisted proof quietly goes stale.

## Milestone 1: close the literal alias path

1. Remove the empty-daemon `has-session` dead end and make bare packaged `zz` create and attach.
   **Complete.**
2. Apply the same nested-session refusal to attaching `new-session`, not only `attach-session`.
   **Complete.**
3. Pin the terminal size and read-only/detach variants in packaged PTY tests. **Complete:** a
   detached `new -d -x 93 -y 29` retains exact window/pane geometry; an attached 97x31 outer PTY is
   published as the client's dimensions; a read-only attach drops ordered terminal input while
   continuing to display externally produced output; and requested detach plus `attach -d` eviction
   both exit zero with `[detached (from session NAME)]` after terminal restoration.
4. Document that `alias tmux=zz` is a shell boundary. Keep the global executable shim opt-in so zz
   does not steal an installed tmux binary. **Complete:** the compatibility guide documents the
   shell-alias limit and the daemon's private shim for shell and status jobs.

This is the smallest milestone that changes the answer from “the alias fails on first run” to “the
alias starts and reconnects correctly.”

## Milestone 2: close the cheap, high-frequency surface

1. Implement prompt history commands. **Complete.**
2. Consolidate native command discovery. **Complete.**
3. Mine the pinned corpus and a small personal-config corpus for actual rejected flags.
   **Complete for the current pin:** bare `list-keys` formatting is the sole remaining rank-5
   runtime hit; `client_last_session` is deferred with client-context work and `new-session -t`
   stays parked with linked sessions.
4. Implement only the rank 4 and rank 5 items those fixtures hit. **Current slice complete:** bare
   `list-keys` default padding, explicit `config_files`, and `pane_dead_time` are differential-clean.
   Further state facts remain demand-driven.
5. Differential-test every landed flag, update its registry entry, and regenerate the report before
   merging the slice.

Do not sweep flags alphabetically. A config-hit flag is worth more than ten unused palette knobs.

## Milestone 3: make script output trustworthy

Close accepted divergences that scripts observe even though the command catalog says the syntax is
supported:

- `capture-pane` stdout/buffer routing and range selection. **Complete for ordinary retained text:**
  `-p` prints without touching `-b`; without `-p`, named and automatic buffers receive the capture
  with the pin's trailing newline. The 23-step differential fixture covers clustered value flags,
  inclusive and reversed bounds, target-scoped format expansion, and silent invalid/out-of-range
  fallback. A fallback `-E` over trailing blank viewport rows still omits tmux's blank newlines;
  saved alternate screens and the six richer capture transports remain explicit gaps.
- `show-buffer` binary policy.
- `list-keys` note rendering and `-N`/`-a`/`-P` selectors. **Complete:** the 19-step fixture pins
  exact rows and deterministic root/prefix ordering. The 2026-08-24 extension closes positional
  filtering plus `-1`/`-O`/`-r`, while keeping only tmux's non-total comparator ties as a bounded
  accepted divergence.
- `source-file` client-relative path resolution. **Complete for registered clients:**
  protocol v72 retains one local caller cwd, SSH omits it, and CLI coverage separates caller and
  daemon cwd. The daemon now keeps that selected base through nested replay, including after an
  ordinary sourced command clears the mutable context cwd and when runtime `source-file` loads the
  active default `zz/mux.conf` through the ordinary path. A direct zz-native `reload-config` carries
  the same selected base for registered clients. The 2026-08-26 session-cwd slice retains one cwd
  per session, selects compound `attach-session` targets before pane-context `-c` expansion, and makes attached source selection
  prefer the invoking client's session cwd. The full attached-client fixture separates command cwd
  from session cwd with decoys, and a focused daemon test adds a third `source-file -t` target cwd
  decoy. Hooks raised by sourced ordinary commands, deferred event hooks, and initial startup replay
  retain separate gaps.
- command-prompt only where a real automation uses it.
- exact exit, stdout, and stderr on the published workload.

This milestone matters more than raising the command count. Scripts consume bytes, not catalog
totals.

## Milestone 4: add one client-context model

The first tracked slice, `clients.cwd-context`, completed on 2026-08-23. Protocol v72 appends one
bounded `ClientHello.working_directory`; local endpoints publish an absolute cwd and SSH endpoints
omit their caller-host path. Non-UTF-8 or oversized local paths are omitted rather than breaking the
connection. The daemon retains the accepted fact per client and resolves relative top-level
`source-file` paths after `-F` expansion and before globbing. It snapshots that selected base for
registered clients and carries it through nested replay, so a sourced ordinary command cannot erase
the next nested source's cwd. Sourcing the active default config through the ordinary runtime loader
forwards the same snapshot. A direct zz-native `reload-config` forwards that snapshot for registered
clients. Startup keeps its separate clientless bootstrap gap. CLI coverage pins literal cwd glob
metacharacters, glob order, declared-path order, quiet continuation, and declared missing-file
diagnostics independently of the daemon cwd. The `clients.attach-session-cwd` slice closed on
2026-08-26: each session retains a cwd seeded from explicit `new-session -c`, its attached source
session, or caller cwd; `attach-session -c` selects a resolved compound target's window and pane,
then expands and stores before terminal validation; attached `source-file` and `reload-config` prefer the invoking client's session
cwd while `source-file -t` remains a separate target. This uses internal state without a protocol or
snapshot-schema change. The 2026-08-27 `clients.attach-flags` slice closes `attach-session -f`,
attaching `new-session -f`, and durable client attach context without changing protocol v81. Typed
daemon state retains tmux's comma mutation grammar across switches, detach, native attach, and TUI
reconnect, and clears it on client teardown. Common requested flags and Control-only flags report
through `#{client_flags}`; `pause-after` follows tmux's unsigned prefix and wrap behavior. The fresh
attached differential covers missing targets, fresh and detached creation, switching, reattach,
teardown, and the deliberate `-r` difference; Rust tests cover terminal-open ordering and `-A`.
zz `-r` adds read-only only, while tmux also adds ignore-size. The completed
`clients.attach-sizing` slice consumes retained client size and `ignore-size` state without a wire
or snapshot-schema change. `resize-window -A` and `-a` choose the largest or smallest width and
height independently, then store that one-shot result as a durable manual extent. Any attached
unignored client globally suppresses ignored candidates; if every attached client is ignored, the
ignored candidates become eligible. Control participates only after explicit `-C`; a per-window
override beats its global size and hard-clamps each final dimension. An empty target candidate set
uses `default-size`, and the final extent clamps to 10,000. Later client size updates do not move
the manual window. The 2026-08-27 `clients.attach-environment` slice appends a bounded UTF-8
environment snapshot in protocol v82. Fresh sessions and later attaches copy the invoking or selected client's
exact and wildcard-matched `update-environment` values. Missing names become unset markers, empty
values stay set, and selected hidden values become ordinary. Existing `new-session -A` follows
attach behavior and ignores `-e`; `-E` preserves the session map. Control, PTY, native attach, and
targeted switch use the same rules, while `switch-client -T` returns before refresh. The session
map survives client disconnect, affects future panes, and does not rewrite existing process
environments. Non-UTF-8 Unix entries remain under `clients.path-encoding`. The full attached
differential passes against pinned tmux. `active-pane` and
`no-detach-on-destroy` retain state but stay open under their own consumer gaps. Deferred event-hook
client selection remains under
`source-file.event-hook-client-cwd`. Startup configuration
still runs before the launching client registers, so `source-file.startup-client-cwd` tracks tmux's
initial `cfg_client` cwd rule. `source-file.sourced-hook-client-cwd` tracks hooks raised by ordinary
replayed commands because those commands still use `ClientId::MAX`. The Unix POSIX glob
dialect slice is closed: source matching now uses `glob(3)` with tmux's bytewise cwd quoting,
backslash rules, leading-dot exclusion, repeated-star behavior, malformed-pattern handling, and
per-pattern order. The tilde slice is also closed: `source-file` leaves a literal leading tilde for
normal relative-path resolution, while tildes expanded by the config parser already arrive as
absolute paths. The nested declared-path slice is closed: loud no-match and glob errors reach the
invoking client with the post-`-F` declared argument, while a quiet no-match stays silent. A direct
Control all-miss aborts its line; a direct partial match ends with `%end` and continues; matched
parser errors remain `%config-error`. Protocol v76 now gives each parser-owned replayed command that
survives command-name resolution a tail-tag-47 `SourcedCommandGuard`. An alias resolved to
`source-file` before replay retains the same recursion path. Unknown or ambiguous command names and
malformed alias names publish a located Warning that Control renders as `%config-error`, without a
guard. Ordinary success and quiet all-miss use an empty flags-1 `%end`; a mixed hit and miss keeps the
declared-path diagnostic inside `%end`; and all-miss, flag or arity failure, runtime failure, or depth
refusal ends `%error`. Runtime failures alone set `client_failure`, and the Control writer defers
guards FIFO until the direct outer frame closes. Matched OS and path read failures follow as typed
standalone Error events, including numeric OS errors and colon-space paths. Invalid UTF-8 config
content remains under `config.non-utf8-file-bytes`: pinned tmux accepts the measured lone-`0xff`
file without a visible diagnostic where zz emits a typed Error and status 1. Config and lexer
Warning prose remains under `control-mode.diagnostic-typing`. The existing loader preflights every
declared path for one source command before recursion. A focused regression and the then-six-step
Control differential prove root missing-path guard, then middle missing-path guard, then leaf output
guard, each exactly once.
That closes `source-file.nested-control-queue` with no production change. The later
`control-mode.source-file-exit-status` closure completes the long-lived Control matrix. Direct and
parser-owned sourced runtime errors plus nonruntime source failures set retval 1. Generic nonzero successes and
flags-1 parse or preparation errors do not set or change it, so a fresh client stays at 0 while a
prior sticky failure stays at 1. A blank line or EOF snapshots the current value. A Return captured
while a preceding non-detach command waits precedes later queued stdin commands, including detach;
a Return observed while self-detach itself waits is discarded on the caller's `Detached` event. Only
a caller-targeted `Detached` event exits 0, so nonself and no-victim detach forms keep the loop alive.
The command response closes before `%exit`.

The synchronous inserted-list slice now retains flags-1 Control identity through foreground
shell-evaluated `if-shell`, immediate `if-shell -F` including `-bF`, and foreground `run-shell -C`.
Per-client and per-thread capture publishes the containing replay command before each inserted
command, and an inserted source before its children. Output, failures, status, and nested ordering
remain command-scoped without folding or leakage. An unsupported zz-only inserted command gets an
empty success guard and later siblings continue, but it does not join `ConfigLoadReport`'s skipped
summary. An unknown child command produces successful parent and source guards, then `%config-error`
without its own guard, matching the pin. The closure reuses protocol v76. The later protocol v77
closures give immediate hook commands and shell-evaluated `if-shell -b` or `run-shell -bC` callbacks
flags 0. Protocol v78 later closed source-read placement and completion numbering.
Hard-disconnect queue cancellation remains under its active Control group. Protocol v81 later closed
foreground Control shell-output placement; ordinary `run-shell -b` now uses zz's native
per-Interactive command-output view.
The nesting limit is closed for guard
placement, depth wording, count, and
later-line continuation: both sides run 50 concurrent source invocations counting the initial
`source-file` as invocation 1, and refuse invocation 51 with `too many nested files` on the
client-specific channel. A malformed invocation at that depth is diagnosed as malformed rather than
refused for depth on both sides, because the pin rejects it while parsing the containing file and
never consults its depth guard, and zz now runs its depth guard after the command's own flag and
positional validation. The later shared arity and flag closures also matched the malformed text.
The pin still abandons the rest of the containing file where zz continues it, which
`config.parser-edge-cases` owns. The refused nested command now uses the same flags-1
`%begin`/`%error` guard as the pin.
Same-line replay
grouping is closed independently: synchronous invalid/runtime errors, depth refusal, and a loud
zero-file source miss or glob error drop only later siblings on the same parser-owned source line,
while later physical lines continue. Matched sources and asynchronous commands do not propagate
child failures into their invoking line. The daemon retains a matched child OS or path read failure
in the load report without using it to prune the parent group. Quiet zero-file misses succeed. In this
slice zz-classified unsupported capability gaps changed from pruning later same-line siblings to
skip-and-continue. That continuation is desirable for zz import capability gaps but remains
pin-unproven because the corresponding commands are unsupported in zz. The synchronous inserted
path shares the continuation policy but does not add its unsupported commands to the load report's
skipped summary. Replayed error delivery is
closed for the pinned target and set-option failures on Command, parser-owned Control, and attached
clients. Successful output plus command-name and parser diagnostics share one per-invocation
transcript. Each invocation
appends its complete `-v` batch, replays every parsed match, then appends buffered command-name,
and parser diagnostics. Source no-match, glob, and actual OS or path read failures retain their
existing error channels.
A nested source inserts its own complete frame at the parent command's replay position, so nested
frames are depth-first. This does not claim physical verbose and replay interleaving. Command clients
receive the transcript once on stdout. For valid successful replay and `-v` output, Interactive
clients open one command-output view without duplicate Info or Warning events. Parser diagnostics
may still publish their existing Warning summary. Successful output leaves stderr empty and status
zero. A runtime failure retains stderr and status 1 while stdout before and after it remains ordered.
Cross-depth parser-owned Control ordering, synchronous inserted flags-1 framing, and
return-versus-detach precedence are closed. The later protocol-v77 slice also closes immediate command
hook flags-0 frames. The later background slice closes callback frames, and protocol v78 closes
parser and hook-source raw read placement plus completion numbering. Config byte input, parser abort,
and error shapes remain with their named groups; the same-line close did not cover those contracts.
Startup accounting is closed: one
budget spans every startup root, the roots do not count, source commands 1 through 50 run, and later
source commands retain their declaring file and line while runtime sequential sources stay
unbounded. Protocol v80 later closes retained delivery and placement. Startup parses every root
before replay, retains normalized root and nested read failures, parser diagnostics, unsupported and
runtime failures, and successful `display-message -p` output, and discards list-style output. Root
causes precede replay causes; replay stays root-ordered and nested depth-first. The detached launch
stays silent and cannot drain the set. Control receives the raw bounded vector with pinned
`ServerHello` or attach-frame placement. An attached Interactive winner receives an ordered,
UTF-8-safe 64 KiB preview that replaces every Unicode control except LF and TAB. Its explicit
Control-mode recovery notice reflects the product decision not to promise exact Interactive
recovery of the retained 1 MiB vector. The startup view alone uses a pinned Ghostty 64 MiB byte
history cap; ordinary output retains its 100,000-byte setting. Attached delivery commits only after
`Attached`, the diagnostic, resync, and mux options remain admitted; admission failure retains the
causes and retires only the startup actor by exact ID. Replayed runtime failures
retain encounter order, use the
invoking client's error channel, set the Command status and parser-owned Control status, capitalize
the attached warning, and continue later physical lines through an outer source. The config parser
group separately retains tmux's first-error file abort and its unusual tilde expansion immediately
after a closing quote.
The `source-file.flags` slice is closed through the existing effect and replay-loader seam. One
ordinary invocation parses every declared and globbed match before replay. A bare assignment in an
earlier file applies during parsing, affects a later file's conditional, and persists. A replayed
`set-environment` runs after the later file was parsed, so it cannot change that branch but persists
after replay. `-n` applies neither assignment nor command effects, later parse-only files see the
assignment as absent, and `-v` still reports the selected branch. `-t` resolves one pane context for path formats and replay, with a quiet empty context on a
miss and no change to the invoking client cwd. `-v` preserves file and line order, inherits through
nested sources, and stays suppressed for Control. Full tmux command, flag, and arity validation
during parse remains under the parser, error-shape, and chain-abort groups. Command and Interactive
transcript presentation and ordering are closed under `config.replayed-command-output`. Protocol
v79 closes the TUI output view's local keyboard contract: live copy tables, line and page movement,
search editing and repetition, selection-to-paste-buffer, and vi/emacs exits. Mouse, OS clipboard,
ordinary TUI pane copy-search editing, and the wider 29-action vocabulary stay outside that closure.
Runtime `source-file` now treats the active default config like every other matched path: one
invocation parses all matches in declared-path and glob order, then replays them in the same order.
Declared default, after, and default files apply as `DAD`; a loud miss
returns status 1 without stopping later matches; and ordinary diagnostics plus `-v` lines retain
declared path and match order. Explicit native `reload-config`, startup first-existing discovery,
and ordered explicit `-f` roots keep their separate behavior. Parse-only and nested paths are
unchanged. Focused CLI and daemon tests, strict clippy, fmt, and the 12-step diagnostics, 40-step
format, and then-six-step Control differentials pass with zero differences and no skips. The later
Control return-status close grows that focused row to eight. Neither focused run refreshes the stored
canonical row, which remains at three steps.
Control source diagnostics now use the existing Error kind and reach standalone `%error` frames
without text classification. Config summaries still use Warning events, so
`control-mode.diagnostic-typing` retains only future or localized config wording.
The new client accepts the old daemon's known source Warning families. The reverse version mix can
hide source diagnostics because the old client ignores Error events; downgrading the app requires a
matching daemon restart.
Byte-preserving non-UTF-8 Unix cwd transport remains separately visible under
`clients.path-encoding` instead of making such a path a connection failure.

Keep the remaining client work split by the state it needs. Session cwd, requested client flags,
attached resize aggregation, bounded UTF-8 environment seeding and refresh, and retained client
format facts have shipped.
`detach-client -E` and the
parent-HUP trio (`attach-session -x`, attaching `new-session -X`, `detach-client -P`) need typed
client exit actions and are not prerequisites for targeting or ordinary detach. Creation-time
`new-session -e/-E` works without a wire change: explicit overlays persist and reach the first pane,
while `-E` suppresses client-sourced update seeding. Existing-session `new-session -A -E` and
`attach-session -E` now preserve the destination session environment.
The separate `new-window`/`split-window` pane-local `-e` and empty-pane `-E` pair belongs to the
daemon-owned spawn effect and has shipped.

## Confirmed terminal-owned blockers

Two small-looking flags are not mux-only work:

- `clear-history -H` makes the pin clear normal history and reset the active screen's hyperlink
  registry. zz's current clear action emits ED3 and clears copy, search, selection, hover, and view
  state, but the terminal API exposes no distinct VT hyperlink-registry reset. This needs one
  terminal-owned mutation action before the mux can accept the flag honestly.
- `resize-pane -T` is not a layout resize. The pin no-ops in an active pane mode; otherwise it
  removes a cursor-derived, history-capped number of history rows, advances the cursor by the same
  amount, and redraws. The mux owns neither live cursor/history state nor terminal mode, and the
  terminal API has no atomic action for the operation. It needs a terminal-owned action and result
  contract, not another layout branch.

Requested flag retention and attached sizing are closed. The remaining attach work keeps
environment refresh and client exit actions as separate contracts; per-client active panes and
destruction fallback are separate consumers of the retained flags. Reuse existing facts within
each contract, but do not make one an artificial dependency of the others.

## Milestone 5: decide whether streams earn their cost

Only after the practical alias gate is green, measure demand for binary stdin/stdout and lock
processes. If required, design one bounded command-stream channel for `display-message -I`,
`split-window -I`, `load-buffer -`, `save-buffer -`, and `source-file -`. Do not build five bespoke
transports.

# Native GUI command direction

The current surface already has the right shape. Improve it by composition:

```sh
# tmux-compatible terminal split
zz split-window -h

# native picker and browser split
zz split-picker -h
zz split-browser -h https://example.com

# materialize a pending pane explicitly
zz select-pane-kind -t %7 agent

# control existing native panes
zz agent-send -t %7 --submit 'review the failing test'
zz set-browser-url -t %8 https://example.com/docs
zz capture-browser -t %8 -o /tmp/browser.png
```

If direct agent creation becomes necessary, prefer one thin `split-agent` command that lowers into
the same pane-kind operation as the picker. Do not give `split-window` an agent flag.

# What stays intentionally different

- `%` and `"` may open `split-picker` in the zz default table.
- `s` and `w` may focus the native sidebar instead of drawing tmux's tree.
- Prompts, menus, popups, copy mode, status, and choosers use native presentation.
- Current window may remain per client rather than per session.
- GUI defaults may keep the persistent-daemon lifecycle until a config explicitly selects tmux
  lifecycle behavior.

Each difference needs a stable registry ID. Use the divergence matrix for detailed rationale and
probe evidence. Imported tmux commands still keep their tmux semantics.

# Permanent exclusions

- `link-window`, `unlink-window`, and grouped `new-session -t`.
- Speaking tmux's private socket protocol.
- Fleet broadcast as a special command; compose `fleet list -F` with a shell loop.

`server-access` and multi-user socket ACLs are parked outside the practical alias target, but no
permanent product decision has been recorded for them.

# Decision log

- 2026-08-08: cell-based resize approved; host selected outside tmux target grammar.
- 2026-08-09: scriptability and TUI tiers defined; `--host` adopted as the server axis.
- 2026-08-16: target upgraded to `alias tmux=zz`; linked sessions and real socket interop excluded.
- 2026-08-20: native GUI defaults may diverge while explicit tmux commands stay exact.
- 2026-08-22: current source re-audited. The target changed from percentage completion to a written
  workload gate, with native commands kept on a separate namespace and missing work ranked by ease.
- 2026-08-23: `compat/tmux-gaps.json` became the sole live status source. Schema 3 separates product
  status from `depends_on` ordering, keeps manifest-owned freshness and closed history, and expands
  the pinned oracle to flag arities, positional bounds, global formats, selected context formats,
  and the existing command, option, hook, and default-binding inventories. The only missing name in
  the selected context rosters is `command`; custom `args_parse` callbacks and open-ended context
  names remain semantic work. The generated report owns the work queue; this roadmap keeps dated
  sequencing and delivery evidence.
- 2026-08-26: agent-backbone superset surface landed as additive daemon verbs and hook-seam formats:
  `agent-send --wait` (request/reply parked on the host's turn waiter), `show-last-output` (read
  twin of `send-last-output`), `#{pane_kind}` and `#{@name}` through `DaemonFormatHooks` rather than
  the pinned format table, and user-option writes signalling `wait-for` `<name>@<target>`. No tmux
  verb changed meaning and nothing new appears in unadorned `-C`/`-CC` output. Phase 2 added
  `send-text` (paste, wait for the echo, then Enter — the only multiplexer-level fix for the
  paste/Enter race every tmux orchestrator works around) and the `@option-changed` user hook;
  then agent panes learned to speak bytes: a PTY-free shadow terminal per Agent pane fed with the
  transcript projection ([design](/designs/agent-pane-projection.md)), so `capture-pane`,
  `show-last-output`, `pipe-pane`, and the alerts work on `%agent` — excluded from client frames
  and from control-mode `%output`.
- 2026-08-24: `send-keys` adopted tmux's two parser boundaries. The command parser rejects outer
  `-C`, `-P`, and `-o`; the window-copy parser recognizes them on their action-specific tables.
  Invalid local syntax stays silent and resets the copy-mode repeat prefix. Four unimplemented
  `copy-line*` actions and the parser-failure redraw remain under `terminal.key-control`.
- 2026-08-24: `list-keys` completed its remaining grammar and presentation surface. Sorting and key
  filtering precede `-1`, facts follow truncation, Interactive clients receive a timed frozen status,
  and Command and Control clients keep stdout. Stock copy tables now expose the pin's zero repeat
  metadata without changing runtime copy repetition. zz uses a documented total order where the
  pin's truncated comparator is non-total.
- 2026-08-25: Oracle schema 4 added a fail-closed inventory for custom command argument callbacks.
  Nine callback bodies reduce to six rules across 14 commands. The protocol catalog mirrors the 12
  implemented commands, while `COMMAND_ARGS_PARSE_BEHAVES` and command-specific tracker items record
  behavior adoption. The two unimplemented callback commands keep their command-level gaps.
- 2026-08-25: vi numeric counts moved onto one flat protocol-v75 terminal action. The first `send`
  or `send-keys` command whose option prefix contains `-X` consumes the count. Its stored `-N` wins;
  otherwise zz inserts separate `-N <count>` arguments before the option argument containing `-X`.
  A list with no qualifying `-X` preserves the pending value. Raw terminal sends stop on
  backpressure. Native browser sends clamp repeats to 9,999 on both sides of the wire.
- 2026-08-26: Protocol v77 renamed tail-tag-47 `SourcedCommandGuard` in place to
  `ControlCommandGuard`, adding explicit frame flags and an independent `sticky_failure` bit.
  Immediate `after-*` and `command-error` hooks now retain the originating Control recipient at flags
  0 without copying parser replay state. Hook arrays, source descendants, failures, unknown commands,
  alias resolution, and status retention match the pin. Background inserted frames and raw matched
  hook-source read placement remained separate at that checkpoint.
- 2026-08-26: Protocol v78 appended `ControlSourceFile` at event tail tag 48. Typed `ReadError`
  events render as raw unframed lines after parser flags-1 or immediate-hook flags-0 source guards
  and retain status 1. Invisible `Complete` events consume one command number after every
  depth-admitted invocation's descendants. Depth refusals and dispatch-time syntax, arity, and flag
  rejections consume none. Invalid UTF-8, source stdin transport, parser abort, hook cwd, deferred
  event hooks, and hard-disconnect queue cancellation retain separate gaps.
- 2026-08-26: Protocol v79 added a nonzero actor ID to every real command-output frame and close,
  with zero plus no viewport reserved for an authoritative no-output resync. The client watermark
  rejects stale traffic. TUI search and resize state now belong to the actor, and the local attached
  fixture closes keyboard navigation over a 96-line output on both mode-key tables. At that
  checkpoint, startup config cause delivery became the next ease-ranked slice.
- 2026-08-26: Protocol v80 appended `StartupConfigCauses` at event tail tag 49. The post-spawn
  Control owner and late attach path now match pinned placement. Attached Interactive clients
  receive an ordered, control-sanitized 64 KiB preview rather than exact recovery of the retained
  1 MiB vector. Startup parsing, ordering, completion-line locations, one-shot admission, and
  restart behavior pass the checksum-attested seven-case pinned probe.
- 2026-08-26: Protocol v81 appended `ControlCommandOutput` at event tail tag 50. Targetless and
  invalid-target foreground shell output now follows its direct or sourced guard as raw text for
  the exact Control recipient without changing retval. Resolved-target and background output use
  zz's native attached-viewer command-output surface. The strict 12-step pinned Control differential
  has no differences or skips.
- 2026-08-26: The full Alert cohort closed on the existing timed-message protocol. Bell, Activity,
  and Silence now share per-client identity, exact bounded `<client> message: <text>` logging,
  replacement, expiry, zero-duration, input dismissal, terminal-publication freeze, and
  full-viewport thaw with ordinary status messages. Repair requests, resync, and popup viewports
  obey the same gate. Control remains outside alert message delivery
  and logging. The terminal publishes one reliable Bell event per occurrence while the mux owns the
  visible flag, closing repeated pre-visit delivery. The attached PTY fixture passed the 1,500 ms
  sticky, 5,000 ms alert, 1.8-second freeze, repeated same-pane Bell, 5.2-second stale-timer drain,
  and zero-duration sequence on zz and pinned tmux. zz keeps identity-safe timer cancellation.
- 2026-08-27: The ninth attach-context slice closed `clients.attach-sizing` without a protocol or
  snapshot-schema change. Valueless `resize-window -A` and `-a` perform one-shot componentwise
  largest or smallest client aggregation and store the result as a durable manual extent. The
  global ignore-size fallback, explicit Control ceilings, `default-size` fallback, and final
  10,000-cell clamp match the pinned contract; later client updates leave manual geometry frozen.
  The expanded attached differential passes. The canonical corpus remains the requested-flags
  checkpoint's 84 scenarios and 1,475 steps with SHA-256
  `5de67222bc2ebb99c57963be14c865ddfdddc387da34ee32dd86962cef8336c9`.
- 2026-08-27: Protocol v82 closed `clients.attach-environment` for bounded UTF-8 environments.
  `ClientHello` now carries one per-connection snapshot for local and SSH-forwarded clients. Fresh sessions,
  existing attach, native attach, Control attach, and targeted switch apply the pin's effective
  `update-environment`, wildcard, missing, empty, hidden, `-A`, `-e`, `-E`, and `-T` rules. Values
  persist in the session after disconnect; future panes see updates and existing processes do not.
  Non-UTF-8 Unix entries remain under `clients.path-encoding`. The full attached differential passes
  for zz and pinned tmux.
- 2026-08-27: Protocol v83 closed `clients.context-formats`. `ClientHello.process_id` supplies the
  last missing process fact. The daemon retains creation, activity, focus, flags, key table,
  attachment, terminal, environment, geometry, and mailbox counters, then expands the same record
  through list, ordinary command, foreground inserted-command, status, and display contexts.
  Defined Interactive and Control empties match the pin. The full attached differential passes for
  zz and pinned tmux, including an attached key binding and implicit `display-message` selection.
- 2026-08-27: The positional-maximum slice closed eight `mux.error-shapes` items without a protocol
  change. Seven commands now accept at most one positional and `select-pane` accepts none. The shared
  catalog emits the pin's exact canonical error before target resolution, buffer mutation, or file
  I/O. Focused mux and daemon tests cover canonical and alias routes; the three-step differential is
  clean. At that checkpoint required positional bounds and the broad arity, flag, and nested-session
  families remained open.
- 2026-08-27: The positional-minimum slice closed the remaining fourteen positional-bound items
  without a protocol change. One exact catalog sidecar supplies minimum one for thirteen commands
  and minimum two for `if-shell`; mux, shared daemon, menu, and confirmation parsers validate it
  after flags but before upper bounds, targets, callbacks, files, buffers, or other effects. The
  three-step differential covers all fourteen canonical names and aliases with exact canonical
  errors and unchanged state. Integration validation also restored Control `%session-changed`
  delivery by keeping hook command variables client-only while adding session identity to the
  Control publication copy and using one tmux-facing client-name ladder for Control snapshot self
  identity.
  At that checkpoint shared arity, flag diagnostics, and nested-session precedence remained open.
- 2026-08-27: The shared command-arity slice removed the partial maximum roster without a protocol
  change. All 72 implemented finite upstream commands now validate the catalog maximum after flags
  and minima but before targets or effects. Stored binding and hook children use the same bounds
  before replacing state. The strict fixture covers 71 generic-CLI-routed canonical names
  and 62 aliases, while an exhaustive daemon test covers all 72 engine paths and aliases. Flag
  At that checkpoint, flag diagnostics and nested-session precedence remained open.
- 2026-08-27: The complete CLI binary and app-library gates exposed stale assertions and two production edges
  after the client-context work. Exact native `attach-session -E` now enters daemon command
  execution for its initial attach and preserves the session environment; automatic reconnect
  behavior is unchanged. The shared attached-client selector accepts the exact published
  `#{client_name}`, including the `client-PID` fallback used by nameless Control clients. Control
  menu targeting and nonself detach therefore consume the same identity that `list-clients`
  reports. Client-flag assertions now accept the pinned `focused` and `UTF-8` facts while preserving
  the deliberate `-r`/`ignore-size` difference, and Control height remains intentionally empty.
  The command palette follows the catalog's zero positional maximum for `select-pane` and offers
  live pane targets only after `-t`. The complete CLI binary and app-library suites pass all 102
  and 639 tests.
- 2026-08-28: The shared command-flag slice replaced the partial daemon roster with one
  catalog-driven parser across all 83 implemented upstream commands and 74 built-in aliases. Mux
  execution, daemon preflight, stored binding and hook children, and exact native attach now agree
  on canonical unknown and invalid flags, pinned help usage, missing required values, required-value
  absorption, optional-value lookahead, and syntax-before-unsupported ordering. Product usage stays
  truthful in `list-commands` and completion through a separate pinned diagnostic accessor. The
  strict three-step fixture reports `COMMAND_FLAG_ERRORS=clean:516` on zz and the pin. At that
  checkpoint, parser-group atomicity, callback-specific grammar, and nested `new-session`
  precedence remained under their existing owners. No wire protocol or version change was needed.
- 2026-08-28: The final `mux.error-shapes` item closed without a protocol or catalog change. A
  mutation-free nested `new-session` preflight now follows the pin through flag and arity parsing,
  target conflicts, expanded window and session names, `-A`, duplicate detection, unresolved
  session-group name validation, and start-directory expansion before refusing nesting. The
  refusal still precedes terminal and size validation. A narrow `-t` routing path exposes this
  order without implementing session groups. Canonical names, built-in aliases, prefixes, user
  aliases, command lists, detached creation, Control clients, and already-attached clients are
  covered by focused tests and the real attached-client differential.
- 2026-08-28: Protocol v84 closed `tracker.args-parse-if-shell`, the first custom callback runtime
  rule. `CommandInvocation` carries zero-based unquoted command-block positions through source-file
  and Control parsing, postcard transport, aliases, bindings, hooks, and stored printing. The
  shared validator accepts typed true and false branches, rejects typed conditions, option values,
  and extra positionals before effects, and keeps quoted braces on the string path. The strict
  three-step scenario finishes `ARGS_PARSE_IF_SHELL=clean:12` on zz and the pin. Five callback rules
  across 11 implemented commands remain.
- 2026-08-28: The existing protocol v84 metadata closed `tracker.args-parse-run-shell` without a
  wire change. A leading `-C`, including combined forms, accepts typed blocks in every positional;
  without it every positional and every option value remains a string. Option scanning stops at
  the first positional or `--`. Under `-C`, only positional 0 executes and valid extras are
  accepted and ignored. Exact failures preserve stored bindings and hooks. The strict three-step scenario runs
  21 source-file and Control checks and finishes `ARGS_PARSE_RUN_SHELL=clean:21` on zz and the pin.
  Four callback rules across 10 implemented commands remain.
- 2026-08-28: The existing protocol v84 metadata closed `tracker.args-parse-set-option` without a
  wire change. `set-option` and `set-window-option` accept typed command blocks only at positional
  1. Option names, option values attached to flags, and extra positionals remain strings, with exact
  type failures preceding arity, target lookup, and effects. Recursive typed printing preserves
  same-line and physical-line command groups before `-F` expansion; empty blocks become empty
  values and quoted braces stay literal. The strict three-step scenario runs 21 source-file and
  Control checks and finishes `ARGS_PARSE_SET_OPTION=clean:21` on zz and the pin. Three callback
  rules across eight implemented commands remain.

# Related

- [live tmux compatibility gaps](/tmux/gaps.md)
- [tmux CLI compatibility audit](/research/2026-08-22-tmux-cli-compatibility-audit.md)
- [tmux compatibility philosophy](/tmux/tmux-compat.md)
- [tmux divergence matrix](/tmux/divergences.md)
- [tmux drop-in plan](/designs/tmux-drop-in.md)
- [tmux commands](/tmux/commands.md)
- [key tables](/tmux/key-tables.md)
- [compatibility harness](/playbooks/compat-harness.md)
- [fleet attach](/designs/fleet-attach.md)
- [TUI client](/designs/tui-client.md)
