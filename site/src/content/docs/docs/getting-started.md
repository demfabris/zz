---
title: Getting started
description: Install zz, learn the object model and the default keys, configure it, and build it from source.
---

zz is a terminal multiplexer whose panes are not all terminals. A pane can be a
shell or a full Chromium, and both live in one tmux layout, answer one prefix
key, and get drawn on one GPU surface. A background daemon owns the sessions, so
closing the window detaches instead of killing your work.

This page takes you from nothing installed to a workspace you can drive, then
covers the knobs and the build.

## Install

Built artifacts live on the
[releases page](https://github.com/demfabris/zz/releases). CI also builds every
platform on every push, so the artifacts of a green run give you the tip of
`main`.

### macOS

Apple Silicon, macOS 11 Big Sur or newer. The cask puts `zz.app` in
`/Applications` and `zz` on your `PATH`, so the window and the CLI come from the
same install:

```sh
brew install --cask demfabris/zz/zz
```

Intel Macs compile from source but nothing tests or ships them.

### Linux

Take `zz-linux-X64.AppImage`, mark it executable, run it:

```sh
chmod +x zz-linux-X64.AppImage
./zz-linux-X64.AppImage
```

Arch users can build a native package from the checkout with
`just pacman-package`, or `just pacman-install` to build and install in one
step.

Wayland is the most exercised host; X11 is compiled in and works. Chromium picks
the backend itself. You need unprivileged user namespaces enabled, because zz
never passes `--no-sandbox`.

### Windows

Windows 10 or newer, x64. Take `zz-windows-X64.zip` and unpack it anywhere, or
run the installer, which defaults to a per-user install under
`%LOCALAPPDATA%\Programs\zz` and needs no elevation.

:::caution[No `zz` on `PATH` yet]
The Windows bundle carries no separate launcher, so the installer adds no `PATH`
entry. `zz.exe` answers every CLI verb; you just have to call it by full path
until that lands.
:::

## First run

Launch it. There is no daemon to install and no service to enable: `zz` looks
for a socket, and if nothing answers within three seconds it starts a daemon
itself and attaches.

The socket lives at `$XDG_RUNTIME_DIR/zz/default.sock`, falling back to
`$TMPDIR/zz-$USER/default.sock`. Windows uses a named pipe,
`\\.\pipe\zz-<user>-default`. Set `ZZ_SOCKET` or pass `--socket <path>` to run a
second, isolated daemon.

If you have a `~/.tmux.conf` or a Ghostty config, zz offers once to bring them
over. Accepting copies your tmux file verbatim into `zz/mux.conf` and writes the
Ghostty appearance keys into `zz/config` as concrete values. Neither original is
touched, and the offer never returns. You can run either import again later from
Settings.

What you get is an empty workspace with the first few keys printed on it. Press
<kbd>Enter</kbd> for a session, and you are in a terminal.

Closing the window **detaches**. The daemon keeps every PTY, layout, and browser
URL alive, and relaunching puts you back where you were. The daemon does not
survive a reboot, and it shuts itself down once no sessions and no clients
remain.

:::note
With the tray icon on (the default), the window's close button hides zz to the
tray rather than detaching. Quit properly to detach.
:::

## The model

zz uses tmux's object graph, with tmux's sigils:

| Object | Sigil | What it is |
| --- | --- | --- |
| Session | `$0` | The top of the tree. Holds windows. |
| Window | `@1` | A *page* of panes with one layout, not an OS window. |
| Pane | `%2` | One terminal or one browser. |
| Split | `^3` | A node in the layout tree. Stable across relayouts. |
| Client | `c4` | One attached connection. A session can hold several. |

Three things surprise people coming from tmux:

- **The OS window shows one window of one session.** Switching mux windows
  repaints it. The sidebar tree is your window switcher, not a second desktop
  window.
- **There is no tab object.** Browser panes have tabs, but they belong to the
  pane and never appear in a `-t` target.
- **Machines sit above sessions.** Remote hosts get their own root in the
  sidebar tree, and they deliberately stay out of the target grammar.

IDs are stable for the daemon's lifetime, which is what makes scripting work.

## The keys

The prefix is <kbd>C-b</kbd>, same as tmux, and `set -g prefix C-a` moves it.
Pressing the prefix twice sends a literal one through to the shell.

<p class="zz-eyebrow">Windows</p>

| Key | Does |
| --- | --- |
| <kbd>c</kbd> | New window |
| <kbd>n</kbd> <kbd>p</kbd> <kbd>l</kbd> | Next, previous, last |
| <kbd>0</kbd>–<kbd>9</kbd> | Select window by index |
| <kbd>,</kbd> | Rename window |
| <kbd>&</kbd> | Kill window |

<p class="zz-eyebrow">Panes</p>

| Key | Does |
| --- | --- |
| <kbd>%</kbd> <kbd>"</kbd> | Split right, split down |
| arrows | Move focus |
| <kbd>o</kbd> <kbd>;</kbd> | Next pane, last pane |
| <kbd>q</kbd> | Number every pane for a second; click or type a digit |
| <kbd>z</kbd> | Zoom toggle |
| <kbd>M-</kbd>arrows | Resize by 5 cells (repeatable) |
| <kbd>C-</kbd>arrows | Resize by 1 cell (repeatable) |
| <kbd>{</kbd> <kbd>}</kbd> | Swap with the previous or next pane |
| <kbd>!</kbd> <kbd>x</kbd> | Break out to its own window, kill |
| <kbd>Space</kbd> | Cycle layouts |
| <kbd>E</kbd> | Spread evenly |
| <kbd>M-1</kbd>–<kbd>M-7</kbd> | The seven named layouts |

<p class="zz-eyebrow">Everything else</p>

| Key | Does |
| --- | --- |
| <kbd>[</kbd> | Copy mode |
| <kbd>]</kbd> <kbd>=</kbd> | Paste buffer, choose a buffer |
| <kbd>s</kbd> <kbd>w</kbd> | Focus the sidebar tree |
| <kbd>$</kbd> | Rename session |
| <kbd>:</kbd> | Command palette |
| <kbd>?</kbd> | Every binding, in a pager |
| <kbd>r</kbd> | Reload config |

A prefix key with nothing bound to it is swallowed, never typed into your shell.
<kbd>d</kbd> is unbound on purpose: closing the window is how you detach.

### Keys the app owns

These are compiled-in GPUI shortcuts rather than mux bindings, so `bind-key`
cannot reach them.

| macOS | Linux and Windows | Does |
| --- | --- | --- |
| `cmd-,` | `ctrl-,` | Settings |
| `cmd-=` `cmd--` `cmd-0` | `ctrl-…` | UI zoom, 50% to 300% |
| `cmd-f` | `ctrl-shift-f` | Find in the terminal |
| `cmd-c` `cmd-v` `cmd-a` `cmd-k` | `ctrl-shift-…` | Copy, paste, select all, clear scrollback |
| `ctrl-=` `ctrl--` | same | Terminal font size, across every terminal pane |
| `cmd-w` `cmd-q` | not bound | Close the active pane, quit |

Terminal panes also take <kbd>shift-PageUp</kbd> and <kbd>shift-PageDown</kbd>
for the scrollback, and mouse selection copies on release without entering copy
mode.

## Picking what goes in a pane

<kbd>C-b</kbd> <kbd>%</kbd> does not open a shell. It opens a **picker**: an
empty pane asking what you want in it. Press <kbd>t</kbd> for a terminal or
<kbd>b</kbd> for a browser. <kbd>Escape</kbd> closes the pane again.

This applies to key bindings only. `zz split-window` from a shell still gives
you a plain terminal, which is what scripts expect.

A browser pane runs a real Chromium off-screen and draws onto the same GPU
surface as your terminals. It takes the browser chords you already know:
`cmd-t` / `ctrl-t` for a tab, `cmd-l` / `ctrl-l` for the address bar, `cmd-r` /
`ctrl-r` to reload, `cmd-alt-i` / `ctrl-shift-i` for DevTools. Clicking a URL in
a terminal opens it in the nearest browser pane of the same window. See
[Browser panes](/zz/docs/browser/).

### Two more pane types, not ready yet

**Agent panes** will run Claude Code or Codex over the Agent Client Protocol,
with the conversation drawn natively. **Editor panes** will be a built-in text
editor with vim mode. Both are upcoming work, compiled out of every release
build, and neither appears in the picker until you turn it on yourself.

Following along means building with the cargo feature:

```sh
cargo build --features agent-pane,editor-pane
```

then setting `experimental-agent-pane = true` in `zz/config`, or using the
toggles under Settings → Advanced → System → Experimental. Without the cargo
feature the config key reads false whatever the file says. Expect rough edges;
see [Agent panes](/zz/docs/agents/).

## Driving it from a shell

The `zz` on your `PATH` is the same binary as the app. Any of the ~70 supported
commands works from any shell against the running daemon:

```sh
zz list-panes
zz send-keys -t %3 'make test' Enter
zz split-window -h
zz new-window -n logs
```

Inside a pane you can usually skip the target. Every terminal pane carries
`ZZ_PANE`, `ZZ_SESSION`, and `ZZ_SOCKET`, and the CLI resolves an untargeted
command against the pane that invoked it, exactly like tmux's `$TMUX_PANE`:

```sh
zz display-message -p '#S:#I.#P is #{pane_id}'
#=> 0:0.1 is %10
```

Targets take tmux syntax: `$session`, `@window`, `%pane`, plus compound forms
like `docs:main.0`. `-F` format strings work on `list-sessions`, `list-windows`,
and `list-panes`.

:::caution[`zz --help` does not exist]
Argument parsing is hand-rolled and there is no help text, no man page, and no
shell completions. `zz tools` prints a short catalog aimed at agents, the
command palette (<kbd>C-b</kbd> <kbd>:</kbd>) completes commands and targets
live, and the [CLI page](/zz/docs/cli/) is the reference.
:::

## Other machines

Add a host and its sessions join your sidebar tree next to the local ones:

```sh
zz fleet add desktop you@desktop
zz fleet list
zz fleet remove desktop
```

That writes one line into `zz/config`, which you can equally type yourself:

```
host-desktop = ssh://you@desktop
host-gpu     = ssh://gpu-box:2222
```

Click a session in the tree to attach, or target the host from a shell with
`zz --host desktop list-sessions`.

The transport is plain OpenSSH. zz shells out to `ssh`, probes for the remote
socket, starts a daemon over the same connection if none is running, and
forwards the socket with `ssh -L`. (Windows cannot forward a unix socket, so it
bridges a `zz proxy` over stdio instead.) Your keys, your agent, and your
`~/.ssh/config` aliases all apply. Nothing to pair, no port to open.

On the remote you need two things: ssh access, and `zz` on the login shell's
`PATH`. The protocol version has to match exactly on both ends.

Browser panes always render on your machine, but a pane opened while you are
attached to a remote host sends its traffic back out through that host, so
`localhost:3000` is the dev server *there*. The same ssh session does the
tunnelling. Set `browser-egress = false` to keep browsing local.

## Configuration

Two files, both plain text, both under a `zz/` directory in your config
location (`~/.config/zz/` on Linux, also
`~/Library/Application Support/zz/` on macOS, `%APPDATA%\zz\` on Windows):

| File | Grammar | Owns | Reload |
| --- | --- | --- | --- |
| `zz/config` | Ghostty-style `key = value` | Chrome, panes, browser, hosts, terminal appearance | Polled every 500 ms, applied live |
| `zz/mux.conf` | tmux commands | Prefix, key bindings, mux options, status line | Daemon start, and <kbd>C-b</kbd> <kbd>r</kbd> |

Neither file is created for you. zz runs on built-in defaults until you write
one, and deleting a file returns you to those defaults.

The fastest way in is the annotated sample, which lists every client-side knob
at its default value:

```sh
mkdir -p ~/.config/zz && cp examples/config ~/.config/zz/config
```

### The knobs you will actually reach for

| Key | Default | Does |
| --- | --- | --- |
| `theme-mode` | `system` | `system`, `light`, or `dark` |
| `chrome-preset` | unset | One of ten paired palettes: `tokyo-night`, `catppuccin`, `gruvbox`, `nord`, `breeze`, `adwaita`, `rose-pine`, `ayu`, `solarized`, `macos-classic` |
| `chrome-background` … `chrome-danger` | unset | Six palette roots. Everything else derives from them |
| `pane-gaps` | `false` | The card treatment. Off pins the next three to zero |
| `pane-margin` | `6` | Gap between panes and at the window edge, 0–32 |
| `pane-corner-radius` | `13.5` | 0–32 |
| `pane-border-width` | `1` | 0–8, zero disables |
| `widget-corner-radius` | `6` | Every button, input, menu, and dialog, 0–24 |
| `theme` | unset | A Ghostty theme file by name, or `light:a,dark:b` |
| `font-family` | platform default | Terminal font stack. `font-size` is `13` |
| `background-opacity` | `1.0` | Per-pane terminal opacity |
| `window-background-blur` | `false` | Blurred backdrop, where the compositor does it |
| `prefix` | `C-b` | The mux prefix |
| `mode-keys` | `emacs` | `vi` or `emacs` in copy mode |
| `history-limit` | `10000` | Scrollback lines, 0–1000000 |
| `set-clipboard` | `external` | OSC 52 policy: `on`, `external`, `off` |
| `tray` | `true` | Tray icon. Read once at startup |
| `quit-daemon-on-exit` | `false` | Kill sessions when the app quits |
| `browser-search-provider` | `google` | `google`, `duckduckgo`, or `brave` |
| `browser-egress` | `true` | Route a remote pane's traffic through its host |
| `show-fps` | `false` | Frame-rate readouts |

Terminal appearance uses Ghostty's spellings, so `theme`, `font-family`,
`font-feature`, `palette`, `cursor-style`, `minimum-contrast`, and per-edge
`window-padding-*` all behave the way your Ghostty config already does.

### How values resolve

Built-in default, then a `theme` file, then your `zz/config` line, then anything
you change at runtime. Later entries in a file beat earlier ones. A bad value
keeps the previous one and logs a diagnostic rather than failing the load.

Mux options are the interesting case: a `prefix = C-a` in `zz/config` outranks
`set -g prefix C-b` in `mux.conf`, whatever order they appear in, because
`zz/config` overrides are replayed after every mux load.

### Rebinding

Mux keys take tmux syntax in `zz/mux.conf`:

```sh
set -g prefix C-a
bind -n F1 select-window -t :1
bind -T copy-mode-vi v send-keys -X begin-selection
```

Two quirks to know before you file a bug. `bind-key` does not cluster its flags,
so write `-n -r` rather than `-nr`, and it wants `-T copy-mode-vi` with a space
rather than `-Tcopy-mode-vi`. Chords chain on a literal `\;`. Application
shortcuts (`cmd-,`, UI zoom, browser tabs) are compiled in and stay where they
are.

### Settings

<kbd>cmd-,</kbd> or <kbd>ctrl-,</kbd> opens Settings as a route inside the
window. Nine pages, grouped as Appearance, Tools, and Advanced. Two of them
(Terminal and Multiplexer) are full text editors over `zz/config` and
`zz/mux.conf`, with syntax highlighting and an Import button. Every structured
row shows where its value came from and offers a Reset that deletes the line.

## How it works

One binary plays four roles: the window, the daemon, the command line, and the
stdio proxy that carries a remote session. Which one you get depends on the
first argument.

**The daemon holds everything.** It owns the mux tree, every PTY, and the frame
fanout, and it is the only thing that knows what your sessions look like. The
window, the TUI client, and the iPad client are all clients of it, attaching over
the same socket with the same protocol. Several can attach to one session at
once, each with its own scroll position, selection, and copy-mode cursor over
shared panes.

**The wire carries changed rows.** Control messages travel one lane as compact
binary; terminal output travels another as packed row patches with shared style
and grapheme dictionaries. The daemon keeps exactly one pending frame per pane
and lets a newer one replace a stale one, so a slow reader never builds a queue,
it just skips ahead. A client that falls behind asks for one pane in full rather
than resyncing everything. The protocol version is an exact-match gate, not a
negotiation: a client and daemon that disagree refuse each other, and the window
offers to restart the daemon for you.

**Rendering is [gpui](https://www.gpui.rs), Zed's renderer**, from a patched
fork. The patches exist mostly so Chromium and the terminal can share one GPU
device, plus Ghostty-parity glyph rendering and the window shaping that a
client-side-decorated frame needs. Terminal rows are cached against revision
numbers the daemon mints during its diff, so an unchanged row replays cached
glyphs instead of being reshaped.

**Terminal emulation is `libghostty-vt`**, Ghostty's own VT engine, compiled
from Zig and reached over FFI. Everything around it (the actor, frames, copy
mode, search, appearance) is written here, and `unsafe_code` is denied across
the workspace. Kitty graphics, OSC 52 clipboard writes, OSC 7 and OSC 8, the
kitty keyboard protocol, bracketed paste, and OSC 10/11/12 color queries all
work. Sixel does not.

**Browser panes run Chromium off-screen** and hand each finished frame to the
GPU without a trip through the CPU, on a path that depends on your platform:

| Platform | Path |
| --- | --- |
| macOS | IOSurface → Metal blit |
| Linux | DMA-BUF → wgpu external texture |
| Windows | Shared handle → D3D11 copy |
| Fallback | BGRA readback |

Frames cross a one-slot mailbox, so the newest always wins and damage from a
dropped frame gets folded into its replacement. A focused pane paints at display
refresh, an unfocused one at 30, and a hidden one not at all.

Everything the daemon holds lives in memory. Detaching keeps your PTYs,
scrollback, layouts, and browser URLs; a reboot starts clean.

## Build from source

### Prerequisites

- **Rust 1.97.0.** Pinned in `rust-toolchain.toml`; rustup selects it for you.
- **Zig 0.16.0.** Pinned in `mise.toml`. Install [mise](https://mise.jdx.dev/),
  then `mise install`. Zig compiles `libghostty-vt`, the VT engine.
- **CMake 3.21+ and Ninja.** The CEF C++ wrapper is built from source.
- **`just`, `git`, and a network connection.** The build clones a pinned Ghostty
  commit and downloads a matching CEF distribution.
- **Linux**, the same list CI installs:

  ```sh
  sudo apt-get install --yes cmake curl desktop-file-utils ninja-build \
    libfontconfig-dev libwayland-dev libx11-xcb-dev libxcb1-dev \
    libxkbcommon-dev libxkbcommon-x11-dev
  ```

- **macOS** needs full Xcode, not just the Command Line Tools: `actool` and the
  Metal toolchain only ship with it.
- **Windows** needs the MSVC Rust toolchain and the Visual Studio C++ build
  tools. Run `just` from Git Bash; the recipes are bash.

### Building

```sh
just build mac       # or: linux, windows -> release bundle in dist/zz
just run mac         # or: linux -> debug build, straight into a window
just dmg             # macOS: dist/zz-macos.dmg
just zip-windows     # Windows: dist/zz-windows.zip
just pacman-package  # Arch: a native package
```

There is no cross-compilation; build each platform on itself. Checks are plain
cargo:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

### CEF

The first build downloads a CEF distribution and compiles its wrapper, which
costs roughly 600 MB on macOS and more on Linux. By default that lands in
`OUT_DIR`, so every worktree pays again. Point them all at one cache:

```sh
export CEF_PATH="$HOME/.cache/cef"
```

The two upcoming pane types are behind cargo features and compiled out by
default. To build with them anyway:

```sh
just run linux --features agent-pane,editor-pane
```

## Where to go next

<div class="zz-cards">
  <a class="zz-card" href="/zz/docs/tmux/"><b>tmux compatibility</b><span>Which commands, key tables, and layouts are in, and what gets rejected.</span></a>
  <a class="zz-card" href="/zz/docs/browser/"><b>Browser panes</b><span>Profiles, cookie import, the element picker, and what Chromium will not do.</span></a>
  <a class="zz-card" href="/zz/docs/agents/"><b>Agent panes</b><span>Upcoming work: where ACP panes are heading, and how to build one early.</span></a>
  <a class="zz-card" href="/zz/docs/cli/"><b>CLI</b><span>Every command, target syntax, and format strings.</span></a>
  <a class="zz-card" href="/zz/docs/configuration/"><b>Configuration</b><span>Both files in full, import behavior, and provenance.</span></a>
</div>
