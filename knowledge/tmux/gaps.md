---
type: Reference
title: tmux compatibility gap report
description: "Live TODO and status report for tmux compatibility gaps, decisions, evidence, and acceptance gates."
resource: compat/tmux-gaps.json
tags: [tmux, compatibility, gaps, tracker]
timestamp: 2026-08-26T00:00:00-03:00
---

# Overview

> `compat/tmux-tracker.py write-report` generates this file. Edit the registry instead.

`compat/tmux-gaps.json` owns the backlog. The compatibility gate checks IDs, decisions,
dependencies, evidence paths, known scenarios, and the source-backed inventories described
below.

Pinned tmux commit: `d77c9dc6aa021e4bc61f0da128c591af695e6466`.

Tracked gap groups: **93**. Classified items: **666**.

- Status: open: 50, blocked: 22, accepted: 21.
- Decision: adopt: 58, native: 15, park: 14, never: 6.
- Priority: next: 9, later: 63, none: 21.
- Closed history entries: 62.
- Surface: command: 9, flag: 76, positional-min: 14, positional-max: 8, args-parse: 12, native-command: 19, option: 75, format: 101, hook: 10, key: 110, binding: 51, native-key: 58, semantic: 111, presentation: 9, protocol: 3.

## Measured surface

The pinned oracle contains 92 commands, 78 aliases, 572 command-flag shapes (318 valueless, 246 required-value, 8 optional-value), positional minimum and maximum bounds, 180 options, 198 global formats, 14 selected context formats, 68 hooks, and 303 default bindings across 5 tables. zz has catalog entries for 83 of those commands. The registry classifies 76 catalogued-unsupported upstream flag pairs, 0 implemented flag-arity mismatches, 14 positional-minimum mismatches, 8 positional-maximum mismatches, 14 callback-bearing commands across 6 effective `args_parse` rules, 12 implemented commands without verified callback behavior, 0 zz-only flags on tmux command names, 19 native command names, 75 options absent from `BEHAVES`, 101 known limited formats, 0 selected context-format gaps, 0 zz-only selected context-format names, 10 currently documented hook-producer gaps, 110 omitted default keys, 51 divergent shared default bindings, 58 zz-only default keys.

## Enforcement boundary

The gate reconciles command names, aliases, flag arities, positional bounds, custom
`args_parse` rules, option names, global and selected context-format names, hook names,
and default key presence against the clean pinned tmux source and binary. It also reconciles
options absent from `BEHAVES`, constant-backed formats against the live registry, omitted
and zz-only default keys against zz's key tables, rendered commands plus repeat bits for
shared default bindings, the native roster against catalog minus oracle, every pinned
canonical prefix against the resolver, and known scenarios against exact tuples.

These structural checks cannot prove that runtime parsing applies each inventoried `args_parse`
rule, open-ended dynamic format contexts, nonconstant format correctness, or whether a hook fires,
or that a structurally matching binding behaves identically at runtime. Differential scenarios,
attached-client fixtures, unit tests, and manual GUI checks supply that behavioral evidence. The
tracker keeps the remaining semantic discovery work explicit instead of treating matching
structure as proof.

## Next

| ID | Gap | Decision | Status | Ease | Owner | Impact | Depends on |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `clients.context-formats` | Back client format facts | adopt | blocked | medium | daemon | scripts, remote | clients.attach-context |
| `clients.event-hooks` | Produce client lifecycle hooks | adopt | blocked | medium | daemon | scripts, remote | clients.attach-context |
| `clients.tui-command-output-navigation` | Route TUI command-output navigation | adopt | open | medium | client | daily, remote | none |
| `config.startup-diagnostic-delivery` | Deliver retained startup configuration causes | adopt | open | medium | client | daily, scripts | none |
| `mux.error-shapes` | Match remaining command errors | adopt | open | medium | protocol | scripts | none |
| `tracker.semantic-coverage` | Close the remaining semantic discovery blind spots | adopt | open | medium | protocol | scripts | none |
| `clients.attach-context` | Complete attach cwd, flags, and sizing | adopt | open | hard | protocol | daily, scripts, remote | none |
| `clients.attach-environment` | Seed and refresh client environments | adopt | open | hard | protocol | scripts, remote | none |
| `keys.copy-mode-binding-fidelity` | Match shared copy-mode binding commands | adopt | open | hard | protocol | daily, remote, scripts | copy-mode.command-fidelity |

## Later

| ID | Gap | Decision | Status | Ease | Owner | Impact | Depends on |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `alerts.message-lifecycle` | Unify alert message lifecycle | adopt | open | medium | daemon | daily | none |
| `aliases.command-bodies` | Support multi-command aliases | adopt | open | medium | mux | scripts | none |
| `aliases.config-parse-unit` | Prepare config aliases as one parse unit | adopt | open | medium | mux | scripts | none |
| `buffers.clipboard-write` | Honor buffer clipboard writes | adopt | open | medium | daemon | scripts, remote | none |
| `choosers.command-flags` | Complete chooser command controls | adopt | open | medium | daemon | daily, scripts | none |
| `clients.path-encoding` | Preserve non-UTF-8 client paths | adopt | open | medium | protocol | scripts | none |
| `config.parser-edge-cases` | Close config parser edge cases | adopt | open | medium | mux | scripts | none |
| `control-mode.async-command-output` | Place asynchronous command diagnostics | adopt | open | medium | client | scripts | none |
| `control-mode.async-copy-pipe-errors` | Frame asynchronous copy-pipe errors for Control | adopt | open | medium | client | scripts | none |
| `control-mode.diagnostic-typing` | Type Control-mode config diagnostics | adopt | open | medium | protocol | scripts | none |
| `control-mode.hook-source-read-diagnostics` | Match Control source read placement and completion numbering | adopt | open | medium | protocol | scripts | none |
| `display-message.format-listing` | List display-message format variables | adopt | open | medium | daemon | scripts, admin | clients.attach-context, formats.mouse-context, formats.pane-process, formats.pane-runtime, formats.session-runtime, formats.terminal-cells, formats.terminal-runtime, formats.window-runtime |
| `display-message.pane-target-grammar` | Complete display-message pane target grammar | adopt | open | medium | mux | scripts | none |
| `formats.session-runtime` | Expose client-derived session formats | adopt | open | medium | protocol | scripts | clients.attach-context |
| `formats.window-runtime` | Expose remaining window formats | adopt | open | medium | daemon | scripts, remote | clients.attach-context |
| `history.hyperlink-reset` | Reset hyperlink history | adopt | blocked | medium | terminal | daily | none |
| `hooks.queue` | Produce after-queue hooks | adopt | open | medium | daemon | scripts | none |
| `keys.copy-mode-prompt-defaults` | Add prompt-backed emacs copy-mode defaults | adopt | open | medium | daemon | daily, remote, scripts | prompt.command-fidelity |
| `keys.copy-mode-unsupported-default-actions` | Implement missing stock copy-mode actions | adopt | open | medium | terminal | daily, remote | copy-mode.action-fidelity |
| `mux.chain-parse-abort` | Abort invalid command groups before effects | adopt | open | medium | mux | scripts | none |
| `options.option-name-format-coverage` | Complete option-name format coverage | adopt | open | medium | mux | scripts | none |
| `options.pane-chrome` | Consume pane chrome options | adopt | open | medium | client | daily, gui | none |
| `options.theme-palette` | Map tmux theme palette options | park | blocked | medium | client | gui | none |
| `pane.break-geometry` | Complete break-pane placement | adopt | open | medium | mux | scripts, daily | none |
| `pane.spawn-flags` | Complete split-window placement flags | adopt | open | medium | mux | scripts, daily | none |
| `rendering.geometry-residue` | Close bounded geometry reporting gaps | adopt | open | medium | client | scripts, gui | clients.attach-context |
| `terminal.key-control` | Complete terminal key control flags | adopt | open | medium | terminal | scripts, daily | none |
| `terminal.resize-pane-trim` | Add terminal history trim action | adopt | blocked | medium | terminal | daily, scripts | none |
| `aliases.remote-client-preflight` | Prepare remote CLI aliases without starting SSH | adopt | open | hard | client | remote, scripts | none |
| `buffers.client-file-context` | Route buffer files through client path context | adopt | open | hard | protocol | scripts, remote | clients.attach-context |
| `clients.detach-exec` | Execute a command after detaching a client | adopt | open | hard | protocol | scripts, remote | none |
| `clients.interactive-refresh` | Complete interactive client commands | park | blocked | hard | client | remote | clients.attach-context |
| `clients.parent-hup-exit` | Signal client parents after forced detach | adopt | open | hard | protocol | scripts, remote | none |
| `config.non-utf8-file-bytes` | Match config-file byte parsing | adopt | open | hard | mux | scripts | none |
| `control-mode.disconnect-cancels-command-queue` | Cancel client-owned Control queues after connection loss | adopt | open | hard | daemon | scripts | none |
| `copy-mode.action-fidelity` | Complete the copy-mode action vocabulary | adopt | open | hard | terminal | daily, remote, scripts | none |
| `copy-mode.command-fidelity` | Complete copy-mode command fidelity | adopt | open | hard | client | daily, remote | clients.interactive-refresh |
| `display-message.mouse-target-context` | Resolve display-message mouse targets | adopt | blocked | hard | mux | scripts, gui | mouse.bound-context |
| `display-message.verbose-trace` | Trace display-message format expansion | adopt | open | hard | mux | scripts, admin | none |
| `display-panes.queue-semantics` | Wait for display-panes overlays | adopt | open | hard | daemon | scripts, daily | clients.interactive-refresh |
| `formats.mouse-context` | Expose mouse event formats | park | blocked | hard | protocol | scripts, gui | mouse.bound-context |
| `formats.pane-process` | Expose remaining pane process formats | adopt | blocked | hard | daemon | scripts | protocol.binary-streams |
| `formats.pane-runtime` | Expose pane mode formats | park | blocked | hard | client | scripts, daily | clients.interactive-refresh |
| `formats.terminal-cells` | Expose terminal cell formats | park | blocked | hard | terminal | scripts | none |
| `hooks.pane-events` | Produce pane focus and clipboard hooks | adopt | blocked | hard | daemon | scripts, gui | clients.attach-context |
| `jobs.environment` | Normalize shell job environments | adopt | open | hard | daemon | scripts, remote | clients.attach-context |
| `keys.strict-validation` | Match tmux key-name validation | adopt | open | hard | protocol | scripts, daily | none |
| `messages.tty-model` | Model tmux message and TTY reports | park | blocked | hard | daemon | admin, remote | clients.attach-context |
| `mouse.bound-context` | Carry bound mouse event context | park | blocked | hard | protocol | daily, scripts, gui | clients.attach-context |
| `options.remain-on-exit-format` | Render remain-on-exit-format in retained panes | adopt | blocked | hard | terminal | daily, scripts | none |
| `options.terminal-behavior` | Consume terminal behavior options | adopt | open | hard | terminal | daily, remote, scripts | none |
| `pane.selection-state` | Model pane selection controls | adopt | open | hard | daemon | daily, scripts | clients.attach-context |
| `prompt.command-fidelity` | Complete command-prompt semantics | adopt | open | hard | daemon | scripts, daily | clients.interactive-refresh |
| `prompt.pane-rendered` | Defer pane-rendered prompts | park | blocked | hard | client | daily | clients.interactive-refresh |
| `source-file.event-hook-client-cwd` | Select the current client cwd for event-hook sources | adopt | open | hard | daemon | scripts | clients.attach-context |
| `source-file.sourced-hook-client-cwd` | Keep the invoking client for hooks raised during source replay | adopt | open | hard | daemon | scripts | none |
| `source-file.startup-client-cwd` | Bootstrap startup sources from the initial client cwd | adopt | open | hard | protocol | scripts | none |
| `capture.rich-transports` | Add rich capture transports | park | blocked | hardest | terminal | scripts | protocol.binary-streams |
| `formats.terminal-runtime` | Expose terminal runtime formats | park | blocked | hardest | terminal | scripts | none |
| `options.lock-program` | Defer tmux lock process execution | park | blocked | hardest | client | remote, admin | clients.interactive-refresh |
| `pane.floating-model` | Defer tmux floating panes | park | blocked | hardest | mux | daily, scripts, gui | none |
| `protocol.binary-streams` | Design one bounded command stream | park | blocked | hardest | protocol | scripts, remote | none |
| `protocol.socket-acl` | Defer multi-user socket ACLs | park | blocked | hardest | daemon | admin, remote | none |

## None

| ID | Gap | Decision | Status | Ease | Owner | Impact | Depends on |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `choosers.native-presentation` | Keep native chooser presentation | native | accepted | none | gui | daily, gui | none |
| `clients.read-only-and-focus` | Retain native client focus semantics | native | accepted | none | daemon | daily, remote | none |
| `commands.native-client-tools` | Use native client tools | native | accepted | none | gui | daily, gui | none |
| `commands.native-superset` | Keep the zz-native command namespace explicit | native | accepted | none | protocol | daily, gui, scripts | none |
| `formats.native-modes` | Keep native mode row formats | native | accepted | none | client | daily, gui | none |
| `formats.session-activity-wake-lifecycle` | Keep native wake lifecycle outside session activity | native | accepted | none | daemon | remote | none |
| `keys.copy-mode-native-mouse` | Keep native copy-mode mouse handling | native | accepted | none | client | daily, gui | none |
| `keys.copy-mode-native-numeric-prefix` | Keep native vi numeric prefix capture | native | accepted | none | protocol | daily, gui, scripts | none |
| `keys.default-prefix` | Keep the native default prefix table | native | accepted | none | protocol | daily, gui | none |
| `keys.move-table` | Replace the floating-pane move table natively | native | accepted | none | gui | daily, gui | none |
| `keys.native-defaults` | Keep zz-native default key tables explicit | native | accepted | none | protocol | daily, gui | none |
| `keys.root-native-mouse` | Keep native root mouse bindings | native | accepted | none | client | daily, gui | none |
| `layout.main-horizontal-upstream-bug` | Reject stale two-pane main layout geometry | never | accepted | none | mux | daily, scripts | none |
| `layout.safety-invariants` | Keep bounded valid layouts | never | accepted | none | mux | daily, scripts | none |
| `layout.spread-mixed-upstream-bug` | Reject corrupt mixed-parent spread | never | accepted | none | mux | daily, scripts | none |
| `list-keys.deterministic-sort-ties` | Keep list-keys sorting total and deterministic | never | accepted | none | mux | scripts | none |
| `options.native-mode-styles` | Keep native mode styling | native | accepted | none | client | daily, gui | none |
| `options.native-overlay-styles` | Keep native overlay styling | native | accepted | none | client | daily, gui | none |
| `presentation.native-status` | Keep native status and lifecycle presentation | native | accepted | none | client | daily, gui, remote | none |
| `protocol.socket-interop` | Do not speak tmux private protocol | never | accepted | none | protocol | admin | none |
| `sessions.linked-groups` | Do not add linked session groups | never | accepted | none | mux | scripts, admin | none |

## Gap details

### `alerts.message-lifecycle`: Unify alert message lifecycle

Alert notifications still publish client-timed TimedClientMessage events directly, so they do not create ActiveClientMessage records, freeze terminal publication as pinned alerts do, share daemon expiry and input dismissal, or reset the pin's sticky message-ignore-keys state.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `daemon`
- User impact: daily
- Items: `semantic:alert-message-lifecycle`
- Depends on: none
- Evidence:
  - `resource:crates/zz-daemon/src/daemon.rs`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Alert-produced status messages use the daemon-owned timed message lifecycle with the pin's frozen terminal publication, replacement, expiry, zero-duration, and input-dismissal semantics.`
  - `A positive alert delay resets the active message's ignore-keys state so alerts remain dismissible after display-message -N.`

### `aliases.command-bodies`: Support multi-command aliases

The typed resolver now refuses matched empty, multi-command, and unparsable bodies without falling through, but the current dispatch chokepoint still executes only one command per supported alias.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `mux`
- User impact: scripts
- Items: `semantic:command-alias-multi-body`
- Depends on: none
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Alias tests cover multi-command and empty bodies with caller arguments appended to the final command.`

### `aliases.config-parse-unit`: Prepare config aliases as one parse unit

Writable config and source-file replay resolves aliases immediately before each daemon dispatch, so an earlier command can change the alias observed by a later command from the same parsed group.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `mux`
- User impact: scripts
- Items: `semantic:command-alias-config-parse-unit`
- Depends on: none
- Evidence:
  - `resource:crates/zz-mux/src/command.rs`
  - `resource:crates/zz-daemon/src/daemon.rs`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Each config or source-file command group expands aliases from one parse-time snapshot before any command in that group executes, without changing source context or nested-file limits.`

### `aliases.remote-client-preflight`: Prepare remote CLI aliases without starting SSH

The local CLI now prepares against an already-running compatible daemon, but --host intentionally retains static routing because the only current SSH forward constructor starts a new lifecycle.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `hard`
- Owner: `client`
- User impact: remote, scripts
- Items: `semantic:command-alias-remote-client-preflight`
- Depends on: none
- Evidence:
  - `resource:crates/zz/src/lib.rs`
  - `resource:crates/zz-daemon/src/endpoint.rs`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `An existing-only remote daemon discovery and forwarding path prepares a whole CLI vector without starting SSH or a daemon merely to classify it, then preserves that immutable vector across command and TUI handoff.`

### `buffers.client-file-context`: Route buffer files through client path context

zz expands and accesses buffer paths in the persistent daemon, while tmux routes file access through the invoking client and selects that client's command cwd or attached session cwd.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `hard`
- Owner: `protocol`
- User impact: scripts, remote
- Items: `semantic:load-buffer-client-path-context`, `semantic:save-buffer-client-path-context`
- Depends on: `clients.attach-context`
- Evidence:
  - `resource:crates/zz-daemon/src/daemon.rs`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Relative load-buffer and save-buffer paths use the pin's invoking command-client cwd or attached session cwd, and remote clients move bytes without assuming a shared daemon filesystem.`

### `buffers.clipboard-write`: Honor buffer clipboard writes

The buffer model exists, but clipboard delivery needs a client capability path.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `daemon`
- User impact: scripts, remote
- Items: `flag:load-buffer:-w`, `flag:set-buffer:-w`
- Depends on: none
- Evidence:
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Both flags match the pin across clipboard-enabled and clipboard-disabled clients.`

### `capture.rich-transports`: Add rich capture transports

The retained UTF-8 text snapshot cannot represent the pin's richer grid and byte views.

- Decision: `park`
- Status: `blocked`
- Priority and ease: `later` / `hardest`
- Owner: `terminal`
- User impact: scripts
- Items: `flag:capture-pane:-C`, `flag:capture-pane:-F`, `flag:capture-pane:-H`, `flag:capture-pane:-L`, `flag:capture-pane:-P`, `flag:capture-pane:-R`, `semantic:capture-pane-saved-alternate`, `semantic:capture-pane-trailing-blank-rows`
- Depends on: `protocol.binary-streams`
- Evidence:
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `resource:knowledge/tmux/divergences.md`
  - `scenario:compat/scenarios/capture-pane.txt`
- Acceptance:
  - `A retained terminal snapshot exposes alternate grids, raw bytes, hyperlinks, line flags, line numbers, and blank viewport rows without approximations.`

### `choosers.command-flags`: Complete chooser command controls

The native chooser already owns rows and actions; the remaining controls need exact command semantics.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `daemon`
- User impact: daily, scripts
- Items: `flag:choose-buffer:-F`, `flag:choose-buffer:-k`, `flag:choose-buffer:-y`, `flag:choose-tree:-F`, `flag:choose-tree:-h`, `flag:choose-tree:-k`, `flag:choose-tree:-y`, `semantic:chooser-key-vocabulary`
- Depends on: none
- Evidence:
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `resource:knowledge/tmux/divergences.md`
  - `file:compat/attached-client.sh`
- Acceptance:
  - `Attached-client tests cover chooser formatting, key actions, and zz-deliverable key names.`

### `choosers.native-presentation`: Keep native chooser presentation

zz uses native chooser surfaces and the sidebar instead of tmux mode screens.

- Decision: `native`
- Status: `accepted`
- Priority and ease: `none` / `none`
- Owner: `gui`
- User impact: daily, gui
- Items: `presentation:chooser-no-preview`, `presentation:find-window-native-chooser`
- Depends on: none
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Native choosers retain tmux command and keyboard semantics without drawing tmux's preview grid.`

### `clients.attach-context`: Complete attach cwd, flags, and sizing

The command-client cwd slice established the protocol pattern, but attached session cwd, client flags, and multi-client sizing still need durable state. Environment refresh, exit actions, and client targeting are tracked independently.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `next` / `hard`
- Owner: `protocol`
- User impact: daily, scripts, remote
- Items: `flag:attach-session:-c`, `flag:attach-session:-f`, `flag:new-session:-f`, `flag:resize-window:-A`, `flag:resize-window:-a`, `protocol:client-attach-context`, `semantic:resize-window-client-sizes`, `semantic:source-file-attached-session-cwd`
- Depends on: none
- Evidence:
  - `resource:knowledge/designs/tmux-superset-roadmap.md`
  - `resource:knowledge/tmux/divergences.md`
  - `file:compat/attached-client.sh`
- Acceptance:
  - `Attach and attaching new-session carry client flags, attach -c updates the selected session cwd in target context, resize-window -A/-a consume retained client sizes, and attached-client tests exercise the same facts.`

### `clients.attach-environment`: Seed and refresh client environments

zz seeds sessions from the daemon environment and retains no per-client environment, so creation and attach-time refresh cannot yet follow the invoking client.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `next` / `hard`
- Owner: `protocol`
- User impact: scripts, remote
- Items: `flag:attach-session:-E`, `semantic:client-environment-seeding`, `semantic:switch-client-environment-refresh`
- Depends on: none
- Evidence:
  - `resource:knowledge/designs/tmux-superset-roadmap.md`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Local and remote clients carry a bounded environment snapshot; new-session and attach-session seed the selected update-environment names, attach-session -E suppresses reseeding, and switch-client refreshes the target session from the selected client.`

### `clients.context-formats`: Back client format facts

Several facts work only in list or recipient contexts and the rest lack retained client metadata.

- Decision: `adopt`
- Status: `blocked`
- Priority and ease: `next` / `medium`
- Owner: `daemon`
- User impact: scripts, remote
- Items: `format:client_activity`, `format:client_cell_height`, `format:client_cell_width`, `format:client_colours`, `format:client_control_mode`, `format:client_created`, `format:client_discarded`, `format:client_flags`, `format:client_height`, `format:client_key_table`, `format:client_last_session`, `format:client_name`, `format:client_pid`, `format:client_prefix`, `format:client_readonly`, `format:client_session`, `format:client_termfeatures`, `format:client_termname`, `format:client_termtype`, `format:client_theme`, `format:client_tty`, `format:client_uid`, `format:client_user`, `format:client_utf8`, `format:client_width`, `format:client_written`, `format:session_last_attached`
- Depends on: `clients.attach-context`
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Client, status, and ordinary target contexts expose the pin's retained client facts with defined empty behavior.`

### `clients.detach-exec`: Execute a command after detaching a client

The daemon can select and detach clients, but it has no typed client-exit action that asks the presentation process to exec a shell command.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `hard`
- Owner: `protocol`
- User impact: scripts, remote
- Items: `flag:detach-client:-E`, `semantic:client-exit-exec`
- Depends on: none
- Evidence:
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `resource:knowledge/designs/tmux-superset-roadmap.md`
  - `file:compat/attached-client.sh`
- Acceptance:
  - `detach-client -E selects victims with bare, -t, -a, and -s exactly as ordinary detach, then replaces each attached client with the requested shell command in that client's execution environment.`

### `clients.event-hooks`: Produce client lifecycle hooks

Storage exists; the missing producers need shared client state and transition points.

- Decision: `adopt`
- Status: `blocked`
- Priority and ease: `next` / `medium`
- Owner: `daemon`
- User impact: scripts, remote
- Items: `hook:client-active`, `hook:client-dark-theme`, `hook:client-focus-in`, `hook:client-focus-out`, `hook:client-light-theme`, `hook:client-resized`
- Depends on: `clients.attach-context`
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
  - `resource:crates/zz-mux/src/tmux_options.rs`
- Acceptance:
  - `Each retained client transition emits its hook once with the target client's format context.`

### `clients.interactive-refresh`: Complete interactive client commands

Mode state and redraw ownership span the daemon, protocol, TUI, and GUI.

- Decision: `park`
- Status: `blocked`
- Priority and ease: `later` / `hard`
- Owner: `client`
- User impact: remote
- Items: `command:switch-mode`, `semantic:copy-mode-headless-target`, `semantic:refresh-client-interactive`, `semantic:switch-mode-transition`
- Depends on: `clients.attach-context`
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
  - `resource:knowledge/designs/tmux-superset-roadmap.md`
  - `file:compat/attached-client.sh`
- Acceptance:
  - `A named workload justifies a cross-client mode and redraw contract, then attached-client tests pin it.`

### `clients.parent-hup-exit`: Signal client parents after forced detach

The existing detached event can distinguish requested and evicted clients, but the wire and TUI have no exit action that sends SIGHUP to the client process parent.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `hard`
- Owner: `protocol`
- User impact: scripts, remote
- Items: `flag:attach-session:-x`, `flag:detach-client:-P`, `flag:new-session:-X`, `semantic:client-exit-parent-hup`
- Depends on: none
- Evidence:
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `resource:knowledge/designs/tmux-superset-roadmap.md`
  - `file:compat/attached-client.sh`
- Acceptance:
  - `attach-session -x and attaching new-session -X evict peer clients with the parent-HUP exit action, detach-client -P applies it to the selected victims, and ordinary requested and stolen detach notices remain unchanged.`

### `clients.path-encoding`: Preserve non-UTF-8 client paths

Protocol v72 uses a portable UTF-8 PathBuf representation and omits unrepresentable cwd values to preserve connection availability; byte-preserving Unix paths need an explicit wire shape.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `protocol`
- User impact: scripts
- Items: `semantic:client-cwd-non-utf8`
- Depends on: none
- Evidence:
  - `resource:knowledge/protocol/wire-protocol.md`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `A local Unix client launched from a non-UTF-8 cwd still resolves relative source-file paths like the pin without weakening path handling on other platforms.`

### `clients.read-only-and-focus`: Retain native client focus semantics

Independent GUI clients and per-client sizing are core zz behavior.

- Decision: `native`
- Status: `accepted`
- Priority and ease: `none` / `none`
- Owner: `daemon`
- User impact: daily, remote
- Items: `semantic:per-client-current-window`, `semantic:read-only-ignore-size`, `semantic:read-only-same-uid`
- Depends on: none
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
  - `scenario:compat/scenarios/last-pane-input-off.txt`
- Acceptance:
  - `The divergence matrix documents per-client focus and single-user read-only policy; imported tmux command syntax keeps its meaning.`

### `clients.tui-command-output-navigation`: Route TUI command-output navigation

The daemon switches each command-output client into the pane's effective copy table, resolves ordinary Key messages there, and routes supported `send-keys -X` effects to the PTY-free output terminal. zz-tui paints that viewport with a VIEW badge, but its command-output branch sends a direct Cancel only for hard-coded Escape or q and returns for every other key. It also turns `TerminalUiCommand::BeginSearch` into an unsupported message. The current attached proof therefore covers transcript presentation and q dismissal only; it does not cover navigation, search, selection, copying, custom bindings, or mode-key selection, and hard-coded Escape gives vi mode the wrong exit behavior.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `next` / `medium`
- Owner: `client`
- User impact: daily, remote
- Items: `semantic:tui-command-output-key-routing`
- Depends on: none
- Evidence:
  - `resource:crates/zz-tui/src/input.rs`
  - `resource:crates/zz-tui/src/app.rs`
  - `resource:crates/zz-tui/src/render.rs`
  - `resource:crates/zz-daemon/src/daemon.rs`
  - `resource:crates/zz-protocol/src/key.rs`
  - `file:compat/attached-client.sh`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `While zz-tui shows command output, press and repeat keys run through the daemon's effective `copy-mode` or `copy-mode-vi` table instead of being swallowed or reaching the underlying pane. Supported line, page, history, word, prompt, selection, rectangle, and copy actions update the retained output viewport and preserve the existing clipboard and paste-buffer effects.`
  - `The effective table follows the target window's `mode-keys` value and live custom bindings. Cancellation is table-driven: stock emacs Escape and q cancel, stock vi q cancels while Escape clears the selection, and a custom q or Escape binding wins. Release events remain inert and closing the view consumes the key once.`
  - `TUI search handles `TerminalUiCommand::BeginSearch`, edits and submits the search prompt, and supports next, previous, and close actions against command output. An attached differential uses output taller than the viewport and proves line and page movement, search, selection and copy, one custom table override, and both mode-key tables. Unsupported `window-copy` actions remain under `copy-mode.action-fidelity`.`

### `commands.native-client-tools`: Use native client tools

These commands draw terminal client chrome that zz replaces with native surfaces.

- Decision: `native`
- Status: `accepted`
- Priority and ease: `none` / `none`
- Owner: `gui`
- User impact: daily, gui
- Items: `command:choose-client`, `command:clock-mode`, `command:customize-mode`, `command:suspend-client`
- Depends on: none
- Evidence:
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Native client, settings, clock, and process controls stay discoverable without claiming tmux command support.`

### `commands.native-superset`: Keep the zz-native command namespace explicit

The GUI superset needs its own names so tmux spellings can keep frozen tmux meaning.

- Decision: `native`
- Status: `accepted`
- Priority and ease: `none` / `none`
- Owner: `protocol`
- User impact: daily, gui, scripts
- Items: `native-command:agent-send`, `native-command:capture-browser`, `native-command:copy-mode-search-prompt`, `native-command:debug-marker`, `native-command:focus-sidebar`, `native-command:new-browser`, `native-command:reload-config`, `native-command:restart-agent-pane`, `native-command:select-pane-kind`, `native-command:send-last-output`, `native-command:set-agent-provider`, `native-command:set-agent-session`, `native-command:set-browser-profile`, `native-command:set-browser-tabs`, `native-command:set-browser-url`, `native-command:set-editor-path`, `native-command:split-browser`, `native-command:split-picker`, `native-command:tools`
- Depends on: none
- Evidence:
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `resource:knowledge/designs/tmux-superset-roadmap.md`
- Acceptance:
  - `Every zz-native exact command name and alias remains explicitly classified and does not equal a tmux exact spelling.`

### `config.non-utf8-file-bytes`: Match config-file byte parsing

Pinned tmux treats a measured config containing only byte 0xff as successful input with no visible error, while zz reads config through read_to_string and rejects it before parsing. The parser needs an explicit byte-input contract rather than a broader claim based on one platform-dependent byte case.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `hard`
- Owner: `mux`
- User impact: scripts
- Items: `semantic:config-file-byte-input`
- Depends on: none
- Evidence:
  - `resource:crates/zz-daemon/src/daemon.rs`
  - `resource:crates/zz-mux/src/parser.rs`
  - `resource:third_party/tmux-reference/UPSTREAM.md`
  - `resource:knowledge/references/tmux-upstream.md`
  - `resource:knowledge/tmux/conf-parser.md`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `For a config file containing only byte 0xff, direct Command sourcing exits 0 with empty streams; direct Control sourcing emits one flags-1 success guard and exits 0; and synchronous if-shell sourcing emits flags-1 parent and source success guards, no visible diagnostic, and continues the root file. zz no longer reports this measured case as `stream did not contain valid UTF-8` through a typed Error with status 1.`
  - `A pinned byte matrix covers isolated and embedded non-UTF-8 bytes, their placement, resulting commands, diagnostics, and status before zz claims general byte compatibility. The implementation neither substitutes lossy text nor treats config-content bytes as the separate source-path encoding problem.`

### `config.parser-edge-cases`: Close config parser edge cases

The normal parser path works; first-error transactionality plus the closing-quote boundary and these filesystem and account lookups remain.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `mux`
- User impact: scripts
- Items: `semantic:config-parse-abort`, `semantic:config-tilde-after-closing-quote-expansion`, `semantic:config-tilde-user-expansion`, `semantic:config-unset-home`
- Depends on: none
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
  - `scenario:compat/scenarios/smoke/config-grammar.txt`
- Acceptance:
  - `Parser probes match the pin for first-error file abort, default discovery, closing-quote tilde expansion, named-user home expansion, and missing HOME errors.`

### `config.startup-diagnostic-delivery`: Deliver retained startup configuration causes

tmux retains clientless startup causes and routes them when a client becomes available. A separate manual pinned d77c9dc6 observation established the display-versus-list distinction; the 12-step runtime scenario does not prove it. zz currently logs and drops the startup ConfigLoadReport, so the detached launch stays quiet but later Control and attached clients miss both errors and retained display output.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `next` / `medium`
- Owner: `client`
- User impact: daily, scripts
- Items: `presentation:attached-startup-config-error-view`, `semantic:config-startup-diagnostic-retention`, `semantic:control-mode-startup-config-error-order`
- Depends on: none
- Evidence:
  - `resource:crates/zz-daemon/src/daemon.rs`
  - `resource:crates/zz/src/control_mode.rs`
  - `resource:third_party/tmux-reference/UPSTREAM.md`
  - `resource:knowledge/references/tmux-upstream.md`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Startup configuration causes survive daemon initialization and reach the first eligible Control or attached client, while a detached Command start still exits 0 with empty stdout and stderr.`
  - `Initial Control writes every `%config-error <declaring-file>:<line>: <cause>` before its first `%begin`; a Control client that attaches after detached startup receives retained causes inside the attach frame; a normal attached client opens the cause view.`
  - `A separate manual probe of pinned tmux d77c9dc6, outside the 12-step runtime scenario, proves that startup `display-message -p` becomes a retained file-and-line config cause while list-style output such as `list-sessions` is discarded. Neither output reaches the detached launching command's stdout or stderr.`

### `control-mode.async-command-output`: Place asynchronous command diagnostics

Control closes nonerror command responses correctly, but an asynchronous run-shell exit diagnostic still rides inside zz's completed response instead of arriving after its %end frame.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `client`
- User impact: scripts
- Items: `semantic:control-mode-run-shell-exit-diagnostic-order`
- Depends on: none
- Evidence:
  - `resource:crates/zz/src/control_mode.rs`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Pinned Control differentials cover frame closure, unframed asynchronous diagnostics, and same-line continuation for nonzero run-shell completion.`

### `control-mode.async-copy-pipe-errors`: Frame asynchronous copy-pipe errors for Control

A copy-pipe worker can fail after the input action finishes and carries no Control request identity. zz preserves Interactive error notification and keeps Control silent instead of emitting an unsolicited standalone begin/error block until the pin's asynchronous notification contract is measured.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `client`
- User impact: scripts
- Items: `semantic:control-mode-copy-pipe-error-delivery`
- Depends on: none
- Evidence:
  - `resource:crates/zz-daemon/src/daemon.rs`
  - `resource:crates/zz/src/control_mode.rs`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `A pinned Control probe determines whether an asynchronous copy-pipe worker failure is silent, a %message notification, or command-associated output, and zz delivers that result without inventing an unsolicited command guard.`

### `control-mode.diagnostic-typing`: Type Control-mode config diagnostics

Config diagnostics remain generic Warning events that Control classifies by English text. Source-read events have closed internal Error identity, while their external raw placement and source-completion numbering remain open under control-mode.hook-source-read-diagnostics.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `protocol`
- User impact: scripts
- Items: `semantic:control-mode-typed-config-diagnostics`
- Depends on: none
- Evidence:
  - `resource:crates/zz-protocol/src/message.rs`
  - `resource:crates/zz-daemon/src/daemon.rs`
  - `resource:crates/zz/src/control_mode.rs`
- Acceptance:
  - `Typed protocol identity routes config diagnostics to %config-error independently of localized or future prose.`

### `control-mode.disconnect-cancels-command-queue`: Cancel client-owned Control queues after connection loss

zz now cancels a background if-shell or run-shell callback when its origin is gone before callback entry, but immediate hook and source replay has no connection-owned cancellation token once command execution has started. Pinned tmux frees the remaining client-owned queue on hard connection loss while letting the in-flight worker finish; graceful Return follows a different drain path.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `hard`
- Owner: `daemon`
- User impact: scripts
- Items: `semantic:control-mode-disconnect-cancels-command-queue`
- Depends on: none
- Evidence:
  - `resource:crates/zz-daemon/src/daemon.rs`
  - `resource:crates/zz/src/control_mode.rs`
  - `file:crates/zz/tests/cli_binary.rs`
  - `resource:third_party/tmux-reference/UPSTREAM.md`
  - `resource:knowledge/references/tmux-upstream.md`
  - `resource:knowledge/protocol/wire-protocol.md`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Killing the originating Control connection while an immediate hook or sourced command is blocked lets the already-started worker finish, cancels every later client-owned source command, hook command, and mutation, and delivers none of the canceled queue's output to a replacement Control client.`
  - `Graceful EOF and a blank Return keep the originating queue alive until its pending hook and source work drains, preserving output and retained status before the client exits.`

### `control-mode.hook-source-read-diagnostics`: Match Control source read placement and completion numbering

zz routes matched source read failures as typed standalone Error frames and does not model the pin's unguarded source completion callback number. Pinned tmux ends the surrounding source guard, writes the read diagnostic raw, sets retval 1, and consumes one invisible completion number per source invocation. Parser replay uses flags 1; immediate hooks use flags 0.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `protocol`
- User impact: scripts
- Items: `semantic:control-mode-hook-source-read-diagnostic-placement`, `semantic:control-mode-parser-source-read-diagnostic-placement`, `semantic:control-mode-source-completion-frame-numbering`
- Depends on: none
- Evidence:
  - `resource:crates/zz-daemon/src/daemon.rs`
  - `resource:crates/zz-protocol/src/message.rs`
  - `resource:crates/zz/src/control_mode.rs`
  - `resource:third_party/tmux-reference/UPSTREAM.md`
  - `resource:knowledge/references/tmux-upstream.md`
  - `resource:knowledge/protocol/wire-protocol.md`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `For parser-owned flags-1 and immediate-hook flags-0 sources, an actual matched OS or path read failure closes the source-file command guard with %end, writes the read diagnostic as raw unframed Control output immediately afterward, sets retained status 1, and lets later physical lines continue.`
  - `Every executed source-file invocation consumes one hidden completion callback command number at its post-descendant position, including success, no-match, parser-error, nested, and hook-owned paths, without emitting a visible frame.`
  - `Exact tests preserve platform-dependent strerror text as error: path, prove sticky EOF and blank-Return status plus multi-file read-before-replay order, and exclude invalid UTF-8, source stdin, parser abort, hook cwd, and deferred event hooks.`

### `copy-mode.action-fidelity`: Complete the copy-mode action vocabulary

The send-keys -X parser maps 66 of the pin's 95 window-copy action names. Twenty-nine names remain absent across seven behavior categories. The seven missing stock default keys expose only five of those actions, so default-key tracking cannot stand in for the complete action vocabulary.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `hard`
- Owner: `terminal`
- User impact: daily, remote, scripts
- Items: `semantic:copy-mode-action-vocabulary`, `semantic:copy-mode-copy-format-and-destination`, `semantic:copy-mode-cursor-geometry`, `semantic:copy-mode-goto-line`, `semantic:copy-mode-jump-page-prompt-actions`, `semantic:copy-mode-logical-line-and-mode-keys`, `semantic:copy-mode-selection-lifecycle`
- Depends on: none
- Evidence:
  - `resource:crates/zz-mux/src/command.rs`
  - `resource:crates/zz-terminal/src/interaction.rs`
  - `resource:crates/zz-terminal/src/session.rs`
  - `resource:knowledge/tmux/copy-mode.md`
  - `resource:knowledge/tmux/key-tables.md`
- Acceptance:
  - `A source-owned inventory keeps all 95 pinned window-copy action names classified: the 66 mapped names retain typed mux and terminal behavior, and the 29 missing names stay explicit across the seven action categories until each has measured behavior or a named product decision.`
  - `Action-specific tests cover cursor geometry, logical-line and mode-key behavior, goto-line, selection lifecycle, jump/page/prompt behavior, and copy formatting and destination effects without using the fixed-row placement close as proof for history-bottom or wider action semantics.`

### `copy-mode.command-fidelity`: Complete copy-mode command fidelity

Copy mode lives per client in zz and needs explicit target-client semantics.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `hard`
- Owner: `client`
- User impact: daily, remote
- Items: `flag:copy-mode:-k`, `flag:copy-mode:-s`, `semantic:copy-mode-command-counts`, `semantic:copy-mode-command-errors`
- Depends on: `clients.interactive-refresh`
- Evidence:
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `resource:knowledge/tmux/divergences.md`
  - `file:compat/attached-client.sh`
- Acceptance:
  - `Attached-client tests cover source pane, initial scroll position, command counts, and mode errors.`

### `display-message.format-listing`: List display-message format variables

The format expander resolves named lookups but exposes no ordered enumeration of the variables defined for one expansion context.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `daemon`
- User impact: scripts, admin
- Items: `flag:display-message:-a`, `semantic:display-message-format-listing`
- Depends on: `clients.attach-context`, `formats.mouse-context`, `formats.pane-process`, `formats.pane-runtime`, `formats.session-runtime`, `formats.terminal-cells`, `formats.terminal-runtime`, `formats.window-runtime`
- Evidence:
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `resource:crates/zz-mux/src/formats.rs`
  - `resource:knowledge/tmux/status-line.md`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `display-message -a lists every defined variable and its current target-context value in the pin's order and output shape; unsupported variable families remain owned by their existing format gaps.`

### `display-message.mouse-target-context`: Resolve display-message mouse targets

The mux has no command-queue mouse record, so bare = currently reaches the ordinary pane parser and falls into the empty CANFAIL result even for a mouse-triggered command.

- Decision: `adopt`
- Status: `blocked`
- Priority and ease: `later` / `hard`
- Owner: `mux`
- User impact: scripts, gui
- Items: `semantic:display-message-bare-mouse-target`
- Depends on: `mouse.bound-context`
- Evidence:
  - `resource:crates/zz-mux/src/command.rs`
  - `resource:knowledge/tmux/tmux-compat.md`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `A display-message command invoked from a mouse binding resolves bare -t = and -t {mouse} to that event's pane, window, and session. Without a mouse event, CMD_FIND_CANFAIL retains an empty target context and stays quiet.`

### `display-message.pane-target-grammar`: Complete display-message pane target grammar

The closed target-client slice proves exact session, window, and pane names plus numeric pane misses. zz's shared pane resolver still models only part of tmux's relative and special target vocabulary.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `mux`
- User impact: scripts
- Items: `semantic:display-message-relative-special-targets`
- Depends on: none
- Evidence:
  - `resource:crates/zz-mux/src/model.rs`
  - `resource:crates/zz-mux/src/command.rs`
  - `resource:knowledge/tmux/tmux-compat.md`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `display-message -t matches CMD_FIND_PANE for relative window offsets and the pin's special window, pane, active, current, and marked target aliases, while preserving the same componentwise CANFAIL state after a miss.`

### `display-message.verbose-trace`: Trace display-message format expansion

The shared expander returns only the final string and has no structured trace sink for nested conditions, modifiers, lookups, and replacements.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `hard`
- Owner: `mux`
- User impact: scripts, admin
- Items: `flag:display-message:-v`, `semantic:display-message-format-trace`
- Depends on: none
- Evidence:
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `resource:crates/zz-mux/src/formats.rs`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `display-message -v prints the pin's ordered parse and replacement trace while producing the same final expansion as the ordinary path.`

### `display-panes.queue-semantics`: Wait for display-panes overlays

zz has no parked command-queue item tied to a client overlay lifetime.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `hard`
- Owner: `daemon`
- User impact: scripts, daily
- Items: `semantic:display-panes-queue-blocking`
- Depends on: `clients.interactive-refresh`
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `A command sequence resumes after the targeted overlay closes unless -b is present.`

### `formats.mouse-context`: Expose mouse event formats

Command expansion does not retain the originating mouse event.

- Decision: `park`
- Status: `blocked`
- Priority and ease: `later` / `hard`
- Owner: `protocol`
- User impact: scripts, gui
- Items: `format:mouse_hyperlink`, `format:mouse_line`, `format:mouse_pane`, `format:mouse_status_line`, `format:mouse_status_range`, `format:mouse_word`, `format:mouse_x`, `format:mouse_y`
- Depends on: `mouse.bound-context`
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Mouse-originated commands receive one normalized event record across terminal and GUI clients.`

### `formats.native-modes`: Keep native mode row formats

zz does not render tmux buffer, client, or tree mode screens.

- Decision: `native`
- Status: `accepted`
- Priority and ease: `none` / `none`
- Owner: `client`
- User impact: daily, gui
- Items: `format:buffer_mode_format`, `format:client_mode_format`, `format:tree_mode_format`
- Depends on: none
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Native choosers remain format-driven through their own row models instead of advertising tmux mode rows.`

### `formats.pane-process`: Expose remaining pane process formats

These runtime facts live outside the mux format context or do not have a retained model yet.

- Decision: `adopt`
- Status: `blocked`
- Priority and ease: `later` / `hard`
- Owner: `daemon`
- User impact: scripts
- Items: `format:pane_pb_progress`, `format:pane_pipe`, `format:pane_pipe_pid`, `format:pane_unseen_changes`
- Depends on: `protocol.binary-streams`
- Evidence:
  - `resource:crates/zz-mux/src/formats.rs`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Pane snapshots retain pipe lifecycle, paste progress, and unseen-change state with target-aware format tests.`

### `formats.pane-runtime`: Expose pane mode formats

Native mode and search state live per view, not on the shared pane.

- Decision: `park`
- Status: `blocked`
- Priority and ease: `later` / `hard`
- Owner: `client`
- User impact: scripts, daily
- Items: `format:pane_in_mode`, `format:pane_key_mode`, `format:pane_mode`, `format:pane_search_string`
- Depends on: `clients.interactive-refresh`
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Per-client native view state has defined aggregation when a pane has multiple viewers.`

### `formats.session-activity-wake-lifecycle`: Keep native wake lifecycle outside session activity

Pinned tmux refreshes session activity when a suspended tty client wakes or unlocks. zz has no suspended attached-client state or wake/unlock protocol message; reconnect and reattach already flow through the ordinary attach activity seam.

- Decision: `native`
- Status: `accepted`
- Priority and ease: `none` / `none`
- Owner: `daemon`
- User impact: remote
- Items: `semantic:session-activity-wake-unlock-lifecycle`
- Depends on: none
- Evidence:
  - `resource:crates/zz-protocol/src/message.rs`
  - `resource:crates/zz-daemon/src/daemon.rs`
  - `resource:knowledge/tmux/status-line.md`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `zz keeps activity on its native attach and input lifecycle and does not fabricate a tmux MSG_WAKEUP or MSG_UNLOCK refresh without a suspended-client protocol state.`

### `formats.session-runtime`: Expose client-derived session formats

Both values depend on the attached client's selected session and working directory.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `protocol`
- User impact: scripts
- Items: `format:session_active`, `format:session_path`
- Depends on: `clients.attach-context`
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Session active and working path have target-aware expansion from the shared attached-client context.`

### `formats.terminal-cells`: Expose terminal cell formats

The mux format engine cannot inspect live terminal grid internals.

- Decision: `park`
- Status: `blocked`
- Priority and ease: `later` / `hard`
- Owner: `terminal`
- User impact: scripts
- Items: `format:cursor_character`, `format:cursor_colour`, `format:pane_bg`, `format:pane_fg`, `format:pane_pb_state`, `format:pane_tabs`
- Depends on: none
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `A bounded terminal snapshot publishes cursor cell, tab stops, and progress state without blocking PTY drain.`

### `formats.terminal-runtime`: Expose terminal runtime formats

The mux currently substitutes constants for live VT state that only the terminal worker owns.

- Decision: `park`
- Status: `blocked`
- Priority and ease: `later` / `hardest`
- Owner: `terminal`
- User impact: scripts
- Items: `format:alternate_on`, `format:alternate_saved_x`, `format:alternate_saved_y`, `format:bracket_paste_flag`, `format:cursor_blinking`, `format:cursor_flag`, `format:cursor_shape`, `format:cursor_very_visible`, `format:cursor_x`, `format:cursor_y`, `format:history_all_bytes`, `format:history_bytes`, `format:history_size`, `format:insert_flag`, `format:keypad_cursor_flag`, `format:keypad_flag`, `format:mouse_all_flag`, `format:mouse_any_flag`, `format:mouse_button_flag`, `format:mouse_sgr_flag`, `format:mouse_standard_flag`, `format:mouse_utf8_flag`, `format:origin_flag`, `format:scroll_region_lower`, `format:scroll_region_upper`, `format:sixel_support`, `format:synchronized_output_flag`, `format:wrap_flag`
- Depends on: none
- Evidence:
  - `resource:crates/zz-mux/src/formats.rs`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `A bounded terminal snapshot publishes cursor, mode, mouse, history, scroll-region, and graphics facts without blocking PTY drain.`

### `formats.window-runtime`: Expose remaining window formats

The data exists in separate state paths but is not retained as target-aware format input.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `daemon`
- User impact: scripts, remote
- Items: `format:window_activity`, `format:window_bigger`, `format:window_cell_height`, `format:window_cell_width`, `format:window_offset_x`, `format:window_offset_y`
- Depends on: `clients.attach-context`
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Window activity timestamps and each client's viewport offsets expand in the correct target context.`

### `history.hyperlink-reset`: Reset hyperlink history

The terminal API lacks a distinct hyperlink-registry reset.

- Decision: `adopt`
- Status: `blocked`
- Priority and ease: `later` / `medium`
- Owner: `terminal`
- User impact: daily
- Items: `flag:clear-history:-H`
- Depends on: none
- Evidence:
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `resource:knowledge/designs/tmux-superset-roadmap.md`
- Acceptance:
  - `A terminal-owned action clears normal history and resets the VT hyperlink registry atomically.`

### `hooks.pane-events`: Produce pane focus and clipboard hooks

Focus is per client and clipboard changes cross client ownership.

- Decision: `adopt`
- Status: `blocked`
- Priority and ease: `later` / `hard`
- Owner: `daemon`
- User impact: scripts, gui
- Items: `hook:pane-focus-in`, `hook:pane-focus-out`, `hook:pane-set-clipboard`
- Depends on: `clients.attach-context`
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
  - `resource:crates/zz-mux/src/tmux_options.rs`
- Acceptance:
  - `Each pane transition emits once with a defined client when multiple clients view the pane.`

### `hooks.queue`: Produce after-queue hooks

The hook bus exists, but zz has no equivalent producer boundary.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `daemon`
- User impact: scripts
- Items: `hook:after-queue`
- Depends on: none
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
  - `resource:crates/zz-mux/src/tmux_options.rs`
- Acceptance:
  - `The hook fires after the same queue boundary as the pin without duplicate command blocks.`

### `jobs.environment`: Normalize shell job environments

Jobs still inherit daemon-only state and omit terminal identity variables.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `hard`
- Owner: `daemon`
- User impact: scripts, remote
- Items: `semantic:shell-job-clean-environment`
- Depends on: `clients.attach-context`
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
  - `scenario:compat/scenarios/smoke/oh-my-tmux.txt`
- Acceptance:
  - `Job tests pin clean inherited state, TERM-family synthesis, and status-job overlays.`

### `keys.copy-mode-binding-fidelity`: Match shared copy-mode binding commands

Cursor-word search, prompted search, goto-line, and character jumps retain 15 divergent stored command shapes; this gap does not assume their native behavior is exact.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `next` / `hard`
- Owner: `protocol`
- User impact: daily, remote, scripts
- Items: `binding:copy-mode-vi:#`, `binding:copy-mode-vi:*`, `binding:copy-mode-vi:/`, `binding:copy-mode-vi::`, `binding:copy-mode-vi:?`, `binding:copy-mode-vi:F`, `binding:copy-mode-vi:T`, `binding:copy-mode-vi:f`, `binding:copy-mode-vi:t`, `binding:copy-mode:C-r`, `binding:copy-mode:C-s`, `binding:copy-mode:F`, `binding:copy-mode:T`, `binding:copy-mode:f`, `binding:copy-mode:t`
- Depends on: `copy-mode.command-fidelity`
- Evidence:
  - `resource:crates/zz-mux/src/compat_manifest_tests.rs`
  - `resource:crates/zz-protocol/src/key.rs`
  - `resource:knowledge/tmux/key-tables.md`
- Acceptance:
  - `Every shared emacs and vi copy binding matches the pin's rendered command or moves to a named native divergence; stock repeat metadata is already exact.`

### `keys.copy-mode-native-mouse`: Keep native copy-mode mouse handling

Mouse gestures belong to each rendering client instead of a shared terminal key table.

- Decision: `native`
- Status: `accepted`
- Priority and ease: `none` / `none`
- Owner: `client`
- User impact: daily, gui
- Items: `key:copy-mode-vi:DoubleClick1Pane`, `key:copy-mode-vi:MouseDown1Pane`, `key:copy-mode-vi:MouseDrag1Pane`, `key:copy-mode-vi:MouseDragEnd1Pane`, `key:copy-mode-vi:TripleClick1Pane`, `key:copy-mode-vi:WheelDownPane`, `key:copy-mode-vi:WheelUpPane`, `key:copy-mode:DoubleClick1Pane`, `key:copy-mode:MouseDown1Pane`, `key:copy-mode:MouseDrag1Pane`, `key:copy-mode:MouseDragEnd1Pane`, `key:copy-mode:TripleClick1Pane`, `key:copy-mode:WheelDownPane`, `key:copy-mode:WheelUpPane`
- Depends on: none
- Evidence:
  - `resource:crates/zz-protocol/src/key.rs`
  - `resource:knowledge/tmux/key-tables.md`
- Acceptance:
  - `Native terminal and GUI selection gestures retain copy, scroll, drag, and multi-click behavior without installing tmux mouse key names.`

### `keys.copy-mode-native-numeric-prefix`: Keep native vi numeric prefix capture

zz keeps numeric capture inside its per-client key engine instead of rendering tmux's command-prompt -P, while preserving the pin's action-level repeat semantics and exposing the native copy-mode-repeat command shape to list-keys.

- Decision: `native`
- Status: `accepted`
- Priority and ease: `none` / `none`
- Owner: `protocol`
- User impact: daily, gui, scripts
- Items: `binding:copy-mode-vi:1`, `binding:copy-mode-vi:2`, `binding:copy-mode-vi:3`, `binding:copy-mode-vi:4`, `binding:copy-mode-vi:5`, `binding:copy-mode-vi:6`, `binding:copy-mode-vi:7`, `binding:copy-mode-vi:8`, `binding:copy-mode-vi:9`
- Depends on: none
- Evidence:
  - `resource:crates/zz-protocol/src/key.rs`
  - `resource:crates/zz-mux/src/command.rs`
  - `resource:knowledge/tmux/key-tables.md`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Digits accumulate per client without opening a pane-cell prompt, and the next copy action carries one -N count without allocating count-proportional action vectors.`
  - `A counted multi-command binding consumes the prefix at its first send or send-keys command whose option prefix contains -X. That command keeps its own -N when present; otherwise zz inserts one separate -N count pair immediately before the option argument containing -X. Later actions do not inherit the prefix, while a list with no qualifying -X command preserves it. Movements, jumps, matching brackets, and repeat-search consume the count; other-end swaps on odd counts, select-line spans count lines, copy-end-of-line spans count rows and copies once, and other toggles, selection, copy, clear-selection, and cancel execute once.`

### `keys.copy-mode-prompt-defaults`: Add prompt-backed emacs copy-mode defaults

These ten keys require command-prompt behavior and stored command blocks, not another direct send-keys -X navigation binding.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `daemon`
- User impact: daily, remote, scripts
- Items: `key:copy-mode:M-1`, `key:copy-mode:M-2`, `key:copy-mode:M-3`, `key:copy-mode:M-4`, `key:copy-mode:M-5`, `key:copy-mode:M-6`, `key:copy-mode:M-7`, `key:copy-mode:M-8`, `key:copy-mode:M-9`, `key:copy-mode:g`
- Depends on: `prompt.command-fidelity`
- Evidence:
  - `resource:crates/zz-mux/src/compat_manifest_tests.rs`
  - `resource:crates/zz-protocol/src/key.rs`
  - `resource:crates/zz-daemon/src/daemon.rs`
  - `resource:knowledge/tmux/key-tables.md`
- Acceptance:
  - `M-1 through M-9 open the pin's numeric repeat prompt and g opens its goto-line prompt with exact stored command shapes, submission behavior, and nonrepeat metadata.`

### `keys.copy-mode-unsupported-default-actions`: Implement missing stock copy-mode actions

These seven absent default keys name five of the 29 actions tracked under `copy-mode.action-fidelity`. Installing the bindings before those five actions have typed behavior would create silent no-ops; the other 24 missing action names do not belong to this default-key group.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `terminal`
- User impact: daily, remote
- Items: `key:copy-mode-vi:P`, `key:copy-mode-vi:r`, `key:copy-mode:C-M-b`, `key:copy-mode:C-l`, `key:copy-mode:M-l`, `key:copy-mode:P`, `key:copy-mode:r`
- Depends on: `copy-mode.action-fidelity`
- Evidence:
  - `resource:crates/zz-mux/src/compat_manifest_tests.rs`
  - `resource:crates/zz-protocol/src/key.rs`
  - `resource:crates/zz-mux/src/command.rs`
  - `resource:crates/zz-terminal/src/interaction.rs`
  - `resource:knowledge/tmux/key-tables.md`
- Acceptance:
  - `previous-matching-bracket, recentre-top-bottom, cursor-centre-horizontal, toggle-position, and refresh-toggle have typed terminal behavior before their seven stock keys are installed.`

### `keys.default-prefix`: Keep the native default prefix table

Picker and sidebar bindings are part of the zz GUI experience.

- Decision: `native`
- Status: `accepted`
- Priority and ease: `none` / `none`
- Owner: `protocol`
- User impact: daily, gui
- Items: `binding:prefix:"`, `binding:prefix:$`, `binding:prefix:%`, `binding:prefix:&`, `binding:prefix:,`, `binding:prefix:0`, `binding:prefix:1`, `binding:prefix:2`, `binding:prefix:3`, `binding:prefix:4`, `binding:prefix:5`, `binding:prefix:6`, `binding:prefix:7`, `binding:prefix:8`, `binding:prefix:9`, `binding:prefix:?`, `binding:prefix:C-Down`, `binding:prefix:C-Left`, `binding:prefix:C-Right`, `binding:prefix:C-Up`, `binding:prefix:M-Left`, `binding:prefix:M-Up`, `binding:prefix:]`, `binding:prefix:r`, `binding:prefix:s`, `binding:prefix:w`, `binding:prefix:x`, `key:prefix:#`, `key:prefix:'`, `key:prefix:(`, `key:prefix:)`, `key:prefix:*`, `key:prefix:-`, `key:prefix:.`, `key:prefix:/`, `key:prefix:<`, `key:prefix:>`, `key:prefix:@`, `key:prefix:BTab`, `key:prefix:C`, `key:prefix:C-z`, `key:prefix:D`, `key:prefix:DC`, `key:prefix:L`, `key:prefix:M`, `key:prefix:M-n`, `key:prefix:M-p`, `key:prefix:PPage`, `key:prefix:S-Down`, `key:prefix:S-Left`, `key:prefix:S-Right`, `key:prefix:S-Up`, `key:prefix:Tab`, `key:prefix:d`, `key:prefix:f`, `key:prefix:g`, `key:prefix:i`, `key:prefix:m`, `key:prefix:t`, `key:prefix:~`, `semantic:default-prefix-remaps`
- Depends on: none
- Evidence:
  - `resource:knowledge/tmux/key-tables.md`
  - `resource:crates/zz-protocol/src/key.rs`
  - `file:compat/attached-client.sh`
- Acceptance:
  - `The native defaults stay documented; imported tmux bindings retain exact tmux command semantics.`

### `keys.move-table`: Replace the floating-pane move table natively

tmux's move table controls a floating mux model that zz deliberately does not share with native overlays.

- Decision: `native`
- Status: `accepted`
- Priority and ease: `none` / `none`
- Owner: `gui`
- User impact: daily, gui
- Items: `key:move:,`, `key:move:.`, `key:move:0`, `key:move:1`, `key:move:2`, `key:move:3`, `key:move:4`, `key:move:Down`, `key:move:Left`, `key:move:M-1`, `key:move:M-2`, `key:move:M-3`, `key:move:M-4`, `key:move:M-Down`, `key:move:M-Left`, `key:move:M-Right`, `key:move:M-Up`, `key:move:Right`, `key:move:Up`
- Depends on: none
- Evidence:
  - `resource:crates/zz-protocol/src/key.rs`
  - `resource:knowledge/tmux/key-tables.md`
- Acceptance:
  - `Native floating surfaces keep direct drag, resize, and placement controls without exposing tmux's floating-pane move table.`

### `keys.native-defaults`: Keep zz-native default key tables explicit

Native chooser navigation, send-last-output, and copy-mode confirmation extend the default key surface beyond tmux.

- Decision: `native`
- Status: `accepted`
- Priority and ease: `none` / `none`
- Owner: `protocol`
- User impact: daily, gui
- Items: `native-key:choose-buffer:/`, `native-key:choose-buffer:?`, `native-key:choose-buffer:C-[`, `native-key:choose-buffer:C-b`, `native-key:choose-buffer:C-f`, `native-key:choose-buffer:C-g`, `native-key:choose-buffer:C-n`, `native-key:choose-buffer:C-p`, `native-key:choose-buffer:C-s`, `native-key:choose-buffer:Down`, `native-key:choose-buffer:End`, `native-key:choose-buffer:Enter`, `native-key:choose-buffer:Escape`, `native-key:choose-buffer:G`, `native-key:choose-buffer:Home`, `native-key:choose-buffer:N`, `native-key:choose-buffer:NPage`, `native-key:choose-buffer:PPage`, `native-key:choose-buffer:Up`, `native-key:choose-buffer:d`, `native-key:choose-buffer:g`, `native-key:choose-buffer:j`, `native-key:choose-buffer:k`, `native-key:choose-buffer:n`, `native-key:choose-buffer:p`, `native-key:choose-buffer:q`, `native-key:choose-tree:+`, `native-key:choose-tree:-`, `native-key:choose-tree:/`, `native-key:choose-tree:?`, `native-key:choose-tree:C-[`, `native-key:choose-tree:C-b`, `native-key:choose-tree:C-f`, `native-key:choose-tree:C-g`, `native-key:choose-tree:C-n`, `native-key:choose-tree:C-p`, `native-key:choose-tree:C-s`, `native-key:choose-tree:Down`, `native-key:choose-tree:End`, `native-key:choose-tree:Enter`, `native-key:choose-tree:Escape`, `native-key:choose-tree:G`, `native-key:choose-tree:Home`, `native-key:choose-tree:Left`, `native-key:choose-tree:N`, `native-key:choose-tree:NPage`, `native-key:choose-tree:PPage`, `native-key:choose-tree:Right`, `native-key:choose-tree:Up`, `native-key:choose-tree:g`, `native-key:choose-tree:h`, `native-key:choose-tree:j`, `native-key:choose-tree:k`, `native-key:choose-tree:l`, `native-key:choose-tree:n`, `native-key:choose-tree:q`, `native-key:copy-mode:Enter`, `native-key:prefix:e`
- Depends on: none
- Evidence:
  - `resource:crates/zz-mux/src/compat_manifest_tests.rs`
  - `resource:crates/zz-protocol/src/key.rs`
  - `resource:knowledge/tmux/key-tables.md`
- Acceptance:
  - `Every zz-only default key remains explicitly classified and does not hide a missing or divergent tmux default.`

### `keys.root-native-mouse`: Keep native root mouse bindings

Root-table mouse commands draw terminal client UI that native clients own directly.

- Decision: `native`
- Status: `accepted`
- Priority and ease: `none` / `none`
- Owner: `client`
- User impact: daily, gui
- Items: `key:root:C-MouseDown1Pane`, `key:root:C-MouseDown1Status`, `key:root:DoubleClick1Pane`, `key:root:M-MouseDown3Pane`, `key:root:M-MouseDown3Status`, `key:root:M-MouseDown3StatusLeft`, `key:root:M-MouseDrag1Border`, `key:root:M-MouseDrag1Pane`, `key:root:MouseDown1Border`, `key:root:MouseDown1Control7`, `key:root:MouseDown1Control8`, `key:root:MouseDown1Control9`, `key:root:MouseDown1Pane`, `key:root:MouseDown1ScrollbarDown`, `key:root:MouseDown1ScrollbarUp`, `key:root:MouseDown1Status`, `key:root:MouseDown2Pane`, `key:root:MouseDown3Pane`, `key:root:MouseDown3Status`, `key:root:MouseDown3StatusLeft`, `key:root:MouseDrag1Border`, `key:root:MouseDrag1Pane`, `key:root:MouseDrag1ScrollbarSlider`, `key:root:TripleClick1Pane`, `key:root:WheelDownStatus`, `key:root:WheelUpPane`, `key:root:WheelUpStatus`
- Depends on: none
- Evidence:
  - `resource:crates/zz-protocol/src/key.rs`
  - `resource:knowledge/tmux/key-tables.md`
- Acceptance:
  - `Terminal and GUI clients preserve focus, selection, scrolling, borders, menus, and status interactions through native pointer events.`

### `keys.strict-validation`: Match tmux key-name validation

zz also accepts long Ctrl- and Alt- modifier aliases that the pin rejects; partial tightening must preserve pin-valid caret, back-tab, and keypad names.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `hard`
- Owner: `protocol`
- User impact: scripts, daily
- Items: `semantic:bind-key-template-validation`, `semantic:key-long-modifier-alias-overacceptance`, `semantic:prefix-key-validation`
- Depends on: none
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
  - `resource:knowledge/tmux/key-tables.md`
- Acceptance:
  - `The shared key parser accepts the pin's vocabulary and rejects invalid modified names before storing bindings.`

### `layout.main-horizontal-upstream-bug`: Reject stale two-pane main layout geometry

The pin leaves invalid stale geometry; zz keeps a valid layout.

- Decision: `never`
- Status: `accepted`
- Priority and ease: `none` / `none`
- Owner: `mux`
- User impact: daily, scripts
- Items: `semantic:main-layout-two-pane-stale-geometry`
- Depends on: none
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
  - `scenario:compat/scenarios/known/known-main-preset-two-panes.txt`
- Acceptance:
  - `The known scenario keeps exactly one geometry divergence and no topology, format, output, or warning divergence.`

### `layout.safety-invariants`: Keep bounded valid layouts

Matching these pin cases would admit invalid or unbounded layouts.

- Decision: `never`
- Status: `accepted`
- Priority and ease: `none` / `none`
- Owner: `mux`
- User impact: daily, scripts
- Items: `semantic:layout-large-extents`, `semantic:layout-lone-pane-extent`, `semantic:layout-single-child-node`, `semantic:layout-validation-order-depth`, `semantic:layout-zero-sized-leaf`, `semantic:window-size-out-of-range`
- Depends on: none
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Parser tests retain minimum pane size, 10000-cell bounds, branching invariants, and bounded depth.`

### `layout.spread-mixed-upstream-bug`: Reject corrupt mixed-parent spread

The pin corrupts geometry when a parent mixes leaves and nested nodes.

- Decision: `never`
- Status: `accepted`
- Priority and ease: `none` / `none`
- Owner: `mux`
- User impact: daily, scripts
- Items: `semantic:spread-mixed-parent-corruption`
- Depends on: none
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
  - `scenario:compat/scenarios/known/known-spread-mixed.txt`
- Acceptance:
  - `The known scenario keeps exactly one geometry divergence and no topology, format, output, or warning divergence.`

### `list-keys.deterministic-sort-ties`: Keep list-keys sorting total and deterministic

The pin's comparator truncates 64-bit key differences into int, returns equality tests as ordering values, and relies on qsort for non-total ties; reproducing that in Rust would violate the ordering contract.

- Decision: `never`
- Status: `accepted`
- Priority and ease: `none` / `none`
- Owner: `mux`
- User impact: scripts
- Items: `semantic:list-keys-deterministic-sort-ties`
- Depends on: none
- Evidence:
  - `resource:crates/zz-mux/src/command.rs`
  - `resource:knowledge/tmux/divergences.md`
  - `scenario:compat/scenarios/list-keys-padding.txt`
- Acceptance:
  - `Distinct-base key ordering, traversal order, reversal, and per-table note ordering match the pin; equal-base modifier and type ties, cross-table ties, inapplicable sort fields, and four-byte Unicode keys use the documented deterministic order.`

### `messages.tty-model`: Model tmux message and TTY reports

zz does not retain tmux's TTY capability and job-message model.

- Decision: `park`
- Status: `blocked`
- Priority and ease: `later` / `hard`
- Owner: `daemon`
- User impact: admin, remote
- Items: `flag:show-messages:-J`, `flag:show-messages:-T`, `flag:show-messages:-t`
- Depends on: `clients.attach-context`
- Evidence:
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `The daemon retains job and terminal capability records with target-aware output matching the pin.`

### `mouse.bound-context`: Carry bound mouse event context

These flags depend on the input event that invoked the command.

- Decision: `park`
- Status: `blocked`
- Priority and ease: `later` / `hard`
- Owner: `protocol`
- User impact: daily, scripts, gui
- Items: `flag:copy-mode:-S`, `flag:move-pane:-M`, `flag:resize-pane:-M`, `flag:send-keys:-M`
- Depends on: `clients.attach-context`
- Evidence:
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `resource:knowledge/designs/tmux-superset-roadmap.md`
- Acceptance:
  - `A normalized mouse record reaches bound commands without changing direct GUI mouse behavior.`

### `mux.chain-parse-abort`: Abort invalid command groups before effects

Protocol v76 separates parse and preparation failures as ServerError::CommandParse from target and runtime failures, and an already-running compatible local daemon rejects typed name or alias-body preparation errors before preprocessing or execution. Cold or failed preparation falls open to static routing, so an autospawn verb may still run before a later unknown command. Flag, arity, and other argument validation still happens per command, while config and source-file replay retain a dispatch-at-a-time boundary.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `mux`
- User impact: scripts
- Items: `semantic:command-chain-parse-abort`
- Depends on: none
- Evidence:
  - `resource:crates/zz-protocol/src/message.rs`
  - `resource:crates/zz-mux/src/command.rs`
  - `resource:crates/zz/src/lib.rs`
  - `resource:crates/zz-daemon/src/daemon.rs`
  - `file:crates/zz/tests/cli_binary.rs`
  - `scenario:compat/scenarios/smoke/cli-chain-parse-abort.txt`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Every command group is parsed and validated before its first effect, including a local CLI chain whose first verb may autospawn a missing daemon, so a later parse or preparation error aborts the whole group while runtime command errors retain tmux queue ordering.`

### `mux.error-shapes`: Match remaining command errors

Scripts can inspect exact errors even when both implementations reject the command.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `next` / `medium`
- Owner: `protocol`
- User impact: scripts
- Items: `positional-max:choose-buffer`, `positional-max:choose-tree`, `positional-max:display-message`, `positional-max:display-panes`, `positional-max:load-buffer`, `positional-max:save-buffer`, `positional-max:select-pane`, `positional-max:set-buffer`, `positional-min:bind-key`, `positional-min:confirm-before`, `positional-min:display-menu`, `positional-min:find-window`, `positional-min:if-shell`, `positional-min:load-buffer`, `positional-min:rename-session`, `positional-min:rename-window`, `positional-min:save-buffer`, `positional-min:set-environment`, `positional-min:set-option`, `positional-min:set-window-option`, `positional-min:source-file`, `positional-min:wait-for`, `semantic:command-arity-errors`, `semantic:command-flag-errors`, `semantic:nested-new-session-error-precedence`
- Depends on: none
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Catalog metadata drives pin-shaped arity, flag, usage, and precedence errors for every command.`

### `options.lock-program`: Defer tmux lock process execution

Spawning lock -np over a native GUI has no useful meaning.

- Decision: `park`
- Status: `blocked`
- Priority and ease: `later` / `hardest`
- Owner: `client`
- User impact: remote, admin
- Items: `option:lock-after-time`, `option:lock-command`, `semantic:lock-program-execution`
- Depends on: `clients.interactive-refresh`
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
  - `resource:crates/zz-mux/src/tmux_options.rs`
- Acceptance:
  - `A terminal-client design defines lock process ownership, timers, cancellation, and GUI behavior.`

### `options.native-mode-styles`: Keep native mode styling

zz renders native mode surfaces instead of tmux cell grids.

- Decision: `native`
- Status: `accepted`
- Priority and ease: `none` / `none`
- Owner: `client`
- User impact: daily, gui
- Items: `option:clock-mode-colour`, `option:clock-mode-style`, `option:copy-mode-current-line-number-style`, `option:copy-mode-line-number-style`, `option:copy-mode-line-numbers`, `option:copy-mode-position-format`, `option:copy-mode-position-style`, `option:copy-mode-selection-style`, `option:fill-character`, `option:switch-mode-match-style`, `option:tree-mode-border-style`, `option:tree-mode-preview-format`, `option:tree-mode-preview-style`, `option:tree-mode-selection-style`
- Depends on: none
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
  - `resource:crates/zz-mux/src/tmux_options.rs`
- Acceptance:
  - `Native copy, tree, switch, and clock surfaces document which tmux style knobs they replace.`

### `options.native-overlay-styles`: Keep native overlay styling

These options style tmux's cell-drawn surfaces, which zz replaces with native chrome.

- Decision: `native`
- Status: `accepted`
- Priority and ease: `none` / `none`
- Owner: `client`
- User impact: daily, gui
- Items: `option:display-panes-active-colour`, `option:display-panes-colour`, `option:message-command-style`, `option:message-format`, `option:message-style`, `option:prompt-command-cursor-colour`, `option:prompt-command-cursor-style`, `option:prompt-cursor-colour`, `option:prompt-cursor-style`, `presentation:display-panes-native-overlay`, `presentation:native-prompts-menus-popups`
- Depends on: none
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
  - `resource:crates/zz-mux/src/tmux_options.rs`
- Acceptance:
  - `Native overlays keep command and keyboard semantics while using zz theme tokens.`

### `options.option-name-format-coverage`: Complete option-name format coverage

The status and display-message paths resolve the consumer-backed option families, but there is no complete source-owned proof for every stored option name.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `mux`
- User impact: scripts
- Items: `semantic:option-name-format-coverage`
- Depends on: none
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
  - `resource:crates/zz-mux/src/formats.rs`
  - `resource:crates/zz-mux/src/command.rs`
- Acceptance:
  - `Every supported option-name format resolves through the pinned target scope and inheritance chain, with source-owned coverage for the complete intended roster.`

### `options.pane-chrome`: Consume pane chrome options

The appearance bridge handles colors but not full border segments, formats, or scrollbars.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `client`
- User impact: daily, gui
- Items: `option:pane-border-format`, `option:pane-border-indicators`, `option:pane-border-lines`, `option:pane-border-status`, `option:pane-colours`, `option:pane-scrollbars`, `option:pane-scrollbars-position`, `option:pane-scrollbars-style`, `option:pane-scrollbars-timeout`, `presentation:border-style-owner-granularity`, `presentation:renderer-style-residue`
- Depends on: none
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
  - `resource:crates/zz-mux/src/tmux_options.rs`
- Acceptance:
  - `GUI and TUI consume meaningful pane chrome fields and document unsupported cell-level attributes.`

### `options.remain-on-exit-format`: Render remain-on-exit-format in retained panes

The retained-pane path marks the pane dead after the terminal worker exits, and terminal core has no post-worker VT injection or frozen-view reconstruction seam.

- Decision: `adopt`
- Status: `blocked`
- Priority and ease: `later` / `hard`
- Owner: `terminal`
- User impact: daily, scripts
- Items: `option:remain-on-exit-format`
- Depends on: none
- Evidence:
  - `resource:knowledge/designs/tmux-drop-in.md`
  - `resource:knowledge/tmux/divergences.md`
  - `resource:crates/zz-terminal/src/session.rs`
- Acceptance:
  - `A retained dead pane renders the target-scoped format after worker exit without reviving the PTY or inventing a second terminal-state owner.`

### `options.terminal-behavior`: Consume terminal behavior options

These values need terminal negotiation, input, width, or process consumers.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `hard`
- Owner: `terminal`
- User impact: daily, remote, scripts
- Items: `option:allow-rename`, `option:alternate-screen`, `option:assume-paste-time`, `option:backspace`, `option:codepoint-widths`, `option:default-client-command`, `option:editor`, `option:extended-keys`, `option:extended-keys-format`, `option:focus-follows-mouse`, `option:get-clipboard`, `option:input-buffer-size`, `option:scroll-on-clear`, `option:terminal-features`, `option:terminal-overrides`, `option:user-keys`, `option:variation-selector-always-wide`, `option:xterm-keys`
- Depends on: none
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
  - `resource:crates/zz-mux/src/tmux_options.rs`
- Acceptance:
  - `Each option has a traced consumer or moves to an explicit native or parked decision.`

### `options.theme-palette`: Map tmux theme palette options

Storage is easy, but native clients need one coherent palette mapping.

- Decision: `park`
- Status: `blocked`
- Priority and ease: `later` / `medium`
- Owner: `client`
- User impact: gui
- Items: `option:dark-theme-black`, `option:dark-theme-blue`, `option:dark-theme-cyan`, `option:dark-theme-dark-grey`, `option:dark-theme-green`, `option:dark-theme-light-grey`, `option:dark-theme-magenta`, `option:dark-theme-red`, `option:dark-theme-white`, `option:dark-theme-yellow`, `option:light-theme-black`, `option:light-theme-blue`, `option:light-theme-cyan`, `option:light-theme-dark-grey`, `option:light-theme-green`, `option:light-theme-light-grey`, `option:light-theme-magenta`, `option:light-theme-red`, `option:light-theme-white`, `option:light-theme-yellow`, `option:theme`
- Depends on: none
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
  - `resource:crates/zz-mux/src/tmux_options.rs`
- Acceptance:
  - `A real configuration workload justifies a documented mapping into zz theme tokens.`

### `pane.break-geometry`: Complete break-pane placement

The mux owns the needed window and layout state.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `mux`
- User impact: scripts, daily
- Items: `flag:break-pane:-W`, `flag:break-pane:-X`, `flag:break-pane:-Y`, `flag:break-pane:-x`, `flag:break-pane:-y`
- Depends on: none
- Evidence:
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `scenario:compat/scenarios/break-pane.txt`
- Acceptance:
  - `Differential geometry tests cover target window creation, name, size, and placement coordinates.`

### `pane.floating-model`: Defer tmux floating panes

tmux floating panes are mux objects; zz native floating surfaces are presentation objects.

- Decision: `park`
- Status: `blocked`
- Priority and ease: `later` / `hardest`
- Owner: `mux`
- User impact: daily, scripts, gui
- Items: `command:new-pane`, `flag:move-pane:-D`, `flag:move-pane:-L`, `flag:move-pane:-P`, `flag:move-pane:-R`, `flag:move-pane:-U`, `flag:move-pane:-X`, `flag:move-pane:-Y`, `flag:move-pane:-z`, `format:pane_floating_flag`, `semantic:floating-pane-model`, `semantic:move-pane-tiled-extension`
- Depends on: none
- Evidence:
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `resource:knowledge/designs/tmux-superset-roadmap.md`
- Acceptance:
  - `A separate mux floating-pane model earns approval without reusing native GUI overlays.`

### `pane.selection-state`: Model pane selection controls

Several flags need client input and marked-pane state that the mux does not retain.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `hard`
- Owner: `daemon`
- User impact: daily, scripts
- Items: `flag:select-pane:-M`, `flag:select-pane:-P`, `flag:select-pane:-d`, `flag:select-pane:-e`, `flag:select-pane:-g`, `flag:select-pane:-m`, `format:pane_marked`, `format:pane_marked_set`, `format:session_marked`, `format:window_marked_flag`, `semantic:window-marked-pane-format`
- Depends on: `clients.attach-context`
- Evidence:
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Pane input, style, title, mark, and clear-mark controls have target-aware state and format tests.`

### `pane.spawn-flags`: Complete split-window placement flags

Most forms extend the existing spawn effect, but some may expose floating or marked state.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `mux`
- User impact: scripts, daily
- Items: `flag:split-window:-R`, `flag:split-window:-S`, `flag:split-window:-T`, `flag:split-window:-W`, `flag:split-window:-k`, `flag:split-window:-m`, `flag:split-window:-s`
- Depends on: none
- Evidence:
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `scenario:compat/scenarios/pane-spawn-options.txt`
- Acceptance:
  - `Differential tests cover the supported tiled meaning or each model-bound flag moves to a parked gap.`

### `presentation.native-status`: Keep native status and lifecycle presentation

Native chrome and a persistent daemon need explicit behavior where tmux assumes a terminal client lifecycle.

- Decision: `native`
- Status: `accepted`
- Priority and ease: `none` / `none`
- Owner: `client`
- User impact: daily, gui, remote
- Items: `presentation:in-ui-error-width`, `presentation:status-block-suppression-threshold`, `semantic:automatic-rename-timing`, `semantic:bare-launcher-attach-current`, `semantic:config-files-native-discovery`, `semantic:empty-daemon-command-query`, `semantic:history-limit-product-default`, `semantic:lifecycle-subscriber-guard`, `semantic:set-titles-empty-expansion`, `semantic:version-suffix`
- Depends on: none
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
  - `resource:knowledge/designs/tmux-superset-roadmap.md`
  - `file:compat/packaged-cli.sh`
- Acceptance:
  - `The divergence matrix records each product decision and packaged-client tests protect the launcher and lifecycle contract.`

### `prompt.command-fidelity`: Complete command-prompt semantics

Exact behavior needs a prompt chain and command queue across attached clients.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `hard`
- Owner: `daemon`
- User impact: scripts, daily
- Items: `flag:command-prompt:-F`, `flag:command-prompt:-l`, `flag:command-prompt:-t`, `option:status-keys`, `semantic:command-prompt-chain`, `semantic:command-prompt-key-spelling`, `semantic:command-prompt-labels`, `semantic:command-prompt-pass-order`, `semantic:command-prompt-vi-editing`, `semantic:prompt-message-freeze`
- Depends on: `clients.interactive-refresh`
- Evidence:
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `resource:knowledge/tmux/divergences.md`
  - `file:compat/attached-client.sh`
- Acceptance:
  - `Attached-client tests cover format expansion, prompt chains, labels, key answers, vi editing, fanout, and queue order.`

### `prompt.pane-rendered`: Defer pane-rendered prompts

zz prompts are native client surfaces rather than pane cell overlays.

- Decision: `park`
- Status: `blocked`
- Priority and ease: `later` / `hard`
- Owner: `client`
- User impact: daily
- Items: `flag:command-prompt:-P`
- Depends on: `clients.interactive-refresh`
- Evidence:
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `A product need defines how pane-rendered prompts coexist with native prompt surfaces.`

### `protocol.binary-streams`: Design one bounded command stream

Five commands need the same bounded binary transport; separate transports would duplicate risk.

- Decision: `park`
- Status: `blocked`
- Priority and ease: `later` / `hardest`
- Owner: `protocol`
- User impact: scripts, remote
- Items: `flag:display-message:-I`, `flag:split-window:-I`, `protocol:command-stream`, `semantic:buffer-standard-streams`, `semantic:non-utf8-command-arguments`, `semantic:show-buffer-binary-policy`, `semantic:source-file-stdin`
- Depends on: none
- Evidence:
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `resource:knowledge/designs/tmux-superset-roadmap.md`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `One reviewed protocol covers stdin, stdout, binary bytes, backpressure, cancellation, and process lifetime for every stream command.`

### `protocol.socket-acl`: Defer multi-user socket ACLs

The current daemon and socket are single-user.

- Decision: `park`
- Status: `blocked`
- Priority and ease: `later` / `hardest`
- Owner: `daemon`
- User impact: admin, remote
- Items: `command:server-access`, `semantic:multi-user-socket-acl`
- Depends on: none
- Evidence:
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `resource:knowledge/designs/tmux-superset-roadmap.md`
- Acceptance:
  - `A multi-user deployment need defines identities, authorization, socket ownership, revocation, and audit behavior.`

### `protocol.socket-interop`: Do not speak tmux private protocol

Private socket compatibility would replace zz's cross-client protocol without improving the shell alias.

- Decision: `never`
- Status: `accepted`
- Priority and ease: `none` / `none`
- Owner: `protocol`
- User impact: admin
- Items: `protocol:tmux-private-socket`
- Depends on: none
- Evidence:
  - `resource:knowledge/designs/tmux-superset-roadmap.md`
- Acceptance:
  - `CLI and config compatibility remain the contract; the private tmux wire stays out of scope.`

### `rendering.geometry-residue`: Close bounded geometry reporting gaps

These mismatches occur where client measurements meet durable mux geometry.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `client`
- User impact: scripts, gui
- Items: `semantic:attached-gui-pane-width`, `semantic:manual-window-size-transition`, `semantic:split-window-zoom-hidden-width`
- Depends on: `clients.attach-context`
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
  - `scenario:compat/scenarios/split-window-zoom.txt`
- Acceptance:
  - `Client convergence and differential tests pin cell geometry during measurement, zoom, and manual policy transitions.`

### `sessions.linked-groups`: Do not add linked session groups

One window belongs to one session in zz.

- Decision: `never`
- Status: `accepted`
- Priority and ease: `none` / `none`
- Owner: `mux`
- User impact: scripts, admin
- Items: `command:link-window`, `command:unlink-window`, `flag:choose-tree:-G`, `flag:kill-session:-g`, `flag:new-session:-t`, `format:session_group`, `format:session_group_attached`, `format:session_group_attached_list`, `format:session_group_list`, `format:session_group_many_attached`, `format:session_group_size`, `format:session_grouped`, `format:window_linked`, `semantic:session-groups`
- Depends on: none
- Evidence:
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `resource:knowledge/designs/tmux-superset-roadmap.md`
- Acceptance:
  - `The catalog rejects group-only syntax loudly and the divergence matrix keeps the permanent exclusion visible.`

### `source-file.event-hook-client-cwd`: Select the current client cwd for event-hook sources

tmux dynamically selects a current or best attached client for event-hook replay, while zz runs deferred event hooks through its sentinel client; exact session-cwd selection belongs in the shared attach context first.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `hard`
- Owner: `daemon`
- User impact: scripts
- Items: `semantic:source-file-event-hook-current-client-cwd`
- Depends on: `clients.attach-context`
- Evidence:
  - `resource:crates/zz-daemon/src/daemon.rs`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `An event hook that sources a relative path selects the same current or best attached client and session cwd as the pin, including when another client caused the event.`

### `source-file.sourced-hook-client-cwd`: Keep the invoking client for hooks raised during source replay

tmux copies the original queue client onto commands loaded from a file. zz executes each ordinary sourced command as ClientId::MAX, so a hook raised by that command starts a new sentinel-client source invocation outside the stable recursion base.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `hard`
- Owner: `daemon`
- User impact: scripts
- Items: `semantic:source-file-sourced-hook-client-cwd`
- Depends on: none
- Evidence:
  - `resource:crates/zz-daemon/src/daemon.rs`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `A hook raised by an ordinary sourced command inherits the outer queue client's selected cwd, so a relative source inside that hook loads the client-root file instead of the containing-file or daemon-home decoy.`

### `source-file.startup-client-cwd`: Bootstrap startup sources from the initial client cwd

Pinned tmux keeps cfg_client available while startup configuration runs, so server_client_get_cwd uses the launching client's cwd. zz loads startup configuration before any client registers and has no initial-client cwd to select.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `hard`
- Owner: `protocol`
- User impact: scripts
- Items: `semantic:source-file-startup-initial-client-cwd`
- Depends on: none
- Evidence:
  - `resource:crates/zz-daemon/src/daemon.rs`
  - `resource:knowledge/tmux/divergences.md`
  - `resource:knowledge/designs/tmux-superset-roadmap.md`
- Acceptance:
  - `When the first client launches a new server from a cwd different from the daemon process, relative startup sources use that initial client cwd until startup completes, including nested replay and a metacharacter-bearing path.`

### `terminal.key-control`: Complete terminal key control flags

The terminal input path exists but lacks several tmux key and reset operations.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `terminal`
- User impact: scripts, daily
- Items: `flag:send-keys:-K`, `flag:send-keys:-R`, `flag:send-keys:-c`, `semantic:send-keys-copy-command-shape`, `semantic:send-keys-empty-copy-count`, `semantic:send-keys-high-hex`, `semantic:send-keys-no-key-count`
- Depends on: none
- Evidence:
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Terminal and attached-client tests cover reset, clear, key-table injection, high bytes, counts, and copy commands; both a bare no-key -N count and `send-keys -N <n> -X` with no action live on the pane mode and survive the pin's cross-client invocation rules.`

### `terminal.resize-pane-trim`: Add terminal history trim action

The mux cannot inspect cursor, history, or terminal mode state.

- Decision: `adopt`
- Status: `blocked`
- Priority and ease: `later` / `medium`
- Owner: `terminal`
- User impact: daily, scripts
- Items: `flag:resize-pane:-T`
- Depends on: none
- Evidence:
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `resource:knowledge/designs/tmux-superset-roadmap.md`
- Acceptance:
  - `One atomic terminal action trims cursor-derived history, advances the cursor, and no-ops in pane mode.`

### `tracker.semantic-coverage`: Close the remaining semantic discovery blind spots

Oracle schema 4 records all 14 callback-bearing tmux commands as six effective `args_parse` rules. The Rust catalog mirrors the 12 implemented commands, while per-command items mark each rule absent from `COMMAND_ARGS_PARSE_BEHAVES`. Hook production, option consumption, open-ended formats, and shared binding behavior still need source-owned registrations.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `next` / `medium`
- Owner: `protocol`
- User impact: scripts
- Items: `args-parse:bind-key`, `args-parse:choose-buffer`, `args-parse:choose-tree`, `args-parse:command-prompt`, `args-parse:confirm-before`, `args-parse:display-menu`, `args-parse:display-panes`, `args-parse:if-shell`, `args-parse:run-shell`, `args-parse:set-hook`, `args-parse:set-option`, `args-parse:set-window-option`, `semantic:tracker-daemon-invalid-flag-runtime`, `semantic:tracker-hook-producer-partition`, `semantic:tracker-key-binding-behavior`, `semantic:tracker-nonconstant-format-behavior`, `semantic:tracker-open-context-format-vocabulary`, `semantic:tracker-option-consumer-registration`
- Depends on: none
- Evidence:
  - `resource:compat/tmux-oracle.py`
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `resource:crates/zz-mux/src/compat_manifest_tests.rs`
  - `resource:knowledge/playbooks/compat-harness.md`
- Acceptance:
  - `Each args-parse item moves to COMMAND_ARGS_PARSE_BEHAVES after tests prove that the runtime parser applies its pinned rule.`
  - `Producer- or consumer-owned inventories reconcile hook production, shared key behavior, nonconstant and open-ended context formats, option consumption, and daemon invalid-flag handling against the live manifest.`

## Known differential scenarios

| Scenario | Gap | TOPO | GEO | FMT | OUT | WARN |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| known/known-main-preset-two-panes.txt | layout.main-horizontal-upstream-bug | 0 | 1 | 0 | 0 | 0 |
| known/known-spread-mixed.txt | layout.spread-mixed-upstream-bug | 0 | 1 | 0 | 0 | 0 |

## Closed history

| ID | Closed | Resolution | Evidence |
| --- | --- | --- | --- |
| `alerts.remaining-edge-cases` | 2026-08-24 | The session_activity_flag and session_silence_flag formats now mirror the resolved target window, so list-sessions reads the active window and list-windows varies per row. Attach clears bell, activity, and silence flags only on the session's active window and releases every terminal bell latch there before producing the snapshot. Alert action gating and message labels are decided once from that active window and fan the same decision to every eligible Interactive client while the broader per-client focus model remains unchanged. Every successful monitor-silence write, including a same-value write or repeated global reset, emits MonitorSilenceChanged and resets every live window timer; a missing local -u and a rejected -o do not. Active status messages are dismissed before dispatch by a surviving bulk Text packet or explicit Paste as well as by writable key presses, while suppressed trailing text and every read-only input leave them armed. Alert-produced messages still bypass the daemon-owned lifecycle, including the pin's terminal-publication freeze, and remain open under alerts.message-lifecycle. | `resource:crates/zz-mux/src/formats.rs`, `resource:crates/zz-mux/src/command.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `scenario:compat/scenarios/alerts.txt`, `resource:knowledge/tmux/divergences.md`, `resource:knowledge/tmux/status-line.md` |
| `aliases.client-preflight` | 2026-08-25 | For local default and explicit socket endpoints, the CLI now asks an already-running compatible daemon to prepare the complete argv chain once without spawning. Canonical identity and alias-match metadata decide native attach, TUI new-session or attach, agent-send stdin capture, and kill-server recovery before any client-owned preprocessing. Exact attach and attach-session shadows stay in command mode, arbitrary aliases to attaching commands enter the TUI, aliases to agent-send read stdin, and an agent-send shadow does not. Command execution reuses the prepared connection; TUI carries the typed immutable vector across its second connection and sends prepared requests without a second alias lookup. The CLI scans every typed result before stdin capture, attach or TUI routing, and execution, so a later preparation error cannot fall through or follow an earlier effect. A bare unaliased kill-server enters verified incompatible-daemon recovery only for transport or handshake failure; alias errors and nonzero results return normally and leave the daemon alive. Raw --kill-server remains an unaliasable process selector, preparation failure falls open to the previous static routing, --restart-daemon still recovers after a failed preflight, and preparation never autospawns. Remote --host preprocessing remains open under aliases.remote-client-preflight, config replay snapshotting remains under aliases.config-parse-unit, and replay-group parse abort remains under mux.chain-parse-abort. | `resource:crates/zz/src/lib.rs`, `resource:crates/zz-daemon/src/client.rs`, `resource:crates/zz-tui/src/lib.rs`, `file:crates/zz/tests/cli_binary.rs`, `scenario:compat/scenarios/smoke/control-alias-prepare.txt`, `scenario:compat/scenarios/smoke/cli-chain-parse-abort.txt`, `resource:knowledge/tmux/divergences.md` |
| `aliases.control-prepare` | 2026-08-25 | Protocol v74 appends request-correlated PrepareCommandList and PreparedCommandList messages and a prepared bit on CommandRequest. The daemon prepares each complete input vector under one mux lock, expanding exactly one live alias layer, preserving source and caller arguments, returning canonical identity plus alias-match and typed error state, and performing no execution, target or format resolution, hook emission, message publication, or authorization. Control prepares the initial argv unit before its flags-0 result and every complete LF line before any flags-1 frame, preserves bare initial unknown errors, whole-line parse-error framing, numbering, ignored lines, queued stdin, and interleaved notifications, then executes the immutable prepared invocations without a second alias lookup. Each request carries a nonzero request id, stale replies are ignored, every ClientKind handles the RPC, and a prepared line keeps the alias snapshot even when an earlier command mutates command-alias. The prepared bit is an internal freeze marker rather than an authority token: a forged prepared request bypasses alias lookup by design but still passes the ordinary read-only authorization gate, and a destructive forged request from a read-only client is rejected without mutation. Local ordinary CLI preprocessing is closed under aliases.client-preflight; remote preprocessing remains under aliases.remote-client-preflight, and actual empty and multi-command alias bodies remain under aliases.command-bodies. | `resource:crates/zz-protocol/src/message.rs`, `resource:crates/zz-daemon/src/client.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `resource:crates/zz/src/control_mode.rs`, `file:crates/zz/tests/cli_binary.rs`, `resource:knowledge/protocol/wire-protocol.md`, `resource:knowledge/tmux/divergences.md` |
| `aliases.typed-resolution` | 2026-08-25 | MuxEngine::resolve_command_alias now returns Miss, Expanded, or MatchedUnsupported with empty, multiple-command, and unparsable reasons. Every mux and daemon caller converts a matched unsupported body to `unknown command: <typed name>` instead of falling through to a shadowed canonical name or catalog alias. Direct execution, daemon routing, terminal-qualified replay, bind-key and set-hook storage, option-command normalization, stored binding dispatch, and read-only preflight all share that result. Writable stored bindings resolve each command immediately before its dispatch, so an earlier command may change the alias observed by a later command; alias-resolution failures use the same command-output and key-command-failed path as other per-command failures. Read-only bindings instead resolve and authorize the whole chain before any effect. Focused regressions prove `kill-server=`, multi-command `list-windows` and `lsw` shadows, and an unparsable `kill-session` shadow cannot shut down, list, kill a session, replace a binding, install a hook, or change an option command. Alias resolution remains one layer, and a malformed shadow refuses before read-only authorization. `aliases.command-bodies` remains open for actual empty and multi-command execution; local CLI preprocessing is closed under aliases.client-preflight, while remote preprocessing remains under aliases.remote-client-preflight. | `resource:crates/zz-mux/src/command.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `resource:knowledge/tmux/divergences.md`, `resource:knowledge/designs/tmux-superset-roadmap.md` |
| `choosers.cross-client-pane-mode-routing` | 2026-08-25 | Choose-tree and choose-buffer now account each writable native action or raw key exactly once against the source session, advance that client as the latest geometry owner, and preserve pane bells. A raw key routes through chooser bindings only when that same client owns the chooser; another client's key follows its own ordinary input path. Read-only raw keys bypass the retained chooser into normal root-table resolution, while read-only dedicated chooser actions and rejected terminal-view input update activity and latest geometry before rejection without clearing the bell. Read-only-safe local view actions also bypass the retained chooser, reach the pane, and account exactly once. Activating a different session records the source-session input first and then the target-session attach, preserving the pin's two legitimate activity boundaries. Pane Focus and ClearLinkHover remain outside chooser activity accounting. | `resource:crates/zz-daemon/src/daemon.rs`, `resource:knowledge/tmux/status-line.md`, `resource:knowledge/tmux/divergences.md` |
| `choosers.presentation-consistency` | 2026-08-24 | Protocol v72 appends a durable filter_no_matches bit to both chooser states. A full daemon rebuild sets it only when an explicit static -f filter produced no rows and the chooser restored its unfiltered rows; a matching filter or no filter clears it, while incremental search and selection deltas preserve it. TUI and GUI render the native status `filter: no matches` without replacing the selectable fallback rows. The GUI reserves its 46px shortcut cell for every rendered row only when at least one rendered row has a nonempty key, and removes the cell for a fully keyless list, matching the TUI's list-level gutter decision. The real attached-client fixture requires the status independently on the current tree and buffer chooser screens for both zz and pinned tmux. Native layout differences remain under choosers.native-presentation, while key vocabulary remains under choosers.command-flags. | `resource:crates/zz-protocol/src/message.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `resource:crates/zz-tui/src/render.rs`, `resource:crates/zz-ui/src/chooser.rs`, `file:crates/zz-client/src/core.rs`, `file:compat/attached-client.sh`, `resource:knowledge/tmux/choose-tree.md` |
| `clients.cwd-context` | 2026-08-23 | Protocol v72 carries a bounded cwd only for local endpoints; top-level command-client source-file resolves after -F and before globbing, with attached session-cwd selection retained in clients.attach-context. | `resource:crates/zz-protocol/src/message.rs`, `resource:crates/zz-daemon/src/client.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `file:crates/zz/tests/cli_binary.rs`, `file:compat/attached-client.sh`, `scenario:compat/scenarios/source-file-format.txt` |
| `clients.detach-targeting` | 2026-08-25 | The catalog accepts detach-client -t, and one no-wire daemon resolver now serves detach-client and switch-client. Explicit targets resolve exact attached client names, native device names, full tty paths, tty paths without /dev/, and one trailing-colon variant before any -s lookup. Without a selector, an attached Interactive or Control client selects itself; a Command client selects the best client on its origin pane's session, then falls back to the best client on the most recently active attached session. detach-client -s wins over -a and quietly does nothing when its source session is missing; -a detaches every attached peer except the resolved target, while bare detach selects only that client. A read-only client may detach only itself. Requested detach publishes the existing Requested reason without a by-client, while attach-session -d eviction retains Evicted; unregister cleanup remains unchanged. Local terminal surfaces always publish a discovered tty, so switch-client and detach-client retain their full-tty and /dev/-stripped aliases even outside nested tmux; remote clients still omit the host tty. The separate client-nested-v1 hello capability is emitted only when TMUX is nonempty and gates nested refusal together with an exact pane-tty match. Focused mux and daemon tests cover parsing, selector and source precedence, origin and fallback selection, aliases, read-only authorization, reasons, victim sets, and nested-marker lifecycle. The attached-client fixture obtains each real outer PTY, targets the client by that tty, and requires the attached client count to reach zero. For nested refusal it removes whitespace from the complete pane history and fixed pin text, snapshots the count of exact normalized substrings before each attempt, requires that count to increase afterward, and holds the attached count at one through a settle for both attach-session and new-session -A. It then proves env -u TMUX admits both paths and uses the retained root tty to detach only the forced nested client. The later clients.tty-basename-targeting closure aligned every supported client-selector caller and removed the display-panes basename widening. The later clients.local-control-terminal-facts closure extended only tty identity and nested intent to a local Control client whose stdin is a terminal. detach-client -E and parent-HUP exit actions remain separate open gaps. | `resource:crates/zz-protocol/src/catalog.rs`, `resource:crates/zz-protocol/src/message.rs`, `resource:crates/zz-mux/src/command.rs`, `resource:crates/zz-daemon/src/client.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `file:crates/zz-mux/tests/hunt_claims.rs`, `file:compat/attached-client.sh`, `resource:knowledge/tmux/divergences.md` |
| `clients.local-control-terminal-facts` | 2026-08-25 | A local Control connection now publishes its bounded working directory plus client-tty-v1 only when stdin has a discoverable tty and client-nested-v1 only when TMUX is nonempty. It does not inspect terminal size, publish client-size-v1, send ClientTerminalSize, infer geometry, retain TERM or terminal-name facts, or expose its tty through ClientFormatFacts. Control geometry remains explicit refresh-client -C state. The established attach-session, new-session -A, and new-session -Ad refusal paths require both the nested marker and a tty that exactly matches a daemon pane when they would attach an existing session. A fresh new-session and a new-session -A miss still create and attach, while duplicate and validation errors keep their existing precedence. Piped Control stdin has no tty identity and is not nested-refused merely because TMUX is set. The change reuses the existing additive hello capability tokens and unregister cleanup, with no field, tag, or protocol-version change. The sequential daemon suite passed 600/600, the focused Control CLI suite passed 30/30, and debug build, strict clippy, and fmt passed. The complete attached-client differential passed for zz and pinned tmux, including terminal-backed Control refusal, fresh-session attachment, and piped-stdin non-refusal. Independent review found the fresh-marker harness sound. This closure makes no canonical-suite claim. Broader attach sizing remains open under clients.attach-context, and clients.context-formats remains open. | `resource:crates/zz-protocol/src/message.rs`, `resource:crates/zz-daemon/src/client.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `resource:crates/zz/src/control_mode.rs`, `file:crates/zz/tests/cli_binary.rs`, `file:compat/attached-client.sh`, `resource:knowledge/protocol/wire-protocol.md`, `resource:knowledge/tmux/commands.md`, `resource:knowledge/tmux/divergences.md` |
| `clients.nested-attach-signal` | 2026-08-25 | The additive client-nested-v1 hello capability records whether an eligible local Command or terminal-surface process inherited a nonempty TMUX value; the existing client-tty-v1 value remains unconditional when that process has a discoverable local terminal. The daemon retains both facts independently and refuses attach-session or an attaching new-session -A only when the nested marker is present and the tty exactly matches one of its panes. Unsetting TMUX therefore forces either attach path without deleting the tty needed by ordinary client targeting. Remote endpoints publish neither host tty nor nested marker. Unregister removes both retained facts. This adds no field, tag, or protocol-version change. Focused client and daemon tests cover marker emission, retention, refusal, forced attach, tty targeting, and cleanup. The attached-client fixture removes whitespace from the complete pane history and fixed refusal literal, snapshots the count of exact normalized substrings before each attempt, requires that count to increase afterward, and holds the attached count at one through a settle. The typed commands contain no refusal text, so neither command echo nor the earlier attach-session refusal can satisfy the new-session -A proof, while terminal-query prefixes and physical-line wrapping remain tolerated. The later clients.local-control-terminal-facts closure applies the same two independent facts to local Control only when stdin supplies the tty identity. | `resource:crates/zz-protocol/src/message.rs`, `resource:crates/zz-daemon/src/client.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `file:compat/attached-client.sh`, `resource:knowledge/protocol/wire-protocol.md`, `resource:knowledge/tmux/divergences.md` |
| `clients.read-only-local-view-actions` | 2026-08-25 | Read-only clients can use direct local scroll, copy-mode entry, movement, history, line, word, paragraph, prompt, bracket, goto-line, set-mark, jump-to-mark, and cancel actions. One typed copy-action classifier also authorizes `send-keys -X`; selection, copying, search, rectangle, jump capture, paste, clear-history, raw mouse, mixed wheel, and application pane-focus effects remain blocked. Non-`-X` `send-keys`, including an otherwise unsupported `-M` request, is rejected as read-only before repeat or full option parsing. Pin-recognized but zz-unimplemented unsafe copy actions reject; genuinely unknown and empty `-X` actions retain the pin's later no-op or no-mode path. Direct command requests and an entire stored binding chain are preflighted before any effect, including one-layer aliases, and rejected paths report `client is read-only` without terminal or browser input. Safe actions bypass retained chooser and display-panes surfaces, reach the pane, and update activity and latest geometry exactly once without clearing bells or dismissing the modal. Rejected non-focus actions, including mouse input, preserve the same bell and accounting behavior. Pane Focus is rejected without activity because v73 ClientFocus owns client-window accounting. A read-only command cannot fan a local view effect into a pane outside its attachment. The accepted uncoupled ignore-size and same-uid differences remain under `clients.read-only-and-focus`; committed-text accounting is closed separately. | `resource:crates/zz-mux/src/command.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `resource:knowledge/tmux/commands.md`, `resource:knowledge/tmux/divergences.md` |
| `clients.tty-basename-targeting` | 2026-08-25 | Every implemented tmux command flag that selects an attached client now shares exact-name, full-tty, and exactly one leading-/dev/-prefix removal, with exactly one optional trailing colon. The shared device-N alias remains, while popup, menu, confirm, refresh, and lock keep their documented numeric and client-N native aliases. A final pathname basename is not a selector: /dev/pts/3 accepts pts/3 but rejects 3 unless 3 is the exact client name. When aliases collide across sessions, the globally oldest attached client by creation id wins; switching sessions does not reorder it. This covers detach-client -t, switch-client -c, display-message -c, display-panes -t, display-popup -c, display-menu -c, confirm-before -t, refresh-client -t, lock-client -t, and load-buffer -t format context. Unsupported command-prompt -t, show-messages -t, send-keys -c, and suspend-client -t remain with prompt.command-fidelity, messages.tty-model, terminal.key-control, and commands.native-client-tools. set-buffer -t remains accepted but inert and is not part of this selector closure. The sequential daemon suite passed 598/598; focused selector tests, a debug build, strict daemon clippy, and fmt passed. Scoped attached-client guards passed for zz and pinned tmux, but the full attached-client harness later blocked on unrelated nested-attach interleaving, so this close makes no full-harness or canonical-suite claim. | `resource:crates/zz-daemon/src/daemon.rs`, `file:compat/attached-client.sh`, `resource:knowledge/tmux/commands.md`, `resource:knowledge/tmux/divergences.md`, `resource:knowledge/designs/tmux-superset-roadmap.md` |
| `commands.native-prefix-isolation` | 2026-08-24 | Exact canonical names and aliases resolve first. Non-exact lookups use tmux canonical names whenever any match exists and consult the guarded 19-name native roster only when tmux has no match. The daemon expands one immutable user-alias layer before read-only authorization and reuses it for dispatch and hooks. Writable stored bindings resolve per command, while read-only bindings resolve and authorize the whole chain before any effect. Non-exact attach prefixes execute through the interactive command path, static agent-send prefixes trigger stdin capture, and protocol v74 prepares Control command units against the live alias table before framing. The manifest gate derives the native roster from catalog minus the pinned oracle and checks every pinned canonical prefix; the strict 29-step scenario covers all 25 affected unique prefixes, exact catalog alias precedence, user command-alias expansion, and ambiguous list-commands exit parity. Local exact attach, stdin, and kill recovery preprocessing now consumes the prepared canonical identity and alias-match state under aliases.client-preflight; remote --host routing remains under aliases.remote-client-preflight. | `resource:crates/zz-protocol/src/catalog.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `resource:crates/zz/src/lib.rs`, `file:crates/zz-mux/src/compat_manifest_tests.rs`, `file:crates/zz/tests/cli_binary.rs`, `scenario:compat/scenarios/native-prefix-isolation.txt` |
| `commands.tmux-name-extensions` | 2026-08-24 | The pin's outer send-keys grammar is c:FHKlMN:Rt:X. zz removed C, P, and o from the outer catalog and returns the exact unknown-flag error when they appear there; the tracked c, K, and R gaps remain under terminal.key-control, and M remains under mouse.bound-context. The copy-mode parser recognizes -C and -P on the pin's 14 copy-family grammar entries and -o on next-prompt and previous-prompt; -- terminates its flag scan. Invalid local flags, actions, and arity produce no command error or copy action and reset the copy-mode repeat prefix to 1. Existing CopyModeCopy clipboard, paste-buffer, and pipe fields retain their behavior. Execution for copy-line, copy-line-and-cancel, copy-pipe-line, and copy-pipe-line-and-cancel remains under terminal.key-control through semantic:send-keys-copy-command-shape. The pin also redraws the first copy-mode line after a local parser failure; zz has no no-op redraw effect, so that presentation residue stays with the same item. | `resource:crates/zz-protocol/src/catalog.rs`, `resource:crates/zz-mux/src/command.rs`, `resource:knowledge/tmux/divergences.md`, `scenario:compat/scenarios/micro-flags.txt` |
| `config.replayed-command-errors` | 2026-08-25 | Config replay now keeps source diagnostics and command runtime failures in one ordered report stream. Typed target failures and semantic errors from a syntactically valid set-option or set-window-option command use the pin's bare message instead of a file-prefixed parser diagnostic. Command clients receive stderr and exit 1. Protocol v76 Control clients receive each parser-owned failed replay command inside its own flags-1 error guard and set retval 1 for a later blank line, EOF, or nonself detach. The later control-mode.source-file-exit-status closure proves that an actual self-detach exits 0 and that a pending Return captured during a preceding non-detach command keeps its arrival-time snapshot ahead of later queued stdin. A Return observed while self-detach itself waits is discarded when the caller's Detached event arrives. Attached clients receive the capitalized warning form. A containing source-file propagates the same message and exit status while inner and outer later lines still run. Unknown command names and lexer or malformed command diagnostics retain the existing file-prefixed `%config-error` path; generic Warning identity remains open under control-mode.diagnostic-typing. Clientless startup still retains no invoking error channel and remains under config.startup-diagnostic-delivery. The later config.replayed-command-output closure preserves stdout before and after a runtime failure and batches only command-name and parser diagnostics into the transcript at their invocation boundary. Runtime, no-match, glob, and actual OS or path read failures retain their existing error channels. The later control-mode.indirect-source-frames closure adds inherited flags-1 framing for foreground shell-evaluated `if-shell`, immediate `if-shell -F` including `-bF`, and foreground `run-shell -C`. At this checkpoint immediate-hook and background-callback flags-0 framing remained open under control-mode.hook-command-frames and control-mode.background-inserted-command-frames. Later closures closed both groups. Non-UTF-8 config content remains under config.non-utf8-file-bytes. At this close, the focused source-file-diagnostics and source-file-control scenarios passed 11 and 5 steps with no topology, geometry, format, output, or warning differences and no skips. Later default-order and nested-queue proofs grew the focused rows to 12 and 6; the return-status closure grew the focused source-file-control run to eight. None of those partial runs refreshed the stored canonical source-file-control row, which remains at three steps. | `resource:crates/zz-mux/src/command.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `resource:crates/zz/src/control_mode.rs`, `file:crates/zz/tests/cli_binary.rs`, `scenario:compat/scenarios/smoke/source-file-diagnostics.txt`, `scenario:compat/scenarios/smoke/source-file-control.txt`, `resource:knowledge/tmux/divergences.md` |
| `config.replayed-command-output` | 2026-08-25 | Runtime source-file now retains one stdout transcript per invocation. It parses every declared and globbed match before replay, appends the complete top-level -v batch in declared-path, glob, and physical-line order, replays the parsed files in the same order, then appends buffered command-name and parser diagnostics. Source no-match, glob, and actual OS or path read failures retain their existing error channels instead of joining the stdout transcript. Non-UTF-8 config content remains under config.non-utf8-file-bytes. Each nested source-file creates the same verbose, replay, command-diagnostic frame at its parent command's replay position, so nested frames run depth-first. This matches the pin's per-invocation batching and does not claim physical verbose and replay interleaving. Command clients receive sourced display-message -p, list-sessions, hook, and continuation output once on stdout. Successful output leaves stderr empty and status zero; a runtime failure keeps its original stderr and status 1 while stdout before, hook output, later output, and list output remain ordered. For syntactically valid successful replay and -v output, Interactive clients open one command-output view without duplicate Info or Warning events. Parser diagnostics may still publish their existing Warning summary. The attached fixture proves transcript presentation and q dismissal; clients.tui-command-output-navigation retains page and line movement, search, selection and copy, custom tables, and mode-key behavior. Direct Control replay keeps -v suppressed and uses its existing per-command guards. The later control-mode.indirect-source-frames closure extends flags-1 framing to synchronous foreground shell-evaluated if-shell, immediate if-shell -F including -bF, and foreground run-shell -C. At this checkpoint immediate-hook and background-callback flags-0 framing remained open under control-mode.hook-command-frames and control-mode.background-inserted-command-frames. Later closures closed both groups. One source-file invocation parses all top-level matches before replay: a bare assignment applies during parsing, affects a later file's conditional, and persists, while a replayed set-environment command runs too late to affect a later file's already parsed branch but persists after replay. With -n, neither assignment nor command effects apply, later parse-only files see the assignment as absent, and -v still reports the selected branch. Clientless startup creates no replay transcript, so the detached launcher keeps empty stdout and stderr. A separate manual probe of pinned tmux d77c9dc6, outside the 12-step runtime scenario, found that startup display-message -p text becomes a file-and-line config cause while list output is discarded; config.startup-diagnostic-delivery owns that later-client delivery. | `resource:crates/zz-mux/src/command.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `resource:crates/zz/src/control_mode.rs`, `file:crates/zz/tests/cli_binary.rs`, `file:compat/attached-client.sh`, `scenario:compat/scenarios/source-file-output.txt`, `resource:third_party/tmux-reference/UPSTREAM.md`, `resource:knowledge/references/tmux-upstream.md`, `resource:knowledge/tmux/conf-parser.md`, `resource:knowledge/tmux/divergences.md` |
| `config.same-line-error-group` | 2026-08-24 | Config and source replay now keys each parser-owned command group by exact source and physical line. A synchronous invalid or runtime command error, a nested source depth refusal, or a loud source-file no-match or glob error with zero matched files drops only later siblings from that group; the next physical line still runs. A quiet no-match is success. A matched path puts source-file on its asynchronous wait path, so child replay and parser failures, child read failures, and mixed missing-and-matched arguments do not prune the invoking line; zz retains a matched child read failure in the load report instead of returning it into parent group control flow. An asynchronous run-shell failure does not prune either. Both sides also keep the same-line sibling for a `-` path, while zz's missing stdin transport remains open under protocol.binary-streams. Equal line numbers in separate source files remain independent. zz-classified UnsupportedCommand results now skip and continue later same-line siblings; before this slice they pruned those siblings. That new continuation is desirable for zz import capability gaps, but this slice has no pinned proof that those gaps represent synchronous tmux failures. Direct CLI and Control chains are unchanged. Replayed error delivery and parser-owned sourced Control frame ownership closed under config.replayed-command-errors and control-mode.sourced-command-frames. The later config.replayed-command-output closure places each nested invocation's complete verbose, replay, and command-name or parser diagnostic transcript frame at this asynchronous source position. Source no-match, glob, and actual OS or path read failures keep their existing error channels at that position. The later source-file.nested-control-queue closure proved cross-depth containing-before-child order for parser-owned recursion. The later control-mode.indirect-source-frames closure covers synchronous foreground inserted lists. At this checkpoint immediate-hook and background-callback flags-0 framing remained open under control-mode.hook-command-frames and control-mode.background-inserted-command-frames. Later closures closed both groups. Unsupported-command continuation still does not prove parity for ConfigLoadReport's skipped summary. Parser abort behavior and error shapes remain open under config.parser-edge-cases and mux.error-shapes. | `resource:crates/zz-daemon/src/daemon.rs`, `file:compat/scenarios/smoke/fixtures/source-file-depth.sh`, `scenario:compat/scenarios/smoke/source-file-depth.txt`, `resource:knowledge/tmux/divergences.md` |
| `control-mode.background-inserted-command-frames` | 2026-08-26 | Shell-evaluated if-shell -b and run-shell -bC now retain the originating Control client separately from replay_client and map its inserted work to protocol v77 flags 0. The triggering flags-1 frame closes first, later flags-1 input may overtake the callback, and each inserted command, source command, and sourced child receives an independent frame in parent-before-child order. Successful and false-branch output stays inside its callback frame. Missing sources and runtime failures end their flags-0 frames with %error and set sticky status 1; a later flags-1 success does not clear that status before Return. Both producers keep a malformed delayed command list silent, unframed, and status-neutral. Ordinary run-shell -b shell jobs remain unframed, immediate if-shell -bF remains synchronous flags 1, and foreground run-shell -C retains the closed synchronous path. The callback clears replay_client and checks that its exact Control client is still registered before callback execution begins. A client disconnected before that point gets no callback frame or inserted side effect, and no output migrates to a later client. control-mode.disconnect-cancels-command-queue owns hard disconnect after an immediate hook or source queue has already started. Non-Control background behavior is unchanged. The implementation reuses protocol v77 without a wire bump. Focused daemon and serialized Control CLI regressions pass, shell syntax and formatting checks pass, and the strict 11-step smoke/source-file-control differential has zero topology, geometry, format, output, or warning differences with no skips. The partial differential leaves the stored canonical summary unchanged. | `resource:crates/zz-daemon/src/daemon.rs`, `resource:crates/zz-protocol/src/message.rs`, `resource:crates/zz/src/control_mode.rs`, `file:crates/zz/tests/cli_binary.rs`, `file:compat/scenarios/smoke/fixtures/source-file-control.sh`, `scenario:compat/scenarios/smoke/source-file-control.txt`, `resource:third_party/tmux-reference/UPSTREAM.md`, `resource:knowledge/references/tmux-upstream.md`, `resource:knowledge/protocol/wire-protocol.md`, `resource:knowledge/tmux/divergences.md` |
| `control-mode.hook-command-frames` | 2026-08-26 | Protocol v77 renames tail-tag-47 SourcedCommandGuard in place to ControlCommandGuard and adds frame flags while renaming the independent retained-status bit to sticky_failure. Parser-owned replay installs a Control command target with flags 1 separately from replay_client. Immediate after-command and command-error hooks retain that recipient, clear replay_client, enter the no-hooks state, and give every hook command, hook source command, and sourced descendant its own flags-0 frame. Multiple hook array entries remain ordered; one failing command stops only its command-list entry, later entries and later parser-owned flags-1 commands continue. Output and errors do not fold into the trigger. A mixed missing-and-matched hook source ends its flags-0 guard with %end but sets sticky_failure, so later success cannot clear retval 1. Unknown or ambiguous sourced commands are rejected before command execution, publish only %config-error, and do not fire command-error. Alias resolution is frozen once before source classification and execution. set-hook -R copies only the hidden Control target into a retargeted hook context, without copying replay-client cwd semantics. At this checkpoint background shell callbacks and deferred event hooks cleared the target, while per-client, per-thread RAII capture prevented cross-thread interception or recipient leakage. The later control-mode.background-inserted-command-frames closure makes background shell callbacks retain their exact Control target through callback entry; deferred event hooks still clear it. Parser and hook-source OS or path read failures retain external raw-placement and invisible source-completion numbering gaps under control-mode.hook-source-read-diagnostics; sourced-hook cwd remains under source-file.sourced-hook-client-cwd, deferred event-hook routing remains under source-file.event-hook-client-cwd, and missing producers remain under hooks.queue. Protocol tests pass 175 unit plus 14 manifest tests, Control tests pass 27, zz-client passes 37 unit plus two simulator tests, source_file and replayed clusters pass 6 and 5, and focused daemon and CLI hook regressions pass. Strict clippy across protocol, client, mux, daemon, and zz, formatting, shell syntax, and diff checks pass. The strict ten-step smoke/source-file-control differential has zero topology, geometry, format, output, or warning differences and no skips. The partial run leaves the stored canonical summary unchanged. | `resource:crates/zz-protocol/src/message.rs`, `resource:crates/zz-mux/src/command.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `resource:crates/zz/src/control_mode.rs`, `resource:crates/zz-client/src/core.rs`, `file:crates/zz/tests/cli_binary.rs`, `file:compat/scenarios/smoke/fixtures/source-file-control.sh`, `scenario:compat/scenarios/smoke/source-file-control.txt`, `resource:third_party/tmux-reference/UPSTREAM.md`, `resource:knowledge/references/tmux-upstream.md`, `resource:knowledge/protocol/wire-protocol.md`, `resource:knowledge/tmux/conf-parser.md`, `resource:knowledge/tmux/divergences.md` |
| `control-mode.indirect-source-frames` | 2026-08-25 | Synchronous inserted command lists reached during parser-owned Control replay now retain the original Control recipient. This covers foreground shell-evaluated if-shell without -b, immediate if-shell -F including -bF, and foreground run-shell -C. The daemon captures nested guard and diagnostic events per client and thread, then publishes one flags-1 frame for the triggering replay command, each inserted command, an inserted source command, and each sourced child in parent-before-child order. Output, source-command errors, runtime client_failure, retained status, unsupported-command continuation, and nested success or failure remain command-scoped without folding, duplication, cross-thread interception, or leakage into the next input command. An unsupported zz-only inserted command emits an empty success guard and later inserted siblings continue, but it does not join ConfigLoadReport's skipped summary; the existing command and semantic coverage still owns that reporting gap. An unknown command inside the child file matches the pin's parent success, source success, then `%config-error` Warning without its own guard, so it creates no new gap. The implementation reuses protocol v76 SourcedCommandGuard and does not bump the protocol. At this checkpoint immediate-hook and background-callback flags-0 framing remained open under control-mode.hook-command-frames and control-mode.background-inserted-command-frames. Later closures closed both groups. Ordinary run-shell -b output remains under control-mode.async-command-output. Focused matrix and cross-thread tests pass; the focused source_file set passes 6 of 6 and the replayed set passes 5 of 5. A fresh debug build, strict daemon clippy, formatting, shell syntax, and scoped and global diff checks pass. The strict nine-step smoke/source-file-control and 12-step source-file-output differentials have zero differences and no skips. The stored canonical source-file-control row remains unchanged at three steps. | `resource:crates/zz-daemon/src/daemon.rs`, `resource:crates/zz-mux/src/command.rs`, `resource:crates/zz-protocol/src/message.rs`, `resource:crates/zz/src/control_mode.rs`, `file:crates/zz/tests/cli_binary.rs`, `file:compat/scenarios/smoke/fixtures/source-file-control.sh`, `scenario:compat/scenarios/smoke/source-file-control.txt`, `scenario:compat/scenarios/source-file-output.txt`, `resource:third_party/tmux-reference/UPSTREAM.md`, `resource:knowledge/references/tmux-upstream.md`, `resource:knowledge/protocol/wire-protocol.md`, `resource:knowledge/tmux/conf-parser.md`, `resource:knowledge/tmux/divergences.md` |
| `control-mode.source-diagnostic-typing` | 2026-08-25 | The daemon marks grouped source-read diagnostics sent to Control as ClientMessageKind::Error. The Control client routes those Error events to standalone %error frames without inspecting text, so numeric OS errors, paths containing colon-space, and localized or platform-specific read errors cannot fall through or change classification with sibling order. This closure covers internal typed identity only. A later pinned probe showed that external parser and hook-source read failures instead close the surrounding guard, write raw unframed text, and consume an invisible source-completion command number; control-mode.hook-source-read-diagnostics owns that placement and numbering. Invalid UTF-8 was a zz-side typed read error at this checkpoint; a later pinned lone-0xff probe showed different config-byte semantics and moved that case to config.non-utf8-file-bytes. Protocol v76 sourced-command guards separately carry the parsed source command's own no-match, glob, and depth diagnostics. Config summaries and lexer-owned diagnostics remain generic Warning events behind the prose classifier tracked under control-mode.diagnostic-typing. The known-family Warning fallback remains for legacy producers, while the exact protocol handshake rejects v75 and v76 client-daemon skew before either event path can mix. | `resource:crates/zz-daemon/src/daemon.rs`, `resource:crates/zz/src/control_mode.rs`, `resource:knowledge/tmux/divergences.md` |
| `control-mode.source-file-exit-status` | 2026-08-25 | The long-lived Control front end now matches the pin's return-status and detach precedence across all eight tracked cases. A direct runtime CommandResponse::Error sets retval 1 unless it is a parse or preparation error. Parser-owned sourced runtime failures, nonruntime source-file failures returned as nonzero Success, and typed actual OS or path source-read failures also set retval 1 without making every %error or generic Warning sticky. EOF and a blank input line return the current retval snapshot. A generic nonzero Success such as run-shell exit 3 and flags-1 parse or preparation failures do not set or change retval: a fresh Control client therefore remains at 0, while a prior sticky failure stays at 1. Explicit self-detach after a completed failure exits 0. A self-detach queued while another command is open also exits 0 when stdin remains open. A self-detach plus EOF queued while a preceding non-detach command is waiting returns the pending EOF snapshot of 1 before the later queued detach runs. More generally, a Return captured while a preceding non-detach command waits keeps the retval observed at arrival and precedes later queued stdin commands, including detach. A Return observed while a self-detach command itself waits is discarded when the caller's Detached event arrives, and the self-detach exits 0. The front end treats only an actual caller-targeted Detached event as self-detach. detach-client -a, -t OTHER, -s that excludes the caller, a no-victim selector, and canonical or user aliases targeting other clients keep the Control loop alive and preserve any pending Return, while bare, built-in-alias, user-alias, explicit-self, and caller-including -s forms exit 0. The command response closes with %end before %exit even when the Detached event arrives first. This uses existing CommandResponse and Detached messages with no wire change. Twenty-seven Control units and 34 serialized Control CLI tests pass, including two race probes run five times each. Debug build, strict clippy, and formatting pass. The focused eight-step source-file-control differential has zero differences and no skips, and a manual detach-client -a probe matches the pin. The focused run does not refresh or prove the canonical suite, whose checked-in row still records three steps. config.replayed-command-output later closed Command and attached transcript delivery without changing these retval rules. The later control-mode.indirect-source-frames closure preserves child guard failure state for synchronous foreground shell-evaluated `if-shell`, immediate `if-shell -F` including `-bF`, and foreground `run-shell -C`. At this checkpoint immediate-hook and background-callback flags-0 framing remained open under control-mode.hook-command-frames and control-mode.background-inserted-command-frames. Later closures closed both groups. Non-UTF-8 config content remains under config.non-utf8-file-bytes. Startup diagnostics and generic config diagnostic typing remain under their existing groups. | `resource:crates/zz-daemon/src/daemon.rs`, `resource:crates/zz/src/control_mode.rs`, `file:crates/zz/tests/cli_binary.rs`, `file:compat/scenarios/smoke/fixtures/source-file-control.sh`, `scenario:compat/scenarios/smoke/source-file-control.txt`, `resource:third_party/tmux-reference/UPSTREAM.md`, `resource:knowledge/references/tmux-upstream.md`, `resource:knowledge/tmux/divergences.md` |
| `control-mode.sourced-command-frames` | 2026-08-25 | Protocol v76 appends EventPayload::SourcedCommandGuard at tail tag 47 with output, error, and client_failure fields. Each parser-owned replayed command that survives command-name resolution gets a guard in recursive file order; command aliases that resolve to source-file before this branch keep the same recursion path. Unknown or ambiguous command names and malformed alias names publish a located Warning that Control renders as %config-error, without a guard. Ordinary success and quiet all-miss commands produce empty flags-1 %end guards. Successful command output stays inside that command's guard. A nested source hit plus miss carries its declared-path diagnostic inside %end, while an all-miss, flag or arity failure, runtime failure, and depth refusal end %error. Runtime failures set client_failure, which sets Control retval 1 independently of the frame terminator; parse and source-command diagnostics can use %error without doing so. zz currently sends a matched child OS or path read failure after its parent guard as a typed standalone Error frame. A later pinned probe showed that the external read diagnostic is raw and unframed after a successful source guard and that every source invocation consumes an invisible completion command number; control-mode.hook-source-read-diagnostics owns those remaining mismatches. Invalid UTF-8 was a zz-side typed error at this checkpoint; config.non-utf8-file-bytes owns the later pinned semantic mismatch. Control defers guards FIFO until the direct outer command closes, allocates fresh frame numbers, and cannot leak them into the next input command. Other config command-name and lexer diagnostics remain Warning events on the existing %config-error prose-classification path, so this close does not claim typed lexer diagnostics. The daemon suite passes 593 tests, protocol tests pass 175 unit plus 14 framing tests, the Control cluster passes 31 tests, and the then-five-step source-file-control differential had zero differences and no skips. That partial scenario run did not refresh or prove the canonical suite. The later source-file.nested-control-queue closure proved cross-depth parser-owned diagnostic order and grew the focused row to six steps. The later control-mode.source-file-exit-status closure completed Control return-status and detach precedence and grew the focused run to eight steps, also with zero differences and no skips. That focused run did not refresh the stored canonical row, which remains at three steps. config.replayed-command-output later closed Command and attached transcript delivery with per-invocation verbose batching. The later control-mode.indirect-source-frames closure extends this flags-1 recipient path to synchronous foreground shell-evaluated `if-shell`, immediate `if-shell -F` including `-bF`, and foreground `run-shell -C`. Immediate hook and background callback flags-0 framing closed later under control-mode.hook-command-frames and control-mode.background-inserted-command-frames. | `resource:crates/zz-protocol/src/message.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `resource:crates/zz/src/control_mode.rs`, `resource:crates/zz-client/src/core.rs`, `file:crates/zz/tests/cli_binary.rs`, `scenario:compat/scenarios/smoke/source-file-control.txt`, `resource:knowledge/protocol/wire-protocol.md`, `resource:knowledge/tmux/conf-parser.md`, `resource:knowledge/tmux/divergences.md` |
| `copy-mode.fixed-row-placement` | 2026-08-25 | The top-line, middle-line, and bottom-line actions now move the copy cursor to column zero at the current frozen viewport's top, middle, or bottom row. Each action preserves the viewport offset and bounds its target to the retained revision. The full zz-terminal library suite passes 197 tests. This close covers only those three fixed-row placements; it does not claim history-bottom, logical-line, scrolling, wrapping, or wider action fidelity, which remains active under copy-mode.action-fidelity. | `resource:crates/zz-terminal/src/session.rs`, `resource:knowledge/tmux/copy-mode.md`, `resource:knowledge/tmux/divergences.md` |
| `display-message.ignore-keys` | 2026-08-25 | The catalog accepts display-message -N, and the mux carries its ignore-keys value to the daemon without changing the wire protocol. A positive-effective-duration ordinary Interactive display-message writes the destination client's sticky bit: -N sets it and a plain message clears it. Omitted -d resolves from the destination client's attached session, independent of the pane selected by -t; explicit -d still wins. A positive-duration Interactive PrintOrMessage producer also clears the bit. Explicit or inherited zero duration, clear, expiry, -p, Control clients, a missing destination client, -a, and -I leave it unchanged; detach keeps it with the registered client, while unregister removes it. An active message with the bit set drops writable terminal Key, standalone or paired Text, Paste, non-hover Mouse and wheel input, and ClientFocus before message dismissal, display-panes teardown, prompt handling, key dispatch, and activity accounting, so the message and terminal-publication freeze remain active. An ignored release retires its swallowed press decision without forwarding. Without -N, non-hover mouse and wheel input dismiss an active message; retaining bare hover is an intentional zz presentation adaptation. Ignored presses create their typed pending entry and suppression debt under the same daemon lock, using the committed character from KeyInput.text. Text matching selects the first entry with the same pane and lane, retires the skipped queue prefix and its linked debt, then retires the matched debt while preserving the later suffix. Browser-before-terminal ordering preserves the later terminal debt; terminal-before-browser ordering retires the skipped terminal debt as stale. Read-only input and native BrowserSurface input keep their prior paths. Focused daemon tests cover sticky replacement, destination-session zero and nonzero inheritance, PrintOrMessage reset, expiry, clear, missing-target CANFAIL, inert branches, unregister, every ignored input form, modal ordering, activity, committed text in both lane orders, release cleanup, read-only bypass, and browser-native input. The strict display-message scenario proves -N and -p -N acceptance beside target-client CANFAIL behavior. | `resource:crates/zz-protocol/src/catalog.rs`, `resource:crates/zz-mux/src/command.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `resource:knowledge/tmux/commands.md`, `resource:knowledge/tmux/divergences.md`, `scenario:compat/scenarios/display-message.txt` |
| `display-message.target-client` | 2026-08-25 | The catalog accepts display-message -c, the mux preserves the destination selector independently from -t, and the daemon resolves it against attached clients by exact client name, device name, full tty, tty without /dev/, or one trailing-colon variant. A nonprinting command with a missing destination quietly does nothing. The pin's CANFAIL target state falls forward componentwise: a missing session leaves pane, window, and session facts empty; a resolved session with a missing window uses that session's current window and active pane; and a resolved window with a missing pane uses that window's active pane. Nonprinting calls stay quiet and successful, while -p expands the retained or empty context, including when -c also misses. Parse, arity, delay, and ambiguous-target failures retain their earlier precedence. Nonprinting Interactive messages, freeze, replacement, and sticky -N state belong to the destination client. Omitted -d inherits display-time from that client's attached session rather than the independent -t pane session; explicit -d overrides it. Control destinations receive only their message event, and read-only destinations can receive a message while read-only callers remain subject to command authorization. Format client facts use the destination only when it belongs to the independently retained pane target's session; otherwise an attached target session uses its best client. Printing returns through the caller, uses the same destination-or-fallback format facts, and never arms message state. An attached target session with no -c still supplies no client facts under clients.context-formats. The later display-message.unattached-session-client-fallback closure handles the distinct valid-unattached-target widening. Focused tests cover same-session and cross-session formats and durations, exact-name CANFAIL targets, missing and aliased clients, caller-versus-destination delivery, freeze, -N inheritance, Control, read-only, and printing. The strict 27-step scenario creates a second window, proves that a missing pane retains that distinct valid window, and covers the missing-window and missing-session clauses with and without -c. Bare mouse targets and relative or special target grammar remain separately tracked. The change adds no wire fields or protocol version. | `resource:crates/zz-protocol/src/catalog.rs`, `resource:crates/zz-mux/src/command.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `resource:knowledge/tmux/commands.md`, `resource:knowledge/tmux/divergences.md`, `scenario:compat/scenarios/display-message.txt` |
| `display-message.unattached-session-client-fallback` | 2026-08-25 | For a valid display-message target whose session has no attached clients, format expansion selects the globally most-active attached client when -c is absent, resolves to another session, or misses. Client activity orders candidates, and the oldest-created client wins an equal-activity tie. client_session comes from that selected client's actual attachment, while session, window, and pane facts remain scoped to the retained -t target. With zero attached clients, client facts stay empty; a missing target session leaves client, session, window, and pane facts empty through the existing CANFAIL path. An attached target session without -c still supplies no client facts and remains under clients.context-formats. The fallback changes only the display-message command-format hook. Delivery, duration selection, printing routing and lifecycle, buffer-path context, and Command-client selection are unchanged. Sequential zz-daemon tests pass 599/599. Focused daemon coverage proves global activity selection and zero-client behavior; scoped attached-client probes pass on zz and pinned tmux for absent, cross-session, and unresolved -c cases. The debug build, strict daemon clippy, and fmt pass. One independent run completed the attached-client harness, but later current runs passed the scoped fallback probes and then failed at unrelated nested-attach terminal-query interleaving. The full harness is not stably green, and this close makes no canonical-suite claim. | `resource:crates/zz-daemon/src/daemon.rs`, `file:compat/attached-client.sh`, `resource:knowledge/tmux/commands.md`, `resource:knowledge/tmux/divergences.md`, `resource:knowledge/designs/tmux-superset-roadmap.md` |
| `display-panes.gpui-escape-fallthrough` | 2026-08-25 | A writable valid display-panes selection is consumed without activity. An unmatched raw key or native GPUI Close tears down the overlay and then follows ordinary key dispatch and activity accounting; Close synthesizes Escape so its press falls through while its later release remains swallowed. Non-hover mouse press, drag, release, and wheel input likewise close the overlay before following ordinary terminal-view input, including latest-geometry and bell accounting. Bare buttonless Motion remains consumed without activity as a deliberate native hover-presentation choice. A deadline timeout closes the overlay without fabricating input or activity. Read-only raw keys and Close bypass overlay consumption into root-table resolution, native selection is rejected after activity and latest-geometry accounting, and safe local view actions reach the pane; each retains the overlay and preserves the pane bell. | `resource:crates/zz-daemon/src/daemon.rs`, `resource:knowledge/tmux/status-line.md`, `resource:knowledge/tmux/divergences.md` |
| `formats.buffer-context` | 2026-08-25 | List-buffers supplies row-local buffer facts to -F and -f, while choose-buffer supplies the same facts to -f and -K. Ordinary expansion selects the newest automatic buffer. All five tracked variables now read one retained name, data value, and creation time; buffer_sample applies tmux-compatible escaping and truncation, buffer_full preserves the complete lossy text, and #{command} remains a command-item fact. buffer_mode_format stays separate because zz uses a native buffer chooser. The focused cargo test -p zz-daemon --lib buffer_ run passes all 15 tests. | `resource:crates/zz-daemon/src/status.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `resource:crates/zz-mux/src/formats.rs`, `resource:knowledge/tmux/status-line.md`, `resource:knowledge/tmux/divergences.md` |
| `formats.buffer-path-expansion` | 2026-08-25 | `load-buffer` and `save-buffer` now expand their positional path exactly once through the shared daemon command hooks before home-directory expansion and filesystem access. A resolvable `load-buffer -t` client supplies its attached session, focused window, and active pane; an unresolved target stays quiet and uses the most-recent mux context instead of the invoker. The targetless path selects the invoking attached client, the best client on the invoking command client's origin session, or the global best attached client before falling back to the most-recent mux context. Canonical names survive built-in aliases, unique prefixes, and one-layer user aliases, while explicit item state retains precedence and replacement text is not expanded again. `load-buffer` rejects a raw or format-produced `-` before I/O and preserves the empty-file no-op before buffer-name validation. `save-buffer` resolves the selected buffer before path expansion and the `-` check. Standard streams remain under `protocol.binary-streams`, `load-buffer -w` remains under `buffers.clipboard-write`, target-client format facts remain under `clients.context-formats`, and relative, attached-session, and remote file ownership remains under `buffers.client-file-context`. | `resource:crates/zz-daemon/src/daemon.rs`, `resource:knowledge/tmux/commands.md`, `resource:knowledge/tmux/divergences.md`, `scenario:compat/scenarios/buffer-path-format.txt` |
| `formats.command-argument-expansion` | 2026-08-24 | This close covers five target-sensitive argument paths: `rename-session` and `rename-window` expand their new names after resolving the target and before changing its old session or window facts; direct `show-options` and `show-window-options` calls perform one expansion of an optional option name in the target context, with `show-hooks` marking its forwarded name as expanded; and `select-pane -T` expands from the original target pane before a direction chooses the destination that receives the title. All five paths retain the canonical command-item name and explicit hook overrides. Both `new-session` names, the shared `new-session`/`new-window`/rename validation and cleaning path, literal `break-pane -n` cleaning, and the `load-buffer` and `save-buffer` file paths closed separately under focused format groups on 2026-08-25. | `resource:crates/zz-mux/src/command.rs`, `scenario:compat/scenarios/command-item-format.txt` |
| `formats.command-item-context` | 2026-08-24 | `#{command}` is now a command-queue-item fact for every command the mux engine runs, not a list-row fact: after `command-alias` expansion and canonical resolution the dispatch chokepoint wraps the incoming format hooks once with the canonical entry name and threads that wrapper through every arm, so `display-message`, the list commands, and every other mux-executed item expand it, a typed alias or unique prefix still reports the canonical name, and it stays empty outside a command item. The wrapper consults the inner and item-state hooks first, so an explicit outer `command` value keeps winning, and list rows keep it invariant while `key_*` and `command_list_*` stay disjoint per row. The daemon-preempted half closed separately under `formats.daemon-command-item-context`. | `resource:crates/zz-mux/src/command.rs`, `file:crates/zz-mux/src/compat_manifest_tests.rs`, `scenario:compat/scenarios/command-item-format.txt` |
| `formats.creation-name-edges` | 2026-08-25 | Without an explicit destination index, new-window -S performs a second format expansion over the cleaned first-pass -n value for lookup while creation keeps the cleaned first-pass value. An explicit destination index bypasses that lookup expansion. An explicit break-pane -n disables window-local automatic-rename after both whole-window relinking and new-window creation. The full zz-mux library suite passes 379 tests, and the strict command-item-format and break-pane differentials pass 125 and 30 steps with zero topology, geometry, format, output, or warning differences and no skips. | `resource:crates/zz-mux/src/command.rs`, `resource:knowledge/tmux/commands.md`, `resource:knowledge/tmux/divergences.md`, `resource:knowledge/designs/tmux-superset-roadmap.md`, `scenario:compat/scenarios/command-item-format.txt`, `scenario:compat/scenarios/break-pane.txt` |
| `formats.daemon-command-item-context` | 2026-08-24 | DaemonFormatHooks now carries an optional canonical command-item name after explicit item-state variables, so an explicit `command` override still wins without copying the name into ExecutionContext. execute_with_mux_source_raw passes the resolved entry through the daemon-owned immediate expansion surfaces: run-shell shell and string -C, if-shell conditions, capture-pane -S/-E, pipe-pane shell, list-buffers and list-clients formats and filters, display-popup title/directory/position, display-menu title/items/position, confirm-before string-command preparation, and the post-spawn PaneFormatOutput re-expansion for new-window and split-window -P/-F. Canonical names survive built-in aliases, unique prefixes, and one-layer user aliases. Typed command blocks skip the parent expansion and their children report their own commands; daemon-preempted hook bodies report their own command while retaining the trigger in `#{hook}`. Confirm prompts and delayed refresh-client subscriptions expand outside a queue item and keep `#{command}` empty. Popup argv and environment assignments remain raw. The pinned 24-step strict differential covers shell and -C routes, every resolution spelling, typed blocks, if-shell parent and child routing, hook routing, list-buffers, and both pane-creation print paths with zero TOPO, GEO, FMT, OUT, or WARN differences. | `resource:crates/zz-daemon/src/status.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `file:compat/scenarios/daemon-command-item-format.conf`, `scenario:compat/scenarios/daemon-command-item-format.txt`, `resource:knowledge/tmux/status-line.md`, `resource:knowledge/tmux/divergences.md` |
| `formats.name-validation-cleaning` | 2026-08-25 | `new-session -n` now expands exactly once through the same command-item format context as `-s`, then rejects ASCII controls and applies tmux vis cleaning before `-s` is touched. Canonical, alias, and unique-prefix spellings expose `new-session`, explicit item state retains precedence, and only a genuinely attached client contributes session, focused-window, and active-pane facts. `new-window -n` expands once in the destination session context with the pin's session format type and then uses the same validation and cleaning helper before reuse lookup or creation. Both `rename-session` and `rename-window` use the resolved target's active pane and the pin's pane format type; each applies the shared helper after one target-context expansion. `break-pane -n` follows the pin's distinct literal path: it is never format-expanded, but it is validated and cleaned before placement, including before `-a` or `-b` can shift window indices. Empty and valid Unicode names survive, literal backslashes are doubled exactly once, cleaned values determine session collision and new-window reuse identity, and duplicate window names remain legal. An existing-session `-A` still expands and validates `-n` first and then ignores it. Every nested detached `-Ad` path now defers its refusal until a returned Attach effect: an expanded existing target is refused before application with target-only command-context restoration that preserves freshly derived client size and cwd, while an expanded miss creates detached even when the raw format text happens to name a literal session. The strict `command-item-format` differential covers formatted creation names, Unicode, backslash cleaning, cleaned reuse and rename collision, literal break-pane format tokens, and rename parity, including `session_format=0` and `pane_format=1`, with no topology, geometry, format, output, or warning differences. Focused mux tests pin the format types, a non-vacuous `break-pane -b` rejection before window 0 could shift to index 1, unchanged pane layout and window indices, ASCII-control rejection, exact `-n`-before-`-s` ordering, and expansion count because the line-oriented differential harness cannot carry control bytes or count hook calls. `formats.creation-name-edges` closed the pin's second `new-window -S` lookup expansion and `break-pane -n` automatic-rename side effect separately on 2026-08-25. Nested non-detached error precedence for the `-t` conflict, invalid window or session names, duplicates, and invalid session-group names remains under `mux.error-shapes`. | `resource:crates/zz-mux/src/command.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `scenario:compat/scenarios/command-item-format.txt`, `resource:knowledge/tmux/commands.md`, `resource:knowledge/tmux/divergences.md` |
| `formats.new-session-name-expansion` | 2026-08-25 | `new-session -s` now expands exactly once through the existing command-item format hooks before `-A` lookup, duplicate lookup, and creation. Attached Interactive and Control clients contribute their actual attached session, focused window, and active pane, including after a detached creation retargets the mutable command context; fresh Interactive and Command clients contribute no session defaults even when their execution context carries a most-recent target. Canonical, alias, and unique-prefix spellings expose `new-session` through `#{command}`, while an explicit outer item value retains precedence. Omitted `-s` keeps numeric allocation. Existing-session `-A` still ignores `-d` and creation-only flags; creation size, environment, cwd, and print handling still run only after the expanded name misses. The adjacent validation and vis-cleaning parity for both `new-session` names, `new-window -n`, both rename commands, and literal `break-pane -n` closed under `formats.name-validation-cleaning`, including the detached formatted `-A` nesting-guard correction. The strict `command-item-format` differential covers this path without duplicating its moving step count here. Nested non-detached duplicate, invalid-name, and `-t` plus `-n` conflict precedence remains documented under `mux.error-shapes`. | `resource:crates/zz-mux/src/command.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `scenario:compat/scenarios/command-item-format.txt`, `resource:knowledge/tmux/divergences.md` |
| `formats.session-activity` | 2026-08-25 | Sessions now retain Unix-second activity apart from the logical MRU counter. Creation initializes activity from the exact retained creation timestamp. Successful same- and other-session attaches, survivor retargeting through the shared attach funnel, ordinary queued terminal keys, read-only queued keys, rejected read-only terminal-view input including mouse motion, writable chooser input, and eligible standalone committed text update the retained and logical activity facts. Writable chooser input also advances latest geometry without clearing bells; cross-session chooser activation then records the target attach as a second legitimate boundary. Read-only raw keys bypass chooser, command-prompt, and display-panes consumption into ordinary key-table resolution, while rejected native actions update latest geometry without clearing bells or mutating the retained modal; safe local view actions bypass chooser and display-panes surfaces and reach the pane with the same once-only accounting. Writable prompt-consumed key or text input and valid display-panes selection do not refresh activity. A bounded per-client ordered queue correlates Key-plus-Text pairs so the Key's result wins and the trailing Text adds no second update. Standalone read-only terminal Text accounts without a PTY write or bell clear. Unmatched display-panes keys, Escape, and non-hover mouse or wheel input close the overlay and fall through ordinary input; only bare buttonless hover Motion remains consumed, and timeout fabricates no activity. Native client-theme notifications, resize, `switch-client -T`, and detached commands also do not refresh activity. Plain `session_activity` and its `t:` form use the retained timestamp, while `S/t` and `list-sessions -O activity` use the logical counter so same-second touches reorder by logical MRU with session name as the tie break. Browser input keeps zz's existing native-superset activity behavior. These daemon input edges closed under `formats.session-activity-daemon-input-edges`, `choosers.cross-client-pane-mode-routing`, `display-panes.gpui-escape-fallthrough`, and `formats.session-activity-text-input`; client focus signaling plus attach, FocusIn latest geometry, and writable modal routing closed under `formats.session-activity-client-focus-signal`, `formats.session-activity-focus-latest`, and `formats.session-activity-focus-modal-consumption`. `formats.session-activity-wake-lifecycle` records tmux wake/unlock as an accepted non-applicable lifecycle difference. | `resource:crates/zz-mux/src/model.rs`, `resource:crates/zz-mux/src/formats.rs`, `resource:crates/zz-mux/src/command.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `resource:crates/zz-daemon/src/status.rs`, `scenario:compat/scenarios/session-activity.txt`, `resource:knowledge/tmux/status-line.md`, `resource:knowledge/tmux/divergences.md` |
| `formats.session-activity-client-focus-signal` | 2026-08-25 | Protocol v73 appends `InputMessage::ClientFocus { focused }` at wire tag 18 and retains exact-version handshake rejection. GPUI seeds desired focus only when construction finds an active window and leaves inactive construction unset until the first activation callback. A written attach opens a pending focus epoch. `Attached` confirms the new epoch and replays the latest desired value once, so reconnect, host switch, and session attach do not depend on another OS activation transition. A rejected same-connection session attach restores the retained session's ready epoch and flushes a focus value that changed while the request was pending. Only a pending MissingTarget or SessionNotFound attach response can recover that epoch; unrelated request-zero errors while ready or pending do not alter focus delivery. Scripted pane selection, sidebar focus, and pane transitions remain pane focus only. The TUI assumes its outer terminal starts in the foreground, caches FocusGained and FocusLost while attachment is pending, sends the latest ClientFocus value once after each Attached event, and deduplicates repeated reports with the same value. A separate protocol-owned attach-attempt marker selects missing-target retry and fallback without consulting focus readiness, then returns to idle on Attached or terminal attach failure. Unrelated request-zero errors change neither state machine. A rejected sidebar session attach restores the retained session's ready epoch instead of entering new-session fallback. Real outer focus events retain pane Focus when the active pane is a terminal; attachment does not synthesize pane focus. `zz_client_attach` returning true means the FFI client wrote the request. iOS waits for `ZZ_EVENT_ATTACHED`, then sends the latest scene state once for initial, selected-session, recovery, and recreated-session attachments without replaying pane focus. Foreground and background still send the separate `zz_client_focus_terminal` transition when a terminal input owner exists. When the server `focus-events` option is on, both client-focus directions update retained session activity, logical MRU, client activity sequence, and client activity time exactly once, including read-only clients. Writable pane Focus continues terminal application forwarding but changes neither activity nor geometry ownership, so paired client and pane signals touch activity once. Read-only pane Focus is rejected and its client-window transition remains owned by ClientFocus. Neither signal clears bells; the client-focus activity path is inert while the server option is off. The daemon routes writable client focus through prompt handling and display-panes teardown under `formats.session-activity-focus-modal-consumption`. | `resource:crates/zz-protocol/src/message.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `resource:crates/zz/src/workspace/view.rs`, `resource:crates/zz-tui/src/app.rs`, `resource:crates/zz-tui/src/input.rs`, `resource:crates/zz-tui/src/state.rs`, `resource:crates/zz-client-ffi/src/ffi.rs`, `file:crates/zz-client-ffi/include/zz-client.h`, `resource:clients/ios/Sources/ZZStore.swift`, `resource:knowledge/designs/tui-client.md`, `resource:knowledge/protocol/wire-protocol.md`, `resource:knowledge/tmux/divergences.md` |
| `formats.session-activity-daemon-input-edges` | 2026-08-25 | Writable choose-tree and choose-buffer raw keys, dedicated actions, and terminal-view input now touch source-session activity exactly once, advance the latest geometry owner, and preserve bells. Read-only raw keys bypass chooser, display-panes, and command-prompt consumption into ordinary key-table resolution. Read-only dedicated modal actions and rejected non-focus terminal-view actions update activity plus the latest geometry owner before rejection, retain the modal, and preserve the pane bell; raw mouse motion is included. Read-only-safe local navigation uses the same once-only accounting, bypasses retained chooser and display-panes surfaces, and reaches the pane under `clients.read-only-local-view-actions`; pane Focus is rejected without activity, and ClearLinkHover remains a safe non-input action. Writable display-panes consumes a valid selection and bare buttonless hover Motion without activity, but an unmatched key, Escape, non-hover mouse action, or wheel tears down the overlay and falls through ordinary input; timeout closes it without activity. Writable command-prompt consumption remains no-activity. Cross-client chooser routing and display-panes fallthrough are recorded as focused closed entries. Client focus signaling plus attach and latest geometry are closed separately. Committed text inherits these modal decisions through `formats.session-activity-text-input`, and client focus follows the writable prompt and display-panes prequeue under `formats.session-activity-focus-modal-consumption`. | `resource:crates/zz-daemon/src/daemon.rs`, `resource:knowledge/tmux/status-line.md`, `resource:knowledge/tmux/divergences.md` |
| `formats.session-activity-focus-latest` | 2026-08-25 | Every successful same- or other-session attach advances the existing geometry-owner sequence and recalculates all affected terminal sizes, independent of `focus-events`; a two-client regression with different retained geometries proves a same-session reattach immediately selects that client's whole geometry. Client FocusIn advances the same sequence and recomputes visible terminal sizes when `focus-events` is on. Under `window-size latest`, rows, columns, and cell metrics all come from that owner; manual, largest, and smallest retain their mode-correct rows and columns while refreshing the owner's cell metrics. FocusOut updates activity without changing the owner or geometry. A zz-side writable two-client daemon regression with different retained geometries mirrors the pinned FocusIn/latest rows-and-columns rule. `ClientFocus` is not CLI-drivable, so this is not a differential-harness proof. A second non-latest regression uses equal rows and columns with distinct cell metrics and observes the terminal process the owner-only resize. The separate read-only fixture proves that zz accepts the notification and updates activity. It does not prove tmux `attach -r` resize behavior because tmux couples read-only with `ignore-size` while zz does not. | `resource:crates/zz-daemon/src/daemon.rs`, `resource:knowledge/tmux/status-line.md`, `resource:knowledge/tmux/divergences.md` |
| `formats.session-activity-focus-modal-consumption` | 2026-08-25 | With `focus-events` enabled, the daemon now runs the pinned writable prequeue before it accounts `ClientFocus`. It dismisses the active status message and resumes frozen terminal publication, then closes `display-panes`, cancels its deadline, and continues through prompts. Key prompts submit the exact `FocusIn` or `FocusOut` text and consume the transition. Numeric prompts submit their buffer without recording prompt history and pass the transition. Text, Single, Incremental, and BackspaceExit prompts consume it and stay open. Choose-tree and choose-buffer bypass the prequeue because tmux handles them as pane modes. Read-only clients retain every modal and account both directions. FocusIn alone advances latest geometry, and neither direction clears bells. When a FocusIn both changes latest geometry and changes an activity-sorted chooser, the daemon publishes the snapshot and independently refreshes the chooser. The daemon snapshots the option gate before a Numeric prompt can change it, so an accepted transition still accounts. This slice adds no wire change and retains the `ClientFocus` shape introduced in protocol v73. Daemon regressions provide the proof because `ClientFocus` is not CLI-drivable. Pane `command-prompt -P` remains blocked under `prompt.pane-rendered`. Synthetic `Any` dispatch after activity and FocusIn geometry accounting closed separately under `keys.client-focus-events`; exact `FocusIn` and `FocusOut` remain invalid as bindable key names. | `resource:crates/zz-protocol/src/message.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `resource:knowledge/tmux/status-line.md`, `resource:knowledge/tmux/divergences.md` |
| `formats.session-activity-text-input` | 2026-08-25 | The daemon now correlates committed text with its preceding key through one bounded ordered queue per client. Every validated press or repeat Key with `text_follows: true` appends an entry that records its pane and Terminal or BrowserSurface lane. Text scans forward to the first entry for its pane and lane, retires only the older skipped prefix, and consumes that match while preserving later entries. It inherits the matched Key's activity and modal result, so the pair contributes at most one activity/latest update; empty Text is inert but also retires any linked dispatch suppression. If no entry matches, the queue stays intact and nonempty Text is standalone. A two-browser-pane regression strands one earlier key and proves the later bound key keeps its suppression debt until its matching Text, so neither the binding nor text is replayed. Bounded eviction and every explicit cleanup retire linked suppression debt. Writable standalone text reaches chooser, command-prompt, and display-panes before activity; terminal command-output text accounts once before it is swallowed, while browser command-output text is consumed before activity. Standalone read-only terminal text accounts once without a bell or PTY write, while read-only browser text keeps zz's existing silent drop. Paired writable chooser input follows its key result; writable prompt and display-panes consumption can remain at zero. Read-only terminal pairs bypass retained modals, account on the key, and never write the trailing text. The queue clears on detach, unregister/reconnect, and successful wire Attach, but survives a synchronous binding-driven `switch-client` so the trailing Text still belongs to its Key. GPUI terminal standalone input and GPUI browser key-plus-text input are proved at their emitters; TUI keys remain unpaired, while FFI and iOS retain their explicit standalone/key contracts. | `resource:crates/zz-daemon/src/daemon.rs`, `resource:crates/zz/src/terminal/view.rs`, `resource:crates/zz/src/browser/view.rs`, `resource:crates/zz-tui/src/input.rs`, `resource:crates/zz-client-ffi/src/ffi.rs`, `resource:clients/ios/Sources/ZZStore.swift`, `resource:knowledge/protocol/wire-protocol.md`, `resource:knowledge/tmux/status-line.md`, `resource:knowledge/tmux/divergences.md` |
| `keys.client-focus-events` | 2026-08-25 | With focus-events enabled, writable ClientFocus runs through the modal prequeue, then session activity and FocusIn-only latest-geometry accounting, then synthetic Any dispatch. The daemon selects transient tables in chooser, copy-mode or command-output, and effective-root order. A transient Any binding wins; an unbound transient table falls back to the effective root without retiring the mode. A root Any binding runs when no transient mode applies. The daemon resolves attachment and pane context again after prompt submission before it dispatches the selected command. Disabled focus bypasses accounting and dispatch. Read-only focus retains its modal bypass, resolves the whole Any binding, authorizes every command before any effect, and rejects a mixed safe and unsafe chain atomically. Exact FocusIn and FocusOut remain invalid key names, and even injected exact bindings do not replace Any. Synthetic ingress preserves pending copy jump capture, numeric prefix state, repeat metadata and deadlines, table fallback and retirement, prefix synchronization, client isolation, and the next real key. The focused daemon cluster passes 9 of 9 tests, and an independent Codex read-only review returned CODE GO. ClientFocus is not CLI-drivable, so this closure makes no differential or canonical-suite claim. | `resource:crates/zz-protocol/src/key.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `resource:knowledge/tmux/key-tables.md`, `resource:knowledge/tmux/status-line.md`, `resource:knowledge/tmux/divergences.md` |
| `keys.copy-mode-action-and-repeat-fidelity` | 2026-08-25 | The emacs M-f default stores the pin's send-keys -X next-word-end action. Vi numeric capture consumes the buffered count at the first send or send-keys command whose option prefix contains -X. If that command already contains -N, its stored repeat wins; otherwise zz inserts one separate -N count pair immediately before the option argument containing -X. The engine does not scan onward after a stored -N, a command list with no qualifying -X leaves the count armed, and later actions do not inherit it. One exhaustive typed policy carries a u32 count through one flat TerminalViewAction::CopyModeCounted action: movements, jumps, matching brackets, and repeat-search execute count times; other-end swaps only for odd counts; select-line spans count logical lines; copy-end-of-line spans through the end of row N and copies once; other toggles, selection, copy, cancel, and later actions execute once. Counted raw key sends carry one repeat field instead of preallocating N tokens. Terminal delivery stops on the first full input queue. Browser events and both clients cap their native repeat path at MAX_BROWSER_KEY_REPEAT (9,999), because tmux has no browser pane. Direct -N parsing expands the last value, accepts 1 through UINT_MAX, preserves attached and clustered forms plus command abbreviations, and reports the pin's invalid, too-small, and too-large errors. Protocol v75 appends flat counted-copy tag 28 and browser-repeat tag 7; tests reject the removed recursive action tag and nested payloads. The strict send-keys-repeat differential covers separate, attached, clustered, duplicate, formatted, alias, prefix, and error forms. Invalid nonempty -X grammar returns the pin's neutral prefix value 1, represented as no buffered count, so the next digit starts a fresh prefix. Multi-digit vi capture remains bounded at 9,999 per client; bare no-key counts and `-N <n> -X` with no action remain pane-prefix residuals under terminal.key-control. The nine digit bindings retain their native copy-mode-repeat list-keys shape under keys.copy-mode-native-numeric-prefix; unrelated cursor-word, search, goto-line, and jump command shapes remain open under keys.copy-mode-binding-fidelity. | `resource:crates/zz-protocol/src/key.rs`, `resource:crates/zz-mux/src/command.rs`, `resource:crates/zz-terminal/src/interaction.rs`, `resource:crates/zz-terminal/src/session.rs`, `resource:crates/zz-protocol/src/message.rs`, `resource:crates/zz-daemon/src/keys.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `resource:crates/zz/src/browser/view.rs`, `resource:crates/zz/src/browser/tui.rs`, `resource:knowledge/tmux/key-tables.md`, `resource:knowledge/tmux/copy-mode.md`, `scenario:compat/scenarios/send-keys-repeat.txt` |
| `keys.copy-mode-defaults` | 2026-08-25 | Added the six remaining stock emacs keys whose typed actions already existed: C-[ runs cancel, C-k runs copy-pipe-end-of-line-and-cancel, C-w runs copy-pipe-and-cancel, N runs search-reverse, R runs rectangle-toggle, and n runs search-again. Each binding stores the pin's single send-keys -X command and carries no repeat bit. KeyEngine tests cover the six bindings and retain the prior Escape, M-w, and C-g bindings; exact equality with the audited stock key set proves that none overwrote another key. Mux tests cover cancel, both copy variants, forward and reverse search repetition, and rectangle toggle as typed terminal effects. The 17 absent stock keys remain under keys.copy-mode-prompt-defaults and keys.copy-mode-unsupported-default-actions. | `resource:crates/zz-protocol/src/key.rs`, `resource:crates/zz-mux/src/command.rs`, `resource:crates/zz-mux/src/compat_manifest_tests.rs`, `resource:knowledge/tmux/key-tables.md`, `resource:knowledge/tmux/copy-mode.md` |
| `keys.copy-mode-navigation-defaults` | 2026-08-25 | Added the pin's 13 previously absent emacs navigation keys with exact send-keys -X command shapes and nonrepeat metadata: C-Down, C-M-Down, C-M-Up, C-M-f, C-Up, End, Home, M-<, M->, M-Down, M-R, M-Up, and Space. Their existing typed actions cover line scrolling, semantic-prompt navigation, matching brackets, line endpoints, history endpoints, half pages, top-line positioning, and page-down without a new terminal action or UI flow. KeyEngine and mux effect tests cover every key and action, and shifted Alt input reaches M-R, M-<, and M->. At this historical closure 23 stock keys remained: six supported non-navigation defaults, ten prompt-backed defaults, and seven keys behind unsupported actions. The later keys.copy-mode-defaults closure reduced the current residual to 17. | `resource:crates/zz-protocol/src/key.rs`, `resource:crates/zz-mux/src/command.rs`, `resource:crates/zz-mux/src/compat_manifest_tests.rs`, `resource:knowledge/tmux/key-tables.md`, `resource:knowledge/tmux/copy-mode.md` |
| `list-keys.remaining` | 2026-08-24 | list-keys now implements the pin's optional positional key, -1, -O, and -r grammar, including clustered and attached options, --, repeated -O last-wins behavior, exact one-positional and missing -O errors, key-before-sort-before-table error precedence, valid-but-absent and note-filtered unknown-key errors, global and per-table note sorting, and reversal only when an order is selected. Sorting and filtering happen before -1, while repeat and width aggregates are computed after truncation. Literal stored space bases render as Space and C-Space, widths use those spellings, and positional matching compares base, type, and modifiers without stored spelling or key flags. Command and Control clients receive the selected line on stdout; attached Interactive clients receive a display-time-backed frozen status message without a command-output overlay. Stock copy-mode and copy-mode-vi bindings now publish no repeat bits while persistent copy-table movement, jump capture, and numeric repetition remain runtime key-engine behavior. Equal-base, cross-table, inapplicable-field, and four-byte Unicode comparator cases are bounded separately under list-keys.deterministic-sort-ties, and the existing long Ctrl-/Alt- spelling overacceptance remains under the strict-key divergence. | `resource:crates/zz-protocol/src/catalog.rs`, `resource:crates/zz-protocol/src/key.rs`, `resource:crates/zz-mux/src/command.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `file:crates/zz-mux/tests/hunt_claims.rs`, `file:compat/attached-client.sh`, `scenario:compat/scenarios/list-keys-padding.txt` |
| `mux.local-cli-chain-parse-abort` | 2026-08-25 | Against an already-running compatible daemon, local default and explicit-socket CLI preflight scans the complete prepared vector for typed name and alias-body errors before stdin capture, attach or TUI routing, or command execution. A later unknown command returns exit 1 with the pinned error shape and no earlier effect; malformed alias bodies use zz's loud unknown-command shape while aliases.command-bodies remains open. Prepared runtime command failures keep sequential queue ordering, including earlier effects and pruning later commands. A focused binary regression uses a malformed live alias after a mutating command, and the strict three-step smoke scenario proves unknown-name parse atomicity and runtime-error ordering with zero differential mismatches. Cold or failed preparation falls open to static routing, so an autospawn verb may still run before a later unknown command. Remote --host remains excluded, and local flag or arity validation plus config or source-file replay remains open under mux.chain-parse-abort. | `resource:crates/zz/src/lib.rs`, `file:crates/zz/tests/cli_binary.rs`, `scenario:compat/scenarios/smoke/cli-chain-parse-abort.txt`, `resource:knowledge/tmux/divergences.md` |
| `mux.resize-pane-optional-values` | 2026-08-25 | This was a catalog-only reconciliation. Runtime already accepted bare -D, -L, -R, and -U with the default amount 1, plus attached and separated integer amounts. Static CommandOptionSpec metadata now marks the four direction values optional while retaining attached-value support, and manifest reconciliation compares that shape with the pinned oracle. No handler, effect, wire field, tag, or protocol version changed. Nine focused resize tests pass, along with 175 protocol unit tests and 14 protocol framing tests. The strict 16-step resize-directions differential has zero differences and no skips. The checked-in canonical summary still records eight steps and remains deferred for final regeneration. resize-pane -M and -T remain open under their existing owners, and mux.error-shapes remains open for its other arity, flag, usage, and precedence items. | `resource:crates/zz-protocol/src/catalog.rs`, `resource:crates/zz-mux/src/compat_manifest_tests.rs`, `file:crates/zz-mux/tests/hunt_claims.rs`, `scenario:compat/scenarios/resize-directions.txt` |
| `options.show-options-hook-rows` | 2026-08-24 | With `-H`, `show-options` augments only no-positional listings with hook arrays in the pin's final option-table block and hook declaration order. Plain listings exclude hooks, named hook queries work without `-H`, server scope has none, and global-session, global-window, and inherited pane listings expose 57, 11, and 7 hooks. Empty, populated, indexed, named, value-only, pane-fallback, and whole-array-shadowing shapes match the pin, including the inherited empty array's `name*` in a full listing and bare `name` in a named query. `show-window-options` retains its surface without `-H`. | `resource:crates/zz-mux/src/command.rs`, `resource:crates/zz-mux/src/tmux_options.rs`, `scenario:compat/scenarios/show-options-hooks.txt`, `resource:knowledge/tmux/commands.md` |
| `options.window-status-separator` | 2026-08-24 | The daemon expands `window-status-separator` after each nonfinal item in the `status-format[]` window loop. It resolves the separator in that window's option and format context, including per-window overrides, nested formats, and style directives; the last item emits no separator. The TUI owns exact tmux row output. The native GUI derives its window controls from snapshot state and does not paint this separator. | `resource:crates/zz-mux/src/tmux_options.rs`, `file:crates/zz-daemon/src/status.rs`, `scenario:compat/scenarios/status-options.txt`, `resource:knowledge/tmux/status-line.md` |
| `source-file.default-config-multi-file-order` | 2026-08-25 | Runtime source-file no longer special-cases the active zz/mux.conf. At this close, every declared path expanded and matched in caller order, and each match entered the ordinary config loader immediately in glob order, producing DAD for default, after, and default. The later config.replayed-command-output closure changed that immediate replay timing: one invocation now parses all matches in declared and glob order, then replays them in the same order while preserving DAD and path order. A loud miss returns status 1 without preventing later matches from loading; quiet misses remain silent. Ordinary diagnostics and -v lines retain declared path and match order. That later closure also pins presentation as one complete verbose batch, then replay, then buffered command-name and parser diagnostics for each invocation. Source no-match, glob, and actual OS or path read failures retain their existing error channels. Explicit zz-native reload-config still rediscovers the first existing default candidate, replaces #{config_files}, resets key tables, rebuilds appearance, and reapplies stored mux overrides. Startup still discovers the first existing zz-owned candidate, while ordered explicit -f files remain the intentional startup roots. Parse-only and nested source paths keep their existing behavior. Focused CLI and daemon tests, strict daemon clippy, and fmt pass. The strict 12-step source-file-diagnostics and 40-step source-file-format differentials have zero differences and no skips. At this close the source-file-control row had five clean steps; the later nested-queue proof grew the current row to six clean steps. This closure makes no canonical-suite claim. | `resource:crates/zz-daemon/src/daemon.rs`, `file:crates/zz/tests/cli_binary.rs`, `scenario:compat/scenarios/smoke/source-file-diagnostics.txt`, `scenario:compat/scenarios/source-file-format.txt`, `scenario:compat/scenarios/smoke/source-file-control.txt`, `resource:knowledge/tmux/conf-parser.md`, `resource:knowledge/tmux/commands.md`, `resource:knowledge/crates/zz-daemon.md`, `resource:knowledge/tmux/divergences.md`, `resource:knowledge/designs/tmux-superset-roadmap.md` |
| `source-file.flags` | 2026-08-25 | source-file now accepts -n, -t, and -v through one mux effect and replay-loader options path. -n parses the whole invocation, retains lexer diagnostics and optional verbose lines, and applies neither parser environment assignments nor commands. It does not claim tmux's full parse-time command-name, flag, and arity validation; config.parser-edge-cases, mux.error-shapes, and mux.chain-parse-abort retain those gaps. -t resolves a pane target once, preserves the invoking client cwd, supplies that context to -F and replayed commands, and follows CMD_FIND_CANFAIL by loading with an empty target context when lookup fails. -v formats canonical command groups with source and physical line in declared-path and glob order, inherits through nested sources, and stays suppressed for Control clients. The later config.replayed-command-output closure proves that Command and Interactive clients receive one complete verbose batch before that invocation's replay and buffered command-name or parser diagnostics. Source no-match, glob, and actual OS or path read failures retain their existing error channels. It also proves that -n applies no bare assignments across files and produces no replay output. The attached proof covers transcript presentation and dismissal; clients.tui-command-output-navigation owns interaction inside the TUI output view. The strict source-file-format differential runs 40 steps with no differences, including parse-only state, target and target-based -F context, a missing target, verbose output, and multi-file order. The source-file-control smoke proves that Control executes an explicit -v source without leaking verbose lines. Runtime error delivery closed separately under config.replayed-command-errors. At this checkpoint, config alias snapshots, native default-config multi-file ordering, nested Control queueing, sourced-hook cwd, and stdin remained in their explicit groups. The later source-file.default-config-multi-file-order closure removed the default-config residue. | `resource:crates/zz-protocol/src/catalog.rs`, `resource:crates/zz-mux/src/command.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `file:crates/zz-mux/tests/hunt_claims.rs`, `file:crates/zz/tests/cli_binary.rs`, `scenario:compat/scenarios/source-file-format.txt`, `scenario:compat/scenarios/smoke/source-file-control.txt`, `resource:knowledge/tmux/conf-parser.md`, `resource:knowledge/tmux/divergences.md` |
| `source-file.glob-semantics` | 2026-08-23 | Unix source-file matching now uses glob(3) with tmux's cwd quoting, backslash escaping, leading-dot exclusion, nonrecursive repeated stars, malformed-pattern handling, per-pattern ordering, and declared-path diagnostics. | `resource:crates/zz-daemon/src/daemon.rs`, `file:compat/scenarios/source-file-format-w-0-0-10.conf`, `file:compat/scenarios/source-file-format-w-0-0-20.conf`, `file:compat/scenarios/source-file-glob/.hidden.conf`, `file:compat/scenarios/source-file-glob/10/10.conf`, `file:compat/scenarios/source-file-glob/20/nested/20.conf`, `file:compat/scenarios/source-file-glob/literalq.conf`, `file:compat/scenarios/source-file-glob/prefix-siblings/zz-client-ffi/match.conf`, `file:compat/scenarios/source-file-glob/prefix-siblings/zz-client/match.conf`, `file:compat/scenarios/source-file-glob/prefix-siblings/zz/match.conf`, `scenario:compat/scenarios/source-file-format.txt`, `scenario:compat/scenarios/smoke/source-file-diagnostics.txt` |
| `source-file.nested-client-cwd` | 2026-08-24 | A source-file invocation from any registered real client now snapshots the same daemon-local base chosen for its top-level paths and carries that immutable base through every replay recursion. Relative nested paths use resolve_source_path with the existing cwd glob quoting even after an ordinary sourced command executes through the sentinel replay client and clears the mutable ExecutionContext cwd. At this checkpoint, sourcing the active default zz/mux.conf took the daemon's native reload branch and forwarded the same captured base. The later source-file.default-config-multi-file-order closure removed that runtime special case, so the same file now enters the ordinary source loader with the captured base. Direct zz-native reload-config later gained the same registered-client base under source-file.reload-config-client-cwd; startup remains clientless under source-file.startup-client-cwd. The pinned differential runs an ordinary command before the nested source and supplies a containing-file decoy; the CLI regressions repeat that shape from a caller cwd containing spaces and glob metacharacters, including the then-active default reload branch with a second decoy beside mux.conf. Deferred event hooks retain their sentinel-client gap under source-file.event-hook-client-cwd, hooks raised by sourced ordinary commands retain their replay-client gap under source-file.sourced-hook-client-cwd, and clients.attach-context still owns exact attached session-cwd selection when the attached client's advertised cwd differs. The later config.replayed-command-output closure preserves successful Command and attached output through this direct recursion as depth-first invocation frames. Parser-owned Control framing closed under control-mode.sourced-command-frames and source-file.nested-control-queue. The later control-mode.indirect-source-frames closure covers synchronous foreground inserted recursion. At this checkpoint immediate-hook and background-callback flags-0 framing remained open under control-mode.hook-command-frames and control-mode.background-inserted-command-frames. Later closures closed both groups. Hook cwd still remains under source-file.sourced-hook-client-cwd. | `resource:crates/zz-daemon/src/daemon.rs`, `file:crates/zz/tests/cli_binary.rs`, `file:compat/scenarios/source-file-nested-client-cwd/leaf.conf`, `file:compat/scenarios/source-file-nested-client-cwd/a/entry.conf`, `file:compat/scenarios/source-file-nested-client-cwd/a/compat/scenarios/source-file-nested-client-cwd/leaf.conf`, `scenario:compat/scenarios/source-file-format.txt`, `resource:knowledge/tmux/divergences.md` |
| `source-file.nested-control-queue` | 2026-08-25 | No production change was required. The existing source loader preflights all declared paths for one parser-owned source-file command before recursing. In a three-level Control replay, the root command therefore publishes its missing-path guard first, the middle command publishes its missing-path guard second, and the leaf publishes its output guard last, each exactly once. The focused nested_source_control_guards_precede_deeper_replay regression passes, the daemon suite passes 601 tests, and the strict six-step source-file-control differential has zero differences and no skips. This close covers parser-owned cross-depth diagnostic and output guard ordering. The later control-mode.source-file-exit-status closure completed Control return-status and detach precedence and grew the focused run to eight clean steps. The stored canonical row remains at three steps. The later control-mode.indirect-source-frames closure carries the Control recipient through synchronous foreground shell-evaluated `if-shell`, immediate `if-shell -F` including `-bF`, and foreground `run-shell -C`. At this checkpoint immediate-hook and background-callback flags-0 framing remained open under control-mode.hook-command-frames and control-mode.background-inserted-command-frames. Later closures closed both groups. Neither focused run makes a canonical-suite claim. | `resource:crates/zz-daemon/src/daemon.rs`, `file:compat/scenarios/smoke/fixtures/source-file-control.sh`, `scenario:compat/scenarios/smoke/source-file-control.txt`, `resource:knowledge/tmux/conf-parser.md`, `resource:knowledge/tmux/divergences.md` |
| `source-file.nested-diagnostic-semantics` | 2026-08-23 | Nested source-file no-match and glob errors now retain the post-F declared argument; a quiet no-match stays silent. Command clients receive stderr and exit 1, and Interactive clients receive a warning. Protocol v76 puts parser-owned Control no-match and glob diagnostics inside the source command's own flags-1 guard; a hit plus miss ends `%end`, while an all-miss ends `%error`. Matched child actual OS or path read failures use zz's typed standalone Error path, whose internal identity closed under control-mode.source-diagnostic-typing. The pinned external raw placement and invisible source-completion numbering remain open under control-mode.hook-source-read-diagnostics. Invalid UTF-8 was a zz-side typed error at this checkpoint; config.non-utf8-file-bytes owns the later pinned semantic mismatch. Per-command framing closed under control-mode.sourced-command-frames, and the later source-file.nested-control-queue closure proved cross-depth parser-owned diagnostic ordering. The later control-mode.indirect-source-frames closure covers synchronous foreground inserted sources. Immediate hook and background callback flags-0 framing closed later under control-mode.hook-command-frames and control-mode.background-inserted-command-frames. Registered-client nested cwd rebasing closed separately under source-file.nested-client-cwd, while startup and deferred event-hook base selection remain active. | `resource:crates/zz-daemon/src/daemon.rs`, `file:crates/zz/tests/cli_binary.rs`, `file:compat/scenarios/smoke/fixtures/source-file-diagnostics.conf`, `scenario:compat/scenarios/smoke/source-file-diagnostics.txt`, `scenario:compat/scenarios/smoke/source-file-control.txt` |
| `source-file.nesting-semantics` | 2026-08-23 | Counting the initial source-file as invocation 1, 50 concurrent source invocations now run and invocation 51 is refused before any of its paths are matched or loaded. Command clients get `too many nested files` on stderr and exit 1, Protocol v76 Control clients get the same lowercase text inside the refused command's own flags-1 `%begin`/`%error` guard while the outer line continues, and attached clients get the pin's capitalized `Too many nested files` status message. `-q` does not suppress it, one diagnostic is emitted per refused command rather than per path, and the containing file keeps running its later lines. The later source-file.nested-control-queue closure proved cross-depth ordering. A malformed invocation at the refused depth is diagnosed as malformed rather than as depth on both sides, because the pin rejects it while parsing the containing file and never consults its depth guard; only that precedence, the stdout stream, and the exit status are closed, while the differing malformed text stays with mux.error-shapes and the pin's abandonment of the rest of the containing file stays with config.parser-edge-cases. Same-line removal closed separately under config.same-line-error-group, and cumulative startup accounting closed separately under source-file.startup-depth-accounting. | `resource:crates/zz-daemon/src/daemon.rs`, `file:crates/zz/tests/cli_binary.rs`, `file:compat/attached-client.sh`, `file:compat/scenarios/smoke/fixtures/source-file-depth.sh`, `scenario:compat/scenarios/smoke/source-file-depth.txt`, `scenario:compat/scenarios/smoke/source-file-control.txt` |
| `source-file.reload-config-client-cwd` | 2026-08-25 | A registered client's direct zz-native reload-config now snapshots the same selected source base as source-file and carries it through the default mux.conf replay. A CLI regression runs from a cwd containing spaces and glob metacharacters, places distinct leaf files in the caller cwd and beside mux.conf, clears the earlier sourced state, and proves direct reload selects the caller-root leaf. A daemon regression proves clientless replay still uses the containing-file fallback. The change reuses the v72 ClientHello cwd and existing daemon state without a protocol change. Startup, attached session-cwd selection, deferred event hooks, and hooks raised during sentinel replay retain their separate tracked gaps. | `resource:crates/zz-daemon/src/daemon.rs`, `file:crates/zz/tests/cli_binary.rs`, `resource:knowledge/tmux/divergences.md`, `resource:knowledge/designs/tmux-superset-roadmap.md` |
| `source-file.startup-depth-accounting` | 2026-08-24 | One startup accounting value now spans every explicit or discovered top-level configuration. The roots do not consume slots; source commands 1 through 50 run, command 51 and later retain the declaring file and line in their cause, quiet misses consume slots, and one command with many paths consumes one slot. Runtime sequential source commands remain unbounded, while the zz-native `reload-config` whole-root replay takes one fresh startup budget of its own so reloading a file lands the same state a fresh start would. Client delivery and placement of retained startup causes remain tracked under config.startup-diagnostic-delivery. | `resource:crates/zz-daemon/src/daemon.rs`, `file:crates/zz/tests/cli_binary.rs` |
| `source-file.tilde-semantics` | 2026-08-23 | source-file no longer rewrites a literal leading ~/ after parsing: parser-expanded leading tildes still arrive as absolute paths, top-level literal tildes pass through cwd resolution, and registered-client nested literal tildes use the stable invoking base closed under source-file.nested-client-cwd. Startup and deferred event-hook base selection remain active. The CLI regression pins the top-level choice against a metacharacter-bearing daemon HOME. | `resource:crates/zz-daemon/src/daemon.rs`, `file:crates/zz/tests/cli_binary.rs`, `file:compat/scenarios/smoke/fixtures/source-file-tilde-decoy.conf`, `scenario:compat/scenarios/smoke/source-file-tilde.txt` |
| `tracker.args-parse-inventory` | 2026-08-25 | Oracle schema 4 parses the pinned `args_parse` callback references and rejects callback bodies outside six recognized rules. It records 14 command-to-rule assignments from nine callbacks, including `display-menu` item groups, `run-shell -C`, and the `set-hook -B` specialization. The Rust catalog carries typed rules for the 12 implemented commands. The manifest gate requires every rule absent from `COMMAND_ARGS_PARSE_BEHAVES` to retain a command-specific `args-parse:` item; `choose-client` and `switch-mode` stay covered by their unimplemented command items. This closes discovery only. Runtime adoption remains open under `tracker.semantic-coverage`. | `resource:compat/tmux-oracle.py`, `resource:compat/tmux-tracker.py`, `resource:crates/zz-protocol/src/catalog.rs`, `resource:crates/zz-mux/src/compat_manifest_tests.rs`, `resource:knowledge/playbooks/compat-harness.md` |
