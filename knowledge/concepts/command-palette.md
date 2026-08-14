---
type: Concept
title: Native command palette and tmux command completions
description: The native top-center GPUI command palette with catalog-driven tmux completions, value prompts, history, and daemon-owned execution.
resource: crates/zz/src/command/palette.rs
tags: [gpui, command-prompt, tmux, completions, ui]
timestamp: 2026-07-24T00:00:00Z
---

# Overview

`zz` exposes daemon-owned tmux `command-prompt` semantics (`C-b :`, plus templated prompts like
`C-b $` rename-session and `C-b ,` rename-window) through native GPUI chrome rather than terminal
cell content. The surface is a top-center floating palette built on zz-ui's `Input`, sourcing
completions from the shared tmux command catalog owned by [`zz-mux`](/crates/zz-mux.md), with the
daemon authoritative over parsing, template expansion, history, and execution.

# What it does

- Renders any daemon-issued `CommandPromptState` as one floating, centered surface instead of a
  bottom bar, without dimming or resizing the terminal/browser workspace beneath it.
- Distinguishes two prompt kinds: `Command` (a raw `command-prompt`, eligible for full catalog/
  history/target completion) and `Value` (a prompt whose input is substituted into a private `%%`
  template, e.g. rename flows) which gets the same input surface but zero command suggestions.
- Sources completions from one static, renderer-neutral catalog in `zz-mux`: canonical command name,
  tmux aliases, description, accepted flags/options, and a completion kind per option value
  (static enum like layouts/key-tables, dynamic like live sessions/windows/panes, or free-form/
  unsuggested). See [`zz-mux`](/crates/zz-mux.md) and the
  [tmux command catalog](/tmux/commands.md) for the catalog itself.
- Ranks suggestions deterministically: exact canonical match, canonical prefix, alias prefix,
  ordered fuzzy match, then description match. Once the command token is known, that set is
  restricted to flags not yet present, the enum values a preceding option requires, and live targets
  resolved from the current `MuxSnapshot`.
- Surfaces a bounded, deduplicated recent-command history ahead of catalog matches when the input is
  empty (`Command` prompts only; `Value` prompts never show history or catalog suggestions).

# How it is triggered

Any daemon flow that currently opens a `command-prompt` triggers the palette identically: `C-b :`
opens a bare `Command` prompt; tmux `command-prompt` supports `-b`/`-p`/`-I`/one `%%` template
argument (see the [tmux command set](/tmux/commands.md)); binding-driven
flows like the default `C-b $` (rename-session), `C-b ,` (rename-window), and sidebar `r` action
open a pre-filled `Value` prompt whose label and current value come from the daemon. The daemon remains the sole
owner of *when* a prompt opens, its label, its initial content and cursor, and (per prompt kind)
whether it carries a private substitution template; the client learns only the prompt's kind,
label, current text, cursor position, and bounded command history.

# How completions are sourced

```
zz-mux command catalog (static, renderer-neutral)
        │  canonical name + aliases + description + option/flag completion kinds
        ▼
Application completion engine (token-aware, over the current MuxSnapshot)
        │  recognizes: command token / option tokens / option values / quoted values / cursor token
        ▼
Ranked suggestions: exact → canonical-prefix → alias-prefix → fuzzy → description
        │  restricted to: unused flags, required enum values, live session/window/pane targets
        ▼
CommandPaletteView renders up to 8 visible 40px rows (scrollable beyond that)
```

The catalog is plain Rust data with no GPUI dependency, so it is unit-testable inside `zz-mux`
(every canonical name unique, every alias resolves to exactly one canonical command, static enum
values valid for their command) independent of rendering. Live target completions (sessions,
windows, panes) are resolved client-side from the same `MuxSnapshot` [`AppView`](/crates/zz.md)
already reconciles panes from. A target that disappears between suggestion and submission is
rejected by the daemon's existing target validation rather than trusted client-side.

# How it is rendered in GPUI

A dedicated `CommandPaletteView` entity owns
one `Entity<InputState>` (native selection/IME/clipboard/undo via `zz_ui::input::Input`), the
current prompt label and kind, computed suggestions, highlighted-suggestion/list-navigation-engaged
state, and the mux snapshot used for live targets. It uses zz-ui's `Input`, `ListItem`,
and `Kbd` primitives rather than `SearchableList` (which owns its own second search input; the
palette keeps focus in the single command input).

[`AppView`](/crates/zz.md) owns or reuses this entity and renders it as an absolute
overlay centered 20–24px below the title bar, capped at 560px wide with 24px side margins on narrow
windows, no backdrop dimming, a theme-aware layered shadow instead of a solid border, and
concentric 8px outer/4px inner corner radii around a 4px inset. Completion kinds render as small
neutral `Tag` components, and each completion row spans the list width so its selected state reuses
the sidebar tree's transparent-gray `background.hover()` fill without an outline. Opening animates a short
opacity/downward-to-rest transition keyed to the prompt instance so retyping does not restart it;
closing is a shorter upward nudge.

Keyboard/pointer contract: Up/Down engage and navigate the suggestion list; Tab accepts the
highlighted suggestion without submitting; Enter submits the input when list navigation has not
engaged, or accepts the highlighted suggestion once it has; clicking a suggestion accepts it and
keeps the palette open (unless it is a complete history command explicitly activated); Escape and
click-outside close it, consuming exactly one pointer event.

# Protocol shape

`CommandPromptAction` variants live under `zz_protocol::InputMessage`:

| Variant | Purpose |
|---------|---------|
| `Update { input, cursor }` | Persist local text + Unicode-scalar cursor after every accepted edit, without the daemon publishing an echo `CommandPrompt` event back (so `InputState` selection/undo is never reset by the client's own edits) |
| `Submit { input }` | Submit the complete current value through the existing daemon parse/template/execute path, guarding against a race between a final edit and execution |
| `Close` | Cancel and remove the daemon-side prompt |

`CommandPromptState { prompt, input, cursor, kind, history }` is published on open, explicit server
change, and resync. `kind` separates command and value prompts; `history` is populated only for
command prompts. The `Update`/`Submit`/`Close` actions and this state shape entered the wire at v32;
the live version is the `PROTOCOL_VERSION` constant in `crates/zz-protocol/src/message.rs`. See
[the wire protocol](/protocol/wire-protocol.md).

# Key files

| File | Role |
| --- | --- |
| `crates/zz/src/command/palette.rs` | Input, suggestion selection, history rendering, pointer dismissal, prompt synchronization |
| `crates/zz/src/command/completion.rs` | Tokenizes and ranks catalog, option, enum, and live-target completions against the current `MuxSnapshot` |
| `crates/zz-protocol/src/catalog.rs` | The renderer-free command catalog shared by execution parsing and UI completion; tests enforce unique canonical names, aliases, and options |
| `crates/zz-protocol/src/message.rs` | Prompt kind/history and the native edit/submit/close actions |
| `crates/zz-daemon/src/daemon.rs` | Prompt state, bounded history, template substitution, and final execution |

`AppView` gives the palette highest focus precedence and renders it as an overlay, so it never
resizes the pane workspace. The native choosers (`chooser/tree.rs`, `chooser/buffer.rs`) and
`pane/display.rs` are separate daemon-driven surfaces.

# Related

- [`zz` crate](/crates/zz.md) . `AppView` hosts the palette; `mux/client.rs` delivers
  `CommandPromptState`; `command/completion.rs` resolves suggestions.
- [`zz-mux` crate](/crates/zz-mux.md) . owner of the shared command catalog and command execution.
- [Tmux command catalog](/tmux/commands.md) . the catalog entries this palette's completions read.
- [Tmux compatibility](/tmux/tmux-compat.md) and [key tables](/tmux/key-tables.md) . the broader
  tmux-compatible surface `command-prompt` is one part of.
- [Wire protocol](/protocol/wire-protocol.md) . carries `CommandPromptState` and
  `CommandPromptAction`.
- [Split-pane layout](/concepts/split-pane-layout.md) . the workspace this overlay never resizes or
  dims.
