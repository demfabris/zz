---
type: Subsystem
title: tmux command set (command.rs)
description: "MuxEngine, the tmux-style command executor: canonical names + aliases, shared option/flag parsing, -t target resolution, and structured MuxEffect side effects for the daemon."
resource: crates/zz-mux/src/command.rs
tags: [tmux, commands, mux-engine, targets, effects]
timestamp: 2026-08-15T00:00:00Z
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
| `-t $N` / session name | `resolve_session` | `$id`, exact session name, unique name prefix (`work` still beats `workshop` exactly; `wor` fails with the candidates named), or current/first session. |
| `-t @N` / `:index` / name | `resolve_window` | `@id`, `:index`, window name (unique within session), or current window. |
| `-t %N` | `resolve_pane` | `%id` only (validated live), or the window's active pane. |
| `-t:.+` / `-t:.-` | `select-pane` | canonical next/previous pane in `pane_order`. |
| `-t` on `new-window` / `new-browser` / `break-pane` | `window_destination` | tmux's target-window-with-index form: the destination index need not exist yet. `$id`, `@id`, and a bare session name pick a session (lowest free index); `[session]:index` and a bare index that names no session pick an exact index, and a taken index fails with `index in use: N` unless `-k` replaces it. |

# Schema: commands (name | aliases | purpose)

When `new-session` omits `-s`, `next_session_name` selects the first unused canonical decimal name
(`0`, `1`, …). It marks numeric names in a bounded `n + 1` occupancy bitmap and materializes only the
winning string; noncanonical lookalikes such as `00` do not reserve `0`.

| Command | Aliases | Purpose |
| --- | --- | --- |
| `new-session` | `new` | Create a session (with its first window + terminal pane) and request attachment for an interactive caller; command-only callers remain detached. `-A` attaches to the `-s` session when it already exists instead of failing on the duplicate name (`-d` keeps that attach silent); `-t` (session groups) is a cataloged but rejected option. |
| `list-sessions` | `ls` | List sessions. |
| `rename-session` | `rename` | Rename a session (rejects duplicate names). |
| `kill-session` | . | Remove a session and its windows/panes. `-a` keeps the target and kills every other session; a bare positional name is the target. |
| `attach-session` | `attach` | Attach the client to a session (`Attach` effect); `-d` detaches the session's other clients. Client flags zz has no model for (`-r` read-only, `-x`, `-E`, `-c`, `-f`) are rejected instead of swallowed. |
| `detach-client` | `detach` | Detach (`Detach(DetachScope)` effect). No flags detaches the caller, `-a` detaches every *other* attached client, `-s` detaches every client on that session (the caller included). `-t` (target client), `-P`, and `-E` are rejected: zz has no client-name selector. |
| `new-window` | `neww` | Create a terminal window at the `-t` destination. `-d` creates without selecting, `-a` inserts after the target and shifts the run of windows above it up, `-k` replaces whatever holds the index, `-S` selects an existing window with the `-n` name instead of creating one. |
| `new-browser` | . | *zz-native:* create a browser window (`-p` profile, URL positional); shares `new-window`'s destination options. |
| `list-windows` | `lsw` | List a session's windows. |
| `rename-window` | `renamew` | Rename a window (duplicates allowed). |
| `select-window` | `selectw` | Activate a window. `-n`/`-p`/`-l` are the `next-window`/`previous-window`/`last-window` commands (`-n` wins over `-p` wins over `-l`, as in tmux); `-T` on a target that is already current behaves like `last-window`. |
| `next-window` / `previous-window` | `next` / `previous` | Step windows (wraps). `-t` picks the session; `-a` steps to the next/previous window holding an alert (any pane with a latched bell) and errors when none does. |
| `kill-window` | `killw` | Remove a window; `-a` keeps the target and kills every other window in its session. |
| `new-pane` | . | *zz-native/internal:* split into a runtime-free picker (`-h` horizontal, else vertical); accepts the `split-window` target/size/cwd options so configured bindings retain their arguments. |
| `split-window` | `splitw` | Split a terminal pane and inherit the target pane's live cwd when invoked directly; key-bound invocations are routed by the daemon to `new-pane` so `.tmux.conf` owns the picker keys. The new pane's share comes from `-l` (cells, or `N%`) or legacy `-p N`; `-b` puts it left/above, `-f` spans the whole window, `-d` leaves focus on the target. |
| `split-browser` | . | *zz-native:* split into a browser pane (`-p` profile, URL positional). |
| `select-pane-kind` | . | *zz-native/internal:* materialize a pending picker as `terminal`, `browser`, `agent`, or `editor` (`-t` target; `-c` supplies an Agent cwd). |
| `break-pane` | `breakp` | Reparent a pane into a new one-pane window (`-n`,`-s`,`-t`,`-d`); `-t` names the destination index, not an existing window. |
| `join-pane` / `move-pane` | `joinp` / `movep` | Insert a pane beside another (`-b`,`-f`,`-h/-v`,`-p`,`-s`,`-t`,`-d`). |
| `set-browser-url` | . | *zz-native:* update a browser pane's URL. |
| `set-browser-profile` | . | *zz-native:* validate and switch a browser pane's persistent zz profile (`-t`, one profile name). |
| `set-agent-session` | . | *zz-native/internal:* atomically persist an Agent pane's opaque ACP session ID and optional absolute cwd (`-t`, `-c`). |
| `set-agent-provider` | . | *zz-native/internal:* persist `codex` or `claude-code` for an Agent pane, clear its provider-bound session ID, and restart its adapter (`-t`). |
| `restart-agent-pane` | . | *zz-native/internal:* replace an Agent pane's daemon-owned ACP adapter (`-t`). |
| `select-pane` | `selectp` | Select by target/direction (`-L/-R/-U/-D`), `-l` last, `-Z` keep zoom; `-T` updates only the pane title. |
| `last-pane` | `lastp` | Return to the previously active pane (`-Z`). |
| `swap-pane` | `swapp` | Exchange two layout leaves (`-U/-D/-s/-t`, `-d` keep slot, `-Z`). |
| `list-panes` | `lsp` | List a window's panes. |
| `resize-pane` | `resizep` | Resize relatively (`-L/-R/-U/-D` by cells, default `1`) or absolutely (`-x`/`-y` in cells or `N%`) or `-Z` toggle zoom. Sizes in cells need the geometry the daemon reports per pane; without it the command errors instead of guessing. |
| `select-layout` | `selectl` | Apply a named preset / `-o` restore / `-E` spread / `-n`/`-p` cycle. |
| `next-layout` / `previous-layout` | `nextl` / `prevl` | Cycle the seven presets. |
| `rotate-window` | `rotatew` | Rotate surfaces through layout slots (`-D`,`-U`,`-Z`). |
| `kill-pane` | `killp` | Remove a pane (removes the window if it was the last); `-a` keeps the target and kills every other pane in its window. |
| `send-keys` | `send` | Send keys/text (`-l` literal, `-H` hexadecimal character codes) or `-X` copy-mode actions. `-N` is a repeat count: the whole key list is sent N times, and an `-X` action is dispatched N times — except a copy or a cancel, which stays single because repeating it would mean something tmux never does. Flags with no zz model (`-R` terminal reset, `-M`, `-K`, `-F`) are rejected rather than dropped. |
| `copy-mode` | . | Enter copy mode (`-u` page up, `-d` page down). `-e` (exit at the bottom of the history) is **rejected**: zz's copy-mode state has no exit-at-bottom latch and carrying one would change the `TerminalViewAction` wire enum, so `copy-mode -e` errors instead of silently entering plain copy mode. |
| `copy-mode-search-prompt` | . | *zz-native:* open the native copy-mode search prompt (`-b` backward). |
| `command-prompt` | . | Open the native command prompt (`-p`,`-I`, `%%` template). `-b` is accepted and already true: the prompt never blocks its caller. |
| `focus-sidebar` | . | *zz-native:* show and focus the workspace sidebar (`-t`). |
| `choose-tree` | . | Open the pane chooser; `-s`/`-w` route to the sidebar (`-s`,`-w`,`-Z`,`-t`). `-Z` is accepted and already true: zz's chooser is a full-window overlay, so there is nothing left to zoom. |
| `choose-buffer` | . | Open the paste-buffer chooser (`-Z`,`-t`); `-Z` is accepted and already true, as for `choose-tree`. |
| `display-panes` | `displayp` | Pane-number overlay (`-d` duration). `-b` is accepted and already true: the effect returns immediately, so nothing was ever blocked. |
| `clear-history` | `clearhist` | Clear a pane's scrollback (`-H` unsupported). |
| `bind-key` / `unbind-key` | `bind` / `unbind` | Add/remove key bindings (`-n`,`-r`,`-T`,`-N`). |
| `list-keys` | `lsk` | Print bindings as `bind-key` lines; `-T` limits the table. tmux's other selectors are rejected, not ignored: `-n` is not a tmux `list-keys` flag at all, and `-1`/`-N`/`-a`/`-O`/`-P`/`-r` plus the `[key]` positional have no zz form. |
| `set-option` / `set-window-option` | `set` / `setw` | Set options (see below). |
| `source-file` | `source` | Load config files: every path is sourced, in order, one `SourceFile` effect each. Without `-q` a path that does not exist is reported to the caller as a warning; `-q` keeps it silent. `-F`/`-n`/`-v`/`-t` are rejected. |
| `reload-config` | . | *zz-native:* reload tmux + Ghostty config (`ReloadConfig` effect, no args). |
| `kill-server` | . | Stop the daemon (`KillServer` effect). |

Options handled by `set-option`/`set-window-option`: `synchronize-panes` (global→window→pane scope,
`-g/-w/-p/-u/-U/-o`), `buffer-limit` (global, default 50), `history-limit` (session, default 10000,
0–1,000,000), `word-separators` (session, `-a` append), `mode-keys` (`vi`→`copy-mode-vi`,
`emacs`→`copy-mode`), `prefix`, `set-clipboard` (`on`/`external`/`off`), and `copy-command`.
`-o` (set only if unset) holds everywhere: a global option always counts as set, so `set -o` on one
errors with `option is already set: NAME` unless `-q` silences it; per-window/per-pane scopes check
their override slot. Each option validates the flags it accepts and rejects the rest, so a flag that
does not apply to that option is never quietly dropped.
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
`WordSeparatorsChanged`, `ModeKeysChanged`, `Attach`, `Detach(DetachScope)` (`Client`, `Others`, or
`Session`, which the daemon maps onto its attached-client table), `SourceFile { path, quiet }`,
`ReloadConfig`, `KillServer`, and `SnapshotChanged` (appended whenever the mux generation advanced).
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
