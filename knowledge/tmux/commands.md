---
type: Subsystem
title: tmux command set (command.rs)
description: "MuxEngine, the tmux-style command executor: canonical names + aliases, shared option/flag parsing, -t target resolution, and structured MuxEffect side effects for the daemon."
resource: crates/zz-mux/src/command.rs
tags: [tmux, commands, mux-engine, targets, effects]
timestamp: 2026-07-26T00:00:00Z
---

# Overview

`command.rs` defines `MuxEngine`, the executor that turns a parsed
[`CommandInvocation`](/crates/zz-protocol.md) into mutations on [`MuxState`](/crates/zz-mux.md) plus a list
of [`MuxEffect`](#effects) side effects the daemon acts on. `MuxEngine` owns the `MuxState`, the
[`KeyTables`](/tmux/key-tables.md), and the tmux options that are not per-window layout: `mode-keys`
(global + per-window), `set-clipboard`, `copy-command`, `buffer-limit`, `history-limit`
(global + per-session), and `word-separators` (global + per-session). `execute` canonicalizes the
command name, dispatches to a handler, and, if the state `generation` advanced, appends a
`SnapshotChanged` effect and repairs the `ExecutionContext` (`session`/`window`/`pane`) so the
current target stays valid after the mutation.

Errors are structured `ServerError`s (`MissingTarget`, `AmbiguousTarget`, `InvalidTarget`,
`UnsupportedCommand`, `InvalidCommand`) and never partially apply after validation fails.
`catalog.rs` is the shared renderer-free source for canonical names, aliases, descriptions,
accepted flags/options, and completion value kinds; `canonical_command` and the native
[command palette](/concepts/command-palette.md) both consume it.

# Argument parsing and `-t` targets

`parse_command_options(command, args)` consults the command catalog and splits args into
`Options { flags, values }` plus positionals. Value options (for example `-t`, `-n`, `-p`, `-s`)
take the next arg or an attached form (`-tfoo`); a bare `--` ends option parsing; clustered short
flags (`-Zs`) split into `-Z -s`. Target resolution lives in `MuxState`:

| Target | Resolver | Accepts |
| --- | --- | --- |
| `-t $N` / session name | `resolve_session` | `$id`, exact unique session name, or current/first session. |
| `-t @N` / `:index` / name | `resolve_window` | `@id`, `:index`, window name (unique within session), or current window. |
| `-t %N` | `resolve_pane` | `%id` only (validated live), or the window's active pane. |
| `-t:.+` / `-t:.-` | `select-pane` | canonical next/previous pane in `pane_order`. |

# Schema: commands (name | aliases | purpose)

When `new-session` omits `-s`, `next_session_name` selects the first unused canonical decimal name
(`0`, `1`, …). It marks numeric names in a bounded `n + 1` occupancy bitmap and materializes only the
winning string; noncanonical lookalikes such as `00` do not reserve `0`.

| Command | Aliases | Purpose |
| --- | --- | --- |
| `new-session` | `new` | Create a session (with its first window + terminal pane) and request attachment for an interactive caller; command-only callers remain detached. |
| `list-sessions` | `ls` | List sessions. |
| `rename-session` | `rename` | Rename a session (rejects duplicate names). |
| `kill-session` | . | Remove a session and its windows/panes. |
| `attach-session` | `attach` | Attach the client to a session (`Attach` effect). |
| `detach-client` | `detach` | Detach (`Detach` effect). |
| `new-window` | `neww` | Create a terminal window. |
| `new-browser` | . | *zz-native:* create a browser window (`-p` profile, URL positional). |
| `list-windows` | `lsw` | List a session's windows. |
| `rename-window` | `renamew` | Rename a window (duplicates allowed). |
| `select-window` | `selectw` | Activate a window. |
| `next-window` / `previous-window` | `next` / `previous` | Step windows (wraps). |
| `kill-window` | `killw` | Remove a window. |
| `new-pane` | . | *zz-native/internal:* split into a runtime-free picker (`-h` horizontal, else vertical); accepts the `split-window` target/size/cwd options so configured bindings retain their arguments. |
| `split-window` | `splitw` | Split a terminal pane and inherit the target pane's live cwd when invoked directly; key-bound invocations are routed by the daemon to `new-pane` so `.tmux.conf` owns the picker keys. |
| `split-browser` | . | *zz-native:* split into a browser pane (`-p` profile, URL positional). |
| `select-pane-kind` | . | *zz-native/internal:* materialize a pending picker as `terminal`, `browser`, or `agent` (`-t` target). |
| `break-pane` | `breakp` | Reparent a pane into a new one-pane window (`-n`,`-s`,`-t`,`-d`). |
| `join-pane` / `move-pane` | `joinp` / `movep` | Insert a pane beside another (`-b`,`-f`,`-h/-v`,`-p`,`-s`,`-t`,`-d`). |
| `set-browser-url` | . | *zz-native:* update a browser pane's URL. |
| `set-browser-profile` | . | *zz-native:* validate and switch a browser pane's persistent zz profile (`-t`, one profile name). |
| `set-agent-session` | . | *zz-native/internal:* atomically persist an Agent pane's opaque ACP session ID and optional absolute cwd (`-t`, `-c`). |
| `set-agent-provider` | . | *zz-native/internal:* persist `codex` or `claude-code` for an Agent pane and clear its provider-bound session ID (`-t`). |
| `select-pane` | `selectp` | Select by target/direction (`-L/-R/-U/-D`), `-l` last, `-Z` keep zoom; `-T` updates only the pane title. |
| `last-pane` | `lastp` | Return to the previously active pane (`-Z`). |
| `swap-pane` | `swapp` | Exchange two layout leaves (`-U/-D/-s/-t`, `-d` keep slot, `-Z`). |
| `list-panes` | `lsp` | List a window's panes. |
| `resize-pane` | `resizep` | Resize (`-L/-R/-U/-D` by cells) or `-Z` toggle zoom. |
| `select-layout` | `selectl` | Apply a named preset / `-o` restore / `-E` spread / `-n`/`-p` cycle. |
| `next-layout` / `previous-layout` | `nextl` / `prevl` | Cycle the seven presets. |
| `rotate-window` | `rotatew` | Rotate surfaces through layout slots (`-D`,`-U`,`-Z`). |
| `kill-pane` | `killp` | Remove a pane (removes the window if it was the last). |
| `send-keys` | `send` | Send keys/text (`-l` literal) or `-X` copy-mode actions. |
| `copy-mode` | . | Enter copy mode (`-u` page up). |
| `copy-mode-search-prompt` | . | *zz-native:* open the native copy-mode search prompt (`-b` backward). |
| `command-prompt` | . | Open the native command prompt (`-b`,`-p`,`-I`, `%%` template). |
| `focus-sidebar` | . | *zz-native:* show and focus the workspace sidebar (`-t`). |
| `choose-tree` | . | Open the pane chooser; `-s`/`-w` route to the sidebar (`-s`,`-w`,`-Z`,`-t`). |
| `choose-buffer` | . | Open the paste-buffer chooser (`-Z`,`-t`). |
| `display-panes` | `displayp` | Pane-number overlay (`-d` duration, `-b` no-op). |
| `clear-history` | `clearhist` | Clear a pane's scrollback (`-H` unsupported). |
| `bind-key` / `unbind-key` | `bind` / `unbind` | Add/remove key bindings (`-n`,`-r`,`-T`,`-N`). |
| `list-keys` | `lsk` | Print bindings as `bind-key` lines. |
| `set-option` / `set-window-option` | `set` / `setw` | Set options (see below). |
| `source-file` | `source` | Load another config file (`SourceFile` effect). |
| `reload-config` | . | *zz-native:* reload tmux + Ghostty config (`ReloadConfig` effect, no args). |
| `kill-server` | . | Stop the daemon (`KillServer` effect). |

Options handled by `set-option`/`set-window-option`: `synchronize-panes` (global→window→pane scope,
`-g/-w/-p/-u/-U/-o`), `buffer-limit` (global, default 50), `history-limit` (session, default 10000,
0–1,000,000), `word-separators` (session, `-a` append), `mode-keys` (`vi`→`copy-mode-vi`,
`emacs`→`copy-mode`), `prefix`, `set-clipboard` (`on`/`external`/`off`), and `copy-command`.
Buffer commands (`capture-pane`, `*-buffer`, `paste-buffer`) are handled by
[the server](/crates/zz-daemon.md), **not** here.

# Daemon-side workspace verbs

Five zz-native verbs are handled by [the daemon](/crates/zz-daemon.md) before the engine sees them,
for the same reason `capture-pane` is: each acts on something `MuxState` does not own. They have no
`catalog.rs` entry and therefore no [command-palette](/concepts/command-palette.md) completion, but
they are ordinary commands from the CLI and from a key binding.

| Command | Purpose |
| --- | --- |
| `tools` | Print the agent-readable catalog of workspace verbs. Pure output; the self-teaching entry point for an agent running in a pane. |
| `agent-send` | `[-t %N] [--submit] [--context PATH[:START[-END]]] [TEXT]` . append text to a GUI-owned Agent composer, or submit it as a prompt. A non-agent or omitted target routes to that window's most recently focused Agent pane. Reads stdin when TEXT is omitted; capped at 1 MiB. See [Agent pane](/concepts/agent-pane.md). |
| `send-last-output` | `-t %N` . route a terminal pane's last completed command and output (OSC 133 marks) into the window's most recently focused Agent pane. Bound to `<prefix> e`. |
| `capture-browser` | `-t %N -o /abs/path.png` . write a browser pane's latest rendered frame to a PNG. The path must be absolute because the GUI process writes it. |
| `debug-marker` | `[NOTE]` . stamp a `user_marker` line into the daemon's log so the moment an incident was noticed is findable later. The GUI's `DebugMark` key (`cmd-shift-m`/`ctrl-shift-m`) forwards here after stamping the app's own log. |

`agent-send` (in both forms) and `capture-browser` are **round trips**: the daemon publishes the
request to the attached GUI and parks the calling command thread on
`ProtocolMessage::GuiResponse` (5 s timeout), because only the GUI knows whether an ACP session is
idle and only the GUI has the CEF frame. The GUI answers from its mux observation rather than its
render loop, so a minimized window still replies. `MuxState::recent_agent_pane` picks the recipient
for `send-last-output` and for any `agent-send` whose target is not itself an Agent pane, with the
same active → focus-history → layout-order rule as `cwd_donor`.

# Effects

`MuxEffect` variants returned to the daemon adapter include: `PaneCreated`, `PanesRemoved`,
`PaneRelocated`, `SendKeys`, `TerminalView` (scroll/copy-mode), `TerminalUi` (search prompt),
`CommandPrompt`, `FocusSidebar`, `ChooseTree`, `ChooseBuffer`, `DisplayPanes`, `BufferLimitChanged`,
`WordSeparatorsChanged`, `ModeKeysChanged`, `Attach`, `Detach`, `SourceFile`, `ReloadConfig`,
`KillServer`, and `SnapshotChanged` (appended whenever the mux generation advanced).
Terminal split effects set `PaneCreated::inherit_cwd_from` to the resolved target pane; initial
session/window panes and browser panes leave it empty.

# Key files

| File | Role |
| --- | --- |
| `crates/zz-mux/src/command.rs` | `MuxEngine::execute`, per-command handlers, option parsing/setters, `MuxEffect`, and `copy_mode_action`. |
| `crates/zz-protocol/src/catalog.rs` | `canonical_command` plus shared canonical names, aliases, descriptions, options, and completion value kinds. |
| `crates/zz-mux/src/model.rs` | `MuxState` mutations and `resolve_session/window/pane` target resolution. |

# Related

- Mutates the [split-pane layout](/concepts/split-pane-layout.md) in [MuxState](/crates/zz-mux.md);
  `-X` actions drive [copy mode](/tmux/copy-mode.md); `choose-*` drive [choosers](/tmux/choose-tree.md).
- Bindings that invoke these come from the [key tables](/tmux/key-tables.md); text comes from the
  [conf parser](/tmux/conf-parser.md). Effects consumed by the [server daemon](/crates/zz-daemon.md).
- Names/aliases checked against the [tmux upstream reference](/references/tmux-upstream.md); see
  [tmux compatibility](/tmux/tmux-compat.md).
