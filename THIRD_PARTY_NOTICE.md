# Third-party notice

zz is licensed `MIT OR Apache-2.0` ([LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE)).

This file records what zz took from other projects, what it bundles, and what it only read.
Each entry says which of the three it is, because the obligations differ. Provenance lives next
to the code it describes; this is the index, and the linked files are the record.

## Forked and vendored source

Code that lives in this repository or is built from a pinned fork. These are the entries that
carry license obligations into zz's own builds.

| Project | What zz carries | License | Record |
| --- | --- | --- | --- |
| [gpui / gpui_platform](https://github.com/zed-industries/zed) (Zed Industries) | The UI framework, consumed through the `demfabris/zed` `zz-patches` branch: around 30 carried patches pinned by revision. | Apache-2.0 | [`scripts/forks.conf`](scripts/forks.conf), [gpui revision](knowledge/references/gpui-revision.md) |
| [gpui-component / gpui-kit](https://github.com/longbridge/gpui-kit) (Longbridge) | `crates/zz-ui`, a full fork of the widget layer taken at `b004e595`. Upstream is no longer a dependency. `crates/zz/src/window/frame.rs` is adapted from its Linux client-side window border. | Apache-2.0, © 2024–2025 Longbridge | [`crates/zz-ui/LICENSE-APACHE`](crates/zz-ui/LICENSE-APACHE), per-module port notes in [`crates/zz-ui/UPSTREAM.md`](crates/zz-ui/UPSTREAM.md) |
| [libghostty-rs](https://github.com/uzaaft/libghostty-rs) (Uzaaft) | A source snapshot of `libghostty-vt-sys` 0.2.1 at `46a9d2ac`, patched over the published crate to build the Zig 0.16 VT library. | MIT OR Apache-2.0 | [`third_party/rust/libghostty-vt-sys/`](third_party/rust/libghostty-vt-sys/UPSTREAM.md) |
| [Ghostty](https://github.com/ghostty-org/ghostty) (Mitchell Hashimoto and Ghostty contributors) | The VT state machine itself, compiled from Ghostty's Zig source through the snapshot above. Separately, `crates/zz-terminal/src/x11-rgb.txt` is copied from Ghostty's `src/terminal/res/rgb.txt` at `cf60af28`, which sources it from the X.Org `rgb` project. | MIT | [`third_party/ghostty-reference/`](third_party/ghostty-reference/UPSTREAM.md) |

zz builds against the Apache-2.0 `gpui` and `gpui_platform` crates only. No GPL-licensed Zed
code is linked into any zz binary.

## Bundled binaries

| Project | What ships | License |
| --- | --- | --- |
| [Chromium Embedded Framework](https://bitbucket.org/chromiumembedded/cef) (Marshall A. Greenblatt, portions © Google) | Browser panes. Release builds bundle a pinned CEF minimal distribution, verified by SHA-1 against CEF's published index before extraction. | BSD-3-Clause, retained at [`third_party/cef/LICENSE.txt`](third_party/cef/LICENSE.txt). The distribution carries Chromium's own licenses, which are bundled with it. |
| [cef-rs](https://github.com/tauri-apps/cef-rs) (Tauri) | The Rust bindings over CEF, as a normal crate dependency. | MIT OR Apache-2.0 |

Pinned artifacts, target matrix, and hashes: [`third_party/cef/ARTIFACTS.md`](third_party/cef/ARTIFACTS.md).

## Rust dependencies

Around 1,085 package versions resolve into the workspace graph across every platform and
feature. They are not listed individually here; the lockfiles are the record, and the current
spread is reproducible:

```bash
cargo metadata --format-version 1 --all-features \
  | jq -r '.packages[] | "\(.license // "UNKNOWN")\t\(.name) \(.version)"' | sort -u
```

The distribution is overwhelmingly permissive: 952 of the 1,085 are `MIT`, `Apache-2.0`, or a
dual of the two, with smaller groups under `Unicode-3.0`, `BSD-2/3-Clause`, `ISC`, `Zlib`,
`0BSD`, `Unlicense`, and `CC0-1.0`. Seven packages are MPL-2.0 (`cbindgen`, `cssparser`,
`cssparser-macros`, `dtoa-short`, `dwrote`, `option-ext`, `selectors`), all reached through
gpui and all used unmodified, so MPL's file-level reciprocity is satisfied by leaving them
alone. Nothing in the graph is GPL or AGPL.

A few dependencies are worth naming because they come from neighboring projects rather than
from the general crate ecosystem:

| Crate | Source | License |
| --- | --- | --- |
| `portable-pty` | [wezterm](https://github.com/wezterm/wezterm) | MIT |
| `agent-client-protocol` | [Agent Client Protocol Rust SDK](https://github.com/agentclientprotocol/rust-sdk) | Apache-2.0 |
| `merman` | [Latias94/merman](https://github.com/Latias94/merman), Mermaid rendering | MIT OR Apache-2.0 |
| `wgpu` | [gfx-rs/wgpu](https://github.com/gfx-rs/wgpu) | MIT OR Apache-2.0 |

## Fonts

All bundled fonts are under the SIL Open Font License 1.1, with the full text retained beside
each face.

| Font | Ships in | Copyright |
| --- | --- | --- |
| [0xProto](https://github.com/0xType/0xProto) | iPhone and iPad client | 0xType Project Authors |
| [Fira Code](https://github.com/tonsky/FiraCode) | iPhone and iPad client | The Fira Code Project Authors |
| [Geist Mono](https://github.com/vercel/geist-font) | iPhone and iPad client | The Geist Project Authors (Vercel) |
| [IBM Plex Sans](https://github.com/IBM/plex) | Browser client | IBM Corp. |
| [Lilex](https://github.com/mishamyrt/Lilex) | Browser client | The Lilex Project Authors |
| [Noto Sans CJK, Noto Color Emoji](https://github.com/notofonts) | Browser client | Google |
| [Inter](https://github.com/rsms/inter) | UI showcase | The Inter Project Authors |
| [Fantasque Sans Mono](https://github.com/belluzj/fantasque-sans) | zzmux.sh | Jany Belluz |

The desktop clients ship no fonts; they render with the system text stack.

## Icons

| Set | Used for | License |
| --- | --- | --- |
| [Tabler Icons](https://tabler.io/icons) | Every glyph in the app, the UI showcase, and the site. Two copies: `crates/zz-ui/assets/icons`, which the app and the showcase both render from, and `site/src/icons`, which the Astro site inlines at build time. A handful are locally redrawn, noted in the zz-ui port table. | MIT, © 2020–2026 Paweł Kuna. Retained as `LICENSE-TABLER` beside each copy. |
| [Simple Icons](https://simpleicons.org) | The vendor brand marks `openai.svg` and `claude.svg`. | CC0-1.0 |

## Color schemes

**Terminal themes.** The iPhone and iPad client bundles 451 terminal color schemes in Ghostty's
theme format, from [iTerm2-Color-Schemes](https://github.com/mbadolato/iTerm2-Color-Schemes)
(MIT, © 2011 to present Mark Badolato). License retained at
[`clients/ios/Resources/Licenses/iTerm2-Color-Schemes.txt`](clients/ios/Resources/Licenses/iTerm2-Color-Schemes.txt).

**Chrome presets.** The 34 window-chrome presets in `crates/zz-client/src/chrome_palette.rs`
take their names and starting colors from published community palettes, then nudge each one in
Oklab to clear zz's 7:1 text and 3:1 edge contrast floors. The values are re-derived rather than
copied, but the palettes are recognizably theirs and the names are used as they were published:

[Tokyo Night](https://github.com/enkia/tokyo-night-vscode-theme) ·
[Catppuccin](https://github.com/catppuccin/catppuccin) ·
[Gruvbox](https://github.com/morhetz/gruvbox) ·
[Nord](https://github.com/nordtheme/nord) ·
[Dracula](https://github.com/dracula/dracula-theme) and its light Alucard ·
[One Dark / One Light](https://github.com/atom/one-dark-syntax) ·
[GitHub](https://github.com/primer/primitives) ·
[Everforest](https://github.com/sainnhe/everforest) ·
[Rosé Pine](https://github.com/rose-pine/rose-pine-theme) ·
[Solarized](https://github.com/altercation/solarized) ·
[Ayu](https://github.com/ayu-theme/ayu-colors) ·
Breeze (KDE) ·
Adwaita (GNOME) ·
Ubuntu (Canonical) ·
macOS Classic (Apple Terminal)

## Protocols and formats

zz speaks these; it ships none of their implementations.

- **tmux command language, control mode, and key tables.** zz's multiplexer is a Rust
  implementation checked against tmux at `d77c9dc6`. See the reference section below.
- **[Agent Client Protocol](https://agentclientprotocol.com)** for agent panes, through the
  Apache-2.0 Rust SDK.
- **Claude Code's peer bus.** The daemon speaks the wire format so agent panes register as
  peers and `agent-send` reaches a Claude Code session. None of Anthropic's code is copied, and
  the daemon hosts no vendor's agent loop.
- **Google Chrome's on-disk profile, cookie, and history formats**, read-only, in
  `crates/zz-chrome-import`. Reading the format required the platform keychain and DPAPI
  storage-key handling; no Chrome code is used.
- **The Kitty graphics protocol** ([kovidgoyal/kitty](https://github.com/kovidgoyal/kitty)) and
  **OSC 52**, both implemented by the VT engine.

## Reference material

Read, pinned, and cited. None of it is compiled, linked, or shipped.

| Project | What it settles | License |
| --- | --- | --- |
| [tmux](https://github.com/tmux/tmux) | Command names, aliases, key-table behavior, format strings, and config syntax, pinned at `d77c9dc6`. No tmux C source is copied into the Rust implementation. | ISC, retained at [`third_party/tmux-reference/`](third_party/tmux-reference/UPSTREAM.md) |
| tmux plugin corpus: [tpm](https://github.com/tmux-plugins/tpm), [tmux-sensible](https://github.com/tmux-plugins/tmux-sensible), [vim-tmux-navigator](https://github.com/christoomey/vim-tmux-navigator), [tmux-yank](https://github.com/tmux-plugins/tmux-yank), [tmux-resurrect](https://github.com/tmux-plugins/tmux-resurrect), [tmux-continuum](https://github.com/tmux-plugins/tmux-continuum), [tmux-fpp](https://github.com/tmux-plugins/tmux-fpp), [Oh My Tmux](https://github.com/gpakosz/.tmux) | The alias compatibility suite runs real plugin initialization against zz at immutable revisions. | MIT, except Oh My Tmux which is MIT and WTFPLv2. Originals retained in [`third_party/tmux-plugin-corpus/`](third_party/tmux-plugin-corpus/UPSTREAM.md) |
| [Ghostty](https://github.com/ghostty-org/ghostty), [kitty](https://github.com/kovidgoyal/kitty), [Alacritty](https://github.com/alacritty/alacritty), [cmux](https://github.com/manaflow-ai/cmux) | Throughput and rendering baselines in `bench/`. | Respective upstream licenses |
| [DOOM-fire-zig](https://github.com/const-void/DOOM-fire-zig) | One of the benchmark fixtures, cloned at a pinned revision into a gitignored cache by `bench/gen-fixtures.sh`. | Upstream license |
| [rootshell](https://github.com/demfabris/rootshell) | Grouped settings, bundled fonts, and pane controls on the iPad client, read at `a5c318c5`. Its split view commits terminal-cell resizes to tmux; zz keeps that division through its own daemon commands. | MIT, © 2026 Rootshell LLC, Kit Knox, retained at [`third_party/rootshell-reference/`](third_party/rootshell-reference/UPSTREAM.md) |

## Prior art

Projects zz studied and wrote down before building the equivalent. No code was taken from any
of them. They are named because the surveys in `knowledge/research/` are part of the repository
and the ideas are not ours.

- [zenbu-labs/terminal-browser](https://github.com/zenbu-labs/terminal-browser) validated
  putting a real browser in a PTY, and its damage-driven frame transport shaped the TUI
  client's design.
- [anomalyco/opencode](https://github.com/anomalyco/opencode) is part of the
  [agent-harness rendering survey](knowledge/research/2026-08-15-agent-harness-rendering-survey.md).
- [herdrdev/herdr](https://github.com/herdrdev/herdr) is part of the agent-to-agent
  [messaging survey](knowledge/research/2026-09-10-agent-messaging-survey.md).
- [cmux](https://github.com/manaflow-ai/cmux) is the source of the pane drag-and-drop model zz
  measured its own against.

## Trademarks

tmux, Ghostty, Zed, Chromium, Google Chrome, Claude, Claude Code, OpenAI, and the other product
names here belong to their respective owners and are used only to say what zz is compatible
with or built on. zz is not affiliated with, endorsed by, or sponsored by any of them.

## Corrections

If something here is wrong, missing, or attributes your work incorrectly, open an issue at
<https://github.com/demfabris/zz/issues> and it gets fixed.
