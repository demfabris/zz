---
type: Research Report
title: Terminal augmentation survey
description: Which tmux plugins, terminal features, and agent-era tools augment the shell with overlays (pickers, hints, command blocks, agent inboxes), how each one works, what zz already has to build them natively, and a ranked idea list with mockups.
tags:
- tmux-plugins
- picker
- hints
- osc-133
- agent
- overlays
- survey
timestamp: 2026-09-25T00:00:00Z
---

# Question

On 2026-09-25 almonk posted a terminal overlay that inserts a path to anything at the shell cursor,
with fuzzy matching plus classic path navigation, "as useful for your `cat` as your `claude`". zz
runs tmux plugins unchanged, so any plugin's idea can also become a native feature. Which ideas are
worth building, how do the existing tools implement them, and what does zz already have?

# Method

One web sweep on 2026-09-25 (x.com, GitHub, vendor docs; star counts from `gh api` that day), and
one source inventory of zz at `ec31d080`. Nothing below was built or run in zz except the fzf
`--popup` probe noted under Traps. Mockups of ten ideas in zz chrome live on a private Design
canvas: https://claude.ai/artifact/SSbu31rWBzNU1E2Gdz3zw1

# The trigger: Superlogical's path picker

The post is https://x.com/almonk/status/2103572918680645870. almonk (Alasdair Monk) cofounded
Superlogical with Mitchell Hashimoto, Jack Pearkes, and Hector Simpson; it is the company behind
the Rex CLI demo zz mined on 2026-09-15. Superlogical announced a $10M seed on 2026-07-29
(https://mitchellh.com/writing/superlogical). Its server owns session state, clients embed
libghostty, and there is only a beta signup. The picker is an unreleased "experiment".

The picker draws over the pane, anchors at the cursor, and types the chosen path into whatever
program runs there. Replies asked for remote `user@host:path` completion and for ranking by
subsequence match first, then recency. almonk also wrote bontree (https://github.com/almonk/bontree),
a file tree pane for copying paths next to agents. The video itself was not downloaded, so the
trigger key and exact navigation keys are unknown.

Shipped equivalents:

- kitty `choose-files` kitten (0.45.0, Dec 2025): fuzzy files with previews; `kitty_mod+p>c`
  inserts a file at the cursor, `>d` a directory. https://sw.kovidgoyal.net/kitty/kittens/choose-files/
- `raine/tmux-file-picker` (62 stars): fd + fzf in a tmux popup, pastes `@path` for Claude Code,
  with `--git-root`, `--zoxide`, `-d`. https://raine.dev/blog/tmux-file-picker-for-ai-agents/
- Warp `@`: files from the git root, symbols, blocks from other sessions. Warp can do it because it
  owns the input editor.
- Zellij `zellij pipe -p filepicker`: blocks and prints the chosen path.

# How tmux plugins build overlays

tmux has no overlay layer, so plugins fake one with three moves: `capture-pane` to read the text,
`display-popup` to run fzf or a TUI on top, and `swap-pane` to swap a rendered copy in for the live
pane (tmux-fingers `src/tmux.cr`, tmux-thumbs `src/swapper.rs`). Results go back with
`send-keys`, `paste-buffer`, or `set-buffer`. The pane underneath stops updating while swapped out.

A native multiplexer can draw the overlay directly while the pane stays live. The daemon also
knows each pane's cwd, foreground command, and OSC 133 prompt marks, which plugins have to scrape
or guess.

# Catalog

## Pickers and on-screen text

| Tool | Stars | What it does | Mechanism |
|---|---|---|---|
| [tmux-fzf](https://github.com/sainnhe/tmux-fzf) | 1.5k | fzf menus for sessions, windows, panes, commands, keybindings, clipboard, processes | `display-popup`, `list-*` |
| [tmux-fingers](https://github.com/Morantron/tmux-fingers) | 1.5k | Vimium-style labels on paths, SHAs, IPs, UUIDs, k8s names; letter copies, shift pastes, ctrl opens | `capture-pane` + `swap-pane` |
| [tmux-thumbs](https://github.com/fcsonline/tmux-thumbs) | 1.1k | Rust hints; uppercase label copies and pastes | `capture-pane` + `swap-pane` |
| [extrakto](https://github.com/laktak/extrakto) | 1.1k | Fuzzy search over words, paths, URLs, lines from scrollback; Tab inserts, Enter copies | `capture-pane -S` + popup |
| [tmux-copycat](https://github.com/tmux-plugins/tmux-copycat) | 1.2k | Preset regex searches; its README says tmux 3.1 regex search covers most of it | copy-mode search |
| [tmux-fzf-url](https://github.com/wfxr/tmux-fzf-url) | 734 | URLs on screen, pick, open | `capture-pane` + popup |
| [tmux-jump](https://github.com/schasse/tmux-jump) | 480 | Easymotion jump to a character | `capture-pane` + `send-keys -X` |
| [PathPicker](https://github.com/facebook/PathPicker) / [tmux-fpp](https://github.com/tmux-plugins/tmux-fpp) | 5.2k / 323 | Pick file tokens off the screen, open or paste | `capture-pane` + new window |
| [tmux-fuzzback](https://github.com/roosta/tmux-fuzzback) | 188 | fzf over scrollback lines, lands copy mode on the match | `capture-pane` + popup |
| kitty [hints](https://sw.kovidgoyal.net/kitty/kittens/hints/) | | url, path, line, word, hash, linenum, hyperlink, regex, ip; `linenum` opens the editor at the line | regex over the screen |
| WezTerm [QuickSelect](https://wezterm.org/quickselect.html) | | 14 patterns, labels from the bottom, one label per distinct text; lowercase copies, uppercase pastes | regex over viewport +-1000 lines |
| iTerm2 [Smart Selection](https://iterm2.com/documentation-smart-selection.html), autocomplete (Cmd-;) | | Regex rules with actions; complete from words on screen | screen regex |
| Zellij `link` plugin (0.44) | | Hover highlights paths, Alt-click opens `$EDITOR` in a floating pane | screen regex |

## Sessions and projects

| Tool | Stars | What it does | Mechanism |
|---|---|---|---|
| [tmux-resurrect](https://github.com/tmux-plugins/tmux-resurrect) / [continuum](https://github.com/tmux-plugins/tmux-continuum) | 13.1k / 4.1k | Save and restore layouts, cwd, some programs; autosave every 15 min | `list-*` formats, a `#()` status job as a timer |
| [tmuxinator](https://github.com/tmuxinator/tmuxinator) / [tmuxp](https://github.com/tmux-python/tmuxp) | 13.7k / 4.6k | Layouts declared in YAML | scripted commands |
| [sesh](https://github.com/joshmedeski/sesh) | 2.8k | One list of sessions, zoxide dirs, config entries; clone, worktrees, bounce to last | `new-session -c`, `switch-client` |
| [tmux-sessionx](https://github.com/omerxx/tmux-sessionx) | 1.4k | Session picker with previews; unmatched queries fall through to zoxide | popup + fzf + `capture-pane` |
| [tmux-sessionizer](https://github.com/ThePrimeagen/tmux-sessionizer) | 469 | Pick a project dir, create or switch to its session | `new-session -c` |
| [tmux-harpoon](https://github.com/Chaitanyabsprip/tmux-harpoon) | 81 | Numbered bookmarks for sessions and panes | `switch-client` |

## Popups, keys, status, glue

| Tool | Stars | What it does | Mechanism |
|---|---|---|---|
| [tmux-floax](https://github.com/omerxx/tmux-floax) | 874 | Hidden scratch session in a popup; resize, fullscreen, embed into the layout, follows cwd | `display-popup` attaching a second session |
| [tmux-which-key](https://github.com/alexwforsythe/tmux-which-key) / [tmux-menus](https://github.com/jaclu/tmux-menus) | 325 / 542 | Menus of actions from YAML / menu trees | `display-menu` |
| [tmux-palette](https://github.com/eduwass/tmux-palette) | 409 | Raycast-style palette over JSON or line sources | popup + tempfile |
| [tmux-yank](https://github.com/tmux-plugins/tmux-yank) | 3.1k | System clipboard; `prefix-y` copies the current command line, `prefix-Y` the cwd | `copy-pipe-and-cancel` |
| [vim-tmux-navigator](https://github.com/christoomey/vim-tmux-navigator) | 6.3k | C-h/j/k/l across vim splits and panes | `if-shell` on `ps -t #{pane_tty}` |
| [tmux-logging](https://github.com/tmux-plugins/tmux-logging) | 1.3k | Log pane output, save history | `pipe-pane`, `capture-pane -S -` |
| [tmux-notify](https://github.com/rickstaa/tmux-notify) | 279 | Polls every 10 s for a prompt char, then notifies | `capture-pane` polling |
| [tmux-ssh-split](https://github.com/pschmitt/tmux-ssh-split) | 119 | A split reconnects to the same ssh host and cwd | process check + OSC 7 |
| [tmux-window-name](https://github.com/ofirgall/tmux-window-name) | 299 | Names windows from program and path | hooks + `rename-window` |
| catppuccin, dracula, tmux-powerline, tmux-cpu, prefix-highlight, mode-indicator | 3.2k, 857, 3.8k, 538, 674, 202 | Status bar themes and modules | formats + `#()` jobs |

tmux 3.7 (July 2026) added non-modal floating panes (`new-pane`); fzf 0.74 `--popup` prefers them.

## Command blocks and history (OSC 133)

| Tool | What it does |
|---|---|
| Warp blocks | Select a block with Cmd-Up, copy command or output, bookmark, filter lines, sticky header, red when failed, attach a failed block to the agent |
| kitty | Jump to prompt, last output in a pager, `copy_last_command_output` (0.45), click to move the cursor |
| Ghostty 1.3 | Jump to prompt, triple-click selects output, `notify-on-command-finish`, threaded scrollback search |
| Zellij 0.45 | `[` `]` jump, `m` selects a command with its output, `c` copies the last output |
| iTerm2 marks | Alert on next mark, limit Find to one command's output, pinned command header |
| [atuin](https://github.com/atuinsh/atuin) (31.8k) | History with cwd, exit, duration; Ctrl-R TUI; tmux popup mode; output capture and an MCP tool (18.21 to 18.23) |
| [navi](https://github.com/denisidoro/navi) (17.6k), Warp Workflows | Cheatsheet commands with fill-in arguments |

## Completion popups

Fig became Amazon Q CLI, then [Kiro CLI](https://kiro.dev/docs/upgrade-guides/migrating-from-q/)
(2025-11-17, closed source). A wrapper process reads the typed line through OSC 697 markers and
places the popup with the macOS Accessibility API. Specs live in
[withfig/autocomplete](https://github.com/withfig/autocomplete) (25.2k, frozen).
[inshellisense](https://github.com/microsoft/inshellisense) (10.7k) draws the same specs inside the
terminal. This is the most expensive idea in the survey.

## Agent-era tools

| Tool | Stars | What it does | State source |
|---|---|---|---|
| [cmux](https://github.com/manaflow-ai/cmux) | 27.4k | Blue ring on the pane, lit tab, Cmd-Shift-U jumps to the latest unread; ships a fake `tmux` for Claude teammates | OSC 9/99/777 + `cmux notify` |
| [herdr](https://github.com/herdrdev/herdr) | 40.8k | working / blocked / idle / done per pane, rolled up to tab and workspace | hooks, then process tree + TOML screen rules + title + OSC 9;4 |
| [opensessions](https://github.com/Ataraxy-Labs/opensessions) | 1.2k | Sidebar with branch, ports, per-thread agent state; HTTP API on :7391 | watches transcript files |
| [tmux-agent-status](https://github.com/samleeney/tmux-agent-status) | 283 | Glyph per agent, switcher, `prefix+N` to the next waiting agent | hooks write status files |
| [tmux-claude-hatch](https://github.com/craftzdog/tmux-claude-session-manager) | 393 | Claude per project in popups, agent picker with live preview | hooks + `claude agents --json` |
| [claude-squad](https://github.com/smtg-ai/claude-squad) | 8.5k | Agents in tmux sessions plus git worktrees | tmux sessions |
| [iTerm2 3.7](https://iterm2.com/documentation-session-status.html) | | Session status via OSC 21337, Cockpit, Claude Code hooks | escape code + hooks |
| Warp, Wave | | Per-tab agent icon; `wsh badge` rolls up to the tab | OSC 777 JSON / CLI |

Every product converged on the same four pieces: per-pane agent state fed by hooks first and screen
matching last, a ring or badge on the pane that needs you, a key that jumps to the next waiting
pane, and a way to attach a command's output to an agent (Warp Cmd-Up, iTerm2 "Explain Output",
VS Code `#terminalLastCommand`, Zed and Wave selections).

## `@` pickers in agent CLIs

- Claude Code has a built-in index and a `fileSuggestion` setting that runs your own command
  (https://code.claude.com/docs/en/settings-reference#filesuggestion). A subagent read the contract
  as `{"query"}` JSON on stdin and paths on stdout; the docs page only confirms the setting exists.
- Codex walks with the `ignore` crate and scores with `nucleo`, limit 20, no recency
  (`codex-rs/file-search/src/lib.rs`).
- opencode and Pi use [FFF](https://github.com/dmtrKovalenko/fff) (10.9k): frecency from git
  history, git-status boosts, typo tolerance. zz's desktop file picker already uses `fff-search`.
- Only tmux-file-picker adds the `@` prefix at the terminal level, and it always adds it. Nobody
  checks whether the foreground program is an agent first.

# What zz has today

Checked in source at `ec31d080`.

| Piece | State | Where |
|---|---|---|
| `display-popup`, `display-menu`, `command-prompt`, `confirm-before`, `choose-tree/buffer/client`, `display-panes` | Daemon-owned; desktop, web/iOS, and TUI all render them (FFI lacks popup state) | `crates/zz-daemon/src/daemon.rs`, `crates/zz-client/src/core.rs`, `crates/zz-client/src/menu.rs` |
| Command palette with `:` `@` `%` `~` modes | Desktop, web, iOS; not TUI | `crates/zz-ui/src/command/palette_view.rs`, `knowledge/concepts/command-palette.md` |
| File picker | Desktop only, behind `editor-pane`/`agent-pane`; `fff-search` 0.10.6 + `neo_frizbee` 0.11; walks the local disk | `crates/zz/src/file_picker.rs` |
| Fuzzy matchers | Five, none shared: palette, command completion, agent completion, daemon `fuzzy_match_columns`, file picker | `palette_model.rs`, `completion.rs`, `agent_completion.rs`, `zz-mux/src/formats.rs` |
| Shell integration | zsh, bash, PowerShell; **no fish** | `crates/zz-terminal/src/shell_integration.rs` |
| OSC 133 marks, exit status, last command + output | Yes; `#{pane_last_command_status}`, `show-last-output`, `send-last-output` (`prefix e`) | `crates/zz-terminal/src/session.rs` (`capture_last_command`) |
| Older command blocks | Only through copy-mode `next-prompt`/`previous-prompt` | |
| Current input-line text and cursor offset | Not exposed | |
| Pane cwd | OSC 7 stored; `#{pane_current_path}` from the foreground process | `daemon.rs` `terminal_working_directory` |
| Typing into a pane | `send-keys -l`, `paste-buffer`, `send-text`, `run-pane` | `daemon.rs`, `zz-terminal/src/interaction.rs` |
| Hint / quick-select mode | Missing; only one-character `jump-*` | |
| URL detection | Pointer hover only (OSC 8 + plain URIs), then `OpenUri`; TUI ignores `OpenUri` | `session.rs` `hover_link_at`, `plain_uri_at` |
| Which-key | Missing; key tables are published (`KeyTablesChanged`) and `PrefixArmed` exists | `knowledge/designs/client-core-and-contract.md` names it as a goal |
| Floating panes | Missing by design (`new-pane` unimplemented, gap `pane.floating-model`) | `crates/zz-protocol/src/catalog.rs` |
| Hooks, `run-shell`, `if-shell`, `@options`, formats, control mode | Full; `zz events` streams hooks as JSON lines | `zz-mux/src/command.rs`, `crates/zz-cli/src/events.rs` |
| TPM and stock plugins | Work through the `tmux` wrapper on PATH; tpm, sensible, vim-tmux-navigator, yank, resurrect, continuum, fpp, oh-my-tmux in the compat corpus. tmux-fzf and sessionx untested | `compat/fetch-corpus.sh`, `compat/scenarios/smoke/plugin-runtime-*.txt` |
| Agent state, respond, context | `#{agent_state}`, `agent-respond`, `agent-send --context PATH:START-END`; no `@` mentions in the agent composer | `crates/zz-daemon/src/status.rs`, `crates/zz-client/src/agent_completion.rs` |

# Ideas for zz, ranked

Most picker plugins share one shape: a source feeds a fuzzy list, and an action runs on the pick.
Sources are files, dirs, screen tokens, URLs, history, command blocks, and projects. Actions are
insert at cursor, copy, open, and send to an agent. Built as one overlay with pluggable sources,
ideas 1, 3, 6, and 7 below become one feature.

1. **Path picker at the cursor.** Rooted at the pane's cwd, inserts `@path` when the foreground
   program is claude, codex, or opencode and a shell-quoted path otherwise, via bracketed paste when
   the program enabled it. The index has to live in the daemon: for an ssh host, the files sit
   where that host's daemon runs, and `file_picker.rs` only walks the local disk. Could also ship a
   `zz file-suggest` that speaks Claude's `fileSuggestion` contract so Claude's own `@` ranks the
   same way.
2. **Hint mode.** Labels over paths, SHAs, URLs, IPs, and `file:line` on the visible screen. Copy,
   insert at the prompt, open (`file:line` in an editor pane, URL in a browser pane), send to an
   agent. Drawn natively, the pane never gets swapped out.
3. **Screen token picker.** extrakto as a second source of idea 1: tokens from this pane or the
   whole window, newest first.
4. **Command blocks.** Jump between prompts, select a block, red gutter on failure, sticky header,
   copy, rerun, send to an agent. Extends `capture_last_command` to any block.
5. **Agent inbox.** Ring on the waiting pane, a toast with Allow/Deny over `agent-respond`, a
   next-waiting key, OS notifications. The cheapest idea, since `agent_state` exists.
6. **History overlay.** Every OSC 133 run with cwd, host, exit, duration, pane, and its captured
   output; filters for this dir, failed only, all hosts; past agent prompts as another source.
7. **Directory mode in the palette.** A `/` mode over frecent cwds the daemon has seen, repos, and
   worktrees, with new session, split, or window as actions. The palette has no directory source
   today.
8. **Floating scratch pane.** Per session, follows the focused pane's cwd, docks into the layout.
   Implementing tmux 3.7 `new-pane` would get this and tmux compat together.
9. **Which-key after prefix.** A sheet of the next keys from the live key table, grouped, with the
   user's own bindings labelled by `bind -N`.
10. **Split that keeps context.** A new split rejoins the same ssh host, container, or venv at the
    remote cwd, read from the foreground command.

Runners-up: iTerm2-style triggers (regex on output runs an action), copying the current command
line (tmux-yank `prefix-y`), a unicode picker, and a Fig-style completion popup (large build).

# Traps

- fzf 0.74+ `--popup` checks `tmux list-commands new-pane`. Run against the installed zz
  (`tmux 3.8-zz`), it exits 1 with `unknown command: new-pane` and fzf falls back to
  `display-popup`. Keep `new-pane` out of `list-commands` until floating panes exist.
- Claude Code sends desktop notifications only to iTerm2, Ghostty, and kitty under `auto`, and OSC
  9;4 progress only to ConEmu, Ghostty 1.2+, and iTerm2 3.6.6+
  (https://code.claude.com/docs/en/terminal-config). Codex sends OSC 9 to a fixed list. Rely on
  hooks, and parse OSC 9;4 before treating other OSC 9 payloads as notifications.
- Kiro autocomplete breaks inside multiplexer panes when `Q_TERM` is inherited
  (https://github.com/herdrdev/herdr/discussions/1895). Scrub it per pane with the rest of the
  agent environment.
- Reading the typed command line off the screen picks up autosuggestion ghost text and right-side
  prompts. Use the OSC 133 input boundaries.

# Open questions

- Superlogical's trigger key, navigation keys, and whether it completes remote paths (video not
  watched).
- Whether the overlay should be a daemon-owned state (every client renders it, like choosers) or a
  client-native view fed by daemon sources (richer on desktop, per-client work).
- One matcher for all five call sites, or keep them separate and add a sixth.
- Claude Code's built-in `@` ranking is only described in changelog entries.

# Sources

- almonk post: https://x.com/almonk/status/2103572918680645870
- Superlogical: https://www.superlogical.com/, https://mitchellh.com/writing/superlogical
- kitty choose-files and hints: https://sw.kovidgoyal.net/kitty/kittens/choose-files/, https://sw.kovidgoyal.net/kitty/kittens/hints/
- WezTerm QuickSelect: https://wezterm.org/quickselect.html
- tmux-file-picker: https://github.com/raine/tmux-file-picker
- Claude Code settings: https://code.claude.com/docs/en/settings-reference#filesuggestion
- Codex file search: https://github.com/openai/codex/blob/main/codex-rs/file-search/src/lib.rs
- Remaining tools are linked inline in the tables above.
