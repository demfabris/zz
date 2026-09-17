---
type: Research Report
title: "Interactive bughunt: Agent transcripts, Markdown, and terminal resizing"
description: Running investigation of disappearing Agent user messages in both providers, inline-code wrapping, and terminal corruption after pane or GUI resizing.
tags:
- bughunt
- agent
- markdown
- terminal
- resize
timestamp: 2026-09-16T22:03:45Z
status: partially_fixed
git_commit: 8d4438c0466a8a01293236511844ed35ce30261b
resource:
- crates/zz/src/workspace/view.rs
- crates/zz/src/agent/controller.rs
- crates/zz-ui/src/widget/text/inline.rs
- crates/zz-terminal/src/session.rs
---

# Overview

Running bughunt requested by fabrico on 2026-09-16. Record additional reports here with stable
IDs, evidence, confidence, and unresolved questions. Fabrico authorized fixes after the initial
investigation. The findings below describe the baseline commit above; subsequent working-tree
fixes have their own status sections. The running daemon may predate that checkout.

| ID | Report | State |
| --- | --- | --- |
| BH-001 | Agent user messages disappear after switching away and back; both Codex and Claude Code | Fixed prompt recording and reconstruction; ordinary same-session window report still needs live verification |
| BH-002 | Wrapped inline code paints empty background and crowds adjacent prose | Fixed per-fragment background geometry and spacing |
| BH-003 | Pane or GUI resize leaves Claude Code terminal text duplicated and displaced | Reported in both `clairvo` panes; similar stored corruption captured in `wire`; cause under investigation |
| BH-004 | Synchronized output exposes intermediate terminal redraws | Fixed frame hold with bounded timeout; relationship to BH-003 remains unproven |

# BH-001: Agent user messages disappear

## Report and scope

User messages vanish after switching windows or sessions and returning. Fabrico explicitly
confirmed both Codex and Claude Code. Do not narrow the report to one provider.

## Source findings

- `crates/zz/src/agent/controller.rs`, `AgentController::prompt`, inserts the submitted prompt
  into the local transcript through `begin_prompt` after sending the request.
- `crates/zz/src/workspace/view.rs`, `retained_agents` and `register_agent_panes`, retain only
  panes from the attached session. `AgentController::retain_panes` removes the other session's
  threads and viewports. Returning creates a new transcript and requests replay.
- `crates/zz-daemon/src/agent/host.rs`, prompt dispatch, emits `TurnStarted { turn_id }` without
  prompt content. `crates/zz-client/src/agent_transcript.rs`, `user_message`, reconstructs user
  entries from adapter `UserMessageChunk` notifications. A locally displayed prompt without an
  adapter echo therefore disappears on reconstruction.
- The daemon also writes prompts to its terminal projection. Missing desktop rows do not prove
  that the provider conversation or every daemon representation lost the prompt.

Commit `dd7d0a3b` (2026-08-15) restricted retention to the attached session, following the
daemon-runtime move in `5b8e253b` (2026-08-14).

## Limits and checks

This establishes a loss path shared by both providers, not a captured replay from the user's
affected Agent pane. A normal window switch within one session retains the transcript; that
part of the report remains unresolved. Journal fallback can issue a new `SessionReset` and
clear a retained transcript, but no runtime evidence ties that branch to this report.

The existing replay-deduplication test uses assistant messages. It does not prove user prompt
preservation across session changes. No new test or live reproduction performed for BH-001.

## Fix and verification, 2026-09-16

Implemented in the working tree after the baseline commit. `runtime.rs` now emits and journals
submitted text and images as standard user-message chunks before dispatch. Each prompt has one
message ID; long text splits on UTF-8 boundaries. Active adapter user echoes are suppressed,
while session-load history remains intact. Queued prompts enter the transcript on dispatch;
deferred cancellation does not record a prompt that was never forwarded.

The daemon Agent suite passes 161 tests. New coverage includes both providers, adapters with and
without echoes, repeated prompt text, attachments, queued dispatch, history loading, journal
replay, and reattachment. The desktop controller suite passes 50 tests, including dropping a
session's local transcript, reconstructing submitted and queued prompts, and replaying twice
without duplicates. Existing incomplete journals are not retroactively filled by this change.

# BH-002: Inline-code wrapping

## Report

The supplied screenshot shows `cargo run -p zz-xtask` wrapping between `-p` and `zz-xtask`.
The first background continues far past the text, and code backgrounds crowd adjacent prose.
Ordinary visual wrapping is sufficient; the screenshot does not establish a source newline.

![User screenshot of the wrapped inline-code background](2026-09-16-inline-code-wrap.png)

## Source findings

`crates/zz-ui/src/widget/text/inline.rs`, `paint_code_fills`, passes whole-paragraph bounds to
`code_fill_bounds`. Every non-final wrapped row uses `text_bounds.right()` instead of the
fragment's last glyph position. Word wrapping can leave unused space after `-p`, but the fill
still reaches the paragraph edge.

`CODE_FILL_PAD_X` adds 3 px on each side during painting. The original text layout reserves no
corresponding width, so the fill consumes part of the spaces separating code and prose.
Commit `9ba4d0f0` (2026-08-17) introduced this custom background geometry.

## Verification

The existing compiled `zz_ui-bf432ff490923e13` test
`a_wrapped_code_fill_splits_per_line` passed. Its assertion explicitly expects the paragraph
edge. The executable dates to September 15; this was not a fresh build. No exact screenshot
reproduction in the running GUI and no rendering changes.

## Fix and verification, 2026-09-16

Implemented in the working tree. `code_fill_bounds` intersects each code range with the text
shaper's visual rows, measures the fragment, and excludes trailing wrap whitespace. Backgrounds
follow centered and right-aligned table text too. Painted horizontal padding is now 1 px, so it
consumes less of the ordinary separating space; source text and selection offsets stay intact.

The Markdown suite passes 59 tests. New checks cover unused space after a wrapped word, code
starting exactly at a wrap boundary, multibyte text, table alignment, and real GPUI text shaping.

# BH-003: Terminal corruption after resizing

## User evidence and recovery

Fabrico reported the issue in both Claude Code terminal panes in session `clairvo`, discovered
as `%4` and `%8` in window `@2`. Both pane resizing and whole GUI resizing can trigger it.
Whether Claude was still writing at the time is unknown.

The pasted transcript repeats the same answer at several widths. In particular, the paragraph
beginning `Total around 12k tokens` recurs, headings move far right, and bullet fragments run
together. These are terminal-display artifacts; the answer's claims about zz's agent tooling
are quoted conversation content, not additional verified bugs in this hunt.

Fabrico subsequently reported that dragging and moving the pane improved the display. Treat
this as partial recovery, not resolution or proof that scrollback was repaired.

## Read-only live observations

- Initial discovery showed `%4` and `%8` at 170 by 103 cells; a later discovery showed both at
  172 by 103. Both ran `claude`. No investigation command resized, moved, or sent input to them.
- The first saved `%8` capture contains the reported `agentic bus` answer once rather than the
  repeated form in the user's paste. The timing of the recovery relative to this capture was
  not recorded, so it is not a controlled pre-recovery sample.
- A second capture of each `clairvo` pane matched its first capture exactly. The captures do
  not bracket the original corruption or isolate what the drag changed.
- Independent pane `%0`, session `zz`, title `wire`, measured 185 by 65 cells. Its daemon-side
  `capture-pane` output contains four occurrences of one response opener and headings displaced
  by 109 leading spaces. This establishes stored terminal-grid or scrollback damage in a live
  Claude pane, beyond a GPU paint-only artifact.
- Installed command versions: `zz 0.10.0`, `Claude Code 2.1.273`. Those commands do not establish
  which executable versions the already-running daemon and Claude processes started with.

Raw captures are local investigation artifacts:
`/tmp/zz-bughunt-2026-09-16-pane-{0,4,8}.txt` and
`/tmp/zz-bughunt-2026-09-16-pane-{4,8}-after-move.txt`. They are temporary and contain unrelated
conversation text. This report preserves the relevant observations without copying that text.

## Current source trace

1. `crates/zz/src/terminal/view.rs`, `TerminalView::update_geometry`, sends each changed measured
   grid during whole-window resizing. Equality deduplication does not coalesce distinct sizes.
2. `crates/zz/src/workspace/view.rs`, split-drag handling, suppresses terminal measurements while
   the visual drag override exists. Releasing sends `ResizeSplit`, then waits for a snapshot
   revision before allowing terminal measurements again. Pane dragging and GUI resizing thus
   generate different resize traffic.
3. `crates/zz-terminal/src/session.rs`, live resize handling, resizes the OS PTY, resizes
   libghostty's terminal, and snapshots the result. On macOS the worker can process a command
   before already-readable PTY output. Output produced for an earlier width could therefore
   reach the parser after a size change. This is an interleaving candidate, not a reproduced
   cause.
4. `crates/zz-ui/src/terminal.rs` paints retained cells within new bounds while waiting for a
   frame. It does not reflow their text. Full-frame replacement and column-changing patches in
   `crates/zz/src/mux/client.rs` invalidate retained history or assign fresh row revisions.
   No persistent desktop row-cache defect was established.

## External evidence

[Claude Code issue 81135](https://github.com/anthropics/claude-code/issues/81135) reports narrow,
wide, then narrow resizing leaving messages at multiple widths and stale text far to the right
with Claude Code 2.1.220 and Ghostty 1.3.1. Its closed/stale status does not establish a fix.
[Issue 53248](https://github.com/anthropics/claude-code/issues/53248) reports six tmux width
changes producing seven copies of the startup banner in an older version.

The [official changelog](https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md) records
resize or duplicated-scrollback fixes in 2.1.116, 2.1.120, and 2.1.144. These reports establish
an external failure class, not responsibility for this zz incident or a fix in 2.1.273.

## Verification and outstanding proof

Existing compiled tests passed:

- `pending_live_resize_projects_retained_rows_from_the_bottom` and
  `retained_row_cache_prunes_only_stale_revisions`: 2/2, September 15 UI executable.
- `saturated_pty_input_does_not_block_resize_or_shutdown`: 1/1, September 16 terminal executable.
  This checks resize responsiveness under blocked input, not output consistency during resize.

No new product build and no mutation of the affected panes. The next decisive evidence is a
controlled trace of resize sizes and emitted PTY bytes, with main-screen terminal state before
and after each resize. Compare an isolated replay against Ghostty before assigning the
corruption to either zz or Claude Code. BH-004 below confirms a separate presentation defect.

# BH-004: Synchronized output does not hold the published frame

## Isolated reproduction

An isolated `TerminalSession` with its own shell emitted this sequence:

```sh
printf BEFORE
sleep 0.5
printf '\033[?2026h\rAFTER!'
sleep 1
printf '\033[?2026l'
sleep 1
```

The harness attached a view, resized once before the sequence, and sampled the published
viewport. The intermediate sample, around 0.76 seconds after startup, already contained
`AFTER!`, although synchronized output had not been released:

```text
initial="BEFORE"
during_sync="AFTER!"
after_sync="AFTER!"
```

A PTY-free terminal experiment produced the same result. The live harness source remains at
`/tmp/zz-sync-live-investigation.rs`. Both experiments linked against existing
`libzz_terminal-f6a5cfb12806d529.rlib`, dated September 16 at 17:36; no workspace rebuild and no
interaction with the user's panes. The shell sequence above preserves the reproduction input
if the temporary harness disappears.

## Source findings and scope

`crates/zz-terminal/src/session.rs` publishes pending output on its content deadline without
checking synchronized-output mode. Snapshot construction calls `render_state.update(terminal)`
unconditionally. The embedded libghostty render-state API updates rows directly; Ghostty's
full desktop renderer separately checks `.synchronized_output` before updating render state
(`src/renderer/generic.zig` in the dependency source).

This confirms exposure of partial redraws. Reading or publishing a snapshot does not itself
duplicate PTY output, so this experiment does not prove the persistent-history failure in
BH-003. The dependency explicitly allows resize to clear synchronized-output mode; that
documented resize behavior is not a second defect.

## Fix and verification, 2026-09-16

Implemented in the working tree. The shared publication helper holds the previous frame while
synchronized output is active. Both worker loops wake on a one-second timeout, clear unfinished
synchronization, and publish the pending content. Parsing and input continue during the hold.
Explicit release, resize, EOF, and process exit also permit publication.

The terminal library suite passes 280 tests with one pre-existing ignored test. Five new tests
exercise live-PTY and PTY-free hold/release, timeout without further output, and resize recovery.
The live tests confirm that capture sees parsed `AFTER!` while the published frame still contains
`BEFORE`. The user panes and running daemon were not restarted for these tests. BH-003 remains
open because holding redraws does not establish a fix for stored duplicate paragraphs.

# Fix validation, 2026-09-16

The following checks passed against the working-tree fixes:

```sh
cargo test -p zz-terminal --lib
cargo test -p zz-ui --all-features --lib widget::text::
cargo test -p zz-daemon --lib agent:: -- --test-threads=4
cargo test -p zz --all-features --lib agent::controller::tests -- --test-threads=4
cargo clippy -p zz-terminal -p zz-ui -p zz-daemon -p zz --all-targets --all-features -- -D warnings
cargo fmt --all --check
```

These suites total 550 passing tests and one ignored test. Knowledge validation reports zero
errors; its existing warning concerns old research documents outside this bughunt. No commit,
installation, or restart of the user's running daemon was performed.

# Related

- [Native Agent pane](/concepts/agent-pane.md)
- [Daemon-owned PTY worker](/concepts/pty-worker.md)
- [Terminal engine](/crates/zz-terminal.md)
- [Terminal frames](/concepts/terminal-frame.md)
