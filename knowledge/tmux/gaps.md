---
type: Reference
title: tmux compatibility gap report
description: "Live TODO and status report for tmux compatibility gaps, decisions, evidence, and acceptance gates."
resource: compat/tmux-gaps.json
tags: [tmux, compatibility, gaps, tracker]
timestamp: 2026-08-24T00:00:00-03:00
---

# Overview

> `compat/tmux-tracker.py write-report` generates this file. Edit the registry instead.

`compat/tmux-gaps.json` owns the backlog. The compatibility gate checks IDs, decisions,
dependencies, evidence paths, known scenarios, and the source-backed inventories described
below.

Pinned tmux commit: `d77c9dc6aa021e4bc61f0da128c591af695e6466`.

Tracked gap groups: **82**. Classified items: **678**.

- Status: open: 42, blocked: 21, accepted: 19.
- Decision: adopt: 49, native: 13, park: 14, never: 6.
- Priority: next: 17, later: 46, none: 19.
- Closed history entries: 15.
- Surface: command: 9, flag: 82, flag-arity: 4, positional-min: 14, positional-max: 8, native-command: 19, option: 75, format: 107, hook: 10, key: 129, binding: 52, native-key: 58, semantic: 99, presentation: 9, protocol: 3.

## Measured surface

The pinned oracle contains 92 commands, 78 aliases, 572 command-flag shapes (318 valueless, 246 required-value, 8 optional-value), positional minimum and maximum bounds, 180 options, 198 global formats, 14 selected context formats, 68 hooks, and 303 default bindings across 5 tables. zz has catalog entries for 83 of those commands. The registry classifies 82 catalogued-unsupported upstream flag pairs, 4 implemented flag-arity mismatches, 14 positional-minimum mismatches, 8 positional-maximum mismatches, 0 zz-only flags on tmux command names, 19 native command names, 75 options absent from `BEHAVES`, 107 known limited formats, 0 selected context-format gaps, 0 zz-only selected context-format names, 10 currently documented hook-producer gaps, 129 omitted default keys, 52 divergent shared default bindings, 58 zz-only default keys.

## Enforcement boundary

The gate reconciles command names, aliases, flag arities, positional bounds, option names,
global and selected context-format names, hook names, and default key presence against the
clean pinned tmux source and binary. It also reconciles options absent from `BEHAVES`,
constant-backed formats against the live registry, omitted
and zz-only default keys against zz's key tables, rendered commands plus repeat bits for
shared default bindings, the native roster against catalog minus oracle, every pinned
canonical prefix against the resolver, and known scenarios against exact tuples.

These structural checks cannot prove custom `args_parse` callback rules, open-ended dynamic
format contexts, nonconstant format correctness, or whether a hook fires,
or that a structurally matching binding behaves identically at runtime. Differential scenarios,
attached-client fixtures, unit tests, and manual GUI checks supply that behavioral evidence. The
tracker keeps the remaining semantic discovery work explicit instead of treating matching
structure as proof.

## Next

| ID | Gap | Decision | Status | Ease | Owner | Impact | Depends on |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `aliases.client-preflight` | Resolve live aliases before client-owned preprocessing | adopt | open | medium | client | daily, scripts | none |
| `clients.context-formats` | Back client format facts | adopt | blocked | medium | daemon | scripts, remote | clients.attach-context |
| `clients.detach-control` | Complete detach targeting and eviction | adopt | open | medium | daemon | daily, scripts, remote | clients.attach-context |
| `clients.event-hooks` | Produce client lifecycle hooks | adopt | blocked | medium | daemon | scripts, remote | clients.attach-context |
| `config.replayed-command-errors` | Report replayed command failures | adopt | open | medium | daemon | scripts | none |
| `config.replayed-command-output` | Deliver replayed command output | adopt | open | medium | daemon | scripts | control-mode.sourced-command-frames |
| `config.startup-diagnostic-delivery` | Deliver retained startup configuration causes | adopt | open | medium | client | daily, scripts | none |
| `display-message.output-modes` | Complete display-message output modes | adopt | open | medium | daemon | scripts, admin | none |
| `keys.copy-mode-defaults` | Complete stock copy-mode keyboard bindings | adopt | open | medium | protocol | daily, remote | copy-mode.command-fidelity |
| `mux.error-shapes` | Match remaining command errors | adopt | open | medium | protocol | scripts | none |
| `source-file.flags` | Complete source-file controls | adopt | open | medium | daemon | scripts | none |
| `source-file.nested-control-queue` | Match nested source-file Control queue semantics | adopt | open | medium | daemon | scripts | control-mode.sourced-command-frames |
| `source-file.path-semantics` | Match source-file path semantics | adopt | open | medium | daemon | scripts | none |
| `tracker.semantic-coverage` | Close the remaining semantic discovery blind spots | adopt | open | medium | protocol | scripts | none |
| `clients.attach-context` | Add one shared attach context | adopt | open | hard | protocol | daily, scripts, remote | none |
| `control-mode.sourced-command-frames` | Preserve frames for sourced Control commands | adopt | open | hard | protocol | scripts | none |
| `keys.copy-mode-binding-fidelity` | Match shared copy-mode binding commands | adopt | open | hard | protocol | daily, remote, scripts | copy-mode.command-fidelity |

## Later

| ID | Gap | Decision | Status | Ease | Owner | Impact | Depends on |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `formats.session-runtime` | Expose remaining session formats | adopt | open | easy | daemon | scripts | none |
| `alerts.message-lifecycle` | Unify alert message lifecycle | adopt | open | medium | daemon | daily | none |
| `aliases.command-bodies` | Support multi-command aliases | adopt | open | medium | mux | scripts | none |
| `buffers.clipboard-write` | Honor buffer clipboard writes | adopt | open | medium | daemon | scripts, remote | none |
| `choosers.command-flags` | Complete chooser command controls | adopt | open | medium | daemon | daily, scripts | none |
| `clients.path-encoding` | Preserve non-UTF-8 client paths | adopt | open | medium | protocol | scripts | none |
| `config.parser-edge-cases` | Close config parser edge cases | adopt | open | medium | mux | scripts | none |
| `config.same-line-error-group` | Drop the rest of a config line after a failed command | adopt | open | medium | daemon | scripts | none |
| `control-mode.async-command-output` | Place asynchronous command diagnostics | adopt | open | medium | client | scripts | none |
| `control-mode.diagnostic-typing` | Type Control-mode diagnostics | adopt | open | medium | protocol | scripts | none |
| `formats.buffer-context` | Expose buffer format facts | adopt | open | medium | daemon | daily, scripts | none |
| `formats.daemon-command-item-context` | Expose the invoking command format for daemon-run items | adopt | open | medium | daemon | scripts | none |
| `formats.window-runtime` | Expose remaining window formats | adopt | open | medium | daemon | scripts, remote | clients.attach-context |
| `history.hyperlink-reset` | Reset hyperlink history | adopt | blocked | medium | terminal | daily | none |
| `hooks.queue` | Produce after-queue hooks | adopt | open | medium | daemon | scripts | none |
| `options.option-name-format-coverage` | Complete option-name format coverage | adopt | open | medium | mux | scripts | none |
| `options.pane-chrome` | Consume pane chrome options | adopt | open | medium | client | daily, gui | none |
| `options.theme-palette` | Map tmux theme palette options | park | blocked | medium | client | gui | none |
| `pane.break-geometry` | Complete break-pane placement | adopt | open | medium | mux | scripts, daily | none |
| `pane.spawn-flags` | Complete split-window placement flags | adopt | open | medium | mux | scripts, daily | none |
| `rendering.geometry-residue` | Close bounded geometry reporting gaps | adopt | open | medium | client | scripts, gui | clients.attach-context |
| `terminal.key-control` | Complete terminal key control flags | adopt | open | medium | terminal | scripts, daily | none |
| `terminal.resize-pane-trim` | Add terminal history trim action | adopt | blocked | medium | terminal | daily, scripts | none |
| `clients.interactive-refresh` | Complete interactive client commands | park | blocked | hard | client | remote | clients.attach-context |
| `copy-mode.command-fidelity` | Complete copy-mode command fidelity | adopt | open | hard | client | daily, remote | clients.interactive-refresh |
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
| `keys.copy-mode-native-mouse` | Keep native copy-mode mouse handling | native | accepted | none | client | daily, gui | none |
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

Alert notifications still publish client-timed TimedClientMessage events directly, so they do not create ActiveClientMessage records, freeze terminal publication as pinned alerts do, or share daemon expiry and input dismissal.

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

### `aliases.client-preflight`: Resolve live aliases before client-owned preprocessing

The daemon now owns non-exact attach-prefix routing, and static agent-send prefixes read stdin by canonical name. Exact attach spellings still enter the native wrapper before server alias expansion, arbitrary live aliases to agent-send cannot trigger stdin capture, Control rejects unknown names before dispatch, and a failing live alias named kill-server falls into incompatible-daemon recovery.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `next` / `medium`
- Owner: `client`
- User impact: daily, scripts
- Items: `semantic:command-alias-control-precheck`, `semantic:command-alias-exact-attach-preflight`, `semantic:command-alias-kill-server-recovery`, `semantic:command-alias-stdin-preflight`
- Depends on: none
- Evidence:
  - `resource:crates/zz/src/lib.rs`
  - `resource:crates/zz/src/control_mode.rs`
  - `file:crates/zz/tests/cli_binary.rs`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `The CLI resolves live command aliases before exact native attach routing and stdin capture without a query-and-execute race, while retaining the --restart-daemon surface.`
  - `Control initial and streamed commands execute unknown-named live aliases with pin-shaped frames.`
  - `An erroring or nonzero live alias shadowing kill-server returns its alias result without entering verified recovery, while unaliased kill-server retains incompatible-daemon recovery.`

### `aliases.command-bodies`: Support multi-command aliases

The current dispatch chokepoint executes one command per alias.

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

### `clients.attach-context`: Add one shared attach context

The command-client cwd slice establishes the protocol pattern; this tranche adds the remaining attach state and exact attached session-cwd selection once.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `next` / `hard`
- Owner: `protocol`
- User impact: daily, scripts, remote
- Items: `flag:attach-session:-E`, `flag:attach-session:-c`, `flag:attach-session:-f`, `flag:attach-session:-x`, `flag:new-session:-X`, `flag:new-session:-f`, `flag:resize-window:-A`, `flag:resize-window:-a`, `protocol:client-attach-context`, `semantic:client-environment-seeding`, `semantic:resize-window-client-sizes`, `semantic:source-file-attached-session-cwd`, `semantic:switch-client-environment-refresh`, `semantic:switch-client-tty-targets`
- Depends on: none
- Evidence:
  - `resource:knowledge/designs/tmux-superset-roadmap.md`
  - `resource:knowledge/tmux/divergences.md`
  - `file:compat/attached-client.sh`
- Acceptance:
  - `One attach context carries environment input, flags, tty aliases, sizes, session cwd selection, and eviction state; differential and attached-client tests consume the same facts.`

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

### `clients.detach-control`: Complete detach targeting and eviction

The daemon has detach notices; the command still lacks the pin's target and eviction controls.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `next` / `medium`
- Owner: `daemon`
- User impact: daily, scripts, remote
- Items: `flag:detach-client:-E`, `flag:detach-client:-P`, `flag:detach-client:-t`, `semantic:client-eviction-state`
- Depends on: `clients.attach-context`
- Evidence:
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `resource:knowledge/designs/tmux-superset-roadmap.md`
  - `file:compat/attached-client.sh`
- Acceptance:
  - `Attached-client tests cover target selection, shell command execution, parent detach, and eviction notices.`

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

### `config.replayed-command-errors`: Report replayed command failures

tmux routes every replayed command's runtime failure through `cmdq_error` to the invoking client and sets that client's exit status, while zz logs the daemon-side error and continues silently at rc 0, and reports the few failures it classifies as invalid commands as `path:line:` parse diagnostics on stdout.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `next` / `medium`
- Owner: `daemon`
- User impact: scripts
- Items: `semantic:config-replayed-command-error-channel`, `semantic:config-replayed-command-exit-status`
- Depends on: none
- Evidence:
  - `resource:crates/zz-daemon/src/daemon.rs`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `A sourced command that fails at runtime reports the pin's bare message on the invoking client's error channel for a missing `kill-session` target and for an unknown `set-option` name, on Command stderr, on the Control error channel, and as the capitalized attached status message, instead of being dropped or printed on stdout with a `path:line:` prefix.`
  - `A `source-file` whose replayed commands include a runtime failure exits 1 while its later physical lines still run, including through an outer `source-file`.`

### `config.replayed-command-output`: Deliver replayed command output

tmux copies the invoking item state onto every command a file loads, so each replayed `cmdq_print` reaches the invoking client, while zz replays every sourced command through the sentinel `ClientId(u64::MAX)` Command client and discards the returned `Execution`, so successful sourced commands apply their effects and print nothing; the sibling `config.replayed-command-errors` owns only the failure channel and exit status, and `control-mode.sourced-command-frames` owns the guards this output would sit inside.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `next` / `medium`
- Owner: `daemon`
- User impact: scripts
- Items: `semantic:config-replayed-command-output`
- Depends on: `control-mode.sourced-command-frames`
- Evidence:
  - `resource:crates/zz-daemon/src/daemon.rs`
  - `resource:crates/zz/src/control_mode.rs`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `A sourced `display-message -p` and a sourced `list-sessions` reach the invoking client in file order on Command stdout, on the Control output channel, and in the attached-client view instead of being discarded with the sentinel replay client, while a detached startup load still writes nothing.`
  - `Replayed success output stays on the success channels: it never reaches Command stderr, the Control error channel, or the attached warning path, and it does not change the invoking client's exit status, so the failure routing owned by `config.replayed-command-errors` and the per-command guards owned by `control-mode.sourced-command-frames` remain separately measurable.`

### `config.same-line-error-group`: Drop the rest of a config line after a failed command

tmux gives each config line its own command group and removes the rest of that group when a command errors, while zz replays each parsed config command independently and only aborts direct CLI and Control chains.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `daemon`
- User impact: scripts
- Items: `semantic:config-same-line-error-group-removal`
- Depends on: none
- Evidence:
  - `resource:crates/zz-daemon/src/daemon.rs`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `A sourced line whose command fails runs no later command separated by `;` on that same line, while the next physical line still runs, probed on both sides with a depth-refused nested source-file, a missing source-file, and a failing kill-session.`

### `config.startup-diagnostic-delivery`: Deliver retained startup configuration causes

tmux retains clientless startup causes and routes them when a client becomes available, while zz currently discards the startup ConfigLoadReport after logging.

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
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Startup configuration causes survive daemon initialization and reach the first eligible Control or attached client, while a detached Command start still exits 0 with empty stdout and stderr.`
  - `Initial Control writes every `%config-error <declaring-file>:<line>: <cause>` before its first `%begin`; a Control client that attaches after detached startup receives retained causes inside the attach frame; a normal attached client opens the cause view.`

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

### `control-mode.diagnostic-typing`: Type Control-mode diagnostics

Control currently classifies generic warning events by English text prefixes, so localized Unix source errors, arbitrary non-Unix traversal errors, or future config wording can be silently dropped or misframed.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `protocol`
- User impact: scripts
- Items: `semantic:control-mode-typed-config-diagnostics`, `semantic:control-mode-typed-source-diagnostics`
- Depends on: none
- Evidence:
  - `resource:crates/zz-protocol/src/message.rs`
  - `resource:crates/zz-daemon/src/daemon.rs`
  - `resource:crates/zz/src/control_mode.rs`
- Acceptance:
  - `Typed protocol identity routes config and source diagnostics to their exact Control frames independently of localized or platform-specific prose.`

### `control-mode.sourced-command-frames`: Preserve frames for sourced Control commands

tmux inherits Control state onto every sourced command-queue item. zz replays commands synchronously through a sentinel Command client inside one request and has no sourced-command guard event.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `next` / `hard`
- Owner: `protocol`
- User impact: scripts
- Items: `semantic:control-mode-sourced-command-frame-per-command`
- Depends on: none
- Evidence:
  - `resource:crates/zz-protocol/src/message.rs`
  - `resource:crates/zz-daemon/src/daemon.rs`
  - `resource:crates/zz/src/control_mode.rs`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Sourcing an ordinary set-option and a nested quiet all-miss each emits its own flags-1 empty %end frame in file order.`

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

### `display-message.output-modes`: Complete display-message output modes

These are server-owned output and format operations with existing state.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `next` / `medium`
- Owner: `daemon`
- User impact: scripts, admin
- Items: `flag:display-message:-N`, `flag:display-message:-a`, `flag:display-message:-c`, `flag:display-message:-v`
- Depends on: none
- Evidence:
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Differential tests cover format variable listing, target-client context, verbose expansion, and message suppression.`

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

### `formats.buffer-context`: Expose buffer format facts

The paste-buffer model exists, but ordinary format expansion receives no selected buffer facts.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `daemon`
- User impact: daily, scripts
- Items: `format:buffer_created`, `format:buffer_full`, `format:buffer_name`, `format:buffer_sample`, `format:buffer_size`
- Depends on: none
- Evidence:
  - `resource:crates/zz-mux/src/formats.rs`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Buffer-targeted list and chooser rows expose creation time, size, sample, full value, and name from one retained context.`

### `formats.daemon-command-item-context`: Expose the invoking command format for daemon-run items

The mux dispatch chokepoint carries the canonical entry name into every command it runs, but the daemon preempts twenty verbs ahead of it and each builds its own format hooks, so `#{command}` expands empty inside those items.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `daemon`
- User impact: scripts
- Items: `semantic:daemon-command-item-command-format`
- Depends on: none
- Evidence:
  - `resource:crates/zz-daemon/src/daemon.rs`
  - `resource:crates/zz-mux/src/command.rs`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `The `run-shell -C` string pre-expansion and the `if-shell` condition expand `#{command}` to `run-shell` and `if-shell`, as do the other daemon-preempted verbs that expand formats, while a `{ ... }` command block stays unexpanded on both sides.`

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

### `formats.session-runtime`: Expose remaining session formats

Activity already exists; the session path should share the client cwd model.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `easy`
- Owner: `daemon`
- User impact: scripts
- Items: `format:session_active`, `format:session_activity`, `format:session_path`
- Depends on: none
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Session activity and working path have retained facts, target-aware expansion, and ordering tests.`

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

The remaining shared keys reach equivalent actions but retain 25 divergent stored command shapes.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `next` / `hard`
- Owner: `protocol`
- User impact: daily, remote, scripts
- Items: `binding:copy-mode-vi:#`, `binding:copy-mode-vi:*`, `binding:copy-mode-vi:/`, `binding:copy-mode-vi:1`, `binding:copy-mode-vi:2`, `binding:copy-mode-vi:3`, `binding:copy-mode-vi:4`, `binding:copy-mode-vi:5`, `binding:copy-mode-vi:6`, `binding:copy-mode-vi:7`, `binding:copy-mode-vi:8`, `binding:copy-mode-vi:9`, `binding:copy-mode-vi::`, `binding:copy-mode-vi:?`, `binding:copy-mode-vi:F`, `binding:copy-mode-vi:T`, `binding:copy-mode-vi:f`, `binding:copy-mode-vi:t`, `binding:copy-mode:C-r`, `binding:copy-mode:C-s`, `binding:copy-mode:F`, `binding:copy-mode:M-f`, `binding:copy-mode:T`, `binding:copy-mode:f`, `binding:copy-mode:t`
- Depends on: `copy-mode.command-fidelity`
- Evidence:
  - `resource:crates/zz-mux/src/compat_manifest_tests.rs`
  - `resource:crates/zz-protocol/src/key.rs`
  - `resource:knowledge/tmux/key-tables.md`
- Acceptance:
  - `Every shared emacs and vi copy binding matches the pin's rendered command or moves to a named native divergence; stock repeat metadata is already exact.`

### `keys.copy-mode-defaults`: Complete stock copy-mode keyboard bindings

The copy engine supports the common path, but the stock keyboard tables still omit navigation, prompt, and refresh bindings.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `next` / `medium`
- Owner: `protocol`
- User impact: daily, remote
- Items: `key:copy-mode-vi:P`, `key:copy-mode-vi:r`, `key:copy-mode:C-Down`, `key:copy-mode:C-M-Down`, `key:copy-mode:C-M-Up`, `key:copy-mode:C-M-b`, `key:copy-mode:C-M-f`, `key:copy-mode:C-Up`, `key:copy-mode:C-[`, `key:copy-mode:C-k`, `key:copy-mode:C-l`, `key:copy-mode:C-w`, `key:copy-mode:End`, `key:copy-mode:Home`, `key:copy-mode:M-1`, `key:copy-mode:M-2`, `key:copy-mode:M-3`, `key:copy-mode:M-4`, `key:copy-mode:M-5`, `key:copy-mode:M-6`, `key:copy-mode:M-7`, `key:copy-mode:M-8`, `key:copy-mode:M-9`, `key:copy-mode:M-<`, `key:copy-mode:M->`, `key:copy-mode:M-Down`, `key:copy-mode:M-R`, `key:copy-mode:M-Up`, `key:copy-mode:M-l`, `key:copy-mode:N`, `key:copy-mode:P`, `key:copy-mode:R`, `key:copy-mode:Space`, `key:copy-mode:g`, `key:copy-mode:n`, `key:copy-mode:r`
- Depends on: `copy-mode.command-fidelity`
- Evidence:
  - `resource:crates/zz-protocol/src/key.rs`
  - `resource:knowledge/tmux/key-tables.md`
- Acceptance:
  - `Default emacs and vi copy tables match the pin for keyboard presence, command action, and prompt behavior; stock repeat metadata is already exact.`

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

### `mux.error-shapes`: Match remaining command errors

Scripts can inspect exact errors even when both implementations reject the command.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `next` / `medium`
- Owner: `protocol`
- User impact: scripts
- Items: `flag-arity:resize-pane:-D`, `flag-arity:resize-pane:-L`, `flag-arity:resize-pane:-R`, `flag-arity:resize-pane:-U`, `positional-max:choose-buffer`, `positional-max:choose-tree`, `positional-max:display-message`, `positional-max:display-panes`, `positional-max:load-buffer`, `positional-max:save-buffer`, `positional-max:select-pane`, `positional-max:set-buffer`, `positional-min:bind-key`, `positional-min:confirm-before`, `positional-min:display-menu`, `positional-min:find-window`, `positional-min:if-shell`, `positional-min:load-buffer`, `positional-min:rename-session`, `positional-min:rename-window`, `positional-min:save-buffer`, `positional-min:set-environment`, `positional-min:set-option`, `positional-min:set-window-option`, `positional-min:source-file`, `positional-min:wait-for`, `semantic:command-arity-errors`, `semantic:command-flag-errors`, `semantic:nested-new-session-error-precedence`
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

### `source-file.flags`: Complete source-file controls

The parser and diagnostic streams exist; target and client context remain.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `next` / `medium`
- Owner: `daemon`
- User impact: scripts
- Items: `flag:source-file:-n`, `flag:source-file:-t`, `flag:source-file:-v`
- Depends on: none
- Evidence:
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `resource:knowledge/tmux/divergences.md`
  - `scenario:compat/scenarios/source-file-format.txt`
- Acceptance:
  - `Differential tests cover parse-only, target context, verbose diagnostics, and ordering across multiple files.`

### `source-file.nested-control-queue`: Match nested source-file Control queue semantics

tmux preflights every source-file argument before recursion and returns WAIT after any match. zz recurses per match, then synthesizes every recognized nested warning as %error.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `next` / `medium`
- Owner: `daemon`
- User impact: scripts
- Items: `semantic:source-file-nested-diagnostic-order`, `semantic:source-file-nested-partial-control-terminator`
- Depends on: `control-mode.sourced-command-frames`
- Evidence:
  - `resource:crates/zz-daemon/src/daemon.rs`
  - `resource:crates/zz/src/control_mode.rs`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `A nested hit-plus-miss source command emits its miss and ends %end; in a three-level source chain the containing command's miss precedes the deeper miss, with no loss or duplication.`

### `source-file.path-semantics`: Match source-file path semantics

Nested sources rebase from the containing file with that parent quoted literally, while event hooks can lose tmux's cwd-selection client.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `next` / `medium`
- Owner: `daemon`
- User impact: scripts
- Items: `semantic:source-file-event-hook-current-client-cwd`, `semantic:source-file-nested-client-cwd`
- Depends on: none
- Evidence:
  - `resource:knowledge/tmux/divergences.md`
  - `scenario:compat/scenarios/source-file-format.txt`
- Acceptance:
  - `Differential and attached-client tests pin nested and event-hook current-client cwd selection and rebasing plus literal quoting of a metacharacter-bearing nested-source parent.`

### `terminal.key-control`: Complete terminal key control flags

The terminal input path exists but lacks several tmux key and reset operations.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `later` / `medium`
- Owner: `terminal`
- User impact: scripts, daily
- Items: `flag:send-keys:-K`, `flag:send-keys:-R`, `flag:send-keys:-c`, `semantic:send-keys-copy-command-shape`, `semantic:send-keys-high-hex`, `semantic:send-keys-no-key-count`
- Depends on: none
- Evidence:
  - `resource:crates/zz-protocol/src/catalog.rs`
  - `resource:knowledge/tmux/divergences.md`
- Acceptance:
  - `Terminal and attached-client tests cover reset, clear, key-table injection, high bytes, counts, and copy commands.`

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

The source gate now reconciles structural names, coarse arity, constant stubs, selected context formats, and rendered shared binding commands plus repeat bits, but open-ended and runtime behavior still needs source-owned registrations instead of inference from matching structure.

- Decision: `adopt`
- Status: `open`
- Priority and ease: `next` / `medium`
- Owner: `protocol`
- User impact: scripts
- Items: `semantic:tracker-command-args-parse-callbacks`, `semantic:tracker-daemon-invalid-flag-runtime`, `semantic:tracker-hook-producer-partition`, `semantic:tracker-key-binding-behavior`, `semantic:tracker-nonconstant-format-behavior`, `semantic:tracker-open-context-format-vocabulary`, `semantic:tracker-option-consumer-registration`
- Depends on: none
- Evidence:
  - `resource:crates/zz-mux/src/compat_manifest_tests.rs`
  - `resource:knowledge/playbooks/compat-harness.md`
- Acceptance:
  - `Producer- or consumer-owned inventories reconcile custom argument callbacks, hook production, shared key behavior, nonconstant and open-ended context formats, option consumption, and daemon invalid-flag handling against the live manifest.`

## Known differential scenarios

| Scenario | Gap | TOPO | GEO | FMT | OUT | WARN |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| known/known-main-preset-two-panes.txt | layout.main-horizontal-upstream-bug | 0 | 1 | 0 | 0 | 0 |
| known/known-spread-mixed.txt | layout.spread-mixed-upstream-bug | 0 | 1 | 0 | 0 | 0 |

## Closed history

| ID | Closed | Resolution | Evidence |
| --- | --- | --- | --- |
| `alerts.remaining-edge-cases` | 2026-08-24 | The session_activity_flag and session_silence_flag formats now mirror the resolved target window, so list-sessions reads the active window and list-windows varies per row. Attach clears bell, activity, and silence flags only on the session's active window and releases every terminal bell latch there before producing the snapshot. Alert action gating and message labels are decided once from that active window and fan the same decision to every eligible Interactive client while the broader per-client focus model remains unchanged. Every successful monitor-silence write, including a same-value write or repeated global reset, emits MonitorSilenceChanged and resets every live window timer; a missing local -u and a rejected -o do not. Active status messages are dismissed before dispatch by a surviving bulk Text packet or explicit Paste as well as by writable key presses, while suppressed trailing text and every read-only input leave them armed. Alert-produced messages still bypass the daemon-owned lifecycle, including the pin's terminal-publication freeze, and remain open under alerts.message-lifecycle. | `resource:crates/zz-mux/src/formats.rs`, `resource:crates/zz-mux/src/command.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `scenario:compat/scenarios/alerts.txt`, `resource:knowledge/tmux/divergences.md`, `resource:knowledge/tmux/status-line.md` |
| `choosers.presentation-consistency` | 2026-08-24 | Protocol v72 appends a durable filter_no_matches bit to both chooser states. A full daemon rebuild sets it only when an explicit static -f filter produced no rows and the chooser restored its unfiltered rows; a matching filter or no filter clears it, while incremental search and selection deltas preserve it. TUI and GUI render the native status `filter: no matches` without replacing the selectable fallback rows. The GUI reserves its 46px shortcut cell for every rendered row only when at least one rendered row has a nonempty key, and removes the cell for a fully keyless list, matching the TUI's list-level gutter decision. The real attached-client fixture requires the status independently on the current tree and buffer chooser screens for both zz and pinned tmux. Native layout differences remain under choosers.native-presentation, while key vocabulary remains under choosers.command-flags. | `resource:crates/zz-protocol/src/message.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `resource:crates/zz-tui/src/render.rs`, `resource:crates/zz-ui/src/chooser.rs`, `file:crates/zz-client/src/core.rs`, `file:compat/attached-client.sh`, `resource:knowledge/tmux/choose-tree.md` |
| `clients.cwd-context` | 2026-08-23 | Protocol v72 carries a bounded cwd only for local endpoints; top-level command-client source-file resolves after -F and before globbing, with attached session-cwd selection retained in clients.attach-context. | `resource:crates/zz-protocol/src/message.rs`, `resource:crates/zz-daemon/src/client.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `file:crates/zz/tests/cli_binary.rs`, `file:compat/attached-client.sh`, `scenario:compat/scenarios/source-file-format.txt` |
| `commands.native-prefix-isolation` | 2026-08-24 | Exact canonical names and aliases resolve first. Non-exact lookups use tmux canonical names whenever any match exists and consult the guarded 19-name native roster only when tmux has no match. The daemon expands one immutable user-alias layer before read-only authorization and reuses it for dispatch and hooks, including per-command stored binding checks. Non-exact attach prefixes execute through the interactive command path, and static agent-send prefixes trigger stdin capture. The manifest gate derives the native roster from catalog minus the pinned oracle and checks every pinned canonical prefix; the strict 29-step scenario covers all 25 affected unique prefixes, exact catalog alias precedence, user command-alias expansion, and ambiguous list-commands exit parity. Exact native attach aliases, arbitrary live aliases that require client stdin, Control's static unknown-name precheck, and failing aliases named kill-server remain open under aliases.client-preflight. | `resource:crates/zz-protocol/src/catalog.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `resource:crates/zz/src/lib.rs`, `file:crates/zz-mux/src/compat_manifest_tests.rs`, `file:crates/zz/tests/cli_binary.rs`, `scenario:compat/scenarios/native-prefix-isolation.txt` |
| `commands.tmux-name-extensions` | 2026-08-24 | The pin's outer send-keys grammar is c:FHKlMN:Rt:X. zz removed C, P, and o from the outer catalog and returns the exact unknown-flag error when they appear there; the tracked c, K, and R gaps remain under terminal.key-control, and M remains under mouse.bound-context. The copy-mode parser recognizes -C and -P on the pin's 14 copy-family grammar entries and -o on next-prompt and previous-prompt; -- terminates its flag scan. Invalid local flags, actions, and arity produce no command error or copy action and reset the copy-mode repeat prefix to 1. Existing CopyModeCopy clipboard, paste-buffer, and pipe fields retain their behavior. Execution for copy-line, copy-line-and-cancel, copy-pipe-line, and copy-pipe-line-and-cancel remains under terminal.key-control through semantic:send-keys-copy-command-shape. The pin also redraws the first copy-mode line after a local parser failure; zz has no no-op redraw effect, so that presentation residue stays with the same item. | `resource:crates/zz-protocol/src/catalog.rs`, `resource:crates/zz-mux/src/command.rs`, `resource:knowledge/tmux/divergences.md`, `scenario:compat/scenarios/micro-flags.txt` |
| `formats.command-argument-expansion` | 2026-08-24 | `rename-session` and `rename-window` expand their new names after resolving the target and before changing its old session or window facts. Direct `show-options` and `show-window-options` calls perform one expansion of an optional option name in the target context; `show-hooks` marks its forwarded name as expanded. `select-pane -T` expands from the original target pane before a direction chooses the destination that receives the title. All five paths retain the canonical command-item name and explicit hook overrides. | `resource:crates/zz-mux/src/command.rs`, `scenario:compat/scenarios/command-item-format.txt` |
| `formats.command-item-context` | 2026-08-24 | `#{command}` is now a command-queue-item fact for every command the mux engine runs, not a list-row fact: after `command-alias` expansion and canonical resolution the dispatch chokepoint wraps the incoming format hooks once with the canonical entry name and threads that wrapper through every arm, so `display-message`, the list commands, and every other mux-executed item expand it, a typed alias or unique prefix still reports the canonical name, and it stays empty outside a command item. The wrapper consults the inner and item-state hooks first, so an explicit outer `command` value keeps winning, and list rows keep it invariant while `key_*` and `command_list_*` stay disjoint per row. Daemon-preempted verbs such as `run-shell -C` pre-expansion and the `if-shell` condition build their own format hooks and still expand it empty, tracked under `formats.daemon-command-item-context`. | `resource:crates/zz-mux/src/command.rs`, `file:crates/zz-mux/src/compat_manifest_tests.rs`, `scenario:compat/scenarios/command-item-format.txt` |
| `list-keys.remaining` | 2026-08-24 | list-keys now implements the pin's optional positional key, -1, -O, and -r grammar, including clustered and attached options, --, repeated -O last-wins behavior, exact one-positional and missing -O errors, key-before-sort-before-table error precedence, valid-but-absent and note-filtered unknown-key errors, global and per-table note sorting, and reversal only when an order is selected. Sorting and filtering happen before -1, while repeat and width aggregates are computed after truncation. Literal stored space bases render as Space and C-Space, widths use those spellings, and positional matching compares base, type, and modifiers without stored spelling or key flags. Command and Control clients receive the selected line on stdout; attached Interactive clients receive a display-time-backed frozen status message without a command-output overlay. Stock copy-mode and copy-mode-vi bindings now publish no repeat bits while persistent copy-table movement, jump capture, and numeric repetition remain runtime key-engine behavior. Equal-base, cross-table, inapplicable-field, and four-byte Unicode comparator cases are bounded separately under list-keys.deterministic-sort-ties, and the existing long Ctrl-/Alt- spelling overacceptance remains under the strict-key divergence. | `resource:crates/zz-protocol/src/catalog.rs`, `resource:crates/zz-protocol/src/key.rs`, `resource:crates/zz-mux/src/command.rs`, `resource:crates/zz-daemon/src/daemon.rs`, `file:crates/zz-mux/tests/hunt_claims.rs`, `file:compat/attached-client.sh`, `scenario:compat/scenarios/list-keys-padding.txt` |
| `options.show-options-hook-rows` | 2026-08-24 | With `-H`, `show-options` augments only no-positional listings with hook arrays in the pin's final option-table block and hook declaration order. Plain listings exclude hooks, named hook queries work without `-H`, server scope has none, and global-session, global-window, and inherited pane listings expose 57, 11, and 7 hooks. Empty, populated, indexed, named, value-only, pane-fallback, and whole-array-shadowing shapes match the pin, including the inherited empty array's `name*` in a full listing and bare `name` in a named query. `show-window-options` retains its surface without `-H`. | `resource:crates/zz-mux/src/command.rs`, `resource:crates/zz-mux/src/tmux_options.rs`, `scenario:compat/scenarios/show-options-hooks.txt`, `resource:knowledge/tmux/commands.md` |
| `options.window-status-separator` | 2026-08-24 | The daemon expands `window-status-separator` after each nonfinal item in the `status-format[]` window loop. It resolves the separator in that window's option and format context, including per-window overrides, nested formats, and style directives; the last item emits no separator. The TUI owns exact tmux row output. The native GUI derives its window controls from snapshot state and does not paint this separator. | `resource:crates/zz-mux/src/tmux_options.rs`, `file:crates/zz-daemon/src/status.rs`, `scenario:compat/scenarios/status-options.txt`, `resource:knowledge/tmux/status-line.md` |
| `source-file.glob-semantics` | 2026-08-23 | Unix source-file matching now uses glob(3) with tmux's cwd quoting, backslash escaping, leading-dot exclusion, nonrecursive repeated stars, malformed-pattern handling, per-pattern ordering, and declared-path diagnostics. | `resource:crates/zz-daemon/src/daemon.rs`, `file:compat/scenarios/source-file-format-w-0-0-10.conf`, `file:compat/scenarios/source-file-format-w-0-0-20.conf`, `file:compat/scenarios/source-file-glob/.hidden.conf`, `file:compat/scenarios/source-file-glob/10/10.conf`, `file:compat/scenarios/source-file-glob/20/nested/20.conf`, `file:compat/scenarios/source-file-glob/literalq.conf`, `file:compat/scenarios/source-file-glob/prefix-siblings/zz-client-ffi/match.conf`, `file:compat/scenarios/source-file-glob/prefix-siblings/zz-client/match.conf`, `file:compat/scenarios/source-file-glob/prefix-siblings/zz/match.conf`, `scenario:compat/scenarios/source-file-format.txt`, `scenario:compat/scenarios/smoke/source-file-diagnostics.txt` |
| `source-file.nested-diagnostic-semantics` | 2026-08-23 | Nested source-file no-match and glob errors now retain the post-F declared argument; a quiet no-match stays silent. Command clients receive stderr and exit 1, Interactive clients receive a warning, and recognized Control warnings carry the same declared text. Per-command sourced guards remain tracked under control-mode.sourced-command-frames; nested partial-match termination and cross-depth ordering remain tracked under source-file.nested-control-queue. Typed classification for localized or platform-specific Control diagnostics remains tracked under control-mode.diagnostic-typing. Relative nested paths still use zz's containing-file base, tracked separately under source-file.path-semantics. | `resource:crates/zz-daemon/src/daemon.rs`, `file:crates/zz/tests/cli_binary.rs`, `file:compat/scenarios/smoke/fixtures/source-file-diagnostics.conf`, `scenario:compat/scenarios/smoke/source-file-diagnostics.txt`, `scenario:compat/scenarios/smoke/source-file-control.txt` |
| `source-file.nesting-semantics` | 2026-08-23 | Counting the initial source-file as invocation 1, 50 concurrent source invocations now run and invocation 51 is refused before any of its paths are matched or loaded. Command clients get `too many nested files` on stderr and exit 1, Control clients get the same lowercase text on their error channel while the outer line continues, and attached clients get the pin's capitalized `Too many nested files` status message. `-q` does not suppress it, one diagnostic is emitted per refused command rather than per path, and the containing file keeps running its later lines. Exact Control frame placement is not closed: the pin carries the refusal inside the rejected nested command's own flags-1 %begin/%error frame while zz synthesizes a standalone %error, so the Control differential pins wording, count, depth, and continuation instead of frame boundaries, and the placement itself belongs to control-mode.sourced-command-frames and source-file.nested-control-queue. A malformed invocation at the refused depth is diagnosed as malformed rather than as depth on both sides, because the pin rejects it while parsing the containing file and never consults its depth guard; only that precedence, the stdout stream, and the exit status are closed, while the differing malformed text stays with mux.error-shapes and the pin's abandonment of the rest of the containing file stays with config.parser-edge-cases. A same-line sibling on the refused source's own line inside the containing config file still runs in zz where the pin drops the rest of that sourced line, tracked under config.same-line-error-group. Cumulative startup accounting closed separately under source-file.startup-depth-accounting. | `resource:crates/zz-daemon/src/daemon.rs`, `file:crates/zz/tests/cli_binary.rs`, `file:compat/attached-client.sh`, `file:compat/scenarios/smoke/fixtures/source-file-depth.sh`, `scenario:compat/scenarios/smoke/source-file-depth.txt`, `scenario:compat/scenarios/smoke/source-file-control.txt` |
| `source-file.startup-depth-accounting` | 2026-08-24 | One startup accounting value now spans every explicit or discovered top-level configuration. The roots do not consume slots; source commands 1 through 50 run, command 51 and later retain the declaring file and line in their cause, quiet misses consume slots, and one command with many paths consumes one slot. Runtime sequential source commands remain unbounded, while the zz-native `reload-config` whole-root replay takes one fresh startup budget of its own so reloading a file lands the same state a fresh start would. Client delivery and placement of retained startup causes remain tracked under config.startup-diagnostic-delivery. | `resource:crates/zz-daemon/src/daemon.rs`, `file:crates/zz/tests/cli_binary.rs` |
| `source-file.tilde-semantics` | 2026-08-23 | source-file no longer rewrites a literal leading ~/ after parsing: parser-expanded leading tildes still arrive as absolute paths, top-level literal tildes pass through cwd resolution, and nested literal tildes follow the separately tracked nested-base rule. The CLI regression pins the top-level choice against a metacharacter-bearing daemon HOME. | `resource:crates/zz-daemon/src/daemon.rs`, `file:crates/zz/tests/cli_binary.rs`, `file:compat/scenarios/smoke/fixtures/source-file-tilde-decoy.conf`, `scenario:compat/scenarios/smoke/source-file-tilde.txt` |
