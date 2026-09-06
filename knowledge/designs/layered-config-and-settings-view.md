---
type: Design Plan
title: Layered configuration & native settings view
description: The decision record behind zz/config, daemon-owned keys crossing the wire, and native settings with split shortcut controls and a mux.conf editor.
status: Complete
tags:
- configuration
- settings
- ui
- theme
- provenance
- design-plan
timestamp: 2026-07-30T00:00:00Z
last_updated: 2026-09-06
---

# Overview

`zz/config` is zz's whole application configuration surface. Every knob zz honors has a spelling in
that one flat file, whether the value is consumed by the GUI client or by the daemon. The native
settings view keeps structured controls for app-level and terminal-appearance choices; Multiplexer
exposes split shortcut controls above the separate zz-owned `mux.conf` editor. Users import Ghostty
appearance on request; the daemon reads tmux config files in place before zz overrides.

This document records the shape and the reasoning. Current grammar and key tables live in
[Application configuration](/configuration/app-config.md),
[Terminal appearance](/terminal/appearance.md), and [tmux config parsing](/tmux/conf-parser.md).

# Resolution model

One resolver per domain produces `(effective value, provenance)`. The app consumes both where
needed. Structured client-local and Terminal Settings rows display provenance. Multiplexer's split
rows read the effective prefix table; its full-file editor covers the remaining commands and options.

| Domain | Ladder | Provenance tiers |
|--------|--------|------------------|
| Client-local knobs | built-in default < `zz/config` | `Default`, `Override` |
| Terminal appearance (`AppearanceConfigKey::ALL`) | built-in defaults < theme file named by a `theme` override < the rest of the `zz/config` override set | `Default`, `ThemeFile`, `Ghostty`, `Override` |
| Mux options | defaults < tmux files (or explicit `-f` roots) < `zz/mux.conf` < `zz/config` override < runtime command | `Default`, `TmuxConfig`, `Override`, `RuntimeCommand` |

Client-local provenance is `Override` whenever the key is *present*, even if its value is invalid, so
Reset can delete a stale bad line instead of presenting it as absent.

Appearance provenance is daemon-resolved and arrives as a complete per-key map. The daemon emits
three of the four tiers (`Default`, `ThemeFile`, `Override`); `Ghostty` belongs to the client-side
import loader, which reads a donor file in-process and publishes no map.

`zz/config` stays a single flat `key = value` file under a bounded-parsing contract: 64 KiB cap,
validated ranges, warn-and-keep on invalid lines, 500 ms poll. No second format, no sections, no
TOML. Value grammars are **reused** . appearance keys feed the existing `ConfigLoader::apply`
machinery, mux keys map onto the already-supported `set-option` arms . so parsing lives in the one
place that already owns each grammar.

## Knob surface

| Group | Owner | Where the grammar lives |
|-------|-------|-------------------------|
| Window chrome, pane geometry, widget radius, theme mode, paired `chrome-preset`, six optional `chrome-*` roots | GUI client | `crates/zz/src/config/mod.rs` |
| Browser-local element-selector hotkey | GUI client | `crates/zz/src/config/mod.rs` |
| Repeatable `chrome-keybind` / `chrome-unbind` overrides for `ui`, `sidebar`, `browser`, and `terminal` actions | GUI client | `crates/zz/src/config/mod.rs` + `crates/zz-client/src/chrome.rs` |
| Three ACP launch keys (`agent-command`, `agent-claude-code-command`, `agent-working-directory`) | GUI client | `crates/zz/src/config/mod.rs`; file-only, no settings row |
| Terminal appearance, including face-specific `font-family*` stacks, `font-feature`, synthetic/thickening policy, colors, palette, padding, policy, `theme`, and the `zz-*` extension keys | daemon | `crates/zz-terminal/src/appearance.rs` |
| `prefix`, `mode-keys`, `history-limit`, `word-separators`, `copy-command`, `set-clipboard`, `buffer-limit`, `synchronize-panes` | daemon | `crates/zz-mux/src/command.rs` |

Daemon-owned pane key tables use tmux config files and zz-owned `zz/mux.conf` overrides. Settings
offers shortcut and pane-type controls for Split below and Split right, with custom commands left
in the text editor. See [Application configuration](/configuration/app-config.md) for the current behavior.
Client-owned chrome gained repeatable file-only overrides through `chrome-keybind` and
`chrome-unbind`, resolved by `ChromeKeymap`. The browser element-selector shortcut remains a
dedicated validated scalar because Settings exposes that one action directly.

The `zz-*` appearance keys exist because zz's own daemon-side knobs previously had nowhere native to
live and were smuggled into the *Ghostty* file. They are `zz/config`-native now; the Ghostty parser
still accepts their spellings, which is what makes a donor file importable.

# Daemon overrides cross the wire, not the filesystem

`zz/config` is client-local by design: **the daemon never reads it.** The client polls the file every
500 ms and sends the daemon-owned subset as `ProtocolMessage::SetConfigOverrides { entries }`,
carrying ordered raw `(key, value)` pairs. The daemon partitions them into the appearance and mux
subsets, stores the latest of each, and logs any key it does not recognize.

- Appearance entries are the final pass of every appearance derivation. The daemon rebuilds from a
  fresh defaults base each time, so an **empty set is meaningful**: it restores built-in defaults.
- Mux entries dispatch the global `set-option` arms in file order after every startup,
  `reload-config`, or `source-file` replay. Invalid values warn and are skipped without stopping
  later entries.
- Overrides are a stored set replayed after every derivation, never a one-shot command. That is what
  keeps a `reload-config` from silently reverting a GUI choice.

Sending raw pairs (rather than typed values) keeps validation in the daemon-side loader and command
engine that already own those grammars, and lets the client stay ignorant of appearance semantics.

This is also the remote-attach-clean shape: under
[scene-streaming remote attach](/designs/scene-streaming-remote.md), a client's local preferences
still arrive as overrides against whatever the remote daemon resolves.

# Import, not adoption

Appearance remains import-and-own: the Ghostty loader serializes concrete appearance values into
`zz/config` with donor-wins replacement. The first-run prompt offers this appearance import when a
Ghostty config exists.

The previous tmux stance said "tmux is copied verbatim to the zz-owned zz/mux.conf". Pinned tmux
`Makefile.am` defines `/etc/tmux.conf:~/.tmux.conf:$XDG_CONFIG_HOME/tmux/tmux.conf:~/.config/tmux/tmux.conf`,
and `cfg.c::start_cfg` loops over all expanded candidates. A first-existing-file copy loses those
layers and drifts from files edited by plugin managers and `source-file` bindings.

zz now reads those tmux files in place in the same order, followed by the selected `zz/mux.conf`.
Explicit `-f` files replace the tmux candidate list while `zz/mux.conf` still loads last. The tmux
copy helper is removed; import entry points explain the in-place behavior. This was decided
2026-09-05 by fabrico for cycle 15; reversible. Neither import flow writes a donor file.

# Settings view

`crates/zz/src/config/settings.rs` renders the `WorkspaceRoute::Settings` route in the main window
(`Cmd+,` / `Ctrl+,`) from zz-ui form widgets, per [UI conventions](/configuration/ui-conventions.md).
`SettingsSection::ALL` has ten pages: Interface, Status bar, Editor, Panes, Multiplexer, Browser,
Terminal, Hosts, System, About, arranged in the Appearance, Tools, and Advanced sidebar groups.
About sets nothing: it shows the mark, the version, the platform and gpui revision this build carries
(copyable as one line for a bug report), and links to the repository, releases, and a new issue. See
[app-config](/configuration/app-config.md) for the page map.

Three rules hold the design together:

- **The file is the only apply path.** Controls perform comment-preserving minimal line edits
  (replace the value bytes in place, append when absent, atomic temp-file + rename) and then rely on
  the existing 500 ms poll → global swap → refresh. Hand edits and GUI edits share one code path;
  there is no parallel in-memory apply channel to drift.
- **The writer edits the last occurrence**, matching the parser's later-entry precedence. Cumulative
  appearance keys are replaced as whole reset-led groups during Ghostty import.
- **Whole-file mux edits are explicit.** Multiplexer keeps its buffer local until Save, enforces the
  1 MiB parser bound, and replaces the target atomically.

Terminal restores the structured five-group appearance surface: effective daemon values,
per-key provenance, bounded inputs, palette swatches, and Reset controls all write through the
comment-preserving `zz/config` writer. Multiplexer mounts the bounded `zz/mux.conf` editor with no
line-number gutter and compact 12px text; it can replace the file verbatim from tmux. A clean mux
editor reloads when entered, while import confirmation warns before discarding a dirty buffer.

Appearance and Terminal are long enough that mounting every off-screen control makes wheel-event
layout proportional to the whole page. They therefore describe their content as individual rows
and render it through GPUI's variable-height `ListState`: only rows in the viewport plus a small
overdraw are constructed, while a uniform height hint gives the scrollbar a useful extent before
each row has been measured. The page-keyed list state retains scroll position independently for
each section. Short pages keep the simpler ordinary scroll column. GPUI's list honours only the
vertical padding of its own style and places every row at its left edge, so a virtualized page
carries the page gutter and the bounded, centered content column *per row*; otherwise its cards sit
flush against the window edge while the scrolled pages stay inset.

The Settings view also defers section-owned state. Its initial Appearance page does not construct
the Terminal input/select/color-picker graph or subscribe all of those controls, and it does not
mount the `mux.conf` editor. Each is initialized once, immediately before its section is first
shown, and then retained so edits and focus survive navigation.

# Hard parts & risks

| Risk | Detail | Mitigation |
|------|--------|------------|
| Override replay ordering | A `reload-config` replays `zz/mux.conf` and must re-apply overrides, or a reload reverts GUI choices | Overrides are a stored set applied as the final pass of *every* derivation, never a one-shot command |
| Echo loops | GUI write → poll → `SetConfigOverrides` → `AppearanceChanged` → theme refresh could re-trigger work | Appearance application is idempotent; the daemon skips the broadcast when the resolved result is unchanged |
| Multi-client fights | Two attached clients with different `zz/config` files both push overrides | Last-writer-wins; per-client overrides become real under [scene-streaming remote attach](/designs/scene-streaming-remote.md) |
| Comment-preserving writer | Repeated keys, inline comments, and `key=value` spacing variants make in-place editing fiddly | Edit the last occurrence, keep unknown lines byte-identical, cover with fixture tests |
| Schema creep | A settings GUI invites unbounded knob growth | Every knob keeps the bounded contract: validated range, warn-and-keep, documented; no key ships without a consumer |
| A preset must survive both palettes | The light and dark built-ins run their elevations in opposite directions, so one fixed preset cannot follow System correctly | Persist one preset family with paired variants; resolve base < active variant < explicit roots, and keep the last actual OS mode separate from any pin |

# Non-goals

- **Writing to** the user's Ghostty or tmux files, ever.
- **A new theme format**: no Zed-theme loading; Ghostty theme files remain the palette source.
- **A general keybinding editor** in the settings view; the single browser-local selector shortcut
  remains a scalar setting.
- **Live donor adoption**: donors are snapshotted by import, not tracked.

# Open questions

- Do the compatibility spellings for the native `zz-*` keys in a Ghostty file deprecate in a future
  release, or stay accepted indefinitely?
- Should the daemon watch `zz/mux.conf` the way the client watches `zz/config`, retiring the manual
  `reload-config` step?

# Related

- [Application configuration](/configuration/app-config.md) . the `zz/config` surface and its keys
- [Terminal appearance](/terminal/appearance.md) . the daemon-side appearance resolver
- [tmux config parsing](/tmux/conf-parser.md) . the `zz/mux.conf` tier
- [UI design conventions](/configuration/ui-conventions.md) . component and token rules the settings view follows
- [Wire protocol](/protocol/wire-protocol.md) . carries `SetConfigOverrides` and both provenance maps
- [Scene-streaming remote attach](/designs/scene-streaming-remote.md) . the remote topology the wire-based override path is designed for
