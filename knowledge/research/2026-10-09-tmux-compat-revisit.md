---
type: Research Report
title: tmux compatibility revisit (refusals and the 3.8 drift)
description: "A re-check of every tmux command, flag and option zz still refuses against today's architecture (raw TUI, caller stream channel, daemon popups), plus the measured CLI delta between the pin d77c9dc6 and the tmux 3.8 release, with fabrico's 2026-10-09 rulings: build floating panes, move the pin to 3.8, defer session sharing and menu scroll capture."
tags: [tmux, compatibility, floating-panes, tmux-3.8, gaps, research]
resource: compat/tmux-gaps.json
timestamp: 2026-10-09T00:00:00-03:00
last_updated: 2026-10-09
---

# Summary

Measured 2026-10-09 at `main` 2f7dfe694 (zz 0.16.0, protocol 107). Three research passes: one
re-read every refused item in `compat/tmux-gaps.json` against the code as it is now, one did the same
for the three structural refusals (floating panes, linked windows, multi-user ACL), and one built the
pin, the tmux 3.8 tag and upstream master and diffed their oracle inventories.

The work this produced is tracked in the catch-up campaign ledger, `compat/catchup/ledger.json`
(rules in `compat/catchup/README.md`).

**Rulings (fabrico, 2026-10-09):**

1. Build floating panes, by making the daemon's per-client popup window-owned.
2. Move the pin to the tmux 3.8 tag.
3. Session sharing with other people (`zz share`): noted for later, see [Deferred](#deferred).
4. Desktop menu input capture (`desktop.overlay-consumers`): noted for later, see [Deferred](#deferred).

# Part 1: what zz still refuses, re-checked

Only three pinned commands are unimplemented: `new-pane`, `link-window`, `unlink-window`
(`crates/zz-protocol/src/catalog.rs`, `UNIMPLEMENTED_TMUX_COMMANDS`). Every refused flag in the
catalog maps to a registry item, except `choose-tree`/`choose-client`/`choose-buffer -NN` (below).

## Stale reasons (adopt now)

| Item | What changed | Recommendation |
|---|---|---|
| `options.client-terminal-negotiation` | The registry says zz never reads terminfo. It does: the daemon runs `infocmp -x` (`crates/zz-daemon/src/status.rs:1124-1165`) and `crates/zz-mux/src/terminfo.rs:451-526` applies `terminal-features` and `terminal-overrides`. The result only feeds `#{client_colours}`, `#{client_termfeatures}` and `show-messages -T`. | Send the computed feature mask (`daemon.rs:40085-40105`) to the raw TUI and let `crates/zz-tui` raise its colour depth and arm extended keys from it. Adopt `user-keys` (bind already parses `UserN`). Keep `extended-keys-format`, `assume-paste-time`, `xterm-keys` native. |
| `protocol.binary-streams` | The caller stream channel (TUI-018, `knowledge/designs/command-stream-channel.md`) landed. `display-message -I`, `split-window -I`, `load-buffer -`, `save-buffer -`, `source-file -` and non-UTF-8 argv all work. | Close the last two: `show-buffer` on binary content (`daemon.rs:19611-19615` refuses; the pin writes raw bytes to a command client and escapes them for attached and control clients, `cmd-save-buffer.c:98-108`, `server-client.c:3024-3026`; `vis_escape` exists at `status.rs:3120`), and `source-file -` with non-UTF-8 stdin (route the config sink through the byte parser file-based `source-file` already uses). |
| `capture.rich-transports` | libghostty now exposes per-row wrap, hyperlink and semantic-prompt flags and per-cell hyperlink URIs; zz reads them (`crates/zz-terminal/src/session.rs:994`, `:8186-8230`). | Adopt `-H` (each unique URI per line, `cmd-capture-pane.c:201-237`). Adopt `-F` for the W, H, P and O line flags; record D and X (tmux grid storage details) as native. Keep `-P` and `-R` refused. |
| `choose-* -NN` (no registry item) | Refused at `crates/zz-mux/src/command.rs:17025-17030`; `divergences.md:710-713` says choosers have no preview, but the raw TUI draws Normal, Off and Big previews (`crates/zz-tui/src/render/chooser.rs:773`). Single `-N` is silently ignored (should open with preview off, `mode-tree.c:578-584`). | Adopt both. |
| `options.native-mode-styles`, `options.native-overlay-styles` | The daemon hard-codes the tree-mode border and selection styles (`chooser_presentation.rs:184, 529, 669`). | Read the tree-mode style options and the prompt cursor style and colour options in the raw TUI. Copy-mode line numbers stay for the 3.8 work. |
| `formats.pane-current-command-empty` | Open item awaiting a ruling. The pin falls back to the start command, then the default shell (`format.c:933-953`); zz returns `""` with no foreground process (`daemon.rs:487-491`). | Adopt the fallback: blank names hurt automatic rename and window names. |
| `clients.interactive-refresh` | `refresh-client -l` is not a pan flag: it reads the client terminal's clipboard with an OSC 52 query and stores a buffer (`cmd-refresh-client.c:244-247`). The catalog's descriptions of `-c -D -L -R -U` are also wrong (`catalog.rs:1594-1600`). | Adopt `-l` in the raw TUI. Fix the descriptions. Keep pan refused. |
| `mouse.bound-context` | Mouse events already carry pane and cell to bound commands (protocol 102). What blocks `copy-mode -S` is the cell scrollbar; what blocks `move-pane -M` is floating panes. | Point the reason at those two items; `move-pane -M` lands with floating panes. |
| `sessions.linked-groups` | Unchanged in substance. | Keep. Make the `new-session -t` error suggest `attach -t <session>`, since every zz client already keeps its own current window. |

## Still valid (keep refused)

- `terminal.resize-pane-trim` (`resize-pane -T`): libghostty still cannot move rows between scrollback
  and the screen.
- `capture-pane -P`, `-R`: dumps of tmux's parser buffer and grid structure.
- `refresh-client` panning (`-c -D -L -R -U`): only meaningful once the raw TUI draws oversized
  windows the pin's way. Today it scales the layout to its screen (`crates/zz-tui/src/state.rs:1080-1110`)
  and crops an oversized pane at its top-left (`render.rs:1035-1037`), which is itself an unrecorded
  divergence.
- `options.lock-program`: the 2026-09-14 ruling stands. Note that the pin's client runs the lock
  program itself (`server-fn.c:173-179`, `client.c:798`) and the raw TUI has a suspend path
  (`app.rs:1049-1080`), so the technical premise is weaker than the record says.
- `options.native-pane-scrollbars`: a cell scrollbar shrinks the pane for every client.
- `options.terminal-engine-limits`: libghostty still only has a clipboard write callback.
- `link-window`/`unlink-window`: windows would need several owners (`Window.session` and `Window.index`
  live on the window, `crates/zz-mux/src/model.rs:234-238`; the snapshot nests windows in sessions,
  `snapshot.rs:565`; about 243 `window_for_pane` call sites). Large cost, small audience.
- Session groups (`new-session -t`, `kill-session -g`, `choose-tree -G`): making `-t X` mean "attach
  to X" is wrong (`new -t main \; set destroy-unattached on` would kill `main` on detach). Real "view
  sessions" cost 1 to 2 weeks for little gain over plain attach. Upstream's unmerged `local_windows`
  and `local_windows_panes` branches add per-client window and pane selection (`select-window -L/-S`,
  `select-pane -s/-S`, `local_active_window`, `pane_local_active`); zz already has that state
  (`daemon.rs:9480-9540`), so adopt those spellings cheaply if they merge.
- `server-access`: admitting another uid hands over every PTY, ssh session, browser cookie jar and
  agent session.

## Floating panes (ruling: build)

What the pin does: a floating pane is a real pane above the tiled layout with its own position, size
and z-order, seen by every client. `new-pane` shares `cmd_split_window_exec` with `SPAWN_FLOATING`
(`cmd-split-window.c:37-53, :95`), cells are marked `LAYOUT_CELL_FLOATING` (`tmux.h:1524`),
`break-pane -W` floats a tiled pane. Default keys: prefix `*`, `@`, `g` (the move table), Tab/BTab.

What zz has: most of the parts, owned per client instead of per window.

- `display-popup` runs a real PTY in the daemon (`display_popup`, `daemon.rs:18319`), stored per client
  in `ClientState.popup` with a synthetic pane id `PaneId(u64::MAX - token)` (`:18675`).
- The daemon already handles popup drag and resize (`popup_pointer`, `:21399`, `PopupPlacement`) and
  can turn a popup into a tiled pane (`popup_make_pane`, `:21753`).
- Every client already draws a terminal over the tiled panes: desktop `popup_overlay`
  (`crates/zz/src/workspace/view.rs:3433`, `FloatingSurface` in `crates/zz-ui/src/pane.rs:280`),
  web and iOS (`clients/gpui-shared/src/app.rs`, `floating.rs`), raw TUI (`crates/zz-tui/src/overlay.rs`).
- `Window` already keeps a `z_order` (`crates/zz-mux/src/model.rs:234-262`).
- The pin's layout dump leaves floating panes out (`layout-custom.c:72`); 3.8's JSON layouts include
  them.
- zz ships the pin's pane menu with "Float" bound to `break-pane -W`, which errors, and registers the
  19 move-table bindings, which do nothing useful.

Upstream master (after 3.8) deleted `popup.c` and turned `display-popup` into a floating modal pane
above zoom (34cd5da4). Making zz's popup window-owned therefore converges on upstream's model rather
than adding a second one. Rough size: 15 to 20 files across zz-mux, zz-protocol (snapshot, wire bump),
zz-daemon, zz-client, zz-tui, the desktop and gpui-shared; 1.5 to 3 weeks.

Risks: positions are in window cells and window size follows the latest active client, so they need
clamping on shrink (popup placement already does this); interaction with zoom; raw TUI hit-testing
must check floating panes top-first.

# Part 2: the tmux 3.8 drift

tmux 3.8 was released 2026-09-09; the tag `3.8` is 7f2a35ad (2026-10-08, after rc3/rc4) on a release
branch cut from master at 5aeacf1c. 3.8 is not an ancestor of master. `d77c9dc6..3.8` is 446 commits,
`d77c9dc6..master` 577 (master is `next-3.9`, 82abcd17 at the time). A copy of `compat/tmux-oracle.py`
with soft asserts, run on a build of the pin, reproduces the committed `compat/tmux-oracle.json`
exactly, so the delta below is measured, not read from CHANGES.

**Already in the pin, not part of the delta:** `new-window -E`, `respawn-pane -E`, `respawn-window -E`,
`-a -f` on the kill commands, `new-pane -B -T -W -L`, `command-prompt -P`, `choose-tree -h -k`,
`choose-client -h -k`, `set-hook -B`, `show-hooks -B`, `switch-mode`, `mouse` on by default, `theme`,
`tree-mode-selection-style`, `fill-character`, the `O:` and `V:` modifiers, prefix `g` and Tab/BTab.

## pin to 3.8

**Commands:** none added or removed, aliases unchanged, no arity change. Flags:

| Command | Change |
|---|---|
| capture-pane | `+-I` (time each line entered history) |
| display-message | `+-j` (parse as JSON and print) |
| display-panes | `-b` removed; `+-k -Z`, `+-s source-window`; `-t` is now a target pane, not a client; it is a pane mode now (`window-panes.c`) |
| new-pane | `+-A -C -D -K -M -O` (modal panes and mouse-drag creation) |
| split-window | `+-B border-lines` |
| set-hook | `+-E` (fire user event `@name`), `+-T` (monitor fires only while true) |
| show-options, show-window-options, show-hooks | `+-F format` |
| wait-for | `+-E -l -v`, `+-F format`, `+-w waiter`; positional `channel` becomes `name` |

**Options:** added `clear-on-attach` (server, on), `copy-mode-current-line-style` (window),
`display-panes-border-style` (window). `display-panes-active-colour`, `-colour`, `-format`, `-time`
moved from session to window scope. `remain-on-exit` gains `failed-key`. Default changes:
`fill-character` becomes a format, `display-panes-format` uses `pane_unzoomed_*`,
`pane-active-border-style` checks `pane_modal_flag`, `status-format[1]` and `[2]` changed.

**Formats:** 16 new `format_table` names: `history_added`, `history_collected`, `history_generation`,
`pane_command_{duration,end_time,running,start_time,status}`, `pane_last_output_time`,
`pane_last_prompt_time`, `pane_modal_flag`, `pane_output_generation`, `pane_private_modes`,
`pane_unzoomed_{width,height}`, `window_modal_pane`. New context formats: `copy_line_numbers`,
`refresh_active`, `clipboard_invalid`, `is_inside`, `is_outside`, `is_environment`, `environment_*`,
`hook_fire_count`, `hook_fire_time`, `hook_monitor_format`, `hook_monitor_target`, `option_*`.
`hook_*` names are now built from event payload keys (old names still resolve). New modifier `A`
(animation frames, status and border formats only). `q:` now also escapes `{`, `}`, `\n`, `\t`.

**Hooks:** 68 to 89. Added `after-swap-window`, `client-created`, `client-closed`,
`marked-pane-changed`, `pane-activity`, `pane-bell`, `pane-command-started`, `pane-command-finished`,
`pane-created`, `pane-mode-entered`, `pane-mode-exited`, `pane-moved`, `pane-prompt-opened`,
`pane-prompt-closed`, `pane-resized`, `pane-shell-prompt`, `session-added-to-group`,
`session-removed-from-group`, `window-created`, `window-closed`, `window-zoomed`, `window-unzoomed`.
Removed `after-queue`.

**Default keys:** 303 to 308. Prefix `T` (`command-prompt -I "#T" { select-pane -T "%%" }`).
`C-MouseDown1Pane` removed; `C-MouseDrag1Pane` and `C-MouseDrag1Empty` run `new-pane -M`;
`MouseDown3Empty` and `M-MouseDown3Empty` open a New Pane / New Window menu; `MouseDown1Border`
becomes `select-pane -t =`. Copy mode: `L` runs `line-numbers-toggle`, `r` becomes `refresh-now`.
Pane menus gain line-number and refresh items. New copy commands: `line-numbers-on`,
`line-numbers-off`, `line-numbers-toggle`, `refresh-now`.

**Contract breaks:**

| Change | Evidence |
|---|---|
| `#{window_layout}` and `list-windows` print JSON v2 (`{"V":2,"L":{...}}`); `select-layout` still accepts v1; control clients get v1 unless they set the `new-layouts` flag | bf43fdc0, d9692f7e |
| The `active-pane` client flag is removed (zz still implements it: `crates/zz-client/src/core.rs:1677`, `crates/zz-cli/src/lib.rs:3597`) | 1ce00006 |
| `display-panes` is a pane mode | 1a02c995 |
| Menus belong to the window and show on every client | ad6832e6 |
| `%%` escapes `'` as `'\''` | a177d0f5, `cmd.c:843-916` |
| Invalid relative targets (`+foo`, `-0`) are errors | 00f3899a |
| Control notifications are queued outside `%begin`/`%end`; notify replaced by events | 6db5175e, d29aa121 |

**Oracle extractor:** `compat/tmux-oracle.py` breaks on 3.8. It expects 9 `args_parse` callbacks
(3.8 has 8; display-panes now uses `cmd_choose_tree_args_parse`), literal format scopes grow from
31/153/108 to 37/204/122, the `hook_*` family comes from `events-payload.c:event_payload_add_formats`,
propagation moved from `notify.c` to `hooks.c:hooks_parse`, modifiers go from 36 to 37. It records
option names only, so default and scope changes do not show in its diff.

## 3.8 to master (not pinned, for awareness)

`display-popup -N` removed and `display-popup` becomes an undocumented compatibility command that
spawns a floating modal pane; `popup.c` deleted (34cd5da4); `popup-style`, `popup-border-style`,
`popup-border-lines` removed (configs setting them fail). `switch-mode -O sort-order`, `-r`. New
`window-default-command`; `pane-border-lines rounded`; copy-mode output commands (`select-output`,
`open-output`, `copy-output`, `pipe-output` and friends, bound to `C-o`, `e`, `M-o`, `M-e`).

Upstream branches worth watching: `hidden-panes` (`resize-pane -H`, several zoomed panes,
`move-pane -P front/back`), `zoomed-floating-panes`, `local_windows` and `local_windows_panes`
(per-client window and pane), `osc133_line_collapsible` (fold command output in copy mode),
`kitty-keys-autodetect`, `4902-image-support`, `command_parser` (rewrites `cmd-parse.y` and changes
`%%` again).

# Other findings

- `attach -r` is not a security boundary: a read-only client can run `switch-client -r` and make
  itself writable, because zz skips the pin's uid check (`cmd-switch-client.c:83-93`). Harmless on a
  single-user socket; it matters the day sessions are shared.
- The raw TUI crops an oversized pane at its top-left corner instead of following the cursor; not
  recorded anywhere yet.

# Deferred

**Session sharing (`zz share`), fabrico 2026-10-09: later.** The intent behind `server-access` is
pairing and letting someone watch. A native version would be a capability bound to the connection:
pinned to one session, read-only unless `-w`, not clearable by the client, snapshot and terminal
streams filtered to that session, browser and agent panes hidden, reached through a token route on
zz-web (exposed with `ssh -R` or `tailscale serve`) or an ssh forced-command attach. Today zz-web only
binds loopback, checks Host and Origin, has no token, and forwards raw bytes, so every web client is
a full owner (`crates/zz-web/src/lib.rs:53-137`); the per-client snapshot stamp (`daemon.rs:43384`)
is where a session filter would go. It contradicts the multi-device design's "sharing with other
humans" non-goal, so it needs its own design and a security review. Rough size 1 to 2 weeks.

**Desktop menu input capture (`desktop.overlay-consumers`), fabrico 2026-10-09: later.** All nine
overlay payload kinds have a desktop consumer (`view.rs:5412`). What is left: mouse motion and wheel
still reach the pane under an open menu. The registry says gpui has no capture-phase hook, but
`Window::on_mouse_event` (gpui `window.rs:5866`, rev 99114cc) receives move and wheel events in the
capture phase, and a full-window `occlude()` layer would also work. The choice is to swallow them like
the pin or record scroll-under-menu as native and close the item.
