---
title: Configuration
description: Appearance settings and tmux configuration with zz-specific overrides.
---

Keep zz-specific settings in two plain-text files:

| File | Format | Covers |
| --- | --- | --- |
| `~/.config/zz/config` | Ghostty-style `key = value` | appearance: fonts, colors, themes, padding, opacity |
| `~/.config/zz/mux.conf` | tmux syntax | overrides for prefix, key bindings, mux options |

Both are picked up without a restart.

## One-shot import

On first launch zz offers to import Ghostty appearance keys into `config`.

The daemon reads tmux configuration in place at startup: `/etc/tmux.conf`,
`~/.tmux.conf`, `$XDG_CONFIG_HOME/tmux/tmux.conf`, then `~/.config/tmux/tmux.conf`.
Your `zz/mux.conf` loads last. `zz -f <file>` replaces the tmux candidate list
while keeping that final zz layer. See [tmux compatibility](./tmux/) for details.

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
