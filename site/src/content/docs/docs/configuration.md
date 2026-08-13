---
title: Configuration
description: Two config files, imported once, live-reloaded.
---

Two files, both plain text:

| File | Format | Covers |
| --- | --- | --- |
| `~/.config/zz/config` | Ghostty-style `key = value` | appearance: fonts, colors, themes, padding, opacity |
| `~/.config/zz/mux.conf` | tmux syntax | prefix, key bindings, mux options |

Both are picked up without a restart.

## One-shot import

On first launch zz offers to import what you already have:

- `~/.tmux.conf` is copied **verbatim** to `mux.conf`
- Ghostty appearance keys are parsed into `config`

Donor files are read once and never touched again; zz reads only its own
files at runtime.

## Ghostty compatibility

The appearance layer speaks Ghostty: `theme` files, per-style font stacks
with OpenType features, `minimum-contrast`, per-edge padding, the full
256-color palette, and OSC 10/11/12 color queries answered, so
terminal-aware TUIs derive their colors correctly.

## Where every value came from

The settings UI shows where every value comes from (default, theme file,
Ghostty import, or your override), and a config-file reload never silently
reverts a choice you made in the UI. Unsupported directives in `mux.conf`
are reported with a diagnostic, not ignored.
