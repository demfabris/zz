---
type: Concept
title: Native Agent pane
description: The daemon-addressable Agent pane, pane-local Codex/Claude Code ACP runtime, provider artifact profiles, nested subagent and terminal streams, sticky controls, approvals, and restore metadata.
resource: crates/zz/src/agent/controller.rs
tags: [agent, gpui, markdown, mermaid, acp, pane, sessions, persistence, keyboard, subagent, terminal]
timestamp: 2026-07-27T00:00:00Z
---

# Overview

The **Agent pane** is zz's third materialized pane surface alongside Terminal and Browser. It owns a
stable `%pane` identity in the mux, participates in split layout, focus, choose-tree, and sidebar
navigation, and hosts a live Agent Client Protocol (ACP) conversation in native GPUI controls.

Pane identity and restore metadata are daemon-owned; pane-local ACP processes and live conversation state are
GUI-owned. This keeps provider-specific traffic out of the zz wire protocol while allowing a GUI to
reconnect to the daemon, recover the same pane and working directory, and ask an ACP agent that
supports `session/load` to replay the conversation.

# Ownership and pane lifecycle

| Layer | Representation | Responsibility |
| --- | --- | --- |
| protocol | `PaneKindSnapshot::Agent(AgentDescriptor)` | Versioned pane identity plus `provider`, `cwd`, and opaque ACP `session_id`; introduced across v22–v32. The live wire version is `PROTOCOL_VERSION` (52) |
| mux | `PaneKind::Agent(AgentDescriptor)` | Stable layout leaf, targeting, titles, and validated restore-metadata updates |
| server | no PTY, CEF, or agent child | Materializes the leaf and captures the donor terminal's live working directory |
| app | shared `Entity<AgentController>` | Launches one provider process per `PaneId`, routes its ACP session, lists and transactionally loads provider-owned history, restores sticky settings, reduces events/configuration, and owns cleanup |
| view/UI | `AgentView`, `AgentTimeline`, `AgentEntry` | History picker, composer, cancellation, approval/auth actions, Markdown, reasoning, plans, tools, and Mermaid |

`new-pane` still creates a runtime-free picker first. Choosing **Agent**, or issuing
`select-pane-kind -t %N agent`, materializes that leaf in place, preserving its `%pane` ID and split
geometry. The daemon resolves the picker donor's current terminal working directory and records it
in the new `AgentDescriptor`.

Materialization is gated on the `experimental-agent-pane` flag: while it is off (the default), the
mux engine rejects `select-pane-kind … agent` from every route . picker, palette, CLI, and
`mux.conf` bindings. The implementation itself is also compiled out of default builds behind the
`agent-pane` cargo feature . `crates/zz/src/agent/mod.rs` is the facade whose stub replaces the
controller and view with inert stand-ins. See the flag contract in
[app configuration](/configuration/app-config.md).

Only a terminal has a live working directory, so `MuxState::cwd_donor` decides which pane that
donor is: the split target when it is a terminal, otherwise the window's most recently focused
terminal (`Window::last_panes`), then layout order, then nothing. Splitting off a browser or an
agent therefore opens the new pane where the user last was rather than at the process default, and
a picker that outlived its donor re-resolves at materialization instead of losing the directory.
Terminal splits go through the same helper, so both paths follow one rule.

`AppView` reconciles visible `AgentView` entities by `PaneId`, while the shared controller retains
the complete set of Agent panes from every daemon session. Switching attached sessions therefore
drops only the inactive view entity, not its live ACP session. Removing the actual daemon pane sends
`session/close` when supported (otherwise `session/cancel`), resolves pending permission requests as
cancelled, removes routing state, and stops that pane's agent process.

# Keyboard ownership

Agent keyboard input is app-local: the composer, completions, history search, and approval controls
own their keys. The one exception is the configured tmux prefix, which the window-root
[prefix claim](/crates/zz.md) intercepts before the composer and forwards to the daemon with the
Agent `PaneId` as command source, so `C-a h` leaves the composer no matter what. A key the daemon
passes back has no Agent sink and is dropped; everything else never crosses the mux protocol.

# ACP connection and session flow

`agent/controller.rs` uses `agent-client-protocol` 1.2.0 and the stable ACP v1 schema:

1. Read the descriptor's built-in provider and parse `agent-command` (Codex) or
   `agent-claude-code-command` (Claude Code) as either a shell-style executable/argument string or an
   ACP stdio JSON configuration, then spawn that pane's child. On macOS,
   `agent/environment.rs` `with_platform_environment` captures the login-shell executable path once
   and supplies it to the ACP child when the command did not configure `PATH` explicitly; a bounded
   capture failure falls back to the system `path_helper` path. This keeps Finder/LaunchServices
   launches able to resolve Homebrew, user-local, and version-manager executables without replacing
   an explicit command environment. Only stderr is sent to zz logs; stdout remains ACP JSON-RPC.
2. Send `initialize` with zz client information and generic session-config-option support. Both
   provider profiles advertise the de-facto `_meta.terminal_output = true` stream convention, and
   Claude Code additionally advertises `_meta["subagent-transcript"] = true`. zz still advertises
   neither standard filesystem nor client-owned terminal methods, so agents must not request those
   ACP operations.
3. For each pane, prefer `session/load` when the agent advertises it and the descriptor has a
   session ID. Routing is installed before the load request so replay notifications are retained.
4. If load is unsupported or fails, create a new session in the resolved absolute working
   directory. The app persists the returned opaque session ID and actual cwd together through the
   internal `set-agent-session -t %N -c /path ID` mux command.
5. Send prompts as text content blocks. Prompt requests run concurrently with the runtime command
   loop, so cancellation and permission responses remain responsive during a turn.

zz does not crawl or decode Codex CLI's or Claude Code's private session files. Their ACP adapters
remain the authority over those local stores: zz discovers conversations through `session/list` and
asks the adapter to replay one through `session/load`. This preserves provider compatibility when
either CLI changes its on-disk layout.

ACP session IDs are treated as opaque, but rejected before persistence if empty, larger than 16 KiB,
or containing control characters. A stale daemon snapshot cannot overwrite a newly established
local session while the metadata command is still making its round trip.

The default provider processes are pinned to exact adapter versions . `@latest` would resolve the
dist-tag against the npm registry on every pane spawn, while an exact version spawns from the npx
cache and works offline. When the agent pane is enabled, app launch fires a background
`npx --package <spec> npm --version` per pinned package to pre-populate that cache (and, on macOS,
the login-shell PATH capture) without executing the adapters; version bumps are deliberate edits to
the defaults in `config/mod.rs`:

```text
npx -y @agentclientprotocol/codex-acp@1.1.7
npx -y @agentclientprotocol/claude-agent-acp@0.63.0
```

`zz/config` can override it and the default cwd:

```text
agent-command = my-agent --stdio
agent-claude-code-command = my-claude-agent --stdio
agent-working-directory = /absolute/project/path
```

Raw ACP stdio JSON is also accepted for explicit arguments and environment variables. A configured
working directory wins for a brand-new pane; a persisted descriptor cwd wins when restoring an
existing session. Configuration reload restarts every pane-local process and reloads retained
sessions through the provider stored in each descriptor.

# Driving a pane from outside: `agent-send`

`agent-send [-t %N] [--submit] [--context PATH[:START[-END]]] [TEXT]` is how a shell (or an agent
running inside another pane) puts work into an Agent pane. TEXT comes from argv, or from standard
input when it is omitted, which is the reason the verb exists: `git diff | zz agent-send`. The
CLI is the only process with a stdin, so it reads the pipe and re-sends it after a `--` so a payload
starting with `-` is not mistaken for a flag. A target that is not itself an Agent pane, including
the implicit current pane when `-t` is omitted, routes to its window's most recently focused Agent
pane via `MuxState::recent_agent_pane`, the same rule `send-last-output` uses; a pipe from a
terminal, or an nvim keymap, can address "the agent next to me" with no addressing at all.
`--context` prepends a `path:start-end` header and
fences the payload in a run of backticks longer than any run inside it. Payloads are capped at
`MAX_AGENT_SEND_BYTES` (1 MiB) and rejected before routing if they carry control characters other
than newline, carriage return, and tab, the same discipline that bounds ACP session IDs.

Two ownership boundaries shape the implementation:

- **The draft is the view's.** An Agent pane in another window has no `AgentView` at all (it may
  even have no `AgentThread` yet), so text lands in the controller-owned
  `AgentController::pending_composer` map, keyed by pane independent of thread registration, and
  `AgentView::render` folds it into the `InputState` when a view exists. That fold happens at render
  because `InputState::set_value` needs a `Window`, which no observer has. Existing draft text is
  kept and separated by a newline, so a send never eats what the user was typing.
- **Only the GUI knows whether a pane is idle.** `--submit` reuses the History picker's gate
  (`accepts_prompt()` and no unresolved permission request), which the daemon cannot evaluate. So
  the daemon publishes `EventPayload::AgentCommand { pane, request_id, command }` and parks the
  calling command thread until the GUI answers with `ProtocolMessage::GuiResponse`, the protocol's
  first client → daemon reply, added in v32 and shared with `capture-browser`. The wait is bounded
  (5 s) and a GUI disconnect fails its waiters immediately. The GUI answers from
  `AppView::drain_gui_requests`, driven by the mux observation rather than by `render`, so a
  minimized or occluded window (which schedules no frames) still applies the command and replies
  instead of letting the CLI sit out the timeout.

`send-last-output -t %N` reuses that delivery: it extracts a terminal pane's last completed command
and output from OSC 133 marks and appends it to `MuxState::recent_agent_pane`'s answer for that
window. Both verbs are daemon-side commands like `capture-pane`; see
[the command set](/tmux/commands.md).

# Workspace environment for the ACP child

Every ACP child is additively given `ZZ_PANE` (its `%id`), `ZZ_SESSION` (the attached session name),
and `ZZ_SOCKET` (the daemon endpoint, passed explicitly because a `--socket` launch leaves it out of
zz's own environment). Injection follows the shape `with_macos_executable_path` established: match
`McpServer::Stdio`, skip any name the user already configured through `agent-command`, push the
rest. A configured value always wins. So an agent inside a pane can run
`zz tools` and then `zz agent-send -t %5 …` with no MCP server; the CLI is the tool surface.

# Session history

When an initialized adapter advertises `sessionCapabilities.list`, the pane header exposes a
**History** picker. Its default scope passes the pane cwd to `session/list`; **All projects** omits
that filter. Opaque pagination cursors are returned to the adapter unchanged. Listed records are
accepted only when their session ID is bounded and control-free, their cwd and additional roots are
absolute, and their optional title/timestamp metadata is bounded. The picker searches title, cwd,
and opaque ID locally and renders its result rows with a uniform virtual list, so a large provider
catalog does not inflate the normal transcript render tree.

Explicit switching is allowed only from an idle pane with no unresolved permission request. zz
routes the selected ID to a staging buffer before issuing `session/load`; replay notifications do
not touch the visible thread while the request is pending. A successful response atomically resets
the reducer, applies the staged replay, adopts the selected cwd/configuration, persists the new
descriptor, and closes the former ACP session when `session/close` is supported. A failed load
discards its partial replay and route, closes the failed target when possible, and returns the pane
to ready with the former transcript and session binding intact. Starting a fresh session uses the
same post-success swap boundary.

The picker supports explicit refresh, cursor-based **Load more**, and new-session creation. Delete
is shown only when the adapter advertises `session/delete`, requires a second destructive
confirmation, and refuses to delete the active session. A session already bound to another pane
for the same provider cannot be opened twice. Its dense two-line rows reserve the first line for the
provider title and flex the cwd across the remaining metadata line instead of letting technical
fields collapse it. Updated timestamps are parsed from RFC 3339 and shown in the user's local time
with compact calendar labels such as **Today, 18:01**; opaque IDs remain searchable but are not
rendered. The narrower, headerless picker opens directly on its compact search and filter toolbar,
keeps a compact labeled **New** action at the same scale as the filter controls, and dismisses
through Escape or a backdrop click. Search refiltering listens only for content changes, so the
input's 500 ms caret repaint cannot reset selection or scroll. Async catalog updates preserve both
keyboard selection by opaque session identity and the user's viewport when that identity survives,
while only pointer movement can hand selection back to a hovered row. Its footer reuses the
tmux command palette's outlined keyboard-hint badges.

# Streaming reducer and rendering

The controller reduces ACP notifications into provider-neutral entries with stable native IDs:

- `User { id, markdown, images }`
- `Assistant { id, markdown, memory_citations }`
- `Reasoning { id, label, markdown, default_expanded }`
- `Plan { id, markdown }`
- `Tool { id, protocol_id, kind, status, label, location, input, output, default_expanded, subagent, children }`
- `Notification { id, task_id, status, summary, result_markdown }`

Tool input and output use the provider-neutral `ToolPayload` variants `Diff { path, old, new }`,
`Text`, `Json`, and `Terminal`. ACP diff and terminal content stay typed instead of being serialized
as JSON containing full escaped files. Text content blocks remain text, while raw input, raw output,
and unsupported content become pretty JSON without Markdown fences. Structured content has priority
over `raw_output`, including when a later raw-output update arrives; raw output is displayed only
while the structured content collection is empty.

`agent/profile.rs` is the only provider-artifact recognition seam. Its streaming scanner carries
partial opening markers across ACP chunk boundaries and recognizes only the explicit table:
Claude's `<task-notification>` and `<system-reminder>`, and Codex's `<oai-mem-citation>`,
`::git-stage{...}`, and `::git-commit{...}`. Unknown XML and directives remain literal. Task
notifications become durable transcript cards even when they arrive as pseudo-user text during
local user-echo suppression; reminders and app-control directives are removed. Memory citation
entries are parsed and retained on the owning assistant entry for provenance, without a citation
chip UI. At the end of a loaded replay, adjacent identical sanitized assistant answers are
deduplicated and their citations merged; live turns are never deduplicated.

The envelope only exists in replayed history: live, the claude adapter never forwards it. Live
cards come from the raw SDK passthrough instead . `session/new` and `session/load` carry
`_meta.claudeCode.emitRawSDKMessages` filters for `task_started`, `task_updated`, and
`task_notification`, and the resulting `_claude/sdkMessage` extension notifications (whose method
arrives with the reserved `_` prefix stripped by the ACP crate's enum parser . matched
prefix-insensitively) parse into `SdkTaskEvent`s. A `task_started` for an agent task
(`task_type: "local_agent"`; background shells are excluded) registers the live task and holds its
Task tool at Running . async Task tools otherwise report `completed` at launch . through later
tool updates and through turn-end force-settling, until the task's notification or terminal
`task_updated` patch lands the real status. Notification cards upsert keyed by `tool_use_id`, so
the live SDK event and a later replayed envelope share one card, and only notifications carrying
an `output_file` (agents, not background shells) become cards at all.

Claude Task tools carry `_meta.claudeCode.subagent = true`. When a later session update carries
`_meta.claudeCode.parentToolUseId`, the reducer attaches the resulting entry to that tool's
`children` instead of the top-level transcript. A nested Task ID maps back to the same root, so
deeper descendants flatten into one depth-1 child timeline; an unknown parent falls back to the
top level rather than losing output. Child mutations bump the owning root tool's revision, and its
disclosure renders the children through the normal entry renderers as an indented mini-timeline.

Terminal-capable tool calls initialize `ToolPayload::Terminal` from `_meta.terminal_info` and the
tool's raw command/cwd, append exact `_meta.terminal_output.data` frames, and add the
`terminal_exit` code/signal without allowing the final structured Terminal content to overwrite
the accumulated stream. The convention is metadata-only: zz does not advertise or implement
client-owned ACP `terminal/*` methods.

Message chunks coalesce by ACP `message_id`; chunks without one coalesce only while their role stays
active. The locally inserted user entry suppresses the matching streamed echo for that prompt.
Tool calls retain one native entry across incremental updates, map ACP kind/status fields, expose the
first affected location, and render structured output. Plan notifications replace one checklist
entry instead of appending duplicates. Session title, current mode, generic config, available
command, and usage updates feed the header/composer; a title update also becomes daemon pane-title
metadata. A successful prompt or completed history replay settles any tool still marked pending,
running, or awaiting approval to completed; prompt failure settles those tools to failed, while
cancellation keeps the canceled terminal state. This boundary prevents an adapter that omits a final
per-tool update from leaving a permanent spinner in an otherwise finished turn. Tool updates that
arrive after the pane has already returned to ready are treated as late replay and settled to
completed immediately; live turn updates remain non-terminal while the pane is running.

The lossless runtime channel remains unbounded, but the GPUI-side receiver drains bursts of up to 256
already queued events into one ordered controller transaction and one pane notification. Each thread
maintains a stable ID-to-index map and a monotonic revision per entry. The reducer remains flat;
the sole structural exception is the depth-1 `children` vector owned by a subagent tool.
`AgentView` folds top-level entries into presentation-only `TimelineRow` values and retains an
entry-index-to-row-index map. Two entry kinds fold (`Tool` and `Reasoning`, the ones that repeat),
and an uninterrupted run of either shares one row once it has at least two members. Any entry of a
different kind ends the run, so a tool between two thoughts splits them and a group never mixes the
two. A member revision therefore remeasures its owning group row rather than its flat entry index.
Appending another member updates the trailing group without a `ListState` splice, regardless of its
label; an append that starts a row splices exactly one row. Tool label changes preserve the fold,
while a replacement that changes an entry's group kind, and other non-append mutations, take the
full reset-and-refold path. Same-length streaming updates continue through `Arc::make_mut` instead
of rebuilding the complete history.

For a changed existing entry, that same sync path updates any already-materialized body Markdown in
the store: a prefix extension goes through `TextViewState::push_str` for incremental background
parsing, while a non-prefix replacement uses `set_text`. Tool payload synchronization instead
compares the typed source with its materialized store entry; equal payloads retain the existing line
cache without splitting or diffing again. The render path never compares or replaces existing
Markdown state.

The transcript is a variable-height GPUI `ListState`, so only visible rows plus 1200 px of overdraw
are measured and rendered. Every row uses a centered 680 px content column with responsive side
padding; the composer extends one pixel beyond that column on each side. Its native tail-follow mode
keeps new output in view while the reader is at the bottom, stops following when the reader scrolls
upward, and resumes when they return to the tail or submit a new prompt. The composer floats over the
bottom of the full-height transcript without a section divider; internal timeline tail clearance
lets content travel beneath it during scrolling while allowing the final row to clear the default
composer when scrolled fully to the end, with 50% extra runway beyond that default footprint. Its
rounded card uses an opaque popover surface, while a pane-background occlusion strip spans the lower
half of the default composer footprint. Transcript rows therefore disappear as they cross behind the
composer and cannot reappear through its bottom gutter. Controller updates for other panes or
unchanged pane metadata do not invalidate this view.

The pane-owned timeline store keys body Markdown and materialized tool content separately by entry
ID and slot, disclosure state by `(entry ID, disclosure kind)`, and expanded-tool uniform-list scroll
handles by entry ID. Reasoning, member-tool, and group disclosures therefore cannot alias even when
a group uses its first member's stable entry ID.
A never-seen virtual row still parses or splits lazily when it first becomes visible; once
materialized, its `TextViewState`, cached tool lines, disclosure choice, and tool viewport survive
frames where transcript virtualization does not render the row. Non-append timeline rebuilds and
entries-gone resets clear the entire store so reused provider IDs cannot alias state across session
switches. Markdown keeps the active light/dark syntax theme and process-stable extension registries.
Assistant-authored fenced `markdown`/`md` blocks are promoted to nested rich Markdown; this bounded
nested plugin is the sole remaining window-keyed Markdown state because it has no timeline-store
access. Other code fences remain literal.

A second plugin recognizes fenced `mermaid` blocks and renders resvg-safe SVG off the UI thread with
`merman`. The renderer maps virtual UI font names to a conservative `system-ui, sans-serif`
measurement stack, feeds semantic colors through Mermaid's theme variables, and adds scoped SVG
overrides for variants that otherwise emit fixed light-theme styles. Gantt and XY charts use the
transcript's 640 px content width; other oversized diagrams keep their native readable size inside a
bounded two-axis scroll viewport instead of being scaled until their labels are illegible. The
resulting raster is stored in a 16-image global cache keyed by source, scale, font, every semantic
color used by the SVG theme, and appearance. Per-node state also includes the source offset so
distinct blocks do not alias, while identical diagrams can share a raster; Mermaid theme
configuration is built only on a cache miss.

User prompts use a neutral input surface. Tool
disclosures are compact, fixed single-line borderless rows with one action icon, an ellipsized
label, a status-only indicator, and a chevron when details are available. Single-line enforcement
lives in code, since style cannot do it: `whitespace_nowrap` and `text_ellipsis` only govern
wrapping, while gpui shapes text by splitting on `\n` first and truncates by cumulative character
width without noticing the breaks, so a label with an embedded newline lays out two full-height
lines in a 28px row and the row's own overflow mask cuts both in half.
Agents send such labels routinely (one Bash call with two
commands arrives as one title with a newline between them), so every label bound for a fixed-height
row goes through `single_line`, which joins the pieces with ` · `. Clickable rows retain a
pointer cursor without painting a hover background. Pending, running, and approval states use an
animated spinner; completed uses a checkmark; failed and canceled use an X. A run of two or more
adjacent tools, regardless of label or action, renders one collapsed group header with the first
member's icon and a first-seen, count-aware action summary such as **Ran command, Edit files, Read
file**. Aggregate status uses worst-first precedence: Failed, Canceled, NeedsApproval, Running,
Pending, then Completed. A run of thoughts collapses the same way, under a **Reasoning · N steps**
header with no status column, since a thought has no outcome to report.
Opening a group reveals the original compact member rows, and each member independently controls its
content as a second disclosure level. Payload previews stay out of collapsed rows and remain
available only inside the expanded detail view.

Expanded tool payloads never enter Markdown or `TextView`. The store splits text and JSON into
monospace rows and computes ACP diffs once with `similar`; a single `uniform_list` for the tool
viewport shapes only visible lines. Diff rows include a path header, a `+`/`−` gutter, and subtle
success/danger line tints. Each payload retains at most 10,000 cached display lines and adds a muted
truncation row when longer. Text, JSON, and diffs retain the first 10,000 lines; a terminal retains
the newest 10,000 and moves its persistent inner scroll handle to the tail on each frame. The typed
source remains uncapped so the Input and Output section headers can offer **Copy** actions for the
complete raw payload. Machine rows are intentionally not text-selectable; copying the complete
section replaces the former partial-selection behavior of Markdown tool output.

Each expanded tool keeps its persistent uniform-list scroll handle and vertical scrollbar. Wheel
input pins the surrounding transcript to its live event-time offset while the inner viewport can
move, then chains back to the transcript at either edge; location-only and input-only tools remain
expandable.
Inline backtick spans use mixed font runs so surrounding prose stays in the UI family while code
uses the primary terminal family resolved from Ghostty. Their fill uses the neutral muted surface
instead of the accent color, while nested bold, italic, strikethrough, and link styling is retained.

# Composer, permissions, and failure states

The native composer is a centered, bordered input card that auto-grows from two to eight lines.
Enter sends when the session is ready; Shift+Enter inserts a newline. During a turn the icon action
becomes Cancel, which emits `session/cancel`. The controller returns to ready on ACP completion and
marks in-flight tools cancelled when the stop reason is `cancelled`.

A view-local sticky strip occupies the absolute composer overlay immediately above the card. It is
derived from durable transcript state: running Claude subagent Task tools remain until their tool
status settles (held at Running for the whole background life of an async agent by the live-task
registry), while task-notification mirror rows show only for notifications positioned after the
last user entry in the timeline . submitting a prompt therefore acknowledges everything on screen,
and a session reload cannot resurrect rows that a later replayed prompt already superseded. Rows
are also individually dismissible; dismissing or acknowledging never removes the transcript card.
An empty strip mounts nothing and contributes zero clearance, leaving the composer
pixel-identical to the no-alert state; all strip chrome uses existing theme tokens.

Pasting an image attaches it to the draft. A text field has nowhere to put one, so `InputState`
forwards it as `InputEvent::PasteImages` instead of dropping it, and the composer holds the
attachments (a draft is the view's, not the controller's) as a wrapping thumbnail strip above the
input, each with the control that removes it. An image alone is a whole prompt, so submit no longer
requires text. `agent::attachment::normalize` runs once on paste: SVG is refused, anything outside
png/jpeg/gif/webp is transcoded to PNG (macOS pasteboards routinely offer only TIFF), and anything
past a 1568px long edge is scaled (1568px is the ceiling Claude applies anyway, so the original only
costs upload). Because it runs once, the composer's thumbnail, the transcript's, and the base64 on
the wire are the same bytes. The prompt travels as `session/prompt` content blocks: the text first, then one
`ContentBlock::Image` per attachment, inline rather than as a path since a screenshot never had a
file and the agent may not share our filesystem. Attaching is gated on the `image` prompt capability
from the handshake. Claude Agent ACP advertises it; an agent that does not is refused at paste
time rather than mid-turn. Sent images render in the user's own message, and because an inbound
`ContentBlock::Image` decodes back into the entry, they survive a `session/load` replay instead of
degrading to an `*[Image: image/png]*` placeholder.

An attachment renders as a fixed square tile (140px in a message, 56px in the composer) with
`ObjectFit::ScaleDown` fitting the picture inside without ever enlarging it, and a click opens it in
a dialog large enough to read. The square is the sizing rule: layout never consults the image, so
it cannot depend on when the decode lands, and a tile measures the same whatever was pasted into it.
Sizing a thumbnail from the image is what broke before: one axis plus a `max_w` bounds the image and
not its parent, because taffy drops max sizes while measuring a node's content contribution
(`SizingMode::ContentSize`), so a wide screenshot painted clamped inside a bubble measured against
its *unclamped* aspect width; `items_end` anchors the bubble's right edge, so the overhang ran off
the pane's left edge where the text was unreadable. The bubble is also capped at 100% of the content
column rather than a number of its own, which keeps it inside a column narrower than
`AGENT_CONTENT_MAX_WIDTH`.

Restoring a session has to undo an agent's own lossy replay. Claude Code replays an attachment as a
`ContentBlock::Image`, which decodes straight back. Codex does not: it stores the attachment
faithfully as `{"type":"input_image","image_url":"data:image/png;base64,…"}`, but `codex-acp`'s
`userInputToContentBlocks` turns an image input back into text (`[@image](data:…)`), so a restored
transcript showed a link where the picture had been, and clicking it asked macOS to open a URL a
megabyte long. `split_inline_images` lifts `data:` image URIs out of replayed user text, taking the
Markdown link they sit in with them, and anything undecodable stays as prose rather than being
silently swallowed. Independently, the Markdown renderer refuses to hand `data:` and `javascript:`
links to the platform opener, since neither means anything to it.

One clipboard limit comes from gpui: every platform's clipboard read returns the first kind it
finds, text before images, so an image copied from a browser (which also offers its URL as text)
pastes as that text. Screenshots and image-only boards are unaffected.

The provider picker lives in the pane header. It offers Codex and Claude Code under their vendors'
marks (the OpenAI and Claude glyphs, Simple Icons artwork beside the Lucide set that `zz-ui`
otherwise ships, since Lucide draws no vendor logos), is disabled during an active turn, and starts
a fresh provider-bound thread on selection. The mux persists the choice via `set-agent-provider`,
clearing the old opaque session ID so it is never loaded by the wrong agent.

The header's other end holds the working-directory picker: the pane's cwd by its last component,
its full path in the tooltip, and a native folder chooser behind a click. An ACP session is bound to
the directory it was created in, so choosing another one is `session/new` there rather than an
in-place move (the same boundary the History picker's **New** crosses), gated the same way on an
idle pane with no unresolved permission request. The pane's `cwd` is left alone until the agent
answers, so a failed switch keeps the pane where it was. The header carries no connection badge:
liveness reads off the composer, whose action is Cancel during a turn, and off the error cards.

ACP `AvailableCommandsUpdate` notifications drive the command-completion popover. Codex uses `$`
as its completion sigil while Claude Code uses `/`; either opens only at the start of a line or after
whitespace. Every matching item advertised by the agent remains available in a bounded, vertically
scrollable menu backed by a uniform virtual list, while only six rows are mounted at once. Up/Down
actions are captured before the multiline input so they select a result and keep it visible; Tab or
Enter accepts, Escape dismisses, and unstructured command input hints remain visible after
insertion. The description column consumes the width left by the type badge before truncating.
Ellipsis-only ACP description placeholders are suppressed while meaningful descriptions remain
visible. The menu closes once the user begins the command argument.

The composer renders generic ACP `Select` options categorized as `Mode`, `Model`, and `ThoughtLevel`
as the permission, model, and effort pickers. It sends each opaque config ID/value through
`session/set_config_option` and adopts the complete option vector returned by the agent. When an
older agent supplies no generic config options, the permission picker falls back to legacy
`SessionMode` and `session/set_mode`. The compact triggers use the same text scale as their entries,
and each popover opens directly on the available choices without a redundant category heading.
Neither provider's concrete values are hard-coded in zz.

Successful user selections for model, effort, and permission mode are sticky. They are stored in a
bounded, versioned `agent-preferences.json` under zz's platform application-data directory, with
user-only directory/file modes on Unix. Keys include provider, initialized agent identity, absolute
cwd, semantic setting kind, and the agent's opaque option ID, preventing one adapter or workspace
from applying values to another. On every new or loaded session, advertised choices are reconciled
serially in model → effort → permission order. Missing or stale choices are ignored; a rejected
restore is skipped for that session instead of looping. Legacy `SessionMode` permission values use
the same mechanism even when unrelated generic options coexist. Provider updates and individual
tool-approval answers are never persisted as user preferences.

An ACP `session/request_permission` request retains its exact responder and option IDs. The UI renders
every agent-supplied allow/reject choice plus an explicit cancel action. Closing a pane, cancelling a
turn, or shutting down responds `cancelled` to every outstanding permission before dropping the
connection. Authentication methods advertised during initialization appear beside retry when a
session fails, and send ACP `authenticate` requests.

Launch, initialize, new/load session, prompt, and authentication failures become pane-local error
cards with retry. Unexpected process exit does not spin in an automatic restart loop; the user can
retry after fixing credentials or configuration. Application shutdown closes every session when
the adapter supports `session/close`, otherwise it sends cancellation; in either case it resolves
approvals, waits for the runtime task, and then allows the ACP child guard to reap the process.

# Persistence boundary

The daemon persists only `AgentDescriptor { provider, cwd, session_id }`, not messages, tool
payloads, credentials, or provider state. Conversation durability therefore depends on the selected agent's
`session/load` implementation. If it can load, its replay notifications reconstruct the native
timeline; otherwise zz creates a fresh session and replaces the descriptor's session ID. This is
deliberately different from terminal persistence: the daemon never owns or keeps the ACP child alive
after all GUI windows close.

Sticky selector preferences are a separate GUI-owned store because they are user intent rather than
conversation data. They contain only opaque advertised option IDs/values and their scope, never
messages, prompts, credentials, approval decisions, or a copy of provider session files.

# Verification

- Reducer tests cover message coalescing, artifact chunk boundaries, notification routing, replay
  deduplication, parent/orphan/depth-flattened subagent routing, terminal info/output/exit frames,
  entry revision/index alignment, stable tool IDs, plan replacement, permission state,
  command/skill discovery, generic options, and session-ID bounds;
  UI tests cover append-only UTF-8 Markdown detection and Mermaid output; composer tests cover
  provider-specific completion sigils, matching `$` skills without requiring the stored prefix,
  complete result sets, suppression of ellipsis-only description placeholders, and real GPUI
  Up/Down/Tab routing ahead of the multiline input.
- An in-process ACP peer test covers initialization, replay during load, load-to-new fallback,
  advertised authentication and surfaced auth failure, client config capability negotiation,
  streamed tool/thought/command/config updates, exact approval/config/mode selection, cancellation
  while waiting for an approval, prompt stop reasons, and graceful shutdown through the real ACP
  connection machinery.
- A second ACP peer test covers capability discovery, cwd-filtered listing and pagination metadata,
  sanitization, additional-directory restoration, discard of partial replay on failed load,
  preservation of the former prompt binding, atomic successful switching, close, delete, and
  shutdown. Preference tests cover bounded private persistence, workspace/provider scoping, and
  model → effort → permission restore order including legacy mode fallback.
- Timeline tests cover both folds: a tool run and a thought run collapse independently, a lone
  member stays a plain row, an entry of the other kind ends a run, and a member revision remeasures
  its owning group row rather than splicing.
- Mux/server tests cover descriptor round trips, session/cwd validation, stable picker
  materialization, live donor-cwd capture, and donor fallback to the last focused terminal when the
  split target has no working directory of its own.
- Config and view-adapter tests cover command/JSON parsing, absolute cwd validation, and every core
  native entry shape.
- Workspace-verb tests cover `agent-send` option parsing (detached, attached, and `--` forms),
  payload bounds and control-character rejection, context-header fencing, the stdin decision the CLI
  shares with the daemon parser, the full round trip through an attached client mailbox, and failure
  when that client disconnects. Environment tests assert `ZZ_PANE`/`ZZ_SESSION`/`ZZ_SOCKET`
  injection and that a user-configured value survives.

# Related

- [System overview](/architecture/overview.md)
- [Mux snapshots](/protocol/snapshots.md)
- [Application configuration](/configuration/app-config.md)
- [tmux command set](/tmux/commands.md)
- [zz](/crates/zz.md) and [zz-daemon](/crates/zz-daemon.md)
- [Split-pane layout](/concepts/split-pane-layout.md)
