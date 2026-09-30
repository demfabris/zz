---
type: Subsystem
title: libghostty-vt embedding
description: How zz-terminal embeds libghostty-vt over a pinned Ghostty snapshot, including line-counted scrollback, terminal color-query replies, and single-worker-thread ownership.
resource: crates/zz-terminal/src/session.rs
tags: [libghostty, ghostty, vt, zig, worker-thread, mode-revision, kitty-graphics]
timestamp: 2026-09-30T12:00:00-03:00
---

# Overview

`libghostty-vt` is the VT engine inside [`zz-terminal`](/crates/zz-terminal.md). The
workspace consumes published `demfabris/libghostty-rs` commit
`8e40135fb20e9ed91c37c374fe1d14570c386d06` on new branch `zz-2026-09-30`, with
`default-features = false`. Its parent `359ef751c189540eafb9110b2de89ad95ce48fc3`
remains on `zz-2026-09-25`. That base
contains the stacked render-hold and resize-scrollback APIs needed by the current C ABI.
The safe wrapper lives in its dependency fork. zz replaces only `libghostty-vt-sys` with
the local snapshot documented in `third_party/rust/libghostty-vt-sys/UPSTREAM.md`.
The published native pin is `7823f65dd55fc9ff420d5eb5cae761cbd1995994` on branch
`zz-2026-09-30`, parent `c39414175ca2aad564b74b3f52196355f2671774`, upstream base
`6301810a48aaa3426887a4316668f18833a40138`. It carries four changes: the C ABI
signal-stack option, spare-page reuse, the history-erase trim fix and owned copy snapshots.
The branch fast-forward retains the trim-fix parent in its history; `zz-2026-09-29`
keeps the spare-page pin `713374af`. zz pins both published copy commits for ordinary
fetched-source builds without source rewriting or a safe-wrapper path patch.
Spare-page reuse keeps a pruned pool page resident for the next grow. The trim fix preserves
live cell blocks after history erase. The signal-stack option removes unused Zig TLS storage
from ReleaseSafe dev and test builds. Copy snapshots share immutable history backing,
keep compressed pages encoded until first read and copy active pages with cursor pointers.
Rust retains ownership of thread startup and signal handling. See the [macOS measurements](/research/2026-09-23-macos-performance.md). The repository pins **Zig 0.16.0** in `.zigversion`,
`mise.toml`, and CI so every native rebuild uses the required compiler. `zz-terminal` enables the
wrapper's `kitty-graphics` feature and leaves the other defaults off. `session.rs` uses
`Terminal::kitty_graphics`, `Terminal::set_kitty_image_storage_limit`, `PlacementIterator`, and the
safe `DecodePng` adapter; it contains no raw libghostty layout cast or unsafe FFI. Each VT actor
installs the wrapper's thread-local PNG decoder before it creates terminal state.
zz-terminal keeps each live terminal on its actor thread; owned frozen snapshots can move
to the search worker under a mutex; no libghostty binding type is
part of the crate's public API, and the [app crate](/crates/zz.md) contains no raw libghostty handles
or unsafe FFI.

# What libghostty provides

The worker uses these libghostty facilities (imports in `session.rs` and `session/mode_revision.rs`):

| Facility | Types used | Used for |
| --- | --- | --- |
| Terminal state | `Terminal<'alloc,'callbacks>`, `Screen`, `Mode` | VT parsing of PTY bytes, grid + scrollback, primary/alternate screens. |
| Render extraction | `RenderState`, `RowIterator`, `CellIterator`, `Dirty`, `CursorVisualStyle` | Walk dirty rows/cells into [`PackedCell`](/concepts/terminal-frame.md) frames. |
| Cell semantics | `CellWide`, `CellSemanticContent`, `RowSemanticPrompt`, `TrackedGridRef`, `PointCoordinate` | Wide-glyph spacers, OSC 133 prompt/input/output marks, stable scroll-safe references. |
| Key encoding | `key::Encoder`, `key::Event`, `key::Key`, `OptionAsAlt` | Encode [`KeyInput`](/terminal/interaction.md) to terminal bytes (Kitty keyboard aware). |
| Mouse encoding | `mouse::Encoder`, `mouse::Event`, `EncoderSize` | Application mouse reporting when the app requests tracking. |
| Selection | `Selection`, `SelectWordOptions`, `SelectLineOptions`, `FormatOptions` | Word/line boundary selection and copy formatting (soft-wrap unwrap, trim). |
| Kitty graphics | `Graphics`, `PlacementIterator`, `Image`, `DecodePng` | Extract placement geometry and bounded decoded image data without exposing raw terminal handles. |
| Styling / color | `RgbColor`, `StyleColor`, `Underline` | Resolve per-cell fg/bg/underline, inverse, explicit-RGB detection. |
| Formatting / misc | `fmt::Formatter`, `focus`, `ScrollViewport`, `SizeReportSize`, `ColorScheme` | `capture-pane` output, focus reporting, scrollback paging, size/scheme reports. |

The worker also registers libghostty callbacks: `on_pty_write` (terminal responses collected in
`PtyEffects` and drained to the PTY writer), `on_size`, `on_color_scheme`, `on_xtversion`, and
`on_clipboard_write`, which answers every write through the request's `reply` before returning.
Default colors and the full 256-color palette are pushed into every terminal via
`apply_terminal_appearance` before any PTY output is processed.

# Scrollback limit

`new_terminal` in `session.rs` builds every Ghostty terminal: `Terminal::new(cols, rows)`, then
`set_scrollback_max_bytes(None)` and `set_scrollback_max_lines(Some(limit))`, with the limit clamped
to `MAX_HISTORY_LIMIT`. Panes pass tmux's `history-limit`, fixed at spawn as tmux does. Before the
2026-09-25 bump the pin read that number as a byte budget, so every pane kept about one page of
history (924 rows at 80 columns) whatever the option said. Ghostty prunes whole pages, so history
settles somewhat under the limit rather than at it: at 80x24, 10,000 keeps 9,883 rows and 2,000 keeps
1,609 (`history_limit_counts_retained_lines`). tmux drops a tenth of the limit when it fills, so both
keep a band below the limit. Small limits keep about one page instead: every limit from 0 to 1,000
kept 427 rows at 80 columns, where tmux keeps exactly that few. Command-output views pass 100,000 and the startup diagnostics view
64 MiB, which the clamp turns into one million lines.

# Replies tmux does not give

At this pin libghostty answers some queries the pinned tmux answers differently or not at all, and
it offers no option to turn them off, so zz passes them through: XTGETTCAP (`DCS + q`) is answered from
Ghostty's own terminfo entry whenever a PTY write callback is installed, where tmux stays silent;
DECRQSS answers SGR, DECSTBM, DECSLRM, and DECSCUSR, where tmux answers only the cursor style and
reports every other request as invalid. ANSI DECRQM is answered by both, with tmux knowing only IRM.
Title reports (`CSI 21 t`) stay off, the libghostty default and the pin's behavior
(`title_report_queries_stay_unanswered_like_the_pinned_tmux`).

# Dynamic color queries

Terminal-aware TUIs query the effective foreground, background, and cursor colors with OSC 10, 11,
and 12 before deriving subtle surfaces. `libghostty-vt` answers those queries through `on_pty_write`
using the defaults installed by `apply_terminal_appearance`, and the actor drains the response back
to the child PTY in order. Codex derives its shaded composer row from those replies instead of
falling back to an unstyled prompt. The `terminal_reports_configured_colors_for_osc_10_and_11_queries`
test covers both the query replies and preservation of ST versus BEL terminators.

# The worker-thread ownership rule

libghostty render-state dirty tracking and viewport snapshots are **stateful and single-threaded**.
zz-terminal keeps each live terminal and render state on its pane actor. Owned frozen snapshots
have independent metadata and mutex-serialized access. Consequences:

- `CommandSender` routes control work and PTY-writing input through separate bounded lanes. The actor
  consumes both on one thread, pauses the input lane while `PtyWriter` has a backlog, and keeps
  draining PTY output so full-duplex children can make progress. The actor serializes PTY writes,
  key/mouse encoding, resize, focus, and snapshot extraction (see
  [pty-worker](/concepts/pty-worker.md)).
- Search runs on a separate thread using an owned `ScreenSnapshot` through a mutex. It
  reconstructs one wrapped logical line at a time and never borrows the live terminal.
- `capture()` blocks only the calling client thread and is answered by the actor; terminal state never
  crosses threads.
- The design intends each subscribed client to own a distinct `RenderState`, so a mutation is extracted
  for every view of the same content generation before libghostty dirty rows are acknowledged.

# Mode revisions

Copy mode and read-only views freeze the native active screen through
`ModeRevision::capture` in `session/mode_revision.rs`. The owned `ScreenSnapshot` retains
history after source output, pruning, ED3 and source destruction. `CopyGrid` converts only
requested rows and caches at most 64, paired with a style/grapheme dictionary.
`ModeRevisionReader` holds one row and its matching dictionary while rendering, navigating,
capturing plain or VT output, and formatting selections. Dictionary compaction can clear the
cache; readers and published frames retain their own immutable storage.

`HistorySearchSnapshot` retains native backing and decodes logical wrapped lines on demand.
Search maps Unicode byte matches back to physical cells, checks cancellation between rows
and recompresses after 512-row batches and at completion. Empty and single-scalar graphemes
skip the general iterator. Captures append glyphs to the destination without per-cell strings.

Frozen resize reflows owned backing, maps the copy cursor, clears selection and rebuilds
search marks. Appearance updates recolor frozen content; an enabled refresh replaces it with
a new source snapshot. Retained dead panes keep the terminal actor and frozen search state
while releasing PTY and input resources. `ZZ_PERF_COPY_CLONE=1` selects the old flat snapshot
and its entry-geometry limit; `ZZ_PERF_NO_COMPRESS=1` suppresses snapshot recompression.

# Related

- Owned and driven by [`zz-terminal`](/crates/zz-terminal.md); frames land in the [terminal frame](/concepts/terminal-frame.md) model.
- Encoders back the [interaction](/terminal/interaction.md) subsystem; resolved colors come from [appearance](/terminal/appearance.md).
- The native tmux copy/view modes on top of mode revisions are described in [copy-mode](/tmux/copy-mode.md).
- Zig toolchain pin in [prerequisites](/playbooks/prerequisites.md); Ghostty pins in [ghostty-color-reference](/references/ghostty-color-reference.md).
