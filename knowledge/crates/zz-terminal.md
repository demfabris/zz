---
type: Rust Crate
title: zz-terminal crate
description: The per-pane terminal engine that owns a PTY child and every libghostty-vt object on a worker thread and publishes immutable renderer-neutral frames.
resource: crates/zz-terminal/src/terminal_core.rs
tags: [terminal, libghostty, pty, actor, worker-thread, frames]
timestamp: 2026-08-06T00:00:00Z
---

# Overview

`zz-terminal` (lib root `src/terminal_core.rs`) is the
crate that turns one shell child into a stream of immutable, renderer-neutral terminal frames. Each
[`TerminalSession`](/concepts/pty-worker.md) spawns a dedicated OS worker thread that exclusively owns
its `portable_pty` child process and every [`libghostty-vt`](/terminal/libghostty-vt.md) object (VT
parser, grid, key/mouse encoders, render state). The GPUI application never touches a PTY or a
libghostty handle directly; it reads the latest [`TerminalViewport`](/concepts/terminal-frame.md)
frame and applies compact patches. The crate holds no GPUI types, so the daemon, the
[protocol](/crates/zz-protocol.md) tests, and the app can all use it. It is consumed by
[`/crates/zz-daemon.md`](/crates/zz-daemon.md), which fans frames out to clients over the
[terminal lanes](/protocol/terminal-lanes.md).

# Architecture

The crate is built as a **single-writer actor**: the public `TerminalSession` handle is a thin
command sender + frame subscriber, while all mutable state lives behind one worker thread.

| Piece | Role |
| --- | --- |
| `TerminalSession` | Public handle. Sends `Command`s, exposes `latest_viewport()` and an event stream. Holds no terminal state. |
| worker thread `zz-terminal` | Owns the `Terminal`, `RenderState`, encoders, PTY writer, per-client views, and the `ViewportDictionary`. Runs `run_terminal`. |
| reader thread `zz-pty-reader` | Blocking `read_pty` loop feeding a recycled buffer pool back to the worker. |
| waiter thread `zz-child-wait` | Owns the `portable_pty` child, parks in `wait()`, and hands the exit status to the worker as a wake event. The worker keeps a `clone_killer()` handle for shutdown. |
| worker thread `zz-output-view` | PTY-free variant (`run_output_view`) that renders frozen native command output through the same view lifecycle. |
| `SearchWorker` thread | Scans an immutable `HistorySearchSnapshot` off-thread so search never borrows or blocks libghostty. |
| `Publisher` | Writes each new frame into `Arc<RwLock<Arc<TerminalViewport>>>` and coalesces a `ViewportReady` notification. |

`CommandSender` routes work through two bounded lanes. A capacity-one control lane carries resize,
capture, copy-mode, and pure view operations. An ordered PTY-input lane carries text, keys,
mouse/focus/paste operations, and pending-paste markers; it caps admission at 256 commands and 64
MiB, charging each command's payload plus a 4 KiB floor. Producers use nonblocking input admission
and reject the whole command when either cap is full. The actor chooses control first and executes
both lanes on its single worker thread, so terminal state mutations cannot interleave. While
`PtyWriter` holds unwritten bytes, the actor pauses further PTY input, continues PTY reads and control
work, and retains libghostty-generated PTY replies in `PtyEffects` until the writer drains.
`command_queue_len` includes writer-held and queued input permits; `pending_pty_input_bytes` exposes
their combined admission charge. See
[pty-worker](/concepts/pty-worker.md) for the actor lifecycle in depth.

On Unix, `run_terminal` obtains its default shell command from `shell_integration.rs`. zsh and modern
Bash receive original zz-owned startup hooks that publish the exact interactive command as OSC 2
immediately before execution, then replace it with the compact current directory at the next
prompt. A program's own later OSC 0/2 title wins while it runs. Shell resources are embedded in the
binary and materialized into a versioned private cache; unsupported shells and explicit opt-out
(`ZZ_SHELL_INTEGRATION=none`) fall back to `portable_pty`'s unchanged default shell. These hooks emit
**OSC 2, OSC 7, and cursor shape only**, not OSC 133. Semantic prompt marks come from the user's own
shell integration (ghostty, kitty, wezterm, starship), and `capture_last_command` needs them.

The worker retains one `active_view` (`Option<(TerminalViewId, Box<TerminalViewState>)>`) plus a map of
`inactive_views`. Only the active view drives the published frame; `attach_view` makes a client the
interactive owner of the snapshot stream, `detach_view` returns it to the live bottom viewport, and
`release_view` drops its retained state.

# Schema

Public surface re-exported from `terminal_core.rs`:

| Symbol | Kind | Purpose |
| --- | --- | --- |
| `TerminalSession` | struct | Actor handle: `spawn*`, `send_text`, `send_key`, `resize`, `attach_view`, `view_action`, `paste_prepared_bytes`, `capture`, `capture_last_command`, `latest_viewport`, `events`, `diagnostics`. |
| `TerminalEvents` / `TerminalEvent` | stream | Single-consumer events: `ViewportReady`, `ViewClosed`, `CopyReady`, `OpenUri`, with bounded reliable-event accounting. |
| `TerminalViewport` / `TerminalViewportPatch` | struct | The immutable [frame](/concepts/terminal-frame.md) and its retained-grid diff. |
| `TerminalAppearance` + loaders | struct/fn | Renderer-neutral [appearance](/terminal/appearance.md) model, Ghostty **config** discovery (`discover_ghostty_config`, the `load_ghostty_appearance*` family), and the `zz/config` overlay pass (`apply_appearance_overrides`). |
| `AppearanceConfigKey` / `AppearanceSource` / `AppearanceProvenance` / `AppearanceLoad` | enum/struct | The 31 supported keys (`AppearanceConfigKey::ALL`), the four provenance tiers (`Default`/`ThemeFile`/`Ghostty`/`Override`), and the resolved load. |
| `GhosttyTheme` / `enumerate_ghostty_themes_for` | struct/fn | The theme layer: one discoverable Ghostty theme file with its parsed appearance, and the enumeration of every valid theme in the same precedence order theme resolution uses. |
| `KeyInput`, `KeyCode`, `Modifiers` | input | Renderer-independent key records for the [interaction](/terminal/interaction.md) encoder. |
| `TerminalViewAction`, `TerminalMouseInput`, `CopyModeAction`, `SearchQuery` | input | Native viewport / selection / copy-mode / search actions. |
| `WordSeparators` | struct | Precompiled tmux word-boundary classifier for selection. |
| `prepare_paste_buffer` | fn | tmux `paste-buffer` byte preparation. |
| `CaptureOptions` / `TerminalCaptureError` | struct/enum | `capture-pane`-style content extraction on the actor thread. |

# Key files

| File | Role |
| --- | --- |
| `src/terminal_core.rs` | Crate root; module tree and the complete public re-export list. |
| `src/session.rs` | The actor: PTY spawn, worker loop `run_terminal`, `Publisher`, `ViewportDictionary`, snapshot extraction, view/copy-mode/search/mouse/link logic. |
| `src/shell_integration.rs` | Default-shell detection, private resource materialization, Bash/zsh startup injection, and opt-out handling. |
| `assets/shell-integration/` | zz-owned Bash and zsh pre-exec/pre-command hooks that publish live OSC titles. |
| `src/session/mode_revision.rs` | `ModeRevision` capture . stable paged history snapshot backing copy/view [modes](/terminal/libghostty-vt.md). |
| `src/model.rs` | Frame data model: `TerminalViewport`, `PackedCell`, `PackedStyle`, `Cursor`, `OverlaySpan`, diff/patch. |
| `src/appearance.rs` | `TerminalAppearance`, Ghostty config loader, X11 named-color table. |
| `src/interaction.rs` | Renderer-neutral pointer/view/copy-mode/search action types. |
| `src/input.rs` | `KeyInput` / `KeyCode` / `Modifiers` packed key records. |
| `src/paste.rs` | `prepare_paste_buffer` tmux safe/literal byte transform. |
| `src/word.rs` | `WordSeparators` boundary classifier. |
| `src/x11-rgb.txt` | Embedded Ghostty/X11 `rgb.txt` named-color data. |

# Related

- Owns and drives [`libghostty-vt`](/terminal/libghostty-vt.md) on the worker thread.
- Publishes the [terminal frame](/concepts/terminal-frame.md) consumed by [`/crates/zz-daemon.md`](/crates/zz-daemon.md) fanout.
- Frames and patches travel the [terminal lanes](/protocol/terminal-lanes.md) and [snapshots](/protocol/snapshots.md) of the [wire protocol](/protocol/wire-protocol.md).
- The [rendering-parity](/terminal/rendering-parity.md) effort maps these frames onto GPUI painting in [`/crates/zz.md`](/crates/zz.md).
- Subsystems: [interaction](/terminal/interaction.md), [appearance](/terminal/appearance.md); actor lifecycle in [pty-worker](/concepts/pty-worker.md).
- Session persistence across daemon restarts is described in [session-persistence](/concepts/session-persistence.md).
