---
type: Subsystem
title: Key tables (key.rs)
description: Root/prefix/copy-mode key resolution with the default C-b prefix, canonical and shifted key encoding, bind/unbind, send-prefix, numeric vi counts, and pending jump-key capture.
resource: crates/zz-mux/src/key.rs
tags: [tmux, keys, bindings, prefix, copy-mode]
timestamp: 2026-08-10T00:00:00Z
---

# Overview

`key.rs` owns tmux-compatible key binding storage and the per-client keypress state machine. Two
types cooperate: `KeyTables` is the shared, mutable set of named tables (`prefix`, `root`,
`copy-mode`, `copy-mode-vi`, plus any custom `-T` table) mapping a canonical key string to a
`Binding`; `KeyEngine` is the cheap per-client cursor that tracks whether the prefix has been pressed
whether a "jump" binding is waiting for its target key, and a vi numeric prefix. `KeyEngine::handle` returns a
`KeyDecision` telling the daemon to pass the key through to the surface, enter the prefix, ignore it,
or run a list of `CommandInvocation`s. Defaults are audited against the pinned tmux tables
(`key-bindings.c`); custom binds come from `bind-key`/`unbind-key` in
[the command layer](/tmux/commands.md) and from [`.tmux.conf`](/tmux/conf-parser.md).

The daemon owns **one** cursor per client over these live tables; it carries both the one-shot
prefix state and the client's active copy-mode table.

# Data model

| Type | Shape | Purpose |
| --- | --- | --- |
| `Binding` | `{ commands: Vec<CommandInvocation>, repeat: bool, note: Option<String> }` | What a key runs; `repeat` keeps the prefix table active (tmux `-r`); `note` is a `-N` description. |
| `KeyTables` | `{ prefix: String, tables: BTreeMap<String, BTreeMap<String, Binding>> }` | Named tables; default `prefix` is `"C-b"`. |
| `KeyEngine` | `{ table, pending, repeat_count }` | Per-client mode: `None` = root, `Some("prefix")` after prefix, `pending` = awaiting a jump target key, `repeat_count` = buffered vi digits. |
| `KeyDecision` | `Pass` \| `Prefix` \| `Ignore` \| `Commands(Vec<CommandInvocation>)` | Result of one keypress. |

# Root vs prefix tables and default prefix

The default prefix is **`C-b`** (settable via `set-option prefix …` or `set_prefix`, which runs the
key through `canonical_key`). Resolution in `KeyEngine::handle`:

- In root mode (`table == None`), a key equal to the prefix returns `Prefix` and switches to the
  `prefix` table. Otherwise the `root` table is consulted; an unbound root key returns `Pass`
  (goes to the routed pane sinks).
- In the `prefix` table, a bound key runs its commands; a **non-repeat** binding then drops back to
  root, while a **repeat** (`-r`) binding stays in `prefix` so e.g. `C-b M-Left M-Left` keeps
  resizing. An unbound prefix key is **discarded** (`Ignore`) and exits prefix mode, matching tmux:
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

The daemon preserves these stored commands exactly. At key-execution time only, a binding whose
canonical command is `split-window` (or `splitw`) is routed to the native `new-pane` picker with the
same arguments. Consequently custom prefixes, prefix-table keys, root bindings, targets, axes, and
cwd forms remain authoritative; direct CLI/command-prompt `split-window` requests stay terminal-only.

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
`shifted_character` (`crates/zz-daemon/src/keys.rs`) uppercases an ASCII lowercase character when shift
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
| `crates/zz-mux/src/key.rs` | `KeyTables`, `KeyEngine` (`handle`), `Binding`, `KeyDecision`, `canonical_key`, default bindings. |
| `crates/zz-mux/src/command.rs` | `bind-key`/`unbind-key`/`list-keys`, `send-prefix`, and `set-option prefix` drive these tables. |
| `crates/zz-daemon/src/keys.rs` | `input_key_name` and the shifted-spelling fold used for every lookup. |
| `crates/zz/src/mux/prefix.rs` | The window-root claim; the only key resolution the client owns is recognizing the configured prefix (plus the daemon-reported armed window). |

# Related

- Bindings run [commands](/tmux/commands.md); copy-mode tables drive [copy mode](/tmux/copy-mode.md).
- Populated at load time by the [conf parser](/tmux/conf-parser.md).
- Audited against the pinned `key-bindings.c` tables; see [tmux compatibility](/tmux/tmux-compat.md) and the
  [tmux upstream reference](/references/tmux-upstream.md). Lives in [crates/zz-mux](/crates/zz-mux.md).
