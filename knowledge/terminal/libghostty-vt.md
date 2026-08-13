---
type: Subsystem
title: libghostty-vt embedding
description: How zz-terminal embeds libghostty-vt v0.2.1 over a pinned Ghostty Zig 0.16 snapshot, including terminal color-query replies and single-worker-thread ownership.
resource: crates/zz-terminal/src/session.rs
tags: [libghostty, ghostty, vt, zig, worker-thread, mode-revision, kitty-graphics]
timestamp: 2026-08-10T14:50:30-03:00
---

# Overview

`libghostty-vt` is the VT engine inside [`zz-terminal`](/crates/zz-terminal.md). The
workspace pins the upstream v0.2.1 release commit `46a9d2ac941ed600cf43c5e6299c8dfd1d3a1ef0`
exactly, with `default-features = false`, because crates.io v0.2.0 ignores OSC dynamic-color queries.
The `-vt` crate is a safe Rust binding over `libghostty-vt-sys`. Until the wrapper publishes its own
Zig 0.16 pin, the workspace replaces only that sys crate with the local snapshot documented in
`third_party/rust/libghostty-vt-sys/UPSTREAM.md`; it statically builds Ghostty commit
`7aa9591746ffa4d2eee458960c76554352832595`. The repository pins **Zig 0.16.0** in `.zigversion`,
`mise.toml`, and CI so every native rebuild uses the required compiler. `zz-terminal` enables the
wrapper's `kitty-graphics` feature and leaves the other defaults off. `session.rs` uses
`Terminal::kitty_graphics`, `Terminal::set_kitty_image_storage_limit`, `PlacementIterator`, and the
safe `DecodePng` adapter; it contains no raw libghostty layout cast or unsafe FFI. Each VT actor
installs the wrapper's thread-local PNG decoder before it creates terminal state.
zz-terminal keeps every libghostty handle on a single worker thread; no libghostty binding type is
part of the crate's public API, and the [app crate](/crates/zz.md) contains no raw libghostty handles
or unsafe FFI.

# What libghostty provides

The worker uses these libghostty facilities (imports in `session.rs` and `session/mode_revision.rs`):

| Facility | Types used | Used for |
| --- | --- | --- |
| Terminal state | `Terminal<'alloc,'callbacks>`, `TerminalOptions`, `Screen`, `Mode` | VT parsing of PTY bytes, grid + scrollback, primary/alternate screens. |
| Render extraction | `RenderState`, `RowIterator`, `CellIterator`, `Dirty`, `CursorVisualStyle` | Walk dirty rows/cells into [`PackedCell`](/concepts/terminal-frame.md) frames. |
| Cell semantics | `CellWide`, `CellSemanticContent`, `RowSemanticPrompt`, `TrackedGridRef`, `PointCoordinate` | Wide-glyph spacers, OSC 133 prompt/input/output marks, stable scroll-safe references. |
| Key encoding | `key::Encoder`, `key::Event`, `key::Key`, `OptionAsAlt` | Encode [`KeyInput`](/terminal/interaction.md) to terminal bytes (Kitty keyboard aware). |
| Mouse encoding | `mouse::Encoder`, `mouse::Event`, `EncoderSize` | Application mouse reporting when the app requests tracking. |
| Selection | `Selection`, `SelectWordOptions`, `SelectLineOptions`, `FormatOptions` | Word/line boundary selection and copy formatting (soft-wrap unwrap, trim). |
| Kitty graphics | `Graphics`, `PlacementIterator`, `Image`, `DecodePng` | Extract placement geometry and bounded decoded image data without exposing raw terminal handles. |
| Styling / color | `RgbColor`, `StyleColor`, `Underline` | Resolve per-cell fg/bg/underline, inverse, explicit-RGB detection. |
| Formatting / misc | `fmt::Formatter`, `focus`, `ScrollViewport`, `SizeReportSize`, `ColorScheme` | `capture-pane` output, focus reporting, scrollback paging, size/scheme reports. |

The worker also registers libghostty callbacks: `on_pty_write` (terminal responses collected in
`PtyEffects` and drained to the PTY writer), `on_size`, `on_color_scheme`, and `on_xtversion`.
Default colors and the full 256-color palette are pushed into every terminal via
`apply_terminal_appearance` before any PTY output is processed.

# Dynamic color queries

Terminal-aware TUIs query the effective foreground, background, and cursor colors with OSC 10, 11,
and 12 before deriving subtle surfaces. `libghostty-vt` answers those queries through `on_pty_write`
using the defaults installed by `apply_terminal_appearance`, and the actor drains the response back
to the child PTY in order. Codex derives its shaded composer row from those replies instead of
falling back to an unstyled prompt. The `terminal_reports_configured_colors_for_osc_10_and_11_queries`
test covers both the query replies and preservation of ST versus BEL terminators.

# The worker-thread ownership rule

libghostty render-state dirty tracking and viewport snapshots are **stateful and single-threaded**.
zz-terminal enforces one invariant: **all libghostty objects for a pane live on that pane's worker
thread and are mutated only by the actor.** Consequences:

- PTY writes, key/mouse encoding, resize, focus, and snapshot extraction are serialized through the
  actor's bounded command channel, so they can never interleave (see [pty-worker](/concepts/pty-worker.md)).
- Search runs on a separate thread but never borrows libghostty; it scans an immutable
  `HistorySearchSnapshot` copied out on demand.
- `capture()` blocks only the calling client thread and is answered by the actor; terminal state never
  crosses threads.
- The design intends each subscribed client to own a distinct `RenderState`, so a mutation is extracted
  for every view of the same content generation before libghostty dirty rows are acknowledged.

# Mode revisions

Copy mode and read-only view mode must stay visually stable while live PTY output continues underneath
them. zz-terminal does this **without a second emulator**: `session/mode_revision.rs` captures a
`ModeRevision`, an immutable, paged snapshot of the entire scrollback taken by walking
`ScrollViewport::Top` then `Delta` pages across the whole `total_rows()` range.

`ModeRevision` (arc-shared, id from the `NEXT_MODE_REVISION_ID` atomic) holds:

| Field | Meaning |
| --- | --- |
| `screen`, `columns`, `viewport_rows` | Captured `Screen` identity and geometry. |
| `foreground`/`background`/`palette` | Resolved colors, used by `matches_terminal_appearance` to detect stale revisions. |
| `cells` + `dictionary` | Interned `PackedCell` plane + shared style/grapheme dictionary. |
| `rows: Vec<ModeRowMeta>` | Per-row wrapped / wrap-continuation / semantic-prompt flags (4 bytes each). |
| `semantics: Vec<u8>` | Per-cell OSC 133 class (`Output`/`Input`/`Prompt`) for prompt jumping and command-output selection. |
| `search: Arc<HistorySearchSnapshot>` | Revision-tagged text + cell offsets scanned by the search worker. |

Capture is bounded (`MAX_MODE_REVISION_BYTES` = 128 MiB; search snapshot capped separately) and restores
the terminal's saved scroll offset when finished. The revision exposes navigation helpers
(`clamp_point`, `move_revision_word`, paragraph/semantic targets) and `format_selection` /
`capture_rows` / `capture_rows_vt` for copy output (the `_vt` form re-emits SGR escapes).

# Related

- Owned and driven by [`zz-terminal`](/crates/zz-terminal.md); frames land in the [terminal frame](/concepts/terminal-frame.md) model.
- Encoders back the [interaction](/terminal/interaction.md) subsystem; resolved colors come from [appearance](/terminal/appearance.md).
- The native tmux copy/view modes on top of mode revisions are described in [copy-mode](/tmux/copy-mode.md).
- Zig toolchain pin in [prerequisites](/playbooks/prerequisites.md); Ghostty pins in [ghostty-color-reference](/references/ghostty-color-reference.md).
