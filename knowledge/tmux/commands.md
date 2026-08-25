---
type: Subsystem
title: tmux command set (command.rs)
description: "MuxEngine, the tmux-style command executor: canonical names + aliases, shared option/flag parsing, -t target resolution, and structured MuxEffect side effects for the daemon."
resource: crates/zz-mux/src/command.rs
tags: [tmux, commands, mux-engine, targets, effects]
timestamp: 2026-08-24T00:00:00-03:00
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
| `-t` on `new-window` / `new-browser` / `break-pane` | `resolve_window_index_target` | The same centralized grammar in tmux's target-window-with-index mode. A numeric or `+N`/`-N` destination may name an unused index; existing names, patterns, and tokens contribute their resolved index. A taken `new-window` index fails with `create window failed: index N in use` unless `-k` replaces it. With `break-pane -a` or `-b`, an unused numeric destination falls back to the destination session's current window before placement. |

# Schema: commands (name | aliases | purpose)

When `new-session` omits `-s`, `next_session_name` selects the first unused canonical decimal name
(`0`, `1`, …). It marks numeric names in a bounded `n + 1` occupancy bitmap and materializes only the
winning string; noncanonical lookalikes such as `00` do not reserve `0`.
Because a Command-spawned daemon starts empty, its first `new-session` receives name `0` and the
first session, window, and pane ids are `$0`, `@0`, and `%0`.

| Command | Aliases | Purpose |
| --- | --- | --- |
| `new-session` | `new` | Create a session (with its first window + terminal pane) and request attachment for an interactive caller; command-only callers remain detached. A targetless Command client starts from the most-recent session, so `-c '#{pane_current_path}'` expands against the same origin the pin selects. A first-window `-n` installs window-local `automatic-rename off`. Repeated `-e NAME=VALUE` entries overlay the normal `update-environment` seed, persist on the new session, and reach its first pane; later values win and entries without `=` are ignored. An `=VALUE` entry remains visible in the session environment as `=VALUE`, but is not exported to the child, matching the pin's split between `environ_put` and `environ_push`. On the creation path, `-E` skips the `update-environment` seed but still applies explicit `-e` values. `-A` attaches when the session already exists and ignores `-e` rather than mutating it; bare `-A` resolves the current session, `-D` detaches its other clients, and `-d` is ignored on that path. `-P`/`-F` print the created pane, and `-x`/`-y` size detached creation. `-X`/`-f` remain rejected; `-t` stays parked with session groups. Existing-session attach still lacks client-environment reseeding, so accepting `-E` closes only its creation-time half. |
| `list-sessions` | `ls` | List sessions with `-f` format filtering, `-O` sort order, and `-r` reversal. Filters run after sorting, and `#{line}` remains the pre-filter sorted index. Without `-O`, `-r` is a no-op like tmux. |
| `rename-session` | `rename` | Rename a session (rejects duplicate names). |
| `kill-session` | . | Remove a session and its windows/panes. Attached clients follow that session's `detach-on-destroy` policy: `off` selects the most recently active survivor, `on` exits, `no-detached` selects the newest unattached survivor, and `previous`/`next` walk session names with wrapping. `-a` keeps the target and kills every other session; `-f` filters those candidates in their session contexts. `-C` clears the session's pane bells and kills nothing, outranking `-a` as in tmux. Positional targets are refused — tmux's bound is zero arguments, and the kill commands are too destructive to guess. |
| `attach-session` | `attach` | Attach the client to a session (`Attach` effect); `-d` detaches the session's other clients and `-r` marks this client read-only. `-x`, `-E`, `-c`, and `-f` remain rejected. The nested-tty check applies here and to an attaching `new-session` before mux state changes. |
| `detach-client` | `detach` | Detach (`Detach(DetachScope)` effect). No flags detaches the caller, `-a` detaches every *other* attached client, `-s` detaches every client on that session (the caller included). `-t` (target client), `-P`, and `-E` remain rejected; the client-name resolver currently belongs only to `switch-client -c`. |
| `switch-client` | `switchc` | Retarget an attached Interactive or Control client. `-t` accepts session, window, pane-index, and `%pane` targets; `-n`/`-p` walk sessions with wrapping and accept `-O` sort order; `-l` returns to the live previous session; `-T` switches only the client's key table; `-r` toggles read-only state and reverses an explicit `-O` walk; and `-Z` preserves zoom around a pane switch. A normal switch resets the target client's table to its session root, while a switch executed by a `bind-key -r` binding keeps that table even when `-c` selects another client. `-c` selects a named client, `-E` is accepted without a client-environment model, and `-F` is accepted and ignored like the pin. Read-only input is enforced at the daemon input funnel; output, resize, detach, and the pin's read-only command roster remain available. |
| `list-clients` | `lsc` | List attached clients from the daemon registry. `-f` filters, `-O` sorts, `-r` reverses an explicit sort, `-t` restricts by session after the global sort, and `-F` expands client formats, including the live attachment, activity, previous session, read-only flags, active key table, and session last-attached time. Natural order is daemon insertion order; `#{line}` is the global pre-target/pre-filter index, so restricted output can contain gaps. zz reports unknown client dimensions as `0x0` and leaves the unavailable terminal name empty. Detached command connections do not appear. |
| `refresh-client` | `refresh` | `-A`/`-B`/`-C`/`-f`/`-F`/`-t` provide control-mode flow, subscriptions, and sizing. A detached command client gets `no current client`; bare redraw, `-S`, and the attached-client redraw/scroll family remain unsupported. |
| `new-window` | `neww` | Create a terminal window at the `-t` destination. `-d` creates without selecting, `-a` inserts after an occupied target, and `-b` inserts before it; `-b` wins when both are supplied, while an explicitly free index stays unchanged. `-k` replaces whatever holds the index. Repeated `-e NAME=VALUE` entries overlay only the new pane's child environment in order, so the last value wins; entries without `=` and empty names do not reach the child, and nothing is stored in the session environment. `-E` creates a live empty pane with no child process and accepts either no command or one empty-string argument; a nonempty command errors before creation. Without an explicit destination index, `-S` expands `-n` and selects the unique existing window with that name; duplicates error, while reuse suppresses creation output and the `after-new-window` hook. `-P` prints a created pane after spawn with the default `#{session_name}:#{window_index}.#{pane_index}` or an `-F` format, including runtime start path/command, PID, and TTY facts; `-F` without `-P` is silent. An explicit `-n` also installs the pin's window-local `automatic-rename off`. |
| `new-browser` | . | *zz-native:* create a browser window (`-p` profile, URL positional); shares `new-window`'s destination options. |
| `list-windows` | `lsw` | List windows with `-f` format filtering, `-O` sort order, and `-r` reversal. `-a` flattens all sessions into one globally sorted winlink vector; without it, only the target session is sorted. `#{line}` is tmux's total row count rather than the row index. |
| `rename-window` | `renamew` | Rename a window (duplicates allowed) and install a window-local `automatic-rename off`, so scripts observe the same explicit-name pin as tmux. |
| `select-window` | `selectw` | Activate a window. `-n`/`-p`/`-l` are the `next-window`/`previous-window`/`last-window` commands (`-n` wins over `-p` wins over `-l`, as in tmux); `-T` on a target that is already current behaves like `last-window`. |
| `next-window` / `previous-window` | `next` / `prev` | Step windows (wraps). A step that would land on the current window errors `no next window` / `no previous window`, tmux's strings; `-t` picks the session; `-a` steps to the next/previous window holding an alert (any pane with a latched bell) with the same error when none qualifies. |
| `kill-window` | `killw` | Remove a window; `-a` keeps the target and kills every other window in its session, with `-f` filtering those candidates in window context. |
| `move-window` | `movew` | Move a stable window to another index or session. `-a` and `-b` insert around an occupied target, `-d` avoids selecting the moved window unless `-k` replaces the current destination, `-k` replaces an occupied index, and standalone `-r` renumbers from `base-index`. An occupied destination without `-k` returns `index in use: N`. |
| `swap-window` | `swapw` | Exchange two window slots within one session or across sessions while window ids, panes, names, and layouts travel together. `-d` selects the destination slots after the swap. |
| `find-window` | `findw` | Accept tmux's `-CiNrTZ` search grammar and validate `-t`. Detached CLI calls return success with no output for both matches and zero matches; zz does not open a GUI chooser. |
| `split-picker` | . | *zz-native:* split into a runtime-free picker (`-h` horizontal, else vertical); accepts the `split-window` target/size/cwd options. By default the picker is revealed and selected, clearing an existing zoom; `-d` creates it in the background, keeps the current pane selected, and preserves zoom. Bound on zz's default `%`/`"` keys. Renamed from `new-pane` (the pinned tmux owns that name for floating panes). |
| `split-window` | `splitw` | Split a terminal pane and inherit the target pane's live cwd; key-bound, CLI, and command-prompt invocations all create a terminal, like tmux. Repeated `-e NAME=VALUE` entries overlay only the new pane's child environment, with entries lacking `=` and empty names omitted and the last named value winning. `-E` creates a live empty pane with no child process and accepts either no command or one empty-string argument; a nonempty command errors after target resolution and before layout mutation. The new pane's share comes from `-l` (cells, or `N%`) or legacy `-p N`; `-b` puts it left/above, `-f` spans the whole window, and `-d` leaves focus on the existing active pane. After a successful split, `-Z` zooms that post-spawn active pane: the new pane normally, or the existing pane under `-d`. Without `-Z`, every successful split clears an existing zoom, including detached splits. `-P`/`-F` use the same post-spawn output contract as `new-window`. |
| `split-browser` | . | *zz-native:* split into a browser pane (`-p` profile, URL positional). By default the browser is revealed and selected, clearing an existing zoom; `-d` creates it in the background, keeps the current pane selected, and preserves zoom. |
| `select-pane-kind` | . | *zz-native/internal:* materialize a pending picker as `terminal`, `browser`, `agent`, or `editor` (`-t` target; `-c` supplies an Agent cwd). |
| `break-pane` | `breakp` | Reparent a pane into a new one-pane window (`-n`,`-s`,`-t`,`-d`). `-a` inserts after the resolved destination window and `-b` inserts before it; `-b` wins when both are supplied, and an unused indexed `-t` falls back around the destination session's current window. Without placement flags, `-t` names the new window index. `-P` prints the moved pane using the default or an `-F` format. |
| `join-pane` / `move-pane` | `joinp` / `movep` | Insert a pane beside another (`-b`,`-f`,`-h/-v`,`-l`,`-s`,`-t`,`-d`; `join-pane` also accepts legacy `-p N`). `-l` accepts cells or `N%`, expands formats in the destination pane context, and uses the destination pane as its size basis unless `-f` selects the whole window. `-p` accepts a bare number from 0 through 100; a `%` suffix is invalid. The pin rejects `move-pane -p`. |
| `set-browser-url` | . | *zz-native:* update a browser pane's URL. |
| `set-browser-profile` | . | *zz-native:* validate and switch a browser pane's persistent zz profile (`-t`, one profile name). |
| `set-agent-session` | . | *zz-native/internal:* atomically persist an Agent pane's opaque ACP session ID and optional absolute cwd (`-t`, `-c`). |
| `set-agent-provider` | . | *zz-native/internal:* persist `codex` or `claude-code` for an Agent pane, clear its provider-bound session ID, and restart its adapter (`-t`). |
| `restart-agent-pane` | . | *zz-native/internal:* replace an Agent pane's daemon-owned ACP adapter (`-t`). |
| `select-pane` | `selectp` | Select by target/direction (`-L/-R/-U/-D`), `-l` last, `-Z` keep zoom; `-T` updates only the pane title. A cross-window target changes that window's active pane without changing the session's current window. |
| `last-pane` | `lastp` | Return to the previously active pane (`-Z` preserves zoom). `-d` disables input to that pane and `-e` enables it without selecting it; `-e` wins when both are present. With exactly two panes and no selection history, the other pane is the last pane. |
| `swap-pane` | `swapp` | Exchange two layout leaves (`-U/-D/-s/-t`, `-d` keep slot, `-Z`). |
| `list-panes` | `lsp` | List panes with `-f` format filtering, `-O` sort order, and `-r` reversal. `-s` lists every window in the target session; `-a` lists every session. Pane sorting remains per-window rather than flattening globally, and `#{line}` is the pane count for that window. |
| `resize-pane` | `resizep` | Resize relatively (`-L/-R/-U/-D` by cells: a shared positional adjustment, tmux's attached form `-R10`, default `1`; adjustments are integers) or absolutely (`-x`/`-y` in cells or `N%`) or `-Z` toggle zoom. Absolute percentages accept 0 through 1000, then normal layout limits clamp the result. With no direction and no `-x`/`-y` the command is a no-op, as in tmux; unknown flags (`-M`, `-T`) are rejected. Sizes in cells need the geometry the daemon reports per pane; without it the command errors instead of guessing. |
| `resize-window` | `resizew` | Set a durable manual window extent with `-x`/`-y`, or adjust it with `-L`/`-R`/`-U`/`-D` and an optional positive cell count. Absolute sizes accept 1 through 10,000; successful use selects a window-local `window-size manual`, preserves zoom, and makes `window_manual_width`/`window_manual_height` available. Client measurements no longer overwrite the layout while manual sizing is active. `-A`/`-a`, which derive the largest/smallest size from attached clients, remain rejected. |
| `select-layout` | `selectl` | Apply a named preset or checksummed layout string, restore with `-o`, spread with `-E`, or cycle with `-n`/`-p`. The first `-o` with no saved layout succeeds silently and saves the current layout for the next restore. A layout string ignores its pane numbers, assigns the current `pane_order` through the leaves, removes extra bottom-right cells, allocates new divider ids, and adopts the encoded window extent. Too few cells fail with tmux's `have N panes but need M` error. |
| `next-layout` / `previous-layout` | `nextl` / `prevl` | Cycle the seven presets. |
| `rotate-window` | `rotatew` | Rotate surfaces through layout slots (`-D`,`-U`,`-Z`). |
| `kill-pane` | `killp` | Remove a pane (removes the window if it was the last); `-a` keeps the target and kills every other pane in its window, with `-f` filtering those candidates in pane context. |
| `respawn-pane` | `respawnp` | Restart a terminal pane in the same stable pane id and layout leaf. A live pane needs `-k`; otherwise the command reports tmux's `pane SESSION:WINDOW.PANE still active`. `-E` starts from an empty environment, `-c` replaces the stored cwd, repeated `-e NAME=VALUE` entries overlay the session environment, and an omitted command/cwd reuses the prior spawn recipe. |
| `respawn-window` | `respawnw` | Restart a terminal window in place. The first pane keeps its id, the other panes are removed, and the layout collapses to that retained leaf. Live panes need `-k`; `-E`, `-c`, repeated `-e`, and stored command/cwd reuse match `respawn-pane`. |
| `send-keys` | `send` | Send keys/text (`-l` literal, concatenating its arguments byte-for-byte like tmux; `-H` hexadecimal ASCII codes, `0x` prefix accepted; high bytes tmux would write raw are refused because `KeyToken::Literal` carries UTF-8) or `-X` copy-mode actions. `-N` is a repeat count: the whole key list is sent N times, and an `-X` action repeats only when its window-copy handler reads tmux's repeat prefix (movements, jumps, searches); everything else runs once. A bare `-N <n>` with no keys and no `-X` arms the client's copy-mode repeat prefix, consumed by the next copy-mode command. `-F` is accepted as tmux's inert flag. The outer grammar rejects `-C`, `-P`, and `-o` with the pin's unknown-flag error. The copy-mode parser recognizes `-C` and `-P` after the action on the pin's 14 copy-family grammar entries, including a `-CP` cluster, and recognizes `-o` after `next-prompt` or `previous-prompt`. A local `--` ends flag parsing. Invalid local flags, actions, or arity run no copy action and reset the repeat prefix to 1. Four action handlers remain open under `terminal.key-control`: `copy-line`, `copy-line-and-cancel`, `copy-pipe-line`, and `copy-pipe-line-and-cancel`. The same tracker item owns the pin's first-line redraw after a local parser failure because zz has no no-op redraw effect. Flags with no zz model (`-R` terminal reset, `-M`, `-K`) are rejected rather than dropped. |
| `copy-mode` | . | Enter copy mode (`-u` page up, `-d` page down, combinable; tmux applies `-u` then `-d`). `-H` hides the native position indicator. `-e` latches exit-at-bottom on fresh entry: scroll-down/page-down/halfpage-down landing at the live bottom with no selection leaves copy mode, and `-ed` at the bottom exits instantly. `-q` pops copy mode and returns. `-M` is tmux's mouse-drag entry; without a mouse event it is a silent no-op. `-k`, `-S`, and `-s` are rejected. |
| `copy-mode-search-prompt` | . | *zz-native:* open the native copy-mode search prompt (`-b` backward). |
| `command-prompt` | . | Open the native command prompt (`-p`, `-I`, `%%` template). `-b` is accepted and already true: the prompt never blocks its caller. `-T command\|search` picks the history ring; the mode flags resolve in the pin's order `-1`, `-N`, `-i`, `-k`, `-e` with `-C` orthogonal. `-1` submits one key, `-k` submits that key's NAME, `-N` collects digits and lets the first non-digit both submit and reach the key tables, `-i` runs the template on every edit with an `=`/`-`/`+` prefix, `-e` exits on a backspace at an empty buffer, and `-C` keeps terminal frames flowing where a plain prompt freezes them. `-l`, `-F`, `-t` and `-P` are still rejected. |
| `show-prompt-history` / `clear-prompt-history` | `showphist` / `clearphist` | Show or clear the separate command and search prompt rings. `-T command` or `-T search` selects one ring; omitting it shows or clears both. Show output numbers entries oldest first with the pin's header and blank lines. Invalid types error, and clears rewrite the configured `history-file`. Runtime saves serialize record/clear races so stale history cannot reappear on disk. |
| `focus-sidebar` | . | *zz-native:* show and focus the workspace sidebar (`-t`). |
| `choose-tree` | . | Open the native hierarchy chooser: panes by default, windows with `-w`, sessions with `-s`. `-f` filters in pane context, `-O` sorts each hierarchy level, and `-r` reverses the default index order or the explicit sort. Zero matches restore the unfiltered tree and show `filter: no matches`. `-Z` is accepted and already true: the full-window overlay has nothing left to zoom. The default `C-b s`/`C-b w` bindings still call zz-native `focus-sidebar` directly. |
| `choose-buffer` | . | Open the paste-buffer chooser (`-Z`,`-t`,`-f`,`-O`,`-r`). It defaults to creation order newest first, preserves source-pane context for filters, and falls back to the unfiltered list with `filter: no matches` on zero matches. `-Z` is accepted and already true, as for `choose-tree`. |
| `show-messages` | `showmsgs` | Print the daemon's message ring newest first with tmux's timestamps. The server-scoped `message-limit` bounds retention at insertion time and defaults to 1,000. Successful command-client invocations produce `command:` entries; failures produce one `message:` entry with the error. `display-message` without `-p` also adds an entry. |
| `display-message` | `display` | Expand a pane-scoped format. With no message or `-F`, both ordinary and `-l` calls use the pin's full timestamp-bearing template; ordinary calls expand its time fields and `-l` preserves them literally. `-p` prints it; otherwise the daemon records it, publishes it to the requesting client, and owns its timer. `-d` overrides the duration in milliseconds (`delay invalid`/`too small`/`too large` on a bad value, `0..=4294967295`); without `-d` it is the target session's effective `display-time`. Zero installs no timer and the message waits for a key, exactly like the pin's `status_message_set` with `delay == 0`. While an ordinary message shows, the daemon freezes terminal publication for that client; PTY parsing stays live while patches stop. The clear publishes one full latest viewport before patches resume. `-C` is the pin's `no_freeze`: the message still shows and still times out, but frames keep flowing. |
| `display-panes` | `displayp` | Pane-number overlay (`-d` duration). `-t` targets an attached client by name, synthetic `device-N` id, full TTY path, one `/dev/`-stripped alias (including Linux `pts/N`), or TTY basename; every form accepts one trailing colon. The overlay uses that client's current window, and client lookup precedes `-d` validation. An omitted `-d` uses the target session's effective `display-panes-time`; zero installs no deadline. `-N` disables pane selection: the first key closes the overlay and continues through ordinary input handling. `-b` is accepted, but zz always returns immediately; unlike tmux, omitting it does not block the command queue until the overlay closes. |
| `clear-history` | `clearhist` | Clear a pane's scrollback (`-H` unsupported). |
| `bind-key` / `unbind-key` | `bind` / `unbind` | Add/remove key bindings (`-n`,`-r`,`-T`,`-N`). `unbind-key -a` removes the selected table (`-T` outranks `-n`), resets clients using it to their session's default table, and `-q` suppresses handler errors but not parser/arity errors, matching the pin. Empty `{}` installs an empty command list, and a single trailing escaped separator is ignored. Payloads validate at bind time (names + flags; tmux validates the full template): unknown names error `unknown command: X`, cataloged commands get their flags checked, and a real-but-unimplemented tmux command errors as unsupported so config import counts it. Daemon-native verbs use the same shared specs, including long-option validation and rendering. |
| `list-keys` | `lsk` | Print bindings as `bind-key` lines with tmux's global repeat, table, and key-column padding. `-T` errors after `unbind-key -a` removes `prefix`, `copy-mode`, or `copy-mode-vi`; `root` remains an implicit empty table because every command client uses it. `-N` prints the `prefix` table followed by `root`, sorts those tables independently, keeps bindings that carry a note, and falls back to the command text when a stored note is empty. `-a` with `-N` includes unnoted bindings; without `-N` it has no effect. `-T` selects one table before note filtering, and `-P` replaces the displayed prefix with a literal string. The optional `[key]` filters every selected table after options, with `--` ending option parsing; a valid absent or note-filtered key reports `unknown key`, while a malformed spelling reports `invalid key`. `-O` accepts tmux's case-insensitive sort names; `key` uses typed tmux key identity, `order` uses traversal order, repeated `-O` takes the last value, and `-r` reverses only an explicit sort. `-1` sorts and filters before selecting one row, then computes the repeat and width facts from that row. Command and Control clients receive the selected row on stdout; an attached Interactive client receives a frozen status message for the effective `display-time` without a command-output overlay. `-F` sees `notes_only`, `key_prefix`, and the per-row note/command plus post-filter repeat and width facts. `-n` is not a tmux `list-keys` flag. |
| `list-commands` | `lscm` | Print the 102 shared command specs in canonical-name order with tmux's `name (alias) usage` line shape: 83 executable tmux verbs and 19 zz-native verbs. Each usage string lists the flags zz accepts, including daemon-parsed commands. `-F` formats rows and an optional command limits the result. The list excludes tmux commands zz does not implement. |
| `set-option` / `set-window-option` | `set` / `setw` | Set typed options (see below) or exact free-form `@name` strings. The option name always expands as a format; `-F` additionally expands the value. `set` uses pane context and `setw` resolves a window target with its active pane for expansion. User options support set, append, and unset at server, global-session, session, global-window, window, and pane scope. Indexed scalars return tmux's `not an array`; table-known arrays parse and take the documented empty-success omission path. |
| `show-options` | `show` | List options stored at the resolved scope or print one named option. `-v` prints only raw values, `-A` includes inherited values with `*` after the name, `-q` suppresses unknown-name and target errors, and `-s`/`-g`/`-w`/`-p` select scope for `@` names. All 180 named options store and read back; arrays support numeric and named indices. String values use the pin's `args_escape` byte shape. On a no-name listing, `-H` retains the ordinary and `@` rows and adds hooks after the ordinary option table: none at server scope, 57 global-session hooks, 11 global-window hooks, or 7 pane hooks under `-p -A`. Empty arrays disappear under `-v`; an all-options listing prints `name` for a local empty and `name*` for an inherited empty. The pin's named-query path drops that star for an inherited empty. Populated inherited arrays carry `*` on each indexed row, and one local array shadows its parent as a unit. A named hook query works with or without `-H`. Plain listings omit hooks. Explicit-name queries still reach zz-native settings. |
| `show-window-options` | `showw` | Window-scoped spelling of `show-options` with the pin's `-g`, `-t`, and `-v` surface; `-H` is rejected. A table-known option still routes by its declared scope, so spelling this command cannot turn a session option into a window option. |
| `set-environment` | `setenv` | Store a global or per-session environment overlay. `-F` expands the value as a format, `-h` hides it from children, `-r` records a child-unset marker, and `-u` deletes the stored entry. New terminal PTYs apply global then session entries over the daemon environment. |
| `show-environment` | `showenv` | List or read the exact global/per-session overlay. The daemon seeds the global map from its boot environment; `new-session` copies the names in the stored `update-environment` array, writes unset markers for missing names, then applies explicit `-e` entries. Creation-time `-E` suppresses the array seed. Normal output is `NAME=value` or `-NAME`; `-s` emits shell-ready export/unset statements with tmux's escaping, `-h` selects hidden entries, and an absent exact name errors `unknown variable: NAME`. A hidden exact name without `-h` succeeds with empty output. |
| `source-file` | `source` | Load config files: `-F` expands every declared path in the command's current pane context before globbing, each path's matches load in glob order, and declared paths remain in caller order. Unix matching uses tmux's `glob(3)` contract: backslash escaping, leading-dot exclusion, nonrecursive repeated stars, and ordinary no-match handling for malformed patterns. Without `-q` a top-level or nested path that matches nothing warns with its post-`-F` declared argument; `-q` keeps a no-match silent and later paths still run. Nested glob errors retain that declared path. Command clients receive stderr and exit 1, and Interactive clients receive a warning. A direct all-miss Control invocation ends its own frame with `%error` and stops that input line; if at least one direct path matched, its missing-path diagnostics remain inside `%end` and the line continues. During config replay, zz currently synthesizes one standalone `%error` only when a nested source command produces recognized diagnostics, grouping that command's messages in argument order before continuing the outer chain. tmux instead emits one guard for every replayed command: a nested partial match ends `%end`, quiet and ordinary commands still get empty `%end` guards, and containing-command diagnostics precede deeper recursion. `control-mode.sourced-command-frames` tracks the missing guards; `source-file.nested-control-queue` tracks nested termination and ordering. Localized Unix errors or arbitrary non-Unix traversal errors can also miss the prose classifier until `control-mode.diagnostic-typing` replaces it with typed identity. Matched parser diagnostics remain `%config-error` and continue. `source-file` does not expand tildes again after parsing: parser-expanded leading tildes arrive as absolute paths, while literal tildes follow normal relative-path resolution. zz still resolves a nested relative path from the containing file where tmux repeats client-cwd selection. Counting the initial `source-file` as invocation 1, both sides run 50 concurrent source invocations and refuse invocation 51 with `too many nested files` before any of its paths are matched or loaded, so `-q` does not suppress it, a refused command emits one diagnostic rather than one per path, and the containing file keeps running its later physical lines. A malformed invocation at the refused depth is diagnosed as malformed rather than as depth on both sides, because the pin rejects it while parsing the containing file and never consults its depth guard; the two sides still print different malformed text, tracked under `mux.error-shapes`, and the pin then abandons the rest of the containing file where zz continues it, tracked under `config.parser-edge-cases`. Exact Control placement for that refusal is the pin's flags-1 `%begin`/`%error` guard on the rejected nested command, tracked with the rest of the sourced-replay framing under `control-mode.sourced-command-frames` and `source-file.nested-control-queue`; dropping a same-line `;` sibling on that sourced line is `config.same-line-error-group`; a replayed command that fails for an ordinary runtime reason is dropped instead of reported and leaves the containing `source-file` at rc 0, tracked under `config.replayed-command-errors`. Startup uses one 50-command budget across every root, with top-level roots excluded, quiet misses counted, and one command with many paths counted once. The daemon retains `<file>:<line>` in each refused startup cause, while `config.startup-diagnostic-delivery` tracks delivery and placement on Control and attached clients. `-` (stdin) is refused loudly because the daemon has no caller stdin. `-n`/`-v`/`-t` are rejected. |
| `reload-config` | . | *zz-native:* reload tmux + Ghostty config (`ReloadConfig` effect, no args). |
| `start-server` | `start` | Ensure the daemon is running, then return success with no output. The CLI's normal connection path starts a missing daemon before the no-op reaches the engine. |
| `kill-server` | . | Stop the daemon (`KillServer` effect). |

Options handled by `set-option`/`set-window-option`: `synchronize-panes` (global→window→pane scope,
`-g/-w/-p/-u/-U/-o`), `buffer-limit` (global, default 50), `message-limit` (server, default 1000),
`history-limit` (session, default 10000, 0–1,000,000), `word-separators` (session, `-a` append), `mode-keys` (`vi`→`copy-mode-vi`,
`emacs`→`copy-mode`), `prefix`, `set-clipboard` (`on`/`external`/`off`), `copy-command`, `status`,
`status-interval`, `status-left`, `status-right`, `base-index`, `pane-base-index`, and
`renumber-windows`; `mouse` (session flag, default `on`), `escape-time` (server milliseconds,
default `10`), `automatic-rename` (window flag, default `on`), `automatic-rename-format` (window
string), `remain-on-exit` (window/pane choice: `off`, `on`, `failed`, or `key`),
`default-terminal` (server string, default `tmux-256color`), `display-time` (session milliseconds,
default `750`), `repeat-time` (session milliseconds, default `500`, maximum `2000000`), and
`aggressive-resize` (global-window/window flag, default `off`). The
matcher checks exact names and unique prefixes against all 180 tmux option names plus
68 hook entries. The matched table entry chooses server, session, window, or pane scope. `set` versus
`setw` and the `-s`/`-w`/`-p` spelling cannot change that declared scope. A table entry declared as
both window and pane lets `-p` select pane scope. `-q` silences unknown or ambiguous names; config
import reports a known unimplemented name as a skip. Names beginning with `@` bypass table matching:
they are exact string keys whose scope comes from the command flags, preserving the plugin-storage
contract without pretending the stored value has behavior.

`automatic-rename` gates runtime-driven window names. Explicit window names pin a window-local
`off`; `automatic-rename-format` expands against the active pane when its runtime command changes.
Retained terminal exits keep the last viewport and expose `pane_dead`, normal-exit
`pane_dead_status`, `pane_dead_time`, and terminating `pane_dead_signal`; input is swallowed, `kill-pane` still removes the
pane, and the respawn commands replace its daemon-owned terminal session. An explicit
`default-terminal` seeds `TERM` for future spawns, with a per-spawn environment override winning;
the unset path restores `tmux-256color`.
`repeat-time` supplies the attached session's repeatable-binding deadline, including zero to disable
the repeat window. With `aggressive-resize` ON, each window takes the componentwise smallest rows
and columns among clients actually viewing it; changing client focus, attachment, or the option
recomputes the affected windows through the normal guarded measurement write-back. Cell-pixel
dimensions come from the latest-input eligible viewer. OFF retains that latest-input owner's whole
geometry. `mouse` and `escape-time` are published and consumed by the TUI; the daemon also gates
terminal-surface mouse input when the option is off.

For the index trio, `-u` and `-U` restore inheritance and ignore a trailing value, `-o` checks the
target override slot and yields to either unset flag, and the handler accepts `-a`. tmux flag values accept
`on`/`off`, `yes`/`no`, and `1`/`0`; `true` and `false` remain valid for zz-native boolean settings.
The six zz-native agent, editor, and history-trickle options keep their command and flag checks.
Buffer commands (`capture-pane`, `*-buffer`, `paste-buffer`) are handled by
[the server](/crates/zz-daemon.md), **not** here. `list-buffers` supports `-f`, `-O`,
and `-r` with newest-first natural/default creation order; unlike the other list commands,
it does not install a `#{line}` format variable. `set-buffer -t` and `load-buffer -t` accept and
ignore the compatibility target client because it only matters to tmux's `-w` clipboard path;
`-w` remains rejected rather than pretending to update a client clipboard. `set-buffer -n` renames
the explicit `-b` source or the newest automatic buffer, replaces an existing destination, and
ignores its optional data argument on the rename path.

# Daemon-side workspace verbs

Five zz-native verbs are handled by [the daemon](/crates/zz-daemon.md) before the engine sees them,
for the same reason `capture-pane` is: each acts on something `MuxState` does not own. They now have
shared `catalog.rs` specs, so `list-commands`, stored-command validation and rendering, and
[command-palette](/concepts/command-palette.md) completion discover them like the other 14 native
verbs. Exact native names resolve before abbreviation lookup. A native abbreviation resolves only
when no tmux canonical name starts with it, which keeps `capture-b` native while `capture` resolves
to tmux's `capture-pane`. Execution remains daemon-owned.

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
