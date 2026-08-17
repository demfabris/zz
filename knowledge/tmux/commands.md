---
type: Subsystem
title: tmux command set (command.rs)
description: "MuxEngine, the tmux-style command executor: canonical names + aliases, shared option/flag parsing, -t target resolution, and structured MuxEffect side effects for the daemon."
resource: crates/zz-mux/src/command.rs
tags: [tmux, commands, mux-engine, targets, effects]
timestamp: 2026-08-17T00:00:00-03:00
---

# Overview

`command.rs` defines `MuxEngine`, the executor that turns a parsed
[`CommandInvocation`](/crates/zz-protocol.md) into mutations on [`MuxState`](/crates/zz-mux.md) plus a list
of [`MuxEffect`](#effects) side effects the daemon acts on. `MuxEngine` owns the `MuxState`, the
[`KeyTables`](/tmux/key-tables.md), and the tmux options that are not per-window layout: `mode-keys`
(global + per-window), `set-clipboard`, `copy-command`, `buffer-limit`, `message-limit`, `history-limit`
(global + per-session), `word-separators` (global + per-session), and the phase-4f behavior-option
overrides. It also owns free-form `@`
option maps at every tmux scope and global/per-session process-environment overlays. `execute` canonicalizes the
command name, dispatches to a handler, and, if the state `generation` advanced, appends a
`SnapshotChanged` effect and repairs the `ExecutionContext` (`session`/`window`/`pane`) so the
current target stays valid after the mutation.

Errors are structured `ServerError`s. Target lookup uses `SessionNotFound`, `WindowNotFound`, and
`PaneNotFound`, displayed as tmux's `can't find TYPE: COMPONENT`; internal stable-ID failures retain
`MissingTarget`. Other command errors use `UnsupportedCommand` and `InvalidCommand`. Most handlers validate before mutation. tmux orders some
mutations before later failures; `select-layout`, for example, unzooms its resolved window before it
parses a custom layout. The daemon publishes any generation change at the command boundary even
when the handler returns an error.
`catalog.rs` is the shared renderer-free source for canonical names, aliases, descriptions,
accepted usage strings, flags/options, and completion value kinds; `canonical_command` and the native
[command palette](/concepts/command-palette.md) both consume it.

# Argument parsing and `-t` targets

`parse_command_options(command, args)` consults the command catalog and splits args into
`Options { flags, values }` plus positionals. Value options (for example `-t`, `-n`, `-p`, `-s`)
take the next arg or an attached form (`-tfoo`); a bare `--` ends option parsing; clustered short
flags (`-Zs`) split into `-Z -s`. Target resolution lives in `MuxState`:

| Target | Resolver | Accepts |
| --- | --- | --- |
| `-t $N` / session name | `resolve_session` | `$id`, exact name, `=name` exact-only escape, unique prefix, unique `fnmatch` pattern (`*`, `?`, `[…]`, with `/` ordinary), or the current/first session. An empty string means current. |
| `-t @N` / `:index` / name | `resolve_window` | `@id`; wrapped `+N`/`-N`; `{start}`/`^`, `{end}`/`$`, `{last}`/`!`, `{next}`/`+`, `{previous}`/`-`; numeric index; exact name; `=name` exact-only escape; unique prefix; unique `fnmatch`; or current. The first session/window component that fails names itself in the error. |
| `-t %N` / pane index | `resolve_pane` | `%id`, pane index, wrapped `+N`/`-N`, `!`/`{last}`, a window target's active pane, or current. Compound failures report only the failing pane component. |
| `-t:.+` / `-t:.-` | `select-pane` | canonical next/previous pane in `pane_order`. |
| `-t` on `new-window` / `new-browser` / `break-pane` | `resolve_window_index_target` | The same centralized grammar in tmux's target-window-with-index mode. A numeric or `+N`/`-N` destination may name an unused index; existing names, patterns, and tokens contribute their resolved index. A taken `new-window` index fails with `create window failed: index N in use` unless `-k` replaces it. |

# Schema: commands (name | aliases | purpose)

When `new-session` omits `-s`, `next_session_name` selects the first unused canonical decimal name
(`0`, `1`, …). It marks numeric names in a bounded `n + 1` occupancy bitmap and materializes only the
winning string; noncanonical lookalikes such as `00` do not reserve `0`.

| Command | Aliases | Purpose |
| --- | --- | --- |
| `new-session` | `new` | Create a session (with its first window + terminal pane) and request attachment for an interactive caller; command-only callers remain detached. A targetless Command client starts from the most-recent session, so `-c '#{pane_current_path}'` expands against the same origin the pin selects. A first-window `-n` installs window-local `automatic-rename off`. `-A` attaches when the session already exists (bare `-A` resolves the current session, `-D` detaches its other clients, and `-d` is ignored on the attach path, as in tmux); `-t` (session groups) and the valued options zz has no model for (`-e`,`-F`,`-f`,`-x`,`-y`) are cataloged but rejected so their values can never leak into the pane command. |
| `list-sessions` | `ls` | List sessions. |
| `rename-session` | `rename` | Rename a session (rejects duplicate names). |
| `kill-session` | . | Remove a session and its windows/panes. `-a` keeps the target and kills every other session; `-C` clears the session's pane bells and kills nothing, outranking `-a` as in tmux. Positional targets are refused — tmux's bound is zero arguments, and the kill commands are too destructive to guess. |
| `attach-session` | `attach` | Attach the client to a session (`Attach` effect); `-d` detaches the session's other clients. Client flags zz has no model for (`-r` read-only, `-x`, `-E`, `-c`, `-f`) are rejected instead of swallowed. |
| `detach-client` | `detach` | Detach (`Detach(DetachScope)` effect). No flags detaches the caller, `-a` detaches every *other* attached client, `-s` detaches every client on that session (the caller included). `-t` (target client), `-P`, and `-E` are rejected: zz has no client-name selector. |
| `list-clients` | `lsc` | List attached clients from the daemon registry, sorted by client name. `-t` filters by session and `-F` expands client formats. zz reports unknown client dimensions as `0x0` and leaves the unavailable terminal name and flags empty. Detached command connections do not appear. |
| `refresh-client` | `refresh` | Parse tmux's complete argument grammar, then return `no current client` for a detached command client. Attached-client redraw and control-mode behavior remain unsupported. |
| `new-window` | `neww` | Create a terminal window at the `-t` destination. `-d` creates without selecting, `-a` inserts after an occupied target but keeps an explicitly free index, `-k` replaces whatever holds the index, `-S` selects an existing window with the `-n` name instead of creating one. An explicit `-n` also installs the pin's window-local `automatic-rename off`. |
| `new-browser` | . | *zz-native:* create a browser window (`-p` profile, URL positional); shares `new-window`'s destination options. |
| `list-windows` | `lsw` | List a session's windows. `-a` lists every session in bytewise name order, then each session's windows by index. |
| `rename-window` | `renamew` | Rename a window (duplicates allowed) and install a window-local `automatic-rename off`, so scripts observe the same explicit-name pin as tmux. |
| `select-window` | `selectw` | Activate a window. `-n`/`-p`/`-l` are the `next-window`/`previous-window`/`last-window` commands (`-n` wins over `-p` wins over `-l`, as in tmux); `-T` on a target that is already current behaves like `last-window`. |
| `next-window` / `previous-window` | `next` / `prev` | Step windows (wraps). A step that would land on the current window errors `no next window` / `no previous window`, tmux's strings; `-t` picks the session; `-a` steps to the next/previous window holding an alert (any pane with a latched bell) with the same error when none qualifies. |
| `kill-window` | `killw` | Remove a window; `-a` keeps the target and kills every other window in its session. |
| `move-window` | `movew` | Move a stable window to another index or session. `-a` and `-b` insert around an occupied target, `-d` avoids selecting the moved window unless `-k` replaces the current destination, `-k` replaces an occupied index, and standalone `-r` renumbers from `base-index`. An occupied destination without `-k` returns `index in use: N`. |
| `swap-window` | `swapw` | Exchange two window slots within one session or across sessions while window ids, panes, names, and layouts travel together. `-d` selects the destination slots after the swap. |
| `find-window` | `findw` | Accept tmux's `-CiNrTZ` search grammar and validate `-t`. Detached CLI calls return success with no output for both matches and zero matches; zz does not open a GUI chooser. |
| `split-picker` | . | *zz-native:* split into a runtime-free picker (`-h` horizontal, else vertical); accepts the `split-window` target/size/cwd options. Bound on zz's default `%`/`"` keys. Renamed from `new-pane` (the pinned tmux owns that name for floating panes). |
| `split-window` | `splitw` | Split a terminal pane and inherit the target pane's live cwd; key-bound, CLI, and command-prompt invocations all create a terminal, like tmux. The new pane's share comes from `-l` (cells, or `N%`) or legacy `-p N`; `-b` puts it left/above, `-f` spans the whole window, `-d` leaves focus on the target. |
| `split-browser` | . | *zz-native:* split into a browser pane (`-p` profile, URL positional). |
| `select-pane-kind` | . | *zz-native/internal:* materialize a pending picker as `terminal`, `browser`, `agent`, or `editor` (`-t` target; `-c` supplies an Agent cwd). |
| `break-pane` | `breakp` | Reparent a pane into a new one-pane window (`-n`,`-s`,`-t`,`-d`); `-t` names the destination index, not an existing window. |
| `join-pane` / `move-pane` | `joinp` / `movep` | Insert a pane beside another (`-b`,`-f`,`-h/-v`,`-p`,`-s`,`-t`,`-d`). |
| `set-browser-url` | . | *zz-native:* update a browser pane's URL. |
| `set-browser-profile` | . | *zz-native:* validate and switch a browser pane's persistent zz profile (`-t`, one profile name). |
| `set-agent-session` | . | *zz-native/internal:* atomically persist an Agent pane's opaque ACP session ID and optional absolute cwd (`-t`, `-c`). |
| `set-agent-provider` | . | *zz-native/internal:* persist `codex` or `claude-code` for an Agent pane, clear its provider-bound session ID, and restart its adapter (`-t`). |
| `restart-agent-pane` | . | *zz-native/internal:* replace an Agent pane's daemon-owned ACP adapter (`-t`). |
| `select-pane` | `selectp` | Select by target/direction (`-L/-R/-U/-D`), `-l` last, `-Z` keep zoom; `-T` updates only the pane title. A cross-window target changes that window's active pane without changing the session's current window. |
| `last-pane` | `lastp` | Return to the previously active pane (`-Z`). |
| `swap-pane` | `swapp` | Exchange two layout leaves (`-U/-D/-s/-t`, `-d` keep slot, `-Z`). |
| `list-panes` | `lsp` | List a window's panes. `-s` lists every window in the target session; `-a` lists every session in bytewise name order. Both keep window-index and pane order within each session. |
| `resize-pane` | `resizep` | Resize relatively (`-L/-R/-U/-D` by cells: a shared positional adjustment, tmux's attached form `-R10`, default `1`; adjustments are integers) or absolutely (`-x`/`-y` in cells or `N%`) or `-Z` toggle zoom. Absolute percentages accept 0 through 1000, then normal layout limits clamp the result. With no direction and no `-x`/`-y` the command is a no-op, as in tmux; unknown flags (`-M`, `-T`) are rejected. Sizes in cells need the geometry the daemon reports per pane; without it the command errors instead of guessing. |
| `select-layout` | `selectl` | Apply a named preset or checksummed layout string, restore with `-o`, spread with `-E`, or cycle with `-n`/`-p`. The first `-o` with no saved layout succeeds silently and saves the current layout for the next restore. A layout string ignores its pane numbers, assigns the current `pane_order` through the leaves, removes extra bottom-right cells, allocates new divider ids, and adopts the encoded window extent. Too few cells fail with tmux's `have N panes but need M` error. |
| `next-layout` / `previous-layout` | `nextl` / `prevl` | Cycle the seven presets. |
| `rotate-window` | `rotatew` | Rotate surfaces through layout slots (`-D`,`-U`,`-Z`). |
| `kill-pane` | `killp` | Remove a pane (removes the window if it was the last); `-a` keeps the target and kills every other pane in its window. |
| `respawn-pane` | `respawnp` | Restart a terminal pane in the same stable pane id and layout leaf. A live pane needs `-k`; otherwise the command reports tmux's `pane SESSION:WINDOW.PANE still active`. `-c` replaces the stored cwd, repeated `-e NAME=VALUE` entries overlay the session environment, and an omitted command/cwd reuses the prior spawn recipe. `-E` is rejected. |
| `respawn-window` | `respawnw` | Restart a terminal window in place. The first pane keeps its id, the other panes are removed, and the layout collapses to that retained leaf. Live panes need `-k`; `-c`, repeated `-e`, stored command/cwd reuse, and the `-E` rejection match `respawn-pane`. |
| `send-keys` | `send` | Send keys/text (`-l` literal, concatenating its arguments byte-for-byte like tmux; `-H` hexadecimal ASCII codes, `0x` prefix accepted — high bytes tmux would write raw are refused because `KeyToken::Literal` carries UTF-8) or `-X` copy-mode actions. `-N` is a repeat count: the whole key list is sent N times, and an `-X` action repeats only when its window-copy handler reads tmux's repeat prefix (movements, jumps, searches); everything else runs once. A bare `-N <n>` with no keys and no `-X` arms the client's copy-mode repeat prefix (tmux's `wme->prefix`), consumed by the next copy-mode command. Flags with no zz model (`-R` terminal reset, `-M`, `-K`, `-F`) are rejected rather than dropped. |
| `copy-mode` | . | Enter copy mode (`-u` page up, `-d` page down, combinable — tmux applies `-u` then `-d`). `-e` latches exit-at-bottom on fresh entry: scroll-down/page-down/halfpage-down landing at the live bottom with no selection leaves copy mode (cursor-down never does), and `-ed` at the bottom exits instantly. `-q` pops copy mode and returns (silent when no mode is active; other flags in the invocation are dead, as in tmux). `-M` is tmux's mouse-drag entry — without a mouse event tmux no-ops silently, and zz commands never carry one, so it emits nothing. `-k`/`-H`/`-S`/`-s` are rejected. |
| `copy-mode-search-prompt` | . | *zz-native:* open the native copy-mode search prompt (`-b` backward). |
| `command-prompt` | . | Open the native command prompt (`-p`,`-I`, `%%` template). `-b` is accepted and already true: the prompt never blocks its caller. |
| `focus-sidebar` | . | *zz-native:* show and focus the workspace sidebar (`-t`). |
| `choose-tree` | . | Open the pane chooser; `-s`/`-w` route to the sidebar (`-s`,`-w`,`-Z`,`-t`). `-Z` is accepted and already true: zz's chooser is a full-window overlay, so there is nothing left to zoom. |
| `choose-buffer` | . | Open the paste-buffer chooser (`-Z`,`-t`); `-Z` is accepted and already true, as for `choose-tree`. |
| `show-messages` | `showmsgs` | Print the daemon's message ring newest first with tmux's timestamps. The server-scoped `message-limit` bounds retention at insertion time and defaults to 1,000. Successful command-client invocations produce `command:` entries; failures produce one `message:` entry with the error. `display-message` without `-p` also adds an entry. |
| `display-message` | `display` | Expand a pane-scoped format. `-p` prints it; otherwise the daemon records it and sends a native toast whose timeout is the target session's effective `display-time`. Zero keeps the toast until manual dismissal. |
| `display-panes` | `displayp` | Pane-number overlay (`-d` duration). An omitted `-d` uses the target session's effective `display-time`; zero installs no deadline and input closes the overlay. `-b` is accepted and already true: the effect returns immediately, so nothing was ever blocked. |
| `clear-history` | `clearhist` | Clear a pane's scrollback (`-H` unsupported). |
| `bind-key` / `unbind-key` | `bind` / `unbind` | Add/remove key bindings (`-n`,`-r`,`-T`,`-N`). Empty `{}` installs an empty command list, and a single trailing escaped separator is ignored. Payloads validate at bind time (names + flags — tmux validates the full template): unknown names error `unknown command: X`, cataloged commands get their flags checked, daemon-side verbs are accepted unvalidated, and a real-but-unimplemented tmux command errors as unsupported so config import counts it. |
| `list-keys` | `lsk` | Print bindings as `bind-key` lines; `-T` limits the table. tmux's other selectors are rejected, not ignored: `-n` is not a tmux `list-keys` flag at all, and `-1`/`-N`/`-a`/`-O`/`-P`/`-r` plus the `[key]` positional have no zz form. |
| `list-commands` | `lscm` | Print `COMMAND_SPECS` in canonical-name order with tmux's `name (alias) usage` line shape. Each usage string lists the flags zz accepts, including daemon-parsed commands. `-F` formats rows and an optional command limits the result. The list excludes commands zz does not implement. |
| `set-option` / `set-window-option` | `set` / `setw` | Set typed options (see below) or exact free-form `@name` strings. User options support set, append, and unset at server, global-session, session, global-window, window, and pane scope. Indexed scalars return tmux's `not an array`; table-known arrays parse and take the documented empty-success omission path. |
| `show-options` | `show` | List options stored at the resolved scope or print one named option. `-v` prints only its raw value, `-A` includes an inherited named/table value with `*` after the name, `-q` suppresses unknown-name and target errors, and `-s`/`-g`/`-w`/`-p` select scope for `@` names. Implemented scalar and stored `@` values also read through valid `name[index]` spellings. Implemented string values use the pin's `args_escape` byte shape; `-H` adds no rows because zz has no hooks. A table-known unimplemented name or array prints nothing. Plain listings expose tmux and `@` names only; explicit-name queries still reach zz-native settings. |
| `show-window-options` | `showw` | Window-scoped spelling of `show-options` with the pin's `-g`, `-t`, and `-v` surface. A table-known option still routes by its declared scope, so spelling this command cannot turn a session option into a window option. |
| `set-environment` | `setenv` | Store a global or per-session environment overlay. `-F` expands the value as a format, `-h` hides it from children, `-r` records a child-unset marker, and `-u` deletes the stored entry. New terminal PTYs apply global then session entries over the daemon environment. |
| `show-environment` | `showenv` | List or read the exact global/per-session overlay. The daemon seeds the global map from its boot environment; `new-session` copies the fixed `update-environment` names and writes unset markers for missing names. Normal output is `NAME=value` or `-NAME`; `-s` emits shell-ready export/unset statements with tmux's escaping, `-h` selects hidden entries, and an absent exact name errors `unknown variable: NAME`. A hidden exact name without `-h` succeeds with empty output. |
| `source-file` | `source` | Load config files: every path is globbed like tmux (matches load in glob order; `conf.d/*.conf` works), then sourced in order. Without `-q` a path or glob that matches nothing warns; `-q` keeps it silent. `-` (stdin) is refused loudly — the daemon has no caller stdin. `-F`/`-n`/`-v`/`-t` are rejected. |
| `reload-config` | . | *zz-native:* reload tmux + Ghostty config (`ReloadConfig` effect, no args). |
| `start-server` | `start` | Ensure the daemon is running, then return success with no output. The CLI's normal connection path starts a missing daemon before the no-op reaches the engine. |
| `kill-server` | . | Stop the daemon (`KillServer` effect). |

Options handled by `set-option`/`set-window-option`: `synchronize-panes` (global→window→pane scope,
`-g/-w/-p/-u/-U/-o`), `buffer-limit` (global, default 50), `message-limit` (server, default 1000),
`history-limit` (session, default 10000, 0–1,000,000), `word-separators` (session, `-a` append), `mode-keys` (`vi`→`copy-mode-vi`,
`emacs`→`copy-mode`), `prefix`, `set-clipboard` (`on`/`external`/`off`), `copy-command`, `status`,
`status-interval`, `status-left`, `status-right`, `base-index`, `pane-base-index`, and
`renumber-windows`; `mouse` (session flag, default `off`), `escape-time` (server milliseconds,
default `10`), `automatic-rename` (window flag, default `on`), `automatic-rename-format` (window
string), `remain-on-exit` (window/pane choice: `off`, `on`, `failed`, or `key`),
`default-terminal` (server string, stored default `screen`), `display-time` (session milliseconds,
default `750`), and `repeat-time` (session milliseconds, default `500`, maximum `2000000`). The
matcher checks exact names and unique prefixes against all 180 tmux option names plus
68 hook entries. The matched table entry chooses server, session, window, or pane scope. `set` versus
`setw` and the `-s`/`-w`/`-p` spelling cannot change that declared scope. A table entry declared as
both window and pane lets `-p` select pane scope. `-q` silences unknown or ambiguous names; config
import reports a known unimplemented name as a skip. Names beginning with `@` bypass table matching:
they are exact string keys whose scope comes from the command flags, preserving the plugin-storage
contract without pretending the stored value has behavior.

`automatic-rename` gates the desktop's active-pane-derived tab label. Explicit window names pin a
window-local `off`; `automatic-rename-format` is stored and readable but the presentation-only
renamer does not evaluate it. Retained terminal exits keep the last viewport and expose
`pane_dead` plus normal-exit `pane_dead_status`; input is swallowed, `kill-pane` still removes the
pane, and the respawn commands replace its daemon-owned terminal session. An explicit
`default-terminal` seeds `TERM` for future spawns, with a per-spawn environment override winning;
the unset path preserves zz's existing `xterm-256color` export despite the stored `screen` default.
`repeat-time` supplies the attached session's repeatable-binding deadline, including zero to disable
the repeat window. `mouse` and `escape-time` are storage-only until the TUI attach client consumes
them.

For the index trio, `-u` and `-U` restore inheritance and ignore a trailing value, `-o` checks the
target override slot and yields to either unset flag, and the handler accepts `-a`. tmux flag values accept
`on`/`off`, `yes`/`no`, and `1`/`0`; `true` and `false` remain valid for zz-native boolean settings.
The six zz-native agent, editor, and history-trickle options keep their command and flag checks.
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

`MuxEffect` variants returned to the daemon adapter include: `PaneCreated`, `PaneRespawned`, `PanesRemoved`,
`PaneRelocated`, `SendKeys`, `TerminalView` (scroll/copy-mode), `TerminalUi` (search prompt),
`CommandPrompt`, `FocusSidebar`, `ChooseTree`, `ChooseBuffer`, `DisplayMessage`, `DisplayPanes`, `BufferLimitChanged`,
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
