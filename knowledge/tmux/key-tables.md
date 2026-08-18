---
type: Subsystem
title: Key tables (key.rs)
description: Root/prefix/copy-mode/chooser key resolution with the default C-b prefix, canonical and shifted key encoding, bind/unbind, send-prefix, numeric vi counts, pending jump-key capture, and wire publication of every table.
resource: crates/zz-protocol/src/key.rs
tags: [tmux, keys, bindings, prefix, copy-mode, choosers]
timestamp: 2026-08-14T00:00:00Z
---

# Overview

`key.rs` owns tmux-compatible key binding storage and the per-client keypress state machine. Since
2026-08-14 it lives in `zz-protocol` (the contract crate) so every client links the same data model
and resolver; `zz-mux` re-exports it and the daemon remains the runtime authority. Two
types cooperate: `KeyTables` is the shared, mutable set of named tables (`prefix`, `root`,
`copy-mode`, `copy-mode-vi`, `choose-tree`, `choose-buffer`, plus any custom `-T` table) mapping a
canonical key string to a
`Binding`; `KeyEngine` is the cheap per-client cursor that tracks whether the prefix has been pressed,
whether a "jump" binding is waiting for its target key, a repeat deadline, and a vi numeric prefix. `KeyEngine::handle` returns a
`KeyDecision` telling the daemon to pass the key through to the surface, enter the prefix, ignore it,
or run a list of `CommandInvocation`s. Defaults are audited against the pinned tmux tables
(`key-bindings.c`); custom binds come from `bind-key`/`unbind-key` in
[the command layer](/tmux/commands.md) and from [`.tmux.conf`](/tmux/conf-parser.md).

The daemon owns **one** cursor per client over these live tables; it carries both the one-shot
prefix state and the client's active copy-mode table. `KeyTables::snapshot()` flattens every table
(command names canonicalized) for `ServerHello.key_tables` and `EventPayload::KeyTablesChanged`, so
clients label hints and render binding help from published truth instead of hardcoded guesses.

The `choose-tree` and `choose-buffer` tables resolve daemon-side too: choosers forward raw key
presses (`ChooseTreeAction::Key` / `ChooseBufferAction::Key`) and the daemon maps them through
`send-keys -X`-style bindings (`cursor-up`, `accept`, `cancel`, `search-forward`, `paste`,
`delete`, …) in `crates/zz-daemon/src/keys.rs` (`choose_tree_key_action`), preferring the typed
character (`?` from shift+`/`) over the folded physical key name. Search-mode editing keys
(Escape/Enter/BSpace/arrows, printable text append) stay fixed, like other search prompts. Chooser
vim navigation is therefore rebindable with `bind-key -T choose-tree …` and identical in the GPUI
app and the TUI, which contain no chooser key maps at all.

# Data model

| Type | Shape | Purpose |
| --- | --- | --- |
| `Binding` | `{ commands: Vec<CommandInvocation>, repeat: bool, note: Option<String> }` | What a key runs; `repeat` keeps the prefix table active (tmux `-r`); `note` is a `-N` description. |
| `KeyTables` | `{ prefix: String, tables: BTreeMap<String, BTreeMap<String, Binding>> }` | Named tables; default `prefix` is `"C-b"`. |
| `KeyEngine` | `{ table, pending, repeat_count, repeat_deadline }` | Per-client mode: `None` = root, `Some("prefix")` after prefix, `pending` = awaiting a jump target key, `repeat_count` = buffered vi digits, and `repeat_deadline` bounds a repeatable binding sequence. |
| `KeyDecision` | `Pass` \| `Prefix` \| `Ignore` \| `Commands(Vec<CommandInvocation>)` | Result of one keypress. |

# Root vs prefix tables and default prefix

The default prefix is **`C-b`** (settable via `set-option prefix …` or `set_prefix`, which runs the
key through `canonical_key`). Resolution in `KeyEngine::handle`:

- In root mode (`table == None`), a key equal to the prefix returns `Prefix` and switches to the
  `prefix` table. Otherwise the `root` table is consulted; an unbound root key returns `Pass`
  (goes to the routed pane sinks).
- In the `prefix` table, a bound key runs its commands; a **non-repeat** binding then drops back to
  root, while a **repeat** (`-r`) binding stays in `prefix` so e.g. `C-b M-Left M-Left` keeps
  resizing. The attached session's effective `repeat-time` sets and refreshes its millisecond
  deadline. The next key after expiry resolves from root, and zero disables the repeat window. An
  unbound prefix key is **discarded** (`Ignore`) and exits prefix mode, matching tmux:
  a mistyped sequence never types into the pane.
- Persistent tables (`copy-mode`, `copy-mode-vi`, or any table set via `switch_table`) consume unbound
  keys as `Ignore` instead of exiting, matching tmux copy-mode behavior. `Any` is honored as a
  catch-all fallback key within a table.
- Jump bindings (`send-keys -X jump-forward|jump-backward|jump-to-forward|jump-to-backward`) set
  `pending` and return `Ignore`; the next key is appended as the jump target (or `Escape` cancels
  with `Ignore`).
- In `copy-mode-vi`, `1` through `9` begin a numeric prefix. More digits, including `0`, extend it;
  the next `send-keys -X` motion is emitted that many times. Escape or an unbound key clears the
  prefix. Without a prefix, `0` keeps its normal `start-of-line` meaning.

# Key encoding (`canonical_key`)

Every key is normalized before lookup, bind, unbind, and prefix comparison:

| Input | Canonical form |
| --- | --- |
| `" "` or `"Space"` | `" "` (a literal space) |
| `Ctrl-x` | `C-x` |
| `Alt-x` | `M-x` |
| otherwise | trimmed as-is (e.g. `C-b`, `M-Right`, `F2`, `PPage`) |

`bind`/`unbind`/`get` all canonicalize their key, so `Ctrl-a` and `C-a` are the same binding.

# bind / unbind semantics

`bind-key` inserts into a table (`prefix` by default; `-n` → `root`; `-T <table>` → named), carrying
`-r` (repeat) and `-N` (note). `unbind-key` removes a key from a table (`prefix` default, `-n`/`-T`
selectors); `unbind-key -a` is explicitly unsupported. `list-keys` renders every binding as
`bind-key -T <table> <key> <commands>`. See [the command layer](/tmux/commands.md) for flag parsing.

The daemon preserves and executes these stored commands exactly — there is no key-time rewriting.
A binding of `split-window` (or `splitw`), whether imported from a tmux config or typed at
`prefix :`, creates a plain terminal split like tmux. zz's *default* `%`/`"` bindings name the
zz-native `split-picker` verb instead, which is what opens the pane-kind picker.

# Default bindings (seeded in `KeyTables::default`)

Prefix table (partial, the canonical zz set):

| Key | Command | Key | Command |
| --- | --- | --- | --- |
| `c` | `new-window` | `%` | `split-window -h` |
| `"` | `split-window -v` | `!` | `break-pane` |
| `x` | `kill-pane` | `&` | `kill-window` |
| `<prefix>` | `send-prefix` | | |
| `n` / `p` | next / previous window | `o` | `select-pane -t:.+` |
| `C-o` / `M-o` | `rotate-window` / `rotate-window -D` | `Space` | `next-layout` |
| `E` | `select-layout -E` | `M-1`…`M-7` | select the seven named layouts |
| `[` | `copy-mode` | `?` | `list-keys` |
| `=` | `choose-buffer -Z` | `s` / `w` | `focus-sidebar` |
| `q` | `display-panes` | `r` | `reload-config` |
| `e` | `send-last-output` *(zz-native)* | | |
| `z` | `resize-pane -Z` | `;` | `last-pane` |
| `{` / `}` | `swap-pane -U` / `-D` | `:` | `command-prompt` |
| `$` | `command-prompt -I #S 'rename-session -- %%'` | `,` | `command-prompt -I #W 'rename-window -- %%'` |
| `Up/Down/Left/Right` | `select-pane -U/-D/-L/-R` (repeat) | `M-Arrow` / `C-Arrow` | `resize-pane` by 5 / by 1 (repeat) |

The prefix key itself is bound to **`send-prefix`** in the prefix table, matching tmux's stock
`bind C-b send-prefix`, so `<prefix> <prefix>` delivers one literal prefix keystroke to the pane.
`set-option prefix` carries that binding to the new key unless the user has rebound it.

Browser page input is the root-table exception. The desktop Browser sends ordinary page keys and
committed text through the protocol's Browser-surface variants, which go directly to the
synchronized pane sinks; `bind -n` and root `Any` do not consume them. The workspace captures only
the configured prefix chord and the armed sequence as key-table input, so prefix bindings retain
their normal behavior from a Browser pane. Terminal input continues to resolve the root table.

`copy-mode` and `copy-mode-vi` seed the native movement, selection, search, and copy actions. The vi
table includes `B/E/W`, `J/K`, `C-e/C-y`, `z`, `%`, `D`, `#/*`, `1` through `9`, `:`, and the stock
control/named-key aliases; search keys `/`,`?` (vi) and `C-s`,`C-r` (emacs) bind
`copy-mode-search-prompt`. Stock vi Escape is `clear-selection`; `q` and `C-c` cancel. The two
keyboard exceptions are `P` (tmux's position-label toggle, redundant with zz's native indicator) and
`r` (tmux live-refresh toggle, incompatible with the frozen revision). Pointer pseudo-bindings stay
in the direct mouse route instead of this keyboard table. See [copy mode](/tmux/copy-mode.md).

# Shifted key spellings

GPUI resolves a shifted **symbol** before zz sees it: `shift+5` arrives as `%` with the shift bit
*cleared*, so `%`, `"`, `!`, `&`, `?`, `=`, `{`, `}`, `:` and `$` look up directly.

A shifted **letter** is the exception: it arrives as the lowercase key with the shift bit still set.
The shared fold in `crates/zz-protocol/src/key.rs` uppercases an ASCII lowercase character when shift
is held, which is what makes the uppercase bindings the copy-mode table is full of (`A`, `B`, `D`,
`E`, `F`, `G`, `H`, `J`, `K`, `L`, `M`, `N`, `T`, `V`, `W`, `X`) resolve instead of hitting their
lowercase twins. The fold reads
the shift bit rather than the keystroke's produced text, because a release carries no text and a
text-derived name would spell the same key differently on press and release.

With control or alt held nothing is uppercased: the binding is spelled off the base key (`C-a`,
`M-x`).

# Key files

| File | Role |
| --- | --- |
| `crates/zz-protocol/src/key.rs` | `KeyTables`, `KeyEngine` (`handle`), `Binding`, `KeyDecision`, `canonical_key`, `input_key_name`, typed-text precedence, and default bindings. |
| `crates/zz-mux/src/command.rs` | `bind-key`/`unbind-key`/`list-keys`, `send-prefix`, and `set-option prefix` drive these tables. |
| `crates/zz-daemon/src/keys.rs` | Daemon overlay-action projection and tmux `send-keys` token conversion; it consumes the shared fold and tables. |
| `crates/zz-client/src/chrome.rs` | Client-side `ui`, `sidebar`, `browser`, and `terminal` chrome tables, resolved before the skin applies an action. |
| `crates/zz/src/mux/prefix.rs` | The desktop window-root claim that forwards the configured prefix and the daemon-reported armed sequence from local widgets. |

# Related

- Bindings run [commands](/tmux/commands.md); copy-mode tables drive [copy mode](/tmux/copy-mode.md).
- Populated at load time by the [conf parser](/tmux/conf-parser.md).
- Audited against the pinned `key-bindings.c` tables; see [tmux compatibility](/tmux/tmux-compat.md) and the
  [tmux upstream reference](/references/tmux-upstream.md). The contract lives in
  [zz-protocol](/crates/zz-protocol.md) and the execution engine in [zz-mux](/crates/zz-mux.md).
