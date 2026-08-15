---
type: Research Report
title: Rendering multi-harness agent output — industry survey
description: How comet, opencode, t3code, Zed, and ~40 other agent clients integrate and render Claude Code, Codex, and other harnesses; the state of ACP v1/v2; why zz's per-provider seams are the industry norm; and the ranked fixes for zz's dangling-spinner and settling bugs.
tags:
- agent
- acp
- claude-code
- codex
- subagent
- rendering
- survey
timestamp: 2026-08-15T00:00:00Z
---

# Overview

Six parallel research passes (August 2026) into how best-in-class apps render coding-agent
harness output, run to answer one question: is there a more modern, stable integration than
zz's ACP v1 + adapter-children architecture, whose per-provider seams (`_meta.claudeCode.*`
passthrough, spinner force-settling, placeholder-title denylists) feel unmaintainable?

**Answer: no. The architecture is correct and industry-standard; the pain is the state of the
art, not a zz defect.** Every high-fidelity client carries the same seams. The wins available
are narrow, specific, and listed at the bottom.

# The three-layer contract everyone converged on

At least four unrelated codebases (zz, comet, t3code, Vercel's `@ai-sdk/harness`)
independently landed on the same shape:

1. **A small normalized core** (~10–50 closed event variants: text/reasoning deltas, tool
   start/update/end, status, turn done, usage, error).
2. **A namespaced vendor-metadata channel** for semantics the core cannot express
   (`_meta.claudeCode.*`, `_meta.codex.*`, `harnessMetadata`).
3. **A raw passthrough escape hatch** (`_claude/sdkMessage`, `{type:'raw'}`).

zz implements exactly this. The differences between apps are in the *reducer discipline*
(settling, identity, reconciliation), not the transport.

# What each app actually does

- **comet (zeronsh/comet)** — the same stack as zz: Rust + gpui fork, ACP v1, the same
  `claude-agent-acp`/`codex-acp` adapters, the same placeholder-title denylist and
  `rawInput._toolName` sniffing. It *migrated to* ACP, deleting ~4,300 lines of bespoke
  stream-json adapters. Its superior Claude rendering is a layered turn-settle ladder
  (`_session/turn_ended` extension → Claude cost-frame hint with 1s grace → 30s quiet-settle
  gated on "fold shows nothing unresolved" → 120s/20s park-not-error watchdog → 1s resume
  gate separating post-turn echoes from self-continued work) plus **honest flattening**: a
  Task subagent is one flat chip resolved through the ordinary tool path. No nesting at all.
- **opencode (anomalyco/opencode)** — wraps no CLI; runs its own agent loop against provider
  APIs with a hand-rolled provider layer. Stability comes from normalizing once at the
  protocol boundary into a closed event vocabulary, a **durable/ephemeral event split**
  (deltas are live-only; every stream's `Ended` carries the complete value, so replay is
  exact and reconnect is "resume after seq N"), a **pure guarded fold** shared by server and
  clients, and **subagents as first-class child sessions** (`parentID`, rendered as a card
  linking to a navigable sub-session). Its own ACP export flattens `task` to a `"think"`
  tool-kind — ACP cannot express a child session.
- **t3code (pingdotgg/t3code)** — per-vendor native transports at full price: Claude via the
  official Agent SDK (49 message subtypes in one adapter), Codex via `codex app-server`
  JSON-RPC (types code-generated from pinned schemas), Cursor/Grok via self-generated ACP,
  opencode via SSE. ~20k LOC of adapters normalizing into a closed 48-variant union, then a
  wire model of closed `tone` + open `kind` + opaque payload. Anti-spinner discipline:
  three-state tool status where neutral flips to ✓ the instant the turn settles; turn
  completion derived from session status leaving `running`, never from a completion event;
  synthetic stable IDs for progress rows; subagent output never enters the transcript (one
  CTA row + a separate Agents panel; liveness reads the coordinator, not members). Their
  issue tracker documents ripping out idle-timer ACP settling and a Grok ACP adapter
  emitting ~1.1 MB/s of cumulative `tool_call_update`s that head-of-line blocked ingestion.
- **Zed** — avoids dangling spinners structurally: panel liveness derives from
  `running_turn.is_some()`, generic tool calls get a static kind icon (never a spinner), and
  only subagent/terminal cards animate. It does not force-settle on clean turn end (its
  terminal cards can dangle — zz is ahead here) and does not nest Claude Task output at all.
  Zed carries the same `_meta` conventions and ~6 named-agent launch hacks.
- **The dead pool** — Crystal, CUI, claude-code-webui, Vibe Kanban, opcode, Terragon: most
  2025-era Claude wrappers are dead or frozen. PTY/TUI scraping is the worst pattern
  surveyed (AWS's orchestrator: 1,305 lines of regex against TUI chrome, broken by a footer
  move). Embedding the vendor SDK does not protect against churn either (claude-code-webui
  died pinning an exact SDK version). Vibe Kanban's maintainer verdict on per-vendor
  executors: "the only realistic path … is if your executor supports ACP."

# State of ACP (August 2026)

- Governance moved to a neutral org (`agentclientprotocol/*`), co-led by Zed and JetBrains,
  ~200 merged PRs/month, 38 registry agents with daily CI conformance probes, five SDKs.
  First-party ACP agents: Gemini CLI, Copilot CLI, Cursor, Goose, Kiro, Devin, ~30 more.
- **The two flagship integrations are shims**: Anthropic and OpenAI ship no ACP.
  `claude-agent-acp` (8,700-line adapter, ~2 releases/week, 121 open issues, zero Anthropic
  commits ever) wraps the Agent SDK; `codex-acp` wraps OpenAI's `app-server` protocol.
  Claude Code's ACP feature request was closed by a stale-bot at 437 👍.
- **Market pattern: "ACP agent yes, ACP client no."** Clients are Zed, JetBrains, Devin
  Desktop, and a long tail. Microsoft built AHP for VS Code (host-owned sessions, N clients,
  replay — architecturally what zz's daemon already is, internally); Google sunset the
  ACP-native Gemini CLI for closed-source `agy` with no ACP; Warp does bespoke per-agent
  integration (its ACP request is 11 months unanswered — clean multi-harness support is an
  open differentiator against them).
- **v1 is what ships; v2 is a draft nothing speaks** (0 of 31 probed agents; Zed pins v1).
  v2 fixes: `state_update` running/idle/requires_action decouples turn end from the prompt
  response (kills force-settling as a concept, allows background work while idle),
  first-class `terminal_update`/`terminal_output_chunk` (the RFD cites the `_meta.terminal_*`
  convention zz uses as its motivation), unified `tool_call_update` upserts, structured
  diffs, open enums with `x-deserialize-default-on-error`. The `tool_call_name` RFD retires
  placeholder-title denylists.
- **Subagents are in neither v1 nor v2.** PR #855 (child sessions, `ToolKind: subagent`) has
  been a stale draft since March 2026; "subagent rendering" is a roadmap bullet with no RFD
  author. Issue #1847 (tool calls that outlive their turn) has zero maintainer replies.
  zz is one of very few shipping clients with nested subagent transcripts — real standing to
  push both.
- Adapter bug inventory that directly explains zz symptoms: claude-agent-acp drops all
  `task_*` events from the wire by default (hence the raw-SDK passthrough); #865 background
  tasks report `completed` at launch and nothing later flips it; #896 PromptResponse and the
  final usage frame are sometimes withheld until cancel; #851 background-subagent permission
  desync deadlocks the session. Maintainer position (#824): out-of-turn updates and
  non-terminal tool statuses are won't-fix in v1 — **client-side settling is the required
  contract, not a workaround**.
- No competing protocol exists for GUI↔local-agent: AG-UI/A2A/A2UI are HTTP/cloud-shaped
  with no stdio story; MCP's 2026-07-28 revision deprecated Sampling/Roots and went
  stateless; Codex app-server is richer than ACP v1 (steering, durable forkable threads,
  first-class `collabToolCall` subagent threads) but single-vendor with version-pinned
  generated types, no stable published spec.
- Commercial footnote: Anthropic's planned June 2026 split of subscription billing for
  third-party surfaces (ACP / `claude -p` / SDK) was announced and then paused; currently
  benign, structurally a live risk to every wrap-the-CLI strategy.
- Client-side field check (source-verified across ~12 open clients, 2026-08-15): **zero have
  begun v2 adoption**; client-owned `terminal/*` is rare (hermes.nvim, the JetBrains Kotlin
  SDK, obsidian-agent-client) and v2 deletes it anyway, so zz declining it remains correct;
  JetBrains ACP is GA across all IDEs since 2025.3 with no AI subscription required.
  **hermes.nvim** (Ruddickmg/hermes.nvim, Rust core on the official
  `agent-client-protocol = "1.0"` crate with `unstable_session_fork`/`unstable_mcp_over_acp`
  enabled, full client trait) is the best public reference for zz's own SDK bump; marimo is
  the cautionary tale, stranded on the deprecated pre-rename 0.4.x SDK.

# Ranked fixes for zz

Near-term (directly addresses dangling subagent spinners and empty "Done" cards):

1. **Bump `claude-agent-acp` 0.63.0 → ≥0.68.0** and handle the `_session/turn_ended`
   extension — deterministic settling for autonomous/self-continued turns. Gate on
   "no prompt outstanding" + session-ID match (comet `acp/mod.rs:2371`).
2. **Widen the raw-SDK filter**: subscribe `tool_progress` (`tool_use_id`,
   `parent_tool_use_id`, `elapsed_time_seconds`, `subagent_type`) and `task_progress` —
   this is precisely "subagent spinner with live elapsed time and current tool" — plus
   **`background_tasks_changed` (REPLACE semantics) as the reconciliation backstop**: swap
   the whole live-task set on each event so a lost `task_notification` cannot leak a
   spinner forever. waku's `BackgroundWorkEvent::{Upsert, ReconcileLive}` is the reference
   shape (edge events + authoritative level snapshots).
3. **Fix the `is_agent` discriminator** (`zz-daemon/src/agent/profile.rs:99`): keying on
   `task_type == "local_agent"` is undocumented; the documented, stable field is
   `subagent_type` presence (reference adapter uses `!!message.subagent_type`).
4. **Reconcile the two task-notification paths on `task_id`**: the prose `<task-notification>`
   scraper is load-bearing only because it alone supplies `result_markdown`; the structural
   `SDKTaskNotificationMessage` should be primary, prose the fallback. Empty summaries must
   not render a bare "Done" card.
5. **Adopt Zed's structural spinner rule**: derive pane busy-chrome from turn liveness, not
   tool statuses; render static kind icons for generic tools, animating only subagent and
   terminal cards. Makes any residual dangling status cosmetically invisible.
6. **Adopt t3code's turn-settle flip**: any tool still neutral when the turn settles renders
   ✓ (zz's force-settle already writes Completed; extend the same rule to notification rows
   and the sticky strip so submitting/settling acknowledges everything).
7. **Add comet's stale-echo filter**: track tool IDs folded per turn segment; drop
   post-`Done` echoes (late `tool_call_update`s within ~1s) instead of splicing phantom
   entries; treat genuinely-new output after the gate as a self-continued turn.

Medium-term:

8. **Bump the ACP SDK** from crate 1.2.0/schema 1.4.0 to crate 2.x/schema 1.20+ (still
   protocol v1): stable elicitation, session fork/resume, `tool_call_name` (unstable) which
   retires the placeholder denylist. The 2.0 API is a substantial redesign; budget for it.
9. **Split the journal durable/ephemeral** (opencode's highest-value idea): journal
   full-value boundary events, not raw deltas; replay becomes exact and cheap, and the
   16 MiB ring stops re-shipping token-level chunks.
10. **Formalize `profile.rs` into a provider-profile trait** — the seams are correct but
    scattered; one trait per provider (artifact scan table, subagent linkage, settle hints,
    SDK filter list) makes the next provider a data file, not a hunt.
11. **Build the v2 negotiation seam now, ship v1**: adopt open-enum tolerance and
    `_`-prefixed extension handling immediately (costs nothing, v2-proof); keep v1 default
    until upstream stabilizes.

Strategic:

12. **Upstream from strength.** File zz's wire captures on ACP PR #855 (subagent sessions)
    and issue #1847 (tools outliving turns); propose a `_meta`-forwarding contract for
    `tool_progress`/`task_progress` in claude-agent-acp (its #56 asks for exactly this).
    The maintainers listed "subagent rendering" as next up with no owner.
13. **Do not build per-vendor native transports** (t3code's path): it works but costs ~20k
    LOC of adapters and permanent churn-chasing for a fidelity delta that upstream ACP work
    is actively closing. Exception worth watching: if zz ever wants deep Codex-only features
    (turn steering, thread forking), `codex app-server` is the stable-contract door, and
    `codex-acp` already vendors its generated types.
