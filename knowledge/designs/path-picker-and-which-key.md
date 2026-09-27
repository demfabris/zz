---
type: Design Plan
title: Shell path picker and which-key
description: Shipped plan for two desktop overlays over protocol v107 - a path picker anchored at the terminal cursor whose listing, git marks, and insert style come from the daemon that owns the pane while the client ranks and pastes, and a which-key sheet that follows the prefix and custom key tables through a new KeyTableActive event, with the stock notes brought to tmux parity.
status: Shipped
tags:
- picker
- which-key
- key-tables
- overlays
- protocol
- design-plan
timestamp: 2026-09-27T00:00:00Z
---

# Summary

Two features from the [terminal augmentation survey](../research/2026-09-25-terminal-augmentation-survey.md):

- **Path picker.** `prefix F` opens a list under the shell cursor, rooted at the pane's cwd. An empty
  query browses the root; typing fuzzy-matches the whole subtree. Enter inserts the path at the cursor,
  quoted for the shell in the foreground, or as a mention when Claude Code or Codex is in the
  foreground. It works for panes on ssh hosts, because the daemon that owns the pane lists the files.
- **Which-key.** When you pause after `C-b` (or inside a `switch-client -T` table), a sheet shows the
  keys you can press next, with their notes.

Both ride one protocol bump to v107. Desktop only in v1; the views live in zz-ui behind small backend
traits so the web and iOS clients can mount them later. The TUI comes in a later pass.

Mockups: the private Design canvas linked from the survey (boards "Path picker in a shell" and
"Which-key after prefix").

# Decisions

Taken with fabrico on 2026-09-27, after a code map and a 51-agent adversarial pass over the first draft.

| Question | Decision |
|---|---|
| Where the picker's work happens | The daemon lists (walk, git status, cwd, insert style); the client ranks, draws, and pastes. Typing never waits on ssh. |
| v1 clients | Desktop. `PathPickerView` and `WhichKeyView` live in zz-ui behind backend traits, like `PaletteBackend`. |
| Search model | Empty query lists the root's entries; typing searches the subtree; Tab re-roots into a dir; Backspace on an empty query goes up; a query starting with `~/`, `/` or `../` re-roots. |
| Insert form | Relative to the live cwd when under it, else absolute; quoted only when needed, per shell; `@path` for Claude, a bare or double-quoted path for Codex; Alt-Enter forces absolute. |
| Trigger | New verb `choose-path`, default `bind -N ... F choose-path`. `F` is unbound in stock tmux. |
| Which-key timing | After `which-key-delay` ms (default 400, 0 turns it off). |
| Which-key tables | Prefix and custom `-T` tables, through a new `KeyTableActive` event in the same v107 bump. |

Rejected: a daemon-ranked overlay (a round trip per keystroke over ssh, plus the whole overlay
plumbing) and a `display-popup` running a picker TUI (looks like a terminal box and lands only roughly
at the cursor on the desktop, whose popup grid is centred rather than pixel-aligned).

# A. Picker data path

## Protocol (v107)

All new variants go at the enum tails (postcard tags by position; `message.rs` comments at the action,
event payload, and message enums say so, and a tail test pins the order).

- `ClientHello` capability `client-path-picker-v1`, advertised only from the desktop's connect sites.
- `EventPayload::OpenPathPicker { pane, start_dir: Option<String> }`, pushed into `direct_events` for
  the invoking client only.
- `ProtocolMessage::PathListRequest { request_id, pane, dir: Option<String> }`. `dir` is the raw text:
  `None` means the pane's cwd; `~`, `~user`, relative (joined on the connection thread against the last root this client requested) and absolute forms are expanded
  and joined by the daemon, never by the client.
- `ProtocolMessage::PathListBegin { request_id, result }`, where `result` is either
  `{ root, display_root, cwd: Option<String>, insert: InsertStyle }` or an error message.
  `InsertStyle = Shell(ShellKind) | Claude | Codex`, `ShellKind = Posix | Fish | Pwsh | Nu`.
- `ProtocolMessage::PathListChunk { request_id, entries: Vec<PathEntry>, done, truncated }`,
  `PathEntry { rel, kind: File | Dir, symlink: bool }`, `rel` always `/`-separated.
- `ProtocolMessage::PathListGit { request_id, marks: Vec<(String, GitMark)> }`,
  `GitMark = Modified | Added | Untracked | Conflicted`. A rel ending in `/` marks everything under it.
  It only annotates entries already sent.
- `ProtocolMessage::PathListCancel { request_id }`.
- No insert message. The client builds the text and sends the existing `TerminalView { Paste }`.

Why no insert message: the Paste arm (`daemon.rs` around 18151-18171) already dismisses the client
message, applies the modal, copy-mode and read-only gates, brackets according to the live DECSET 2004
state, and strips control bytes through `libghostty_vt::paste::encode`. `send-keys -l` must not be used:
it writes raw bytes with no brackets and no filtering (`keys.rs` literal path, `session.rs` worker text
command). Synchronize-panes fans the paste out, the same as a typed or pasted path.

## choose-path

`choose-path [-t target-pane] [-c start-directory]`, modelled on `focus-sidebar`:

- Catalog: a `COMMAND_SPECS` entry, `NATIVE_COMMAND_NAMES` (sorted), and the executable-arm coverage set
  in `crates/zz-protocol/src/catalog.rs`. `-c` matches 8 of the 9 pinned start-directory flags.
- zz-mux: `MuxEffect::ChoosePath { pane, start_dir }`, parsed like `focus_sidebar` (reject positionals,
  resolve `-t`), `-c` format-expanded against the target pane like `split-window -c`.
- Daemon dispatch next to `FocusSidebar`, in this order: interactive subscriber check; capability check
  (error "choose-path needs the zz desktop app", before any overlay is dismissed); pane attached and in
  the session's current window; not read-only; target pane not in copy mode for this client; terminal
  pane. Then `dismiss_overlays`, `swallowed_keys.remove`, push `OpenPathPicker`.
- `path_picker_clients: BTreeSet<ClientId>` beside `native_chooser_clients`, filled at hello, removed at
  teardown.
- Default binding: `bind -N "Pick a path and insert it at the cursor" F choose-path`.

## Listing

Handled in the per-client connection loop, next to `HomeDirectoryRequest`, before the catch-all arm.
One active walk per client: a local `Option<(request_id, Arc<AtomicBool>)>`; a new request,
`PathListCancel`, or loop exit sets the flag. Clients that are read-only, lack
`client-path-picker-v1`, or are not attached to the pane's session get an error `Begin`.

**Root**, in order, each step checked with `is_dir()`:

1. `-c` start directory or the request's `dir`, expanded on the daemon (reuse the lookup inside
   `resolve_home_directories` for `~user`). When `dir` is given and is not a directory, the `Begin` is
   an error ("<dir> is not a directory") and the client's relative base goes back to what it was before
   that request, so a failed Tab, `..` or typed root never lands silently in the cwd. The steps below
   only run when `dir` is `None`.
2. The live `terminal_working_directory(terminal)` `PathBuf` (foreground process group leader's cwd).
3. The OSC 7 reported path, through a small parser: strip `file://` (or `kitty-shell-cwd://`), split
   host from path, try the raw path first (zz's zsh, bash and PowerShell emitters do not encode), then a
   tolerant percent-decode; strip the `/` before a Windows drive letter. No host check: the fallback only
   runs when the live cwd is missing, and a foreign path fails `is_dir()`.
4. `start_path`.

A root that is not UTF-8 or contains control characters is refused. `cwd` in `Begin` is filled only from
step 2, because relative inserts are only safe against the live cwd.

**Refusals in Begin**: foreground basename `ssh`, `mosh`, `mosh-client`, `tmux` or `zz_cli`, since the files would be
the wrong machine's.

**Insert style**, computed off the `inner` lock from the foreground process group `fg` (`tcgetpgrp`):

- Claude: a claude peer record for the pane with `zz.is_none()` (zz's own terminal peer records must not
  match) whose process group equals `fg`. Add `process_group(pid)` next to `parent_pid` in
  `agent/claude_peers.rs` (macOS `pbi_pgid`, Linux `/proc/<pid>/stat`).
- Codex: a member of the `fg` group whose basename is `codex` (one `ps -axo pid=,pgid=,comm=` on macOS,
  `/proc` on Linux). This also catches the npm `node` leader with a native `codex` child, and rejects a
  suspended or background codex.
- Both checks exist only under `cfg(all(feature = "agent", unix))`, and are skipped when the
  foreground basename is a shell at its prompt (`sh`, `bash`, `zsh`, `fish`, `dash`, `ksh`, `nu`,
  `pwsh`, `powershell`), which spares a `ps` spawn per listing on macOS. Elsewhere, and otherwise:
  `Shell(kind)` from the foreground basename, lowercased, `.exe` stripped: `fish`, `pwsh`/`powershell`,
  `nu`, else `Posix`.
- `@agent_state` is never consulted: it stays set after the agent exits.

**Walker**: the breadth-first loop of `scan_directories` (`crates/zz/src/file_picker.rs` 122-216)
moved into zz-daemon and extended to files. `ignore` walks depth-first only, so the loop keeps a
`VecDeque` and runs one `WalkBuilder::max_depth(Some(1))` per directory.

- `git_ignore`, `git_exclude`, `git_global` on; `hidden(!inside_repo)` from one check at the root.
- `filter_entry` drops `.git`, `.jj`, names that are not UTF-8, and names with any control character (so
  no descendant of a bad directory carries it in `rel`).
- `node_modules`, `target`, `__pycache__`, `venv`, `go/pkg/mod` are listed but never entered. When the
  root is `$HOME`, the depth-0 media roots (Applications, Library, Movies, Music, Pictures, Public,
  Templates, Videos, snap) are listed but not entered. One shared function holds these rules for both
  walkers.
- Symlinks: one `metadata()` call sets `kind`; `symlink: true`; never entered.
- Stay on the root's `st_dev`, checked before a child directory is queued. A root of `/` lists its
  children only.
- Limits: depth 8, 50k entries, 3 s; `truncated` when one stops the walk. A daemon-wide cap of 4 live
  walker threads bounds leaks from hung mounts; a request over the cap gets its `Begin` and then an empty `done, truncated` chunk. Walks for one client run one at a time behind a per-connection lock; a superseded request that is still waiting sends nothing, and one that waits more than 3 s gets an error `Begin`. Git marks go out after the last entry chunk and before a final empty `done` chunk, so they only name entries already sent.

**Flow control.** The client outbound queue closes a client that falls 256 reliable messages behind
(`MAX_RELIABLE_MESSAGES`, `close_outbound_too_far_behind`), and terminal frames share that queue. So:

- chunks are capped at about 64 KiB encoded;
- before each enqueue the walker waits, polling from 1 ms backing off to 50 ms and checking cancel,
  while `queued_reliable()` reports at least `CONTROL_PENDING_MESSAGE_LIMIT` messages or 256 KiB;
- it stops on cancel, on `queued_reliable() == None`, when `enqueue_reliable` returns false (the
  result is `#[must_use]`), or when the queue has not drained at all for 5 s, so a stalled client
  cannot hold a walker slot forever.

**Git**, alongside the walk, with `GIT_OPTIONAL_LOCKS=0` and `-c core.fsmonitor=false`. The root can
be any directory the user browsed to, including an untrusted repo, so git must not run repo-configured
commands:

- `git -C <root> rev-parse --show-prefix`, then
  `git -C <root> config -z --name-only --get-regexp '^filter\..*\.(clean|process)$'` (reading config
  runs nothing), then
  `git -C <root> status --porcelain=v1 -z --no-renames --ignore-submodules=dirty -- .` with
  `-c filter.<name>.clean= -c filter.<name>.process= -c filter.<name>.required=false` for every
  driver found. A driver name containing `=` means no marks. LFS files whose stat does not match the
  index can show a false Modified.
- The walk's cancel flag also kills git, so fast navigation does not stack `git status` runs.
- Strip the prefix; drop paths outside it. `??` = Untracked; `U` in either column, `AA` or `DD` =
  Conflicted; `A` in X = Added; other `M`, `T`, `A` = Modified; `D` skipped.
- 2 s deadline and 2 MiB cap; any failure, cap hit included, means no marks.
- The bounded runner (`run_output_until` and helpers) moves out of `agent/git_summary.rs` into a module
  gated on the `daemon` feature, taking the deadline as a parameter. `git_summary` keeps its 5 s.

**Dependencies.** `ignore = "0.4.33"` moves into `[workspace.dependencies]`; zz-daemon takes it as an
optional dependency enabled by the existing `daemon` feature. No new feature. iOS, web and FFI builds
link zz-daemon with default features off and stay free of it.

# B. Insert text and the desktop UI

## Insert text

One pure function in zz-client, `insert_text(root, rel, kind, cwd, style, absolute) -> Option<String>`,
tested as a table:

- Path: `root` joined with `rel` using `/`, compared as path parts (not `std::path::Path`, whose rules follow the client's OS while the host may differ). Relative when every part of `cwd` prefixes it and `absolute` is false (empty
  becomes `.`), else absolute. Never a `~/` form (the shell's `$HOME` may differ from the daemon's).
  Dirs end in `/`.
- `Posix`: bare when every character is in `[A-Za-z0-9_./:@%+,=-]`, else POSIX single quotes (`'`
  becomes `'\''`). A relative path starting with `-`, `=` or `+` gets `./` first, in every shell kind (`+cmd` is an
  ex command to `vim` and `less` even when quoted).
- `Fish`: single quotes, with `\` written `\\` and `'` written `\'`.
- `Pwsh`: single quotes with `''`; backslash is literal and allowed bare; names starting with `@` and
  names containing `,` are quoted.
- `Nu`: `r#'...'#` with one more `#` than the longest run following a `'` in the path.
- `Claude`: `@path` when every character is in `[A-Za-z0-9_./-]` and the last one before any
  trailing `/` is alphanumeric or `_`; else `@"path"` when the path has no `"` and no `#`; else the
  plain absolute path. Claude Code reads a bare mention up to a word boundary, so `@./`, `@build-/`
  and `@v1./` would name nothing or the wrong dir.
- `Codex`: the bare path, or `"path"` when it contains whitespace and no `"`. No `@`.
- One trailing space in every form. Returns `None` for a path with control characters (backstop; the
  walker already drops them).

The client then sends `TerminalView { Paste(text) }` to the pane.

## Desktop wiring (crates/zz)

- `MuxClient`: `PathListBegin`, `PathListChunk` and `PathListGit` are handled in
  `handle_unreduced_message` (the `PastedImage*` precedent) and emitted to `AppView`, which forwards them
  to the open picker only when the request id matches. `OpenPathPicker` becomes a one-shot
  `CoreEvent`. `request_path_list` and `cancel_path_list` on the zz-daemon client mint ids the way
  `request_home_directories` does.
- `AppView`: `OverlayKind::PathPicker`, returned by `visible_overlay` after Menu, Confirm, Popup and
  CommandPalette. Counted in `overlay_open`, so `C-b` does not arm while it is open. On open:
  `send_prefix_cancel`, focus the picker, reset `synchronized_signature`. On close: `focus_active_pane`.
  A transparent occluding layer closes it on an outside click. It also closes on attached-host change,
  detach, pane removal, active pane or window change, and palette open; closing sends `PathListCancel`
  from `reset_session_state` while the old host is still attached.
- Mounted at workspace level, never inside `TerminalView`: terminal panes are `.cached`, and a picker in
  their tree would re-render the terminal on every keystroke.
- Placement: a new `TerminalView::cursor_bounds()` accessor, read once at open. When it is `None`, or
  pane search is open, or IME text is composing, use the grid's bottom-left instead (second accessor).
  The position stays fixed while chunks stream. `deferred(anchored())`, flipped above by hand when the
  maximum height (10 rows) does not fit below (as `image_hover_popover` does, because gpui's
  `SwitchAnchor` keeps the same point and would cover the cursor row), `snap_to_window_with_margin(8)`.

## View (zz-ui)

`PathPickerView` with `Rc<dyn PathPickerBackend>`:

```rust
pub trait PathPickerBackend {
    fn list(&self, dir: Option<&str>, cx: &mut App) -> Option<u64>;
    fn cancel(&self, request_id: u64, cx: &mut App);
    fn insert(&self, root: &PathListRoot, entry: &PathEntry, absolute: bool, cx: &mut App);
}
```

- The view never builds insert text: the host's backend calls `zz_client::path_insert::insert_text` and
  pastes the result. `display_root` arrives ending in `/`.
- Surface: `CommandPaletteSurface` with the `display_root` as the input prefix (`~/dev/zz/`), rows from
  `command_palette_entry` (Folder/File icon, muted directory prefix, byte-range match highlights, the git
  letter on the right), `PaletteHint` footer. At most 10 visible rows in a `uniform_list`.
- Keys: Up/Down and C-n/C-p move; Enter inserts; Alt-Enter inserts absolute; Tab (captured
  `IndentInline`, or Root's Tab binding moves focus) re-roots into the selected dir; Backspace on an empty
  query goes to the parent (copy the palette's `leave_layer` capture, not a key-down listener); a query
  starting with `~/`, `/` or `../` re-roots; Esc closes.
- Re-root without a round trip when the target dir sits inside a walk that finished without truncation:
  filter the collected entries. Otherwise send a new request.
- Empty query: the entries whose `rel` has no `/`, dirs first, then by name. The depth-1 set is complete
  once the first deeper entry or `done` arrives.
- The view emits a dismissed event; `AppView` observes it the way `observe_command_palette` does.

## Ranking (zz-client)

- The scoring in `file_picker.rs` 280-318 moves to `zz_client::path_rank::rank(query, labels, prior,
  limit)`: `neo_frizbee::match_list`, typo budget `chars / 4` capped at 6, smart case; order by score,
  then prior, then label; 500 rows. The desktop picker's directory search calls the same function.
- The shell picker's prior: git mark (changed files first), shallowness, and a basename bonus when the
  whole match falls inside the file name.
- `path_rank::highlights(query, kept)` runs `match_list_indices` only on kept rows and returns byte
  ranges.
- Ranking runs on a background task per keystroke or chunk batch, with a generation guard; the previous
  rows stay on screen until the new result lands. Chunks are stored as `Vec<Arc<[PathEntry]>>`, so a
  snapshot clones Arcs, not entries.
- `neo_frizbee = "=0.11.0"` joins zz-client (zero dependencies, scalar fallback for wasm);
  `clients/web/Cargo.lock` is updated in the same change.

# C. Which-key

## Daemon

- Two read-only accessors on `KeyEngine` (`crates/zz-protocol/src/key.rs`), whose fields are private:
  - `shown_table(now) -> Option<(&str, bool)>`: `None` for mode tables and for tables named copy-mode or copy-mode-vi, once
    the repeat deadline has passed, and for `prefix` after prefix-timeout when no repeat is held (`>=`,
    so a wake landing exactly on the deadline counts). The bool is "repeat held".
  - `next_deadline() -> Option<Instant>`: the repeat deadline, else the prefix deadline.
- The engine is never mutated by the timer. It keeps expiring lazily on the next key, so
  `#{client_prefix}` and `#{client_key_table}` (which read `active_table()`) keep matching the pin, and
  `mode_table_after_prefix` restore and `C-b Up Up` under a short prefix-timeout keep working.
- `sync_prefix_armed` becomes `sync_key_table(client, force)`, with a per-client
  `published_key_tables: BTreeMap<ClientId, (Option<String>, bool)>` in place of the `prefix_armed` set:
  - value = `shown_table(now)`, dropped when it equals the key-table of the client's attached session;
  - publishes `PrefixArmed` when `table == Some("prefix")` flips (unchanged meaning for the TUI, iOS
    and FFI), then `KeyTableActive` when the pair changes;
  - with `force`, re-sends `KeyTableActive` for every key decided inside a table, so a table that
    re-enters itself shows the sheet again after each pause;
  - called at every current `sync_prefix_armed` site and at the sites that skip it today: the unbind
    reset in `publish_key_tables_if_changed`, detach, the async command-output close,
    `close_overlays` retiring an output, and `follow_command_output_focus`. Disconnect only removes the
    entry.
- Timer: a per-client deadline thread in the `start_display_panes_deadline_dispatcher` pattern. The sync
  sends `Schedule(client, next_deadline())`, which overwrites the client's entry. On wake it runs the
  sync and nothing else; a stale wake publishes nothing. This fixes `PrefixArmed` staying true after
  `C-b Up` until the next key.
- Existing bug fixed on the way: the `previous_key_table` capture around command output must not save
  `prefix` or a repeat-held table. Today closing the output restores `prefix` with both deadlines
  cleared, so the prefix never times out.

## Notes

- Every stock prefix key zz shares with the pin gets the pin's `-N` string (`key-bindings.c` 381-471),
  except `r`, whose zz binding differs. The four arrow notes change to the pin's wording ("Select the
  pane above the active pane", and so on). zz writes notes for `r` ("Reload the configuration"), `e`
  (send last output) and `F`.
- `Binding::send_prefix()` keeps its inline note, because `set_prefix` rebuilds that binding.
- `C-b ?` goes from 35 to 78 rows (the pin shows 92). The move table stays as is; it has no prefix `g`
  to reach it (`key:prefix:g`).
- Parity check: add `#{key_note}` to the `list-keys -F` format in `compat/tmux-oracle.py`, regenerate
  `tmux-oracle.json`, and compare shared prefix keys in `compat_manifest_tests.rs` with an exception
  list starting with `r`.

## Client

- `ClientCore` stores the last `KeyTableActive`, resets it with `prefix_armed`, and emits one
  `CoreEvent`. New variants join every exhaustive match (`apply_core_event`, the workspace test helper,
  the TUI's no-op list).
- The desktop claims keys while any non-root table is active, in both claim helpers. Today a custom
  `-T` table cannot be pressed from the desktop at all.
- Timer: restarts on every `KeyTableActive { Some, repeat: false }`; shows after `which-key-delay`
  (`ConfigValue<f32>`, range 0 to 2000, default 400, 0 off, read through `resolved_config` when the
  timer starts, one Settings row). Hides on `None`, on `repeat: true`, on any key-down seen in
  `intercept_keystroke` (after its modifier-only early return; never on key-up, which would hide the
  sheet on the prefix key's own release), while an overlay or dialog is open, during a pane drag, and on
  the Settings route.
- Rows, built in zz-client:
  - stock prefix bindings equal to the default at the same key go into groups by a fixed key-to-group
    map (Panes, Windows, Sessions, Copy and paste, Other), which names every stock key explicitly (`:`,
    `?`, `i`, `~`, `r`, `e` and the send-prefix key go to Other), with a unit test that every default
    prefix key is in the map;
  - Yours: bindings whose `(commands, repeat)` differ from `KeyTables::default()` with the client's
    prefix applied, or that the default lacks (notes ignored);
  - custom tables render as one flat list; mouse and wheel keys are dropped; `-r` keys are marked;
  - key caps come from `display_keystroke`, the call the zero-session panel already uses.
- `WhichKeyView` (zz-ui): a passive, occluding bottom sheet over the workspace with `popover_style`,
  `Kbd` caps, a column per group and Yours last. It is never focused and never counted in
  `overlay_open`, and it never sends `CancelPrefix`.

# D. Testing and bookkeeping

Tests:

- zz-protocol: round-trip and tail-order for every new variant; `shown_table` / `next_deadline` with
  prefix-timeout 300 ms and a 500 ms repeat window (`Up` at 100 ms and 400 ms still selects; both
  published values clear after the repeat deadline with no key).
- zz-daemon:
  - walker: gitignore, hidden rules, `.git`, a newline in a file name, ESC in a directory name (no
    children appear), non-UTF-8 on Linux, symlinks, `st_dev`, `/` children only, limits, cancel;
  - flow: a stalled outbound never closes the mailbox, cancel during the stall stops the walk, three
    back-to-back walks stay under `CONTROL_PENDING_MESSAGE_LIMIT`;
  - git mark parsing, the OSC 7 parser;
  - insert style: a background or suspended claude or codex with zsh in front gets the shell style; a
    `node` leader with a `codex` child gets Codex;
  - `choose-path`: every refusal, and TUI, web and FFI clients get the capability error;
  - `sync_key_table`: copy-mode stays silent, `switch-client -T` shows, prefix clears, the stuck-prefix
    regression (`bind -r x list-keys`, press, close the output, `active_table()` is `None`).
- zz-client: the insert-text table (every shell kind, Claude, Codex, `-rf.txt` becomes `./-rf.txt`,
  `=foo` becomes `./=foo`, `+q` becomes `./+q`, picking the cwd gives `./` and `@"./"`), `path_rank`.
- zz-ui: gpui tests for picker keys, and for which-key: after `C-b`, its key-up, and
  `KeyTableActive{Some("prefix")}`, the sheet shows past the delay; a key-down in a custom table hides it.
- Manual: zz Dev with zsh, fish, claude and codex panes, on the local host and an ssh host.

Bookkeeping in the same change:

- `PROTOCOL_VERSION` 107, the two asserts, and the history in `knowledge/protocol/wire-protocol.md`.
- `compat/tmux-gaps.json`: `native-command:choose-path` and `native-key:prefix:F` in sorted position with
  dated reasons; manifest counts 366 to 367 and 91 to 92; the `COMMAND_SPECS` count in
  `crates/zz-mux/tests/hunt_claims.rs`; `python3 compat/tmux-tracker.py write-report`; a zz-native row in
  `knowledge/tmux/commands.md`.
- `just web-build` for the lockfile.

# Build order

Each step is useful on its own:

1. Which-key daemon side and notes. Fixes the stale armed state, the stuck prefix after command output,
   and `C-b ?` parity before any UI exists.
2. Picker protocol, `choose-path`, and the daemon listing.
3. zz-client insert text and `path_rank` (the desktop picker switches to it).
4. zz-ui views and desktop wiring for both features.

# Later

- TUI renderers for both (floating layer at the pane's cursor cell; which-key needs an option, since the
  TUI status row stays pin-identical).
- Web and iOS mounts (move `parse_keystroke` and friends from zz-config into zz-client first; iOS flips
  against `fully_visible_bounds` to stay clear of the soft keyboard).
- More picker sources as tabs: screen tokens, history, recent directories.
- Percent-encode OSC 7 in the zsh, bash and PowerShell emitters.
