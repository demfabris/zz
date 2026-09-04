---
type: Subsystem
title: Key tables (key.rs)
description: Root/prefix/copy-mode/chooser key resolution with the default C-b prefix and optional prefix2, canonical and shifted key encoding, bind/unbind, send-prefix (-2), numeric vi counts, pending jump-key capture, and wire publication of every table.
resource: crates/zz-protocol/src/key.rs
tags: [tmux, keys, bindings, prefix, copy-mode, choosers]
timestamp: 2026-08-25T00:00:00Z
---

# Overview

`key.rs` owns tmux-compatible key binding storage and the per-client keypress state machine. Since
2026-08-14 it lives in `zz-protocol` (the contract crate) so every client links the same data model
and resolver; `zz-mux` re-exports it and the daemon remains the runtime authority. Two
types cooperate: `KeyTables` is the shared, mutable set of named tables (`prefix`, `root`,
`copy-mode`, `copy-mode-vi`, `choose-tree`, `choose-buffer`, plus any custom `-T` table) mapping a
canonical key string to a
`Binding`; `KeyEngine` is the cheap per-client cursor that tracks whether the prefix has been pressed,
whether a "jump" binding is waiting for its target key, a repeat deadline, and a copy-mode numeric prefix. `KeyEngine::handle` returns a
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
| `KeyTables` | `{ prefix: String, prefix2: Option<String>, tables: BTreeMap<String, BTreeMap<String, Binding>> }` | Named tables; default `prefix` is `"C-b"`, `prefix2` defaults unset. |
| `KeyEngine` | `{ table, pending, repeat_count, repeat_deadline, prefix_deadline, last_repeat_key }` | Per-client mode: `None` = the effective root table, `Some("prefix")` after prefix, `pending` = awaiting a jump target key, `repeat_count` = buffered vi digits, and the deadline/key fields implement prefix and repeat timing. |
| `KeyDecision` | `Pass` \| `Prefix` \| `Ignore` \| `Commands(Vec<CommandInvocation>)` | Result of one keypress. |

# Root vs prefix tables and default prefix

The default prefix is **`C-b`** (settable via `set-option prefix …` or `set_prefix`, which runs the
key through `canonical_key`). An optional second prefix arms the same table: `set-option -g
prefix2 <key>` stores the scalar and syncs `KeyTables::set_prefix2` (default `None` = unset; a
literal `None` value also reads as unset). Resolution in `KeyEngine::handle`:

- In root mode (`table == None`), a key equal to either prefix (`KeyTables::is_prefix`) returns
  `Prefix` and switches to the `prefix` table. Otherwise the attached session's effective
  `key-table` is consulted (`root` by default); an unbound effective-root key returns `Pass` and
  goes to the routed pane sinks.
- In the `prefix` table, a bound key runs its commands; a **non-repeat** binding then drops back to
  the effective root, while a **repeat** (`-r`) binding stays in `prefix` so e.g.
  `C-b M-Left M-Left` keeps resizing. `prefix-timeout` bounds the first prefix lookup.
  `initial-repeat-time` bounds the first or newly changed repeat key, and `repeat-time` bounds
  same-key continuation. The next key after expiry resolves from the effective root. Zero
  `prefix-timeout` disables prefix expiry, zero `repeat-time` disables repeat mode, and zero
  `initial-repeat-time` falls back to `repeat-time`. An unbound prefix key is **discarded**
  (`Ignore`) and exits prefix mode, matching tmux:
  a mistyped sequence never types into the pane.
- Persistent tables (`copy-mode`, `copy-mode-vi`, or any table set via `switch_table`) consume unbound
  keys as `Ignore` instead of exiting, matching tmux copy-mode behavior. `Any` is honored as a
  catch-all fallback key within a table.
- `KeyEngine::handle_synthetic_any_with_repeat_metadata` resolves non-key ingress through `Any` in
  the effective explicit or root table. `handle_transient_mode_synthetic_any` checks a transient mode
  table first and then the effective root. Both helpers preserve pending jump capture, and the first
  helper retains ordinary repeat deadlines and table retirement. Exact `FocusIn` and `FocusOut` names
  remain invalid key spellings and never become direct lookup keys. With `focus-events` enabled, the
  daemon routes writable `ClientFocus` through the modal prequeue, activity and FocusIn-only latest
  geometry, then synthetic `Any`. It checks choose-tree, choose-buffer, active copy or command-output
  mode, and the effective root in that order. A transient `Any` wins; an unbound transient table
  falls back to root without retiring the mode. Read-only focus resolves and authorizes the complete
  binding before any effect. Disabled focus bypasses both accounting and dispatch. The focused daemon
  cluster passes 9 of 9 tests, and an independent Codex review returned CODE GO. `ClientFocus` is not
  CLI-drivable, so this closure makes no differential or canonical-suite claim.
- Jump bindings (`send-keys -X jump-forward|jump-backward|jump-to-forward|jump-to-backward`) set
  `pending` and return `Ignore`; the next key is appended as the jump target (or `Escape` cancels
  with `Ignore`).
- In `copy-mode-vi`, `1` through `9` begin a numeric prefix, and in `copy-mode`, `M-1` through
  `M-9` begin the same one. More digits, including `0`, extend it in either table;
  the next copy action carries that count once as `send-keys -N <count> -X ...`. The typed count
  policy repeats prefix-consuming movements, jumps, matching brackets, and repeat-search actions;
  `other-end` swaps only for odd counts, `select-line` spans the requested lines, and the
  copy-end-of-line family selects through the end of the Nth row before copying once. Other
  toggles, selection, copy, clear-selection, cancel, and later actions in the same binding run once.
  Resolution stops at the first `send` or `send-keys` command whose option prefix contains `-X`.
  That command's stored `-N` wins over the buffered count; otherwise the engine inserts a separate
  `-N <count>` pair immediately before the option argument containing `-X`. It never scans onward to
  a later action. A command list with no qualifying `-X` leaves the count armed for a later binding.
  Invalid nonempty `-X` grammar restores the neutral count of one, represented as no buffered count,
  so a following digit begins a fresh prefix. An unbound key preserves the prefix. `Escape` reaches
  its stock `clear-selection` binding once. Without a prefix, `0` keeps its normal `start-of-line`
  meaning. Empty `send-keys -N <n> -X` pane-prefix persistence remains tracked under
  `terminal.key-control`.

# Key encoding (`canonical_key`)

Native client keys are normalized before lookup and prefix comparison:

| Input | Canonical form |
| --- | --- |
| `" "` or `"Space"` | `" "` (a literal space) |
| `Ctrl-x` | `C-x` |
| `Alt-x` | `M-x` |
| otherwise | trimmed as-is (e.g. `C-b`, `M-Right`, `F2`, `PPage`) |

The tmux command layer now validates key arguments before they reach the shared tables. It accepts
case-insensitive short modifiers, caret forms, the pinned named-key aliases, exactly `F1` through
`F12`, and the pin's prefix-consuming 32-bit `User` and `0x` number grammar. It rejects long
`Ctrl-` and `Alt-` aliases before changing bindings or options; native clients may continue to use
those aliases through the shared native parser.

Caret followed by a literal control byte from 1 through 31 works across bind, list, filter, unbind,
`prefix`, `prefix2`, and `backspace`. Printable ASCII hex keys keep identity separate from their
literal forms while displaying the same character, so `A` and `0x41`, or `Space` and `0x20`, may
coexist and be filtered or removed independently. Raw DEL, caret plus DEL, and textual `0x7f` also
retain distinct identities. They render as `C-?`, `C-C-?`, and a raw DEL byte respectively, and can
coexist, be filtered, and be removed independently. Raw and textual-hex DEL send one literal DEL
through `send-prefix` and configured `BSpace`; the caret-modified value sends nothing, matching the
pinned tmux behavior.

# bind / unbind semantics

`bind-key` inserts into a table (`prefix` by default; `-n` → `root`; `-T <table>` → named), carrying
`-r` (repeat) and `-N` (note). With no command, it ensures the selected table and mutates only an
existing binding: `-N` replaces its note, `-r` sets repeat without clearing it later, and commands
plus unspecified metadata remain unchanged. An absent key is a silent no-op. A later command-bearing
bind replaces the complete binding normally. `unbind-key` removes a key from a table (`prefix` default, `-n`/`-T`
selectors); `-a` removes the selected table and `-q` suppresses handler errors while preserving
parser and arity errors. Removing a table resets every client using it to its session's configured
default table. If that default is the removed table, the daemon recreates it empty. Bare
`list-keys` renders every binding as
`bind-key -T <table> <key> <commands>`. Its `-N` view selects `prefix` then `root`, filters on
stored notes unless `-a` is present, and accepts `-P` as the displayed prefix string. See
[the command layer](/tmux/commands.md) for flag parsing.

Key-table storage folds `Space` and `C-Space` to literal-space bases. `list-keys` maps those bases
back to `Space` and `C-Space`, computes widths from the displayed spelling, and matches a positional
key by tmux base, type, and modifier identity. Stored spelling and key flags do not affect that
filter.

The daemon preserves stored commands except for one tmux-compatible copy-mode operation. An armed vi
count is consumed by the first `send` or `send-keys` command whose option prefix contains `-X`.
That command keeps its own `-N`; otherwise the engine inserts separate `-N <count>` arguments before
the argument containing `-X`. Other commands and later actions stay unchanged. A binding of
`split-window` (or `splitw`), whether imported from a
tmux config or typed at `prefix :`, creates a plain terminal split like tmux. zz's *default* `%`/`"`
bindings name the zz-native `split-picker` verb instead, which opens the pane-kind picker.

# Default bindings (seeded in `KeyTables::default`)

The pinned 303 defaults and zz's 268 defaults contain 210 shared keys. The live manifest owns 93
missing keys, 58 zz-native keys, and 45 shared command-or-repeat divergences. The remaining 165
shared entries match structurally: 61 in `copy-mode`, 72 in `copy-mode-vi`, and 32 in `prefix`.
That structural equality does not claim complete command or action behavior; the existing consumer
groups retain those runtime contracts.

Prefix table (partial, the canonical zz set):

| Key | Command | Key | Command |
| --- | --- | --- | --- |
| `c` | `new-window` | `%` | `split-picker -h` |
| `"` | `split-picker -v` | `!` | `break-pane` |
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

## Default-prefix compatibility boundary

The default zz prefix table is intentionally not a copy of the pin. zz has 60 default prefix
bindings, the pin has 92, and 59 keys overlap. zz adds `e -> send-last-output`. It omits these 33
stock keys:

`#`, `'`, `(`, `)`, `*`, `-`, `.`, `/`, `<`, `>`, `@`, `BTab`, `C`, `C-z`, `D`, `DC`, `L`,
`M`, `M-n`, `M-p`, `PPage`, `S-Down`, `S-Left`, `S-Right`, `S-Up`, `Tab`, `d`, `f`, `g`, `i`,
`m`, `t`, and `~`.

Several shared keys also name different commands:

| Keys | Pinned tmux | zz default |
| --- | --- | --- |
| `%` | `split-window -h` | `split-picker -h` |
| `"` | `split-window` (vertical by default) | `split-picker -v` |
| `&`, `x` | `confirm-before` around kill | immediate kill |
| `]` | `paste-buffer -p` | `paste-buffer` |
| `?` | `list-keys -N` | `list-keys` |
| `r` | `refresh-client` | `reload-config` |
| `s`, `w` | `choose-tree` | `focus-sidebar` |
| `M-Up`, `M-Left`, `C-Up`, `C-Left` | floating-aware `if-shell` resize | direct tiled-pane `resize-pane` |

The numeric `0` through `9` bindings select the same windows on both sides, but their stored command
text differs: zz uses `select-window -t :N`, while the pin uses `select-window -t :=N`.

This is a product choice, not permission to reinterpret tmux syntax. A user's imported binding that
names `split-window`, `choose-tree`, or `refresh-client` keeps the tmux command. The practical alias
target promises command/config semantics and documents the default-key delta; it does not erase the
picker and sidebar behavior that make the native GUI useful.

The prefix key itself is bound to **`send-prefix`** in the prefix table, matching tmux's stock
`bind C-b send-prefix`, so `<prefix> <prefix>` delivers one literal prefix keystroke to the pane.
`set-option prefix` carries that binding to the new key unless the user has rebound it.
`prefix2` carries no stock binding and `set_prefix2` never touches the tables, matching the pin's
default bindings; `send-prefix -2` sends the second prefix, and is a silent success while
`prefix2` is unset (the pin injects `KEYC_NONE`, which writes nothing).

Browser page input is the root-table exception. The desktop Browser sends ordinary page keys and
committed text through the protocol's Browser-surface variants, which go directly to the
synchronized pane sinks; `bind -n` and root `Any` do not consume them. The workspace captures only
the configured prefix chord and the armed sequence as key-table input, so prefix bindings retain
their normal behavior from a Browser pane. Terminal input continues to resolve the root table.

`copy-mode` and `copy-mode-vi` seed the native movement, selection, search, and copy actions. The vi
table includes `B/E/W`, `J/K`, `C-e/C-y`, `z`, `%`, `D`, `#/*`, `1` through `9`, `:`, and the stock
control/named-key aliases. Search keys `/`,`?` (vi) and `C-s`,`C-r` (emacs) carry the pin's
`command-prompt -P` shapes, the emacs pair incremental with `-I "#{pane_search_string}"`. Stock vi
Escape clears the selection; `q` and `C-c` cancel.

The emacs table includes the pin's direct navigation aliases for scroll lines, prompt marks,
matching brackets, line and history endpoints, half pages, top-line positioning, and page-down. The
13 keys are `C-Down`, `C-M-Down`, `C-M-Up`, `C-M-f`, `C-Up`, `End`, `Home`, `M-<`, `M->`,
`M-Down`, `M-R`, `M-Up`, and `Space`. Each key stores the pin's `send-keys -X` command and carries
`repeat = false`.

The emacs table also includes the pin's supported cancel, copy, rectangle, and repeat-search aliases:
`C-[`, `C-k`, `C-w`, `N`, `R`, and `n`. Their stored commands and nonrepeat metadata match the pin.
The table retains the existing `Escape`, `M-w`, and `C-g` bindings. Exact equality with the audited
stock key set guards against accidental overwrite, and shifted Alt input tests reach `M-R`, `M-<`,
and `M->`.

Five of those stock action keys are now installed: `copy-mode-vi P` and `copy-mode P` send
`toggle-position`, `copy-mode C-M-b` sends `previous-matching-bracket`, `copy-mode C-l` sends
`recentre-top-bottom`, and `copy-mode M-l` sends `cursor-centre-horizontal`, each with the pin's
stored command and nonrepeat metadata.

No stock action key is left open. The emacs table binds `M-1` through `M-9` to zz's own numeric
capture and `g` to the goto-line prompt vi `:` opens, and both `r` keys now send `refresh-toggle`,
which unfreezes the copy backing rather than switching off a live pane: measured on the pin,
`#{scroll_position}` holds at 0 after entry, climbs while `refresh-on` re-clones the backing on
its 50ms timer, and freezes again after `refresh-toggle`. That re-sync rides the daemon's copy
revisions, and `keys.copy-mode-unsupported-default-actions` closed on 2026-09-02. The action
vocabulary sat under `copy-mode.action-fidelity`, whose source audit classifies all 95 pinned
actions across seven behavior categories; that group closed on 2026-09-04, leaving
`scroll-to-mouse` as the one unmapped name, accepted because zz hands the daemon no scrollbar
slider position. Pointer pseudo-bindings stay in the direct mouse route.

Copy-mode movement, jump capture, and numeric repetition do not read the binding repeat field;
`copy-mode-repeat`, `repeat_count`, and the copy action's runtime repeat policy own them. The nine vi
digit bindings and the nine emacs `M-1` through `M-9` bindings retain zz's per-client
`copy-mode-repeat` command shape instead of tmux's pane-cell numeric prompt, and a bare digit typed
after either extends the count in either table. Prefix-table and user-created `bind-key -r` bindings
still carry and use their repeat bit. The 16 cursor-word, search, goto-line and jump bindings that
were the last divergent shared shapes now store the pin's exact commands. Fourteen of them (vi `/`,
`?`, `:`, `f`, `F`, `t`, `T` and emacs `g`, `C-r`, `C-s`, `f`, `F`, `t`, `T`) render through
`command-prompt -P`; the pin draws that prompt over the target pane's last row at redraw time, and zz
takes the flag as a presentation hint its own client surface honours, so the stored command matches
and only the drawing differs. The remaining two, vi `#` and `*`, carry
`send-keys -FX search-backward -- "#{copy_cursor_word}"` and its forward twin. `#{pane_search_string}`
in the emacs `-I` value still expands empty in zz, so `C-s` and `C-r` open with an empty input rather
than the last search.
See [copy mode](/tmux/copy-mode.md).

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
| `crates/zz-protocol/src/key.rs` | `KeyTables`, `KeyEngine` (`handle`, synthetic `Any`, and transient-mode `Any`), `Binding`, `KeyDecision`, `canonical_key`, `input_key_name`, typed-text precedence, and default bindings. |
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
