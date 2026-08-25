---
type: Design Plan
title: tmux drop-in plan
description: "The original alias-tmux=zz campaign and its shipped phases, followed by the live compatibility ledger: tmux names retain tmux meaning, zz power uses superset verbs, and linked windows plus real-tmux socket interop stay excluded."
status: Original nine phases shipped 2026-08-20; the approved core campaign closed 2026-08-22; selected F and G follow-up slices shipped, while client, terminal, stream, and model contracts remain open or parked
tags:
- tmux
- compatibility
- drop-in
- layout
- control-mode
- roadmap
timestamp: 2026-08-25T00:00:00-03:00
last_updated: 2026-08-25
---

# Overview

Original goal: `alias tmux=zz` works for a tmux user's invocations, config, scripts, and common
key habits, while zz-only power lives in superset verbs that never collide with tmux names. The
current target is the narrower, testable “compatible enough” workload in the
[tmux superset roadmap](/designs/tmux-superset-roadmap.md). The live deltas are enumerated in the
[divergence matrix](/tmux/divergences.md) and the source-anchored
[2026-08-22 audit](/research/2026-08-22-tmux-cli-compatibility-audit.md).

Revised same day after an adversarial review, every claim of which was verified against the
tree: the differential harness moved off control mode onto `list-* -F`, the exec/consent story
was rebuilt around the fact that config already executes shell ungated, and the TTY attach
contract — the part of the alias the original six phases never scheduled — became phase 8.

The target splits in two:

- **Config/script drop-in** (phases 0–7): configs import and behave, scripts that create,
  query, target, and kill sessions work, TPM and resurrect-class plugins run. This is the bulk
  of the alias and none of it needs a TTY-attaching client.
- **Full drop-in** (phase 8): bare `tmux`, `tmux new -s foo`, and `tmux attach` attach the
  calling terminal, via the [TUI client](/designs/tui-client.md). **Closed 2026-08-20.**

**Decisions (2026-08-16, fabrico):**

1. The exec-family refusal (`run-shell` etc.) is lifted. The consent gate guards the
   *import flow* only — a UX safeguard, not doctrine (details in phase 5).
2. Cell-authoritative layout is approved. Verified to have no rendering or benchmark
   performance impact; the only visible GUI change is dividers stepping by cells (with a
   smooth-drag/commit-on-release option).
3. Linked windows and session groups (`new-session -t`) are **skipped permanently** — the
   single "100% minus" item. One window belongs to one session; the rejection stays loud.
4. Interop with a real tmux binary over its private socket protocol is a non-goal. The alias
   means zz handles tmux's argv everywhere; it never speaks tmux's client-server wire format.

**Decisions (2026-08-20, fabrico):**

5. Protocol bumps proceed append-only as waves need them; the hardware-smoke list is
   verification debt, not a merge gate. `switch-client` takes v70, C3 takes the next.
6. Surface rule for client-level behavior (`switch-client`, `detach-on-destroy`, the
   lifecycle flags, `mouse`, `escape-time`, the `[detached]` notice): **zz-tui is tmux-exact
   including defaults** — its session dying exits to the shell unless the config says
   otherwise; **the GUI keeps zz behavior at defaults** and honors these options only when a
   config sets them explicitly. `switch-client` on the GUI is a sidebar focus change, never
   a detach.
7. The flag-level sweep runs to **zero non-parked flags**; "100% of the command grammar"
   means flags too. The divergence matrix's flag table is the tracker.
8. Execution loop unchanged: codex implements from a settled plan, full gates, grok-4.6
   adversarial pin review, close.

**Decisions (2026-08-21, fabrico):**

9. The core campaign is approved in the recommended order: `A3 -> B with C3 title
   production -> C except C5 -> E -> D2 -> D4 -> D3 -> D1`, with the approval-audit
   amendments below folded in. C5, lock/process execution, binary streaming, and
   session-scoped prefixes stay parked.
10. Each reviewed wave commits to `main` locally; pushes stay explicit.

# Where this stands (2026-08-22)

**All nine original implementation phases shipped.** A human typing `tmux new -s foo` lands inside
the session, explicit attach works, and bare packaged `zz` now lazily creates and attaches session
zero on an empty daemon. Strict
config/script indistinguishability is not a current claim: the live ledger still has unsupported
flags and documented semantic divergences.

| Phase | State |
| --- | --- |
| 0 — the floor | shipped 2026-08-16 |
| 1 — superset rework + stock-binding blockers | shipped 2026-08-16 |
| 2 — the differential harness | shipped 2026-08-16 |
| 3 — cell-authoritative layout | shipped 2026-08-17 |
| 4 — the grind (six waves) | shipped 2026-08-17 |
| 5 — the exec family | complete 2026-08-18 |
| 6 — control mode (`-C`/`-CC`) | complete 2026-08-18 |
| 7 — the binary surface | complete 2026-08-18 |
| 8 — the attach contract | shipped 2026-08-20; empty-daemon regression repaired 2026-08-22 |

The last canonical acceptance inventory contains 79 differential scenarios and 1,296 executable
steps against pinned tmux `d77c9dc6`, including 17 config/plugin smokes. That complete strict run on
2026-08-25 left every ordinary row clean, with each known row at exactly its one documented GEO
difference and every other channel clean. A fresh canonical run is deferred until the current slices
settle. The checked-in summary is intentionally stale for `buffer-path-format`,
`command-item-format`, `display-message` (27 steps), `resize-directions` (16 focused steps versus 8
stored), `send-keys-repeat`, `smoke/cli-chain-parse-abort`,
`smoke/control-alias-prepare`, `smoke/source-file-control`, `smoke/source-file-diagnostics`, and
`source-file-format`; root will replace the totals and rows from that final run rather than hand-edit
them. The combined summary still records the attached-client fixture separately as `PASS`, and the
drift check is correctly failing on those named rows meanwhile.
`compat/attached-client.sh` drives real inner zz/tmux attaches through pinned-tmux PTYs, covering copy
mode, choosers, prompts, prefix tables, buffers, and nested attach. The Cargo launcher and a verified
built-bundle smoke both pass the six bare/new/attach × empty/existing cases through a spaced path.
Gate 0 provides durable evidence for the surfaces it exercises. The live tracker still records CLI
and TUI gaps, including startup config cause delivery, typed Control diagnostics, and asynchronous
Control output; native GUI rendering still needs separate visual smoke evidence.

**Options: all 180 of the pin's named options store; 105 have a behavior consumer.** The
remaining 75 are storage-only. `window-status-separator` joined on 2026-08-24 through the
daemon-expanded window loop. `BEHAVES` and its drift test make that split explicit. Some
consumers remain bounded rather than byte-exact, so the roster means “reaches behavior,” not
“fully compatible.”

**Commands: 83 of the pin's 92 verbs run; 9 are recognized but unimplemented** (four
native-chrome superseded, linked windows permanently excluded, and `new-pane`, `switch-mode`, and
`server-access` parked on separate missing models).
**Twenty-three of the 83 implemented commands still reject tmux flags:** exactly 85
catalog-declared pairs, inventoried in the matrix and enforced by
`the_unsupported_flag_ledger_matches_the_catalog`. Sixty commands have no catalog-declared
flag gap, but accepted semantic divergences remain outside that count.

**The current queue** is dependency-ordered in the
[tmux superset roadmap](/designs/tmux-superset-roadmap.md): mine real config hits for the next
rank-4/5 slice, then repair script-visible output before
closing the remaining bounded client contracts. Client targeting and ordinary detach are complete;
every implemented attached-client selector now shares exact name, full tty, exactly one leading
`/dev/` removal, exactly one optional trailing colon, no final basename, and global creation-order
collision precedence. Native aliases remain. Unsupported `command-prompt -t`, `show-messages -t`,
`send-keys -c`, `suspend-client -t`, and inert `set-buffer -t` keep their separate owners. Attached
`display-message` formats now widen a valid unattached target to the globally most-active attached
client when `-c` is absent, names a client attached to another session, or does not resolve. Equal
activity chooses the oldest-created client. The selected client's attachment supplies
`client_session`; session, window, and pane
facts stay on the target. An attached target without `-c` remains under `clients.context-formats`.
This format-only fallback does not alter delivery, duration, printing routing or lifecycle, buffer
paths, or Command-client selection. Attached cwd/flags/sizing, environment refresh, and exit actions
are tracked separately. Binary
streaming and process control require a separate design. Floating panes stay parked; linked windows and real socket interop stay permanently
excluded; ACLs remain parked outside the practical alias gate.

**Historical 2026-08-20 queue**, retained as campaign context rather than current status:

0. ~~**Oh My Tmux into the smoke corpus**~~ — SHIPPED 2026-08-20 (scenario green: 15 steps,
   zero divergences; after Wave 2a, zz's baseline was one skip, `send-prefix -2`, driven
   to zero by Wave C run 2 on 2026-08-21).
   Flushed out and fixed the same day: shell jobs missing the
   `set-environment` overlay, and stored-command rendering ignoring `args_print`.
1. ~~**`switch-client` and `detach-on-destroy`** (v70)~~ — **SHIPPED 2026-08-20**.
   Daemon-owned retargeting now covers `-c`/`-t`/`-T`/`-l`/`-n`/`-p`/`-r`/`-Z`, unsolicited
   `Attached` convergence, client/session activity and previous-session formats, every
   `detach-on-destroy` policy, reasoned detaches, and tmux-exact TUI exit notices. Oh My
   Tmux's `prefix BTab` now binds and the warning baseline fell from five skips to four.
   This was the single most common thing real
   scripts did that zz couldn't: 12 corpus hits across tmux-sessionizer (whose `$TMUX` branch
   7a made it take), tmux-resurrect's restore (8 calls), and Oh My Tmux's `prefix C-f` /
   `prefix BTab`. The protocol v70 wave reused `ProtocolMessage::Attached` rather than
   adding a second retarget message. The post-implementation pin review closed three mismatches:
   typed switches now reset the target client's key table unless a real `bind-key -r` binding
   invoked them, including switches aimed at another client with `-c`,
   attached connection contexts follow same-session window retargets, and same-session switches
   emit `%session-changed`. The divergence matrix records zz's per-client window focus and the
   unmodeled `focused`/`UTF-8` client flags. Surface rule: decision 6.
2. ~~**The flag-level sweep, Waves 2a and 2b**~~ — **SHIPPED 2026-08-20**.
   `set`/`setw -F` and `new-window`/`split-window`/`break-pane -P -F` removed eight
   pairs; shared filter/sort semantics for five list commands, both choosers, and
   `switch-client -O` removed another 22. The exact catalog ledger moved from 159 pairs
   across 38 tmux commands to 129 across 30. The proposed Wave 2e "TUI" bucket was too
   broad: its 45 remaining pairs split into 25 server/core behaviors, 13 true
   client/presentation behaviors, and 7 parked context/model cases. Future tranches follow
   that ownership instead of moving server work into zz-tui.
3. **The TUI consumes its options and client-surfaced state** — zz-tui does not depend on zz-mux and reads no tmux
   option: `mouse`, `escape-time`, `status-position`/`-justify`, the status styles, plus the
   13 genuinely client/presentation-owned flag pairs and the attach residue that shares the
   surface (the nested-tty refusal,
   `-x -`/`-y -` from the client's size — `ClientHello` carries no
   size today).
4. **The C3 knob batch** (v71) — largely landed: run 1 (2026-08-21) shipped parse-time
   `command-alias`, the `update-environment` seeding list, and the lifecycle trio; run 2
   (same day) shipped the full alert set (`monitor-*`, `*-action`, `visual-*`, the
   `#`/`~` window flags, `window-status-activity-style`) and `prefix2` with
   `send-prefix -2` (Oh My Tmux's smoke is zero-warning). The `set-titles` pair shipped
   with the B2/B3/title slice. Run 3 (2026-08-22) shipped `display-panes-format` and
   the renderer styles (`pane-*border-style` colors, `window-style`/
   `window-active-style` tinting, `mode-style` and the copy-mode match styles). Still
   open from this batch: `remain-on-exit-format` (parked on the terminal injection
   seam). The `BEHAVES` drift test tracks each slice.
5. **`source-file` diagnostics on the CLI** — exit 1 with the pin's `path:line: message`
   where today the plain CLI exits 0 silent. Revalidation found that parse and glob failures
   fit the current response types, but the zz-only unsupported summary cannot reach stderr
   with exit 0 because Command responses have no stderr channel. Keep the whole item parked
   until that response contract is approved rather than splitting one command's diagnostics.
6. **Optional waves** — the 7c error-wording appendix (25 `needs a value` sites, `unknown
   flag -X`, the `usage:` fallback), key-string parity (`C-zz` prefix
   strictness), the prompt-history
   pair, lock-program spawning on the TUI.
7. **Parked by decision** — `status-keys` vi prompt (half-vi is worse than none), linked
   windows and session groups (decision 3), floating-pane state (`new-pane`, `move-pane` placement
   flags), pane-mode transition (`switch-mode`), real-tmux socket interop (decision 4), the 21
   theme-palette options and `tree-mode-*` (no demand).

Hardware-pending items that need fabrico rather than code: the wave-B status-bar visual
smoke, a live `allow-passthrough` image smoke, the DCS-filter bench A/B, and an iTerm2
`-CC` run.

# Final compatibility campaign (planned at `57ae502`)

**CLOSED 2026-08-22.** The approved order `A3 → B with C3 title production → C except C5 →
E → D2 → D4 → D3 → D1` shipped in eleven slices, each implemented from a settled brief,
reviewed independently read-only against the pinned tmux source, iterated on findings, and
committed only after a MERGE-READY follow-up verdict. Measured results: `BEHAVES` 67 → 104
(the recomputed C target, 105 minus parked C5); unsupported command-and-flag pairs 129 →
113 across 29 commands, now MACHINE-ENFORCED by
`the_unsupported_flag_ledger_matches_the_catalog` (a literal roster cross-checked against
the catalog in both directions — it has already caught three moves); a campaign run reported the
differential corpus moving 48 → 56 scenarios with zero SKIPs, though at campaign close the
checked-in summary still contained the older 48-scenario evidence. The 2026-08-22 alias tranche
subsequently refreshed it to 58 scenarios and 846 steps; the rank-5 output/fact fixtures then raised
the report to 61 scenarios and 874 steps. The capture and manual-geometry slices then brought the
inventory to 63 scenarios and 905 steps; the later placement, environment, input, buffer, source,
zoom, and micro-flag fixtures raised the canonical report to 71 scenarios and 1,032 steps. Protocol
v71 shipped as one append-only bundle whose every field is now consumed.

The campaign's own discipline held: divergences are ledgered rather than silent, with three
lapses caught and fixed in-wave. Six times an implementer measured the pinned binary and
found the brief wrong — mouse defaults ON in next-3.8, `display-panes-format`'s real
default, `\;` chain-stop semantics, the freeze not being hermetic, prompt-vs-message freeze
stickiness, and refusal tests that never existed — and each time the measurement won.

At campaign close, the live residues were per-window copy-mode styles, nested-layout border-style
ownership, client-relative `source-file` paths, script-visible capture/list behavior, bell
attach-clear convergence, F6's attached harness, and F0's dispatcher collapse. The current alias
tranche completed the F6 harness and now covers copy mode, choosers, prompts, buffers, prefix tables,
and nested attach through real PTYs. The remaining current gaps live in
[the divergence matrix](/tmux/divergences.md). Supported client-selector targeting and detach
selection have since shipped; the remaining attach work is split into cwd/flags/sizing, environment refresh, detach exec,
and parent-HUP exit actions.

This campaign closes items 3 through 6 from the live queue, then works through the
remaining flag ledger. File-and-line anchors in this section refer to commit `57ae502`.
Before each wave starts, revalidate its anchors and assumptions against the current tree.

The unattended implementation boundary allows CI fixes and client consumption of behavior
the daemon already owns. Append-only contract plumbing requires prior approval. Work that
changes mux, daemon, session, lifecycle, parsing, targeting, geometry, wire contracts, or
command semantics stays planned until fabrico approves it. The review after each wave must
re-check this boundary.

## Reviewed execution split (2026-08-20)

Three read-only source reviews checked this section at `57ae502`. The unattended work is a
no-wire safe prelude:

1. Restore CI on Linux, Windows, and macOS. The macOS failure is the isolated
   `control_notifications_layout_and_output_follow_the_live_socket_stream` event-ordering
   failure in addition to the Linux and Windows failures listed below.
2. Add the 67-name `BEHAVES` roster and drift test.
3. Move the complete style parsing unit into `zz-protocol`: `TmuxStyle`, `TmuxColour`,
   `parse_styled_segments`, `parse_style`, and `parse_tmux_colour`, including the X11 color
   dependency now reached through `zz-mux::formats`. Re-export the public surface from
   `zz-mux`.
4. Fix the TUI's existing-wire status bug: parse the styled `left`, `right`, and
   `status_label` strings it already receives, and build the window list from
   `status_label`. Position, justify, mouse, escape-time, title, tty facts, client size, and
   read-only policy wait for the protocol and core campaign.
5. Park `source-file` diagnostics. Parse and glob errors fit the existing response variants,
   but the required unsupported-command summary cannot reach stderr with exit 0 without a
   response-contract change or brittle CLI text classification.

Protocol v71 stays a single bump, but it starts after fabrico approves the core semantics.
This keeps the bump from carrying undefined prompt fields, a custom terminal-codec change,
an ownership-changing `PaneIndicator` string, and optional lock messages before their
behavior is settled.

## Campaign choices

1. Use one append-only protocol bump, v71, at the start of the approved core campaign. The
   safe prelude makes no wire change.
2. Move the complete style and tmux-color parser unit from `zz-mux` to `zz-protocol`.
   `style.rs` is not pure at `57ae502`: it imports `TmuxColour` and `parse_tmux_colour`
   from `zz-mux::formats`, whose named-color path calls `zz_terminal::parse_x11_color`.
   Move that unit together and re-export it from `zz-mux` so existing consumers remain
   stable.
3. Reuse existing channels where they preserve semantics. Terminal appearance uses the
   per-pane appearance bridge. Initial tty and size facts use `ClientHello.capabilities`;
   later size changes use the v71 client-size input. Park `remain-on-exit-format` until
   terminal core owns a post-worker VT injection or frozen-view reconstruction seam.

The starting ledger has 129 unsupported command-and-flag pairs. Waves B through D removed
16, leaving 113 (the pre-wave estimate said 15 and 114; the difference is arithmetic, not a
missed pair). The revised G plan assigns 85 implementations and names 28 parked pairs. It counts
the shipped `move-pane -l` once, on the implementation side.

## Wave 0 - CI green

**Implemented and reviewer-closed locally 2026-08-21.** Linux gates the macOS-only helper
and test imports. The Windows lock now resolves `gpu-allocator 0.28.0` against the same
`windows 0.62.2` types as `wgpu-hal 30.0.0`; an MSVC cross-check passes. The macOS
control-notification failure was a test filter that removed automatic rename only at the
start of the stream. The test now removes asynchronous rename events from the positional
sequence and asserts the explicit rename against the unfiltered stream. Claude's second
read-only review returned MERGE-READY after the first round found both test issues.

- Linux: gate `startup_directory_environment` and its tests in
  `crates/zz/src/bin/zz_cli.rs` with the same macOS cfg as `launch_application`.
- Windows: resolve the `gpu-allocator` and `windows` crate
  `ResourceCategory: From<&D3D12_RESOURCE_DESC>` mismatch at the registry dependency edge.
  `gpu-allocator 0.28.0` resolves `windows 0.59.0`, while `wgpu-hal 30.0.0` resolves
  `windows 0.62.2`. The carried zed fork does not own either registry pin, so
  `just fork-rebase zed` is not the fix route.
- macOS: isolate
  `daemon_autostart::control_mode::control_notifications_layout_and_output_follow_the_live_socket_stream`.
  The failed CI run observed `%window-renamed` before `%sessions-changed`; establish whether
  this is the recorded load-only ordering flake before changing production ordering.

## Wave A - foundations

**Safe foundation slice implemented locally 2026-08-21.** `BEHAVES` now publishes and
test-pins the 67 consumer-traced options. The complete tmux color and styled-segment parser
lives in `zz-protocol`, with the existing `zz-mux` exports preserved. Protocol v71 and every
behavior-changing field remain parked in A3. Claude Sonnet's independent read-only review
returned MERGE-READY with no blockers or should-fix findings.

### A1. `BEHAVES` roster and drift test

Add `pub const BEHAVES: &[&str]` beside `OPTION_TABLE_ORDER` in
`crates/zz-mux/src/tmux_options.rs`, seeded with the 67 behaving names from
`knowledge/tmux/divergences.md`. Add a test beside
`listing_order_covers_every_non_hook_table_option_once` that requires every roster entry to
exist in the table and pins the initial count to 67. Each later wave moves names into the
roster and updates the assertion.

### A2. Shared style parser

Move the complete parser unit to `crates/zz-protocol/src/style.rs`: `TmuxStyle`,
`TmuxColour`, `parse_style`, `parse_styled_segments`, and `parse_tmux_colour`, including the
named X11 color path. Re-export the same symbols from `zz-mux` so existing consumers do not
move. Keep the parser tests with the implementation.

### A3. Protocol v71 (shipped 2026-08-21)

Implemented and reviewer-closed 2026-08-21: one append-only bundle, all gates green, an
independent adversarial review plus a follow-up delta verdict of MERGE-READY. Zero client
behavior change, with two named exceptions recorded for B1's ledger: `StatusLine.position`
publishes the real effective value (no client reads it yet), and encode-time
`StatusLine::validate` failures are discarded at the enqueue seam until B1 surfaces them.
The three new mux keys shipped publication-only — `from_config_key` deliberately omitted
`mouse`/`escape-time`/`prefix2` until their consuming waves opened the config surface
(B2/B3 opened the first two; Wave C run 2 opened `prefix2`).

The approval audit completed read-only on 2026-08-21. Append the following fields and
variants as one bump. Postcard structs and enums stay append-only; the manually encoded
terminal lane retains its explicit byte layout.

| Contract | Exact shape | Consumer and ownership |
| --- | --- | --- |
| Mux options | Append `MuxOptionKey::{Mouse, EscapeTime, Prefix2}` with tags 14 through 16. Keep `Prefix` and `Prefix2` global in v71 because the shared `KeyTables` own one prefix pair; tmux-style session-scoped prefixes require a separate core refactor. | B2, B3, C2. `Mouse` publishes the attached session's effective value; `EscapeTime` and `Prefix2` publish the global values. |
| Status | Append `StatusLine.{title: String, base_style: String, rows: Vec<String>, position: StatusPosition, message_line: u8, customized: bool}`. Keep the existing `left` and `right` first for v70 layout. Move `StatusPosition` into `zz-protocol` and re-export it from `zz-mux`; keep justify inside the expanded row formats. Cap rows at five and every string at 4 KiB. Reject a sixth row before allocation, require `base_style` to parse as a style, and require `message_line == 0` for no rows or less than `rows.len()` otherwise. | B1 and B4, C3. `rows` is the authoritative personalized status block and drives `is_empty`: zero rows means off, blank rows still consume geometry, and sparse `status-format` indices do not compact. `base_style` paints blank rows. `customized` controls zz-native hints even when an explicit value equals the default. Title still publishes while status is off. |
| Display panes | Append `PaneIndicator.label: String`, bounded to 1 KiB; remove its current `Copy` shape and borrow it in helpers. | C4. The daemon expands `display-panes-format` separately in each pane context and both clients paint the result. |
| Pane borders | Append `PaneSnapshot.{border_colour, active_border_colour}: Option<TmuxColour>` and give `TmuxColour` validated wire serialization. `None` means client fallback. | C9. Pane-scoped fields preserve pane to window to global inheritance; one pair on `WindowSnapshot` cannot represent distinct pane overrides. The raw TUI consumes the fields. The GPUI client keeps pane chrome under its local theme. Keep non-color attributes ledgered. |
| Prompt | Keep `CommandPromptKind::{Command, Value}`. Append `prompt_type: CommandPromptType`, `mode: CommandPromptMode`, and `no_freeze: bool`; types are `Command|Search` and `Text|Single|Numeric|Incremental|Key|BackspaceExit`. Do not add a prompt-key action. | D1. `kind` remains presentation state, while `-T` is independent. Resolve mode flags with pinned priority `-1`, `-N`, `-i`, `-k`, `-e`; preserve `-C` independently. Route raw special-mode keys through the existing pane-targeted `InputMessage::Key`. The daemon prompt state machine returns handled or pass so `-N` can submit digits and then feed the first non-digit into normal key processing. |
| Copy mode | Append `hide_position: bool` after `TerminalMode::Copy.total`; encode Copy as tag plus two `u64`s plus one canonical bool byte. Append `TerminalViewAction::EnterCopyModeWith { scroll_exit, hide_position }` at tag 27 while preserving both old variants. | D2. The action shape lets `-e` and `-H` compose; both clients suppress only the position text. |
| Choosers | Append bounded `key: String` to `ChooseTreeItem` and `ChooseBufferItem`; cap it at 64 bytes and use empty for no shortcut. | D4. Existing actions already carry `KeyInput`. Default rows use `0..9`, then `M-a..M-z`; invalid keys become empty and duplicate keys select the first row before navigation. `-N` preview state remains internal. |
| Command stderr | Append `stderr: String` after `CommandResponse::Success.exit_code`, using the existing frame bound. Preserve the current output-only client API and add a stream-aware result for the CLI. | Wave E. v71 initializes it empty; Wave E later populates exact stdout and stderr independently, including stderr with exit 0. |
| Timed-message lifecycle | Append `message_id: u64` to `EventPayload::TimedClientMessage` and append `EventPayload::TimedClientMessageCleared { message_id }` at tag 46. The daemon assigns identities and owns timers. | D3 and D1. The daemon freezes terminal publication per client for an ordinary message or prompt, keeps PTY parsing live, then publishes one full latest viewport before patches resume. `display-message -C` and incremental or `-C` prompts skip the freeze. Identity prevents an old timer from clearing a replacement; an explicit clear makes duration-zero and TUI behavior converge. |
| Client terminal facts | Introduce `client-tty-v1:` and `client-size-v1:` as new value tokens in `ClientHello.capabilities` (none exist today; `zz-client` currently sends empty capabilities, and the existing value-token precedent is `zz-startup-reentry=`); append `InputMessage::ClientTerminalSize { columns, rows }` at tag 17 for later TUI `SIGWINCH` updates. | B5. The daemon uses current per-client facts for tty targeting, nested-session checks, dash dimensions, and client width/height formats. Terminal-surface and Command connections collect initial size facts before connect; the TUI publishes later size changes. Control keeps explicit `refresh-client -C` geometry and does not use `ClientTerminalSize`. The 2026-08-25 repair separated nested intent into additive `client-nested-v1`, emitted only for a nonempty `$TMUX`, while keeping tty publication unconditional for eligible local terminal-surface and Command endpoints. A later local Control closure reuses those two additive identity tokens only for terminal stdin and nonempty `$TMUX`; it emits no size token and adds no protocol shape or version. |

`MuxOptionsChanged` remains a complete replacement map but becomes per-recipient for
session-effective values. Publish the effective map after attach and client switch. A global
mouse write recomputes every attached client because session overrides may mask it; a
target-session write refreshes only clients attached there. The mux effect must carry scope.
`StatusLine` stays independently personalized and equality-deduplicated.

Exclude the optional lock pair from v71. If F5 is later approved, give
`EventPayload::Lock { command }` and `InputMessage::Unlocked` their own protocol bump and
explicit client process-execution policy rather than freezing that optional contract here.

Update the protocol constant, frozen byte tests, protocol claim tests,
`knowledge/protocol/wire-protocol.md`, `knowledge/protocol/snapshots.md`,
`knowledge/crates/zz-protocol.md`, managed protocol index, and root protocol summary in the
bump. Add boundary and malformed-decode tests for every new bounded or typed field, plus
two-client/two-session convergence tests for personalized mux options and status metadata.
Add bounded-row tests for zero through five rows, sparse and explicit-empty arrays,
message-line clamping, base style, title with status off, and rejection before allocation.

**Approval-audit amendments (2026-08-21, verified against `4164524`):**

- The bundle spans three crates, not one. `TerminalViewAction::EnterCopyModeWith` and
  `TerminalMode::Copy.hide_position` land in `zz-terminal` (`interaction.rs`, `model.rs`),
  and the Copy change edits the manual terminal lane in three coordinated places
  (`encoded_mode_len` 17 -> 18, `encode_mode`, `decode_mode`); the length arm `Copy` shares
  with `View` must split, or `View` frames hit a capacity mismatch.
- `StatusPosition` (today in `zz-mux/src/status.rs`, no serde derives, `#[default]` on the
  tag-1 `Bottom` variant) and `TmuxColour` (`Rgb(u32)` packs 24-bit color) both need
  validated wire serialization written, not just relocation.
- Three wire-append rows carry non-append source semantics the review must name:
  `StatusLine::is_empty` changes meaning (two callers depend on the current one),
  `MuxOptions::validate` is an exact-set check so all 17 keys ship atomically, and dropping
  `PaneIndicator`'s `Copy` touches three by-value consumers.
- Version-pin surfaces the bump must update: `zz-protocol/tests/hunt_claims.rs` (version
  assert, literal hello frame bytes, `MuxOptionKey::ALL` length, tag-13 pin), the
  `message.rs` frozen-tail tests, and the protocol knowledge pages stating 70.
- `escape-time` already parses, stores, and reads back in `zz-mux` with the pinned default
  10 (`command.rs` `escape_time_ms`); the v71 work is publication and consumption, not the
  option itself.

## Wave B - TUI option consumption and attach residue

**Shipped 2026-08-21 in three reviewed runs** (daemon status-row production, the shared
compositor with both clients rendering, then mouse/escape-time/titles/attach-residue/
read-only), each closed by an independent adversarial review and a MERGE-READY follow-up.
`BEHAVES` moved 67 to 81 (+14: the plan's 12 plus the `set-titles` pair the approved order
folds in from C3); the flag ledger fell 129 to 128 (`attach-session -r`). One brief error
was caught against the pin mid-wave: the pinned tmux builds with mouse ON by default
(`TMUX_MOUSE=1`), and zz already matched. Pane-requested mouse forwards with the option
off, pin-exact. At the 2026-08-21 Wave B ship point, the ledgered bounds were status-row
window-option scoping (fix scheduled into
Wave C via the `Expander::lookup` loop-item seam), the status-block suppression
threshold, the empty-title expansion edge, read-only decoupled from `ignore-size`, and
the then-open nested `new-session` check.

The nested `new-session` bound closed on 2026-08-22: attaching forms now run the same tty refusal
as `attach-session` before the mux mutates.

At the source anchor, the TUI ignored `CoreEvent::MuxOptionsChanged`, stored no mux option
state, and printed daemon-authored `#[style]` markers as text. The safe prelude fixed the
existing-wire text and style path. This wave adds the remaining option and row contracts.

The safe prelude implements only styled parsing and `status_label` consumption in item 1,
using the v70 fields already on the wire. The rest of Wave B waits for v71 and core approval.

**Existing-v70 slice implemented locally 2026-08-21.** The TUI now parses styled runs for
the daemon-expanded left, right, and per-window labels; uses `status_label` instead of
reconstructing window names; and includes style state in its repaint cache. Its existing
three-row sidebar, bottom-row placement, left/right gap policy, indicators, detach hint, and
character-count clipping remain unchanged. The focused 71-test TUI suite, full 51-test CLI
binary suite, live PTY styled-status proof, and focused no-dependency clippy pass. Claude
Sonnet's independent read-only review returned MERGE-READY with no blockers or should-fix
findings after rerunning those gates.

1. Status line:
   - Have `zz-mux` expose the effective sparse `status-format` array. The daemon expands
     each active row in personalized client context, including window, pane, and session
     loops, time, styles, ranges, and list metadata. Resolve `status-style`, then apply
     non-default `status-fg` and `status-bg` precedence into `base_style`.
   - Publish no rows for `status off`; publish indices 0 through `status-1` for values one
     through five. Default row 0 contains status-left, the window list, and status-right;
     row 1 contains the pane list; row 2 contains the session list; rows 3 and 4 are blank.
     Missing numeric entries paint blank rows and do not pull later entries upward.
   - Keep `left`, `right`, and `WindowSnapshot.status_label` for legacy/native summary
     surfaces, but stop reconstructing the tmux status block from them.
   - Add one shared status-row compositor for both clients. It owns display width, style,
     alignment, fill, list focus and truncation, markers, and window/pane/session hit ranges.
     Widen pane and session range identifiers to `u64` before using them for input.
   - Put the status block at the top or bottom. Top shifts the main canvas by `rows.len()`;
     bottom shrinks it without shifting. Suppress the block when terminal height cannot
     leave pane content. Blank rows still consume height and paint `base_style`.
   - Keep the three-row zz-native sidebar beside the main content. Render every tmux row
     across the main columns rather than replacing pane/session rows with sidebar content.
     GPUI gets a dedicated top or bottom N-row container.
   - Treat row count and position changes as layout events: recompute geometry, repaint,
     resend terminal sizes, and resynchronize browser surfaces.
   - `message_line` selects the row that prompts and messages replace. With status off,
     create one virtual message row at the configured top or bottom position.
   - Keep `PREFIX` and `COPY` state indicators. Drop the literal `Ctrl-\\ detach` hint when a
     user explicitly sets a status option.
   - Track global and session explicit status writes separately from effective
     `StatusFormats`, including explicit values that equal defaults and later unsets. Track
     explicit `status`, `status-*`, and `status-format` writes.
   - Make scoped `status-format` array writes refresh attached clients; current generic
     array writes emit no status effect.
   - Move `status-format`, `status-justify`, `status-position`, `pane-status-style`,
     `pane-status-current-style`, `window-pane-status-format`,
     `window-pane-current-status-format`, `session-status-style`,
     `session-status-current-style`, and `message-line` into `BEHAVES` with this slice.
2. `mouse`: gate outer-terminal mouse modes on the session's `Mouse` option, re-emit or
   retract them on `MuxOptionsChanged`, and reject mouse events at the input seam when off.
3. `escape-time`: replace the hardcoded 25 ms receive timeout with the `EscapeTime` option.
   The pinned default is 10 ms, so tests must cover the default path.
4. Title source and sinks:
   - Implement C3's `set-titles` and `set-titles-string` expansion first. Publish the title
     even when `status off` bypasses status-row rendering.
   - Write OSC 2 from the TUI when a non-empty `StatusLine.title` changes and set the GPUI
     window title from the same field.
5. Attach residue through capability tokens:
   - Collect tty and size before the connection. `client-tty-v1:/dev/ttysN` retains the local tty
     for targeting, while additive `client-nested-v1` records a nonempty inherited `$TMUX`; the
     daemon requires both that marker and a pane-tty match for the pinned nested-session refusal.
   - `client-size-v1:COLSxROWS` supplies the initial `-x -` and `-y -` creation dimensions
     and `ClientFormatFacts.width` and `height`; publish later `SIGWINCH` changes through the
     v71 client-size input.
   - Define tty discovery for Command clients rather than limiting the check to TUI attach.
   - Local Control now discovers tty identity from stdin only and adds the nested marker for a
     nonempty `$TMUX`. Piped stdin adds neither a tty nor implicit geometry; Control still sizes
     only through explicit `refresh-client -C`.
6. Read-only clients: insert the client on `attach-session -r`, then gate terminal and
   browser key/text, paste, mouse, divider resize, prompt and chooser actions, uploads, and
   every direct raw-input route at the daemon input funnel. Two pre-existing seams to fix
   with it: the CLI natively rejects `-r` before any daemon round-trip
   (`crates/zz/src/lib.rs` native-attach parser), and `switch-client -r` already toggles
   `read_only_clients` that nothing enforces — it also spuriously reports `ignore-size` in
   `client_flags`.

Use the phase-8 PTY fixture in `crates/zz/tests/cli_binary.rs` to prove that styled status
content contains no literal `#[`, top status occupies row zero, `mouse off` emits no
`?1003h`, and nested attach prints the pinned refusal.

## Wave C - C3 knob batch

**Closed 2026-08-22 in three reviewed runs** (lifecycle trio + `command-alias` +
`update-environment`; alerts + `prefix2`; `display-panes-format` + renderer styles + the
status-row scoping closure), each with an independent adversarial review and a
MERGE-READY verdict. `BEHAVES` moved 81 to **104** — the recomputed C target, 105 minus
the parked `remain-on-exit-format` — and the flag ledger fell 128 to 127
(`send-prefix -2`). The B1-era status-row window-option scoping divergence is closed and
proven end-to-end by the restored per-window PTY smoke. Two divergences opened with the
new surfaces and are ledgered: border-style owner granularity (one colour per divider
where the pin resolves per cell span) and the one-appearance channel bounding per-window
copy-mode styles. The run-2 attach-clear bell asymmetry and the remaining alert state edges
closed on 2026-08-24; alert message freeze and lifetime remain separately tracked.

1. Alerts: **shipped 2026-08-21 (run 2).** The bell path generalized into the pin's
   alerts.c model: `monitor-bell` gates `raise_pane_bell`, PTY output (the coalesced
   `output_activity` seam) raises the per-window activity flag, and `monitor-silence`
   arms per-window deadlines on a dedicated dispatcher thread copied from the
   display-panes pattern (`SilenceDeadline` keyed by window, token-validated, re-armed
   on output, selection, expiry, and any `monitor-silence` write via
   `MuxEffect::MonitorSilenceChanged` — the pin's `alerts_reset_all`). Selection clears
   flags then requeues activity like `session_set_current`; the `*-action` and
   `visual-*` options choose one outcome from the session's active window and fan that
   outcome to eligible clients through `window_alert_notifications`, with the pin's texts. Formats:
   `window_activity_flag`/`window_silence_flag` backed for real, `#`/`!`/`~` in
   `window_flags` in pin order, `session_alert`/`session_alerts` aggregate all three, the
   misleadingly named `session_activity_flag`/`session_silence_flag` mirror the target window,
   and `window-status-activity-style` layers into the status label where the bell style
   does (bell wins when not `default`). Everything is inert at defaults: no timers, no
   flag writes, and the bell path unchanged with `monitor-bell on`. The 2026-08-24 follow-up
   made every successful silence write reset all timers, made attach clear every alert kind and
   terminal bell latch on the active window, and brought writable text/paste message dismissal
   into the ordinary status-message path. Alert-produced messages still bypass that daemon-owned
   lifecycle and leave terminal publication unfrozen, while the pin arms `TTY_FREEZE` through
   `status_message_set(..., no_freeze = 0)`. Silence timing stays daemon-test-only; the 62-step `alerts` differential covers
   flags, actions, readback, duplicate writes, and format output with timing-free triggers.
2. `prefix2` and `send-prefix -2`: **shipped 2026-08-21 (run 2).** `KeyTables` carries an
   optional canonical second prefix (`set_prefix2` never touches bindings — the pin has
   no stock `send-prefix -2` binding), both `KeyEngine` arming sites accept either
   prefix, and the GPUI client claims prefix2 keystrokes beside the primary. The value
   lives in the `prefix2` stored scalar, syncs into the key tables on every
   global-session write, publishes through `MuxOptionKey::Prefix2`, and became
   config-writable (`from_config_key`). `send-prefix -2` sends the second prefix and is
   a silent rc-0 no-op while unset, matching the pin's `KEYC_NONE` injection. Oh My
   Tmux's smoke baseline is zero warnings.
3. `set-titles` and `set-titles-string`: execute this source half before B4's client sinks.
   Expand the title per client and publish it independently of status visibility.
4. `display-panes-format`: **shipped 2026-08-22 (run 3).** `build_display_panes_state`
   expands the session-effective format separately in each pane's context
   (`expand_format_values`, no strftime — the pin's `format_single`) into
   `PaneIndicator.label`, char-boundary-capped at the 1 KiB wire bound and rebuilt live
   by `refresh_display_panes`. The TUI composes the label across the pane header row
   through `compose_status_row` (alignment honored, exact-width clipped); the GUI parses
   the styled segments into alignment buckets on a top strip of the indicator overlay,
   clipped at the pane edge. `display-panes-colour`/`-active-colour` stay store-only;
   the label's base colours are theme chrome (ledgered).
5. `remain-on-exit-format`: park until the terminal actor has an approved post-worker VT
   injection or frozen-view reconstruction seam. The current retained-pane path marks the
   pane dead after the live PTY/VT actor exits, so the proposed feed path does not exist.
   The tracker keeps whole-catalog option-name format coverage in a separate mux-owned group;
   separator and hook-listing evidence does not close that audit.
6. `command-alias`: **shipped 2026-08-21.** `MuxEngine::resolve_command_alias` resolves one
   layer through the config tokenizer, appends caller arguments, and never recurses (the
   pin's `CMD_PARSE_NOALIAS`). Its typed result distinguishes a miss, a supported expansion, and a
   matched empty, multi-command, or unparsable body; the last case refuses instead of falling
   through to a shadowed canonical or catalog-alias command. Standalone mux execution resolves once
   before canonical lookup.
   Daemon execution expands once before read-only authorization, then sends the same immutable
   invocation through logging, native preemption, mux dispatch, and hooks without another alias
   lookup. Stored bindings prepare and authorize each command immediately before it runs, so an
   earlier command can change the alias seen by the next command; an expansion failure uses the
   ordinary command-output and `key_command_failed` warning path. Read-only clients instead prepare
   and authorize the whole stored chain before any command runs. Bind-key, set-hook, and
   option-command validation also resolve one layer and refuse matched unsupported bodies.
   Protocol v74 closes the former Control-side static name check. Control asks the daemon to prepare
   the complete initial argv unit or LF line under one lock before allocating frames, then executes
   the immutable returned invocations without a second alias lookup. The protocol-internal
   `prepared` bit freezes alias observation only: daemon authorization still runs for every command,
   including forged read-only requests. The six stored defaults match the pin; no catalog alias
   deletion was needed. Ledgered: exact native attach spellings enter client-owned routing before
   live server aliases, arbitrary live aliases cannot request stdin capture, and a valid one-command
   Local default and explicit-socket CLI invocations now prepare their complete vector through v74
   before those three decisions. Exact attach shadows stay in command mode, arbitrary aliases to
   attach or attaching new-session enter the TUI with the immutable vector, stdin follows the
   prepared canonical command, and kill recovery requires an unaliased transport or handshake
   failure. Raw `--kill-server` remains unaliasable, and a failed preparation falls open to the old
   static route so `--restart-daemon` can recover. The local path scans every prepared result before
   stdin capture, attach or TUI routing, and execution, so a later typed name or alias-body error
   aborts the vector before any earlier effect. Runtime command failures keep tmux's sequential
   queue behavior. The pin proves the unknown-name diagnostic shape; zz's malformed alias-body
   diagnostic remains a local choice while `aliases.command-bodies` is open. If preparation cannot
   reach a live compatible daemon, the static fail-open path remains: an autospawn command may run
   before a later unknown command. Remote `--host` preparation remains under
   `aliases.remote-client-preflight`;
   local flag or arity validation, config replay groups, config alias snapshots, and actual empty or
   multi-command bodies remain under their separate tracker entries.
7. `update-environment`: **shipped 2026-08-21.** `seed_session_environment` and
   `global_tmux_option_value` both read the stored array; the frozen constant is gone from
   `command.rs`. Creation-time `new-session -e/-E` shipped 2026-08-22. Client-sourced values,
   attach re-seeding, `attach-session -E`, and `fnmatch` value patterns stay ledgered because the
   wire carries no client environment.
8. Lifecycle trio: **shipped 2026-08-21.** A dedicated `scalar_option_explicit` accessor
   over the stored-scalar maps gives presence-means-set semantics at each option's pin
   scope; `should_shutdown_if_empty` consults `exit_empty_explicit` /
   `exit_unattached_explicit` and otherwise keeps the latch rule byte-identical, and
   `enforce_destroy_unattached` reproduces `server_check_unattached` after attach, detach,
   switch, and unregister. The `subscribers.is_empty()` conjunct survives every policy, and
   policies are dormant inside the startup bracket. `keep-last`/`keep-group` follow their
   ungrouped-session reading while linked session groups remain the permanent skip. All of
   it is covered by in-process daemon tests, never compat scenarios.
9. Renderer styles: **shipped 2026-08-22 (run 3).**
   - `window-style`/`window-active-style` colour halves feed the per-pane appearance
     bridge: `terminal_worker_options` resolves the explicit pane → window → global
     values, expands conditionals in the pane's context, and patches the pane's terminal
     fg/bg with the pin's per-channel active-over-base fallback (`tty_default_colours`).
     Option writes ride `TerminalKnobsChanged`; selection and relocation refresh through
     `publish_snapshot`, gated on `has_window_style_settings` and deduplicated by an
     appearance-hash guard in `TerminalSession::set_appearance`, so defaults stay
     zero-cost. `mode-style` and the copy-mode match styles patch the published
     appearance (selection and search-overlay colours); the mark style resolves but zz
     renders no mark. Attributes, `dim`, and per-window copy-mode granularity are
     ledgered.
   - `pane-border-style`/`pane-active-border-style` explicit COLOURS resolve per pane
     during personalized snapshot stamping (`stamp_pane_border_colours`, formats expanded
     in pane context) onto the v71 `PaneSnapshot` fields. The TUI colours dividers
     (style-owner: active-adjacent pane first) and pane headers, with its normal colours
     as the fallback when a field is `None`. The GPUI client ignores both fields and
     derives pane frames, split hairlines, and active-split highlights from `cx.theme()`.
     Non-colour attributes stay ledgered.

The full B and C target moves `BEHAVES` from 67 to 105: 12 Wave B consumers and 26 Wave C
consumers. The C tranche, if approved without `remain-on-exit-format`, stops at 104. Wave C
run 1 (items 6, 7, 8) took it 81 to 86; run 2 (items 1 and 2) took it 86 to 95 and dropped
the flag ledger to 127 across 29 (`send-prefix -2`).
Regenerate the option rosters in `knowledge/tmux/divergences.md` and enforce both expected
deltas in tests.

## Wave D - daemon-owned interactive state

1. **Shipped 2026-08-22.** `command-prompt -1 -k -N -T -i -C -e` on the v71 prompt fields,
   with no wire change. `CommandPrompt` per client now carries `prompt_type`, `mode`,
   `no_freeze` and `last`, and the mux resolves the pin's ladder verbatim
   (`cmd_command_prompt_exec`: `-1` then `-N` then `-i` then `-k` then `-e`, `-C` orthogonal;
   measured pair by pair on the pin, including `-1` beating `-N` on a digit and `-k` beating
   `-e` on `C-g`). `-T` maps `command`/`search` and answers `unknown type: %s` at rc 1 for
   anything else, exactly as measured.
   **The three key-reading modes are daemon-owned**, so both clients stop editing and relay
   the press on the existing pane-targeted `InputMessage::Key` (no prompt-key action, as A3
   decided): `-k` answers with `input_key_name`, `-1` folds one key into a character through
   the pin's normalisation (BSpace to `DEL`, control to `0x1f`, arrows ignored) and closes
   with NOTHING submitted when the buffer is not exactly one character — the `-1 -I abc`
   quirk, measured — and `-N` collects digits and returns the pin's
   `PROMPT_KEY_NOT_HANDLED` so the first non-digit both submits and reaches the key tables.
   A relayed character's trailing text half is repaid through the existing `suppressed_text`
   ledger, so nothing types into the pane behind the prompt; a PASSED-THROUGH character is
   deliberately not suppressed, because that is the pin's `window_pane_key`.
   `-i` starts with an empty buffer and `-I` in `pr->last`, fires `=` before the first key
   (`prompt_incremental_start`, measured as `I[=]` appearing with no key pressed), fires
   `prefix + buffer` on every edit through both the key path and the client `Update` action,
   flips the prefix on `C-r`/`C-s`, closes on `Up`/`Down`/`PPage`/`NPage`, and treats Enter
   as history-plus-close because every edit already ran.
   **The freeze composes with Wave D run 3's through one predicate**:
   `client_terminal_publication_frozen` now reads `message.freeze || prompt.freeze` against
   the same single `.filter(...)` on `publish_terminal_for_pane`, and the prompt half lives on
   the `command_prompts` record that already exists rather than a second parallel map — the
   reviewer's ruling was "not `client_messages`, and one predicate", and `command_prompts` is
   the parallel record, with a lifetime identical to the freeze it carries. Every retire of a
   freezing prompt pushes one full latest viewport per visible pane (`resume_client_terminals`,
   `CLIENT_ALLREDRAWFLAGS`), and raising a prompt clears the message it covers, matching
   `status_prompt_set`'s `status_message_clear`.
   **This is the one place D1 changes an existing prompt's behaviour**, and it is intended:
   a plain `C-b :` now stops terminal frames for the client that opened it, because that is
   what the pin does (measured). The brief's "zero visible change for prompts that use none of
   the new flags" is about the prompt's own presentation and editing, which are untouched;
   item 2 of this wave asked for the freeze explicitly. Worth watching in the GUI, where the
   palette is a large modal a user may keep open far longer than tmux's one-line prompt.
   Command and Search histories are separate rings, persisted in the pin's `type:entry` file
   format that zz already wrote and half-read. Ledger 120 -> 113.
   **`-l`, `-F` and `-t` stay rejected on purpose** (`-P` stays parked). `-l` opts out of a
   comma-split multi-prompt chain zz does not implement, so accepting it would advertise a
   feature; `-F` needs the pin's full `format_single_from_target` over the template and zz's
   prompt-side expander understands only `#S`/`#W`; `-t` belongs to the client-fanout
   contract. All three are in the divergence matrix with their reasons.
   **One finding worth carrying forward**: the prompt path has a stickiness analogous to
   `display-message -N`'s, and it is worse. `status_message_clear` drops `TTY_FREEZE` only
   `if (c->prompt == NULL)`, so a message's freeze survives the message whenever any prompt is
   open — including a `-C` prompt that asked not to freeze. Measured: a `-C` prompt ticking
   normally, a `-d 0` message freezing it, the dismissing key delivering exactly one catch-up
   frame and then nothing until the prompt closed. zz's predicate is derived rather than
   latched and resumes immediately; the divergence is deliberate and ledgered.
   No corpus scenario: `command-prompt` refuses every non-interactive client
   (`command-prompt requires an interactive client`), so it is unreachable from
   `compat/diff-scenario.sh`'s bare CLI for the same reason copy mode and the choosers are —
   F6 is the fix for all three. Every pin measurement in this run used the nested rig, and had
   to `set -g status-keys emacs` first, because the pin picks vi prompt keys from `$EDITOR`
   (`tmux.c:543-554`) — which also means zz's `status-keys`/`mode-keys` defaults diverge from
   the pin's on any box with `EDITOR=vim`, now recorded in the matrix. Two more prompt
   divergences were found and left alone rather than fixed under a zero-visible-change brief:
   the pin's prompt LABEL carries a trailing space and becomes `(<command name>) ` when a
   template is given with no `-p`, and `-N`'s pass-through runs its two commands in the
   opposite order because the pin's interleave is a `cmdq_insert_after` artifact. Both are in
   the matrix; the label one is a three-line pickup for the F error/label tranche.
2. **Shipped 2026-08-22.** `copy-mode -H` rides the v71 `EnterCopyModeWith
   { scroll_exit, hide_position }` action (tag 27, previously produced by nothing): the mux
   emits it only when `-H` is present, so `-e` alone and bare entry keep their old variants
   byte for byte. `CopyModeState` latches `hide_position` at entry beside `scroll_exit`, both
   snapshot builders publish it on `TerminalMode::Copy`, and the two clients drop the
   position text alone — the TUI's badge becomes `COPY`, the GPUI tag keeps `COPY MODE` and
   any `+N output`. That is exactly what the pin hides: `window_copy_init` reads
   `args_has(args, 'H')` into `data->hide_position` and `window_copy_write_line` guards the
   whole `copy-mode-position-format` draw on `!data->hide_position`, measured on the pin as
   the top-right `HH:MM [N/M]` overlay vanishing with nothing else moving. The state is
   observable nowhere else: neither side exposes it to formats (`#{?hide_position,…}` is not
   a pin format, and zz pins `pane_in_mode`/`pane_mode`), so coverage is unit and daemon
   tests only. A corpus scenario was attempted and withdrawn: zz's `copy-mode` exits 1 with `pane is not attached: %N` whenever no client is
   attached to the target pane — the mode lives on the per-client terminal view in zz and on
   the pane in tmux — so every step of a CLI-driven scenario diverges on exit class before any
   flag matters. Ledgered under accepted-grammar divergences, and scheduled as F6, which
   fixes the harness rather than the model; the same gate keeps the choosers off the corpus.
3. **Shipped 2026-08-22.** `display-message -C` and `-d` ride the v71 timed-message
   lifecycle. The daemon now assigns the identity, owns the timer, and owns the freeze:
   `ActiveClientMessage { token, deadline, freeze }` per client is the pin's
   `c->message_string` + `c->message_timer` + `TTY_FREEZE` triple, `arm_client_message`
   mirrors `status_message_set` (no `-d` means `display-time`, `delay > 0` arms the timer,
   `delay == 0` installs none and waits for a key, `-C` is the `no_freeze` argument) and
   `retire_client_message` mirrors `status_message_clear`. A `zz-client-message` dispatcher
   thread keyed per client — the `zz-monitor-silence` template, token-validated at both
   schedule and expiry — fires `expire_client_message`, which publishes
   `TimedClientMessageCleared { message_id }`, tag 46's first producer.
   **What the freeze suppresses is exactly one thing**: `publish_terminal_for_pane` skips
   the subscriber while the client holds a frozen message, the zz analogue of
   `tty_client_ready` returning 0 under `TTY_FREEZE`. PTY parsing, the mux model, copy
   session reconciliation and the pane-mode hooks all stay live, and every retire of a
   frozen record pushes one full latest viewport per visible pane through `send_full` —
   `status_message_clear`'s `CLIENT_ALLREDRAWFLAGS`, "was frozen and may have changed".
   Measured on a nested rig (inner server attached from an outer pane, capturing the outer
   pane): a plain `-d 3000` pinned the outer capture at `TICK56` for the whole window and
   then jumped straight to `TICK73`, while `-C -d 3000` kept ticking throughout; the inner
   `capture-pane` read `TICK219` while the wire still showed `TICK212`, proving parsing
   never stopped. Also measured and matched: replacing a frozen message pushes one
   catch-up frame and re-freezes, a key retires the message and unfreezes, `-d 0` pins
   until a key, `-C` never stops the timer, and `-d` validates with the pin's `strtonum`
   strings (`delay invalid` / `too small` / `too large`) over `0..=4294967295`.
   Writable non-release keys, explicit paste actions, and nonempty bulk `Text` remaining after
   suppression dismiss before prompt, key-table, or pane dispatch. A bound key's fully suppressed
   trailing text leaves the message it just raised intact, and read-only input does not dismiss.
   Control clients are excluded outright, matching the pin: `cmd_display_message_exec`
   routes `CLIENT_CONTROL` through `server_client_print` and never reaches
   `status_message_set`, so `%message` keeps carrying no duration and gets no clear.
   Residues at this run were bounded: zz's alert message producers kept client-side timing
   and did not arm the daemon freeze (the pin's `alerts.c:318` calls `status_message_set` with
   `no_freeze` clear, which sets `TTY_FREEZE`), `display-popup` frames were not frozen, and
   `-N` had not shipped yet. The pinned measurement established that `message_ignore_keys`
   is sticky client state and `delay == 0` skips writing it, so a `-N -d 5000` leaves a later
   plain `-d 0` message un-clearable by key. The 2026-08-25 `display-message -N` closure
   supersedes that last residue. Ledger 122 → 120.
4. **Shipped 2026-08-22.** `choose-tree`/`choose-buffer -K -N`. The daemon fills the v71
   `ChooseTreeItem.key`/`ChooseBufferItem.key` per visible row with the pin's stock ladder —
   `0`-`9`, then `M-a`-`M-z`, then nothing from row 36 on — which the pin reaches through
   `WINDOW_TREE_DEFAULT_KEY_FORMAT`/`WINDOW_BUFFER_DEFAULT_KEY_FORMAT`, whose `#{line}`
   arithmetic expands to the identical strings that `mode_tree_build_lines` hard-codes as its
   no-callback fallback. Keys are assigned always, not only under `-K`: the pin's choosers
   always pass a key callback, and a live pin draws `(0)`, `(M-a)` on every row by default.
   `-K` replaces the ladder with a per-row format expansion in that row's own context
   (session, window, or pane, mirroring `window_tree_get_key`'s `format_defaults` by item
   type; `#{line}` is the row index); an expansion that names no pressable key becomes no key,
   the way `key_string_lookup_string` answers `KEYC_UNKNOWN`. A press scans the rows before
   the navigation table and activates the FIRST row holding that key, so a `-K j` shadows
   "down" exactly as `mode_tree_key` does by breaking on the first match and rewriting the key
   to `\r`; a chooser mid-search keeps typing instead. Both clients draw the key in a gutter
   sized once for the list, blank for keyless rows. One `-N` is accepted as a no-op — zz's
   choosers have no preview pane, so no-preview is already their only layout — and a repeated
   `-N` is ledgered as `unsupported command: <cmd> -NN`, the pin's `MODE_TREE_PREVIEW_BIG`
   (`args_has(args, 'N') > 1`), which zz has no presentation for.

**Wave D is complete.** All four items shipped 2026-08-22, and with it the approved campaign
order `A3 -> B/C -> E -> D2/D4/D3/D1`. Two notes for whoever picks up F:

- The brief for D1 asked for "control-mode and PTY tests" and for deleting "the matching
  refusal assertions". Neither existed in the shape the brief assumed. `command-prompt`
  refuses control clients outright (`cmd_display_message_exec`'s sibling rule), so the honest
  control-mode coverage is that refusal, now asserted for `Command` AND `Control`; and the
  only refusal test in the tree was a single `-F` assertion inside
  `command_prompt_builds_a_native_client_effect`, which still holds because `-F` is still
  rejected. The mechanical sweep that really guards the flags is
  `the_unsupported_flag_ledger_matches_the_catalog`, and it was updated in the same change.
  The PTY coverage is real: the freeze tests drive a live PTY through
  `attached_message_fixture` and assert on the publication gate.
- The deadline-dispatcher collapse F0 schedules is still at three threads. D1 added no fourth
  — a prompt has no timer.

## Wave E - `source-file` CLI diagnostics

**Shipped 2026-08-22.** A3's `CommandResponse::Success.stderr` now carries source-file
diagnostics for Command clients only, routed by a per-request `CommandStreams` slot in
`ServerState` that models the pin's per-client stdout/stderr files and `c->retval`.
`CommandClient::execute_streams` is the stream-aware result; `CommandClient::execute` keeps
the output-only shape for every existing consumer. CLI text classification stayed rejected.

**One brief-versus-pin correction, in the pin's favour.** The plan said the CLI should stop a
`\;` chain on the first nonzero exit. The pin does not: `cmdq_next` drops the rest of a group
only when a command returns `CMD_RETURN_ERROR`, while a nonzero exit merely overwrites
`c->retval`. `cmd_source_file_exec` returns `CMD_RETURN_WAIT` once any file matched, so
`tmux source-file bad.conf \; display-message -p AFTER` prints the diagnostic AND `AFTER` at
rc 1; `run-shell 'exit 3' \; display-message -p AFTER` likewise runs both at rc 3, and a chain
member that fails outright (`kill-window -t nosuch`) does stop the chain at rc 1. zz now
matches: the chain stops on a response `Error`, a nonzero exit lets the chain continue, and
the LAST nonzero exit wins. Keeping the stop-on-nonzero rule would have opened a brand-new
divergence in the very command this wave fixed.

Two audit facts shape this wave: Command clients are never subscribed to the event stream
at all (`daemon.rs` subscribes Interactive and Control only), so the CLI cannot receive
warning events even in principle — the stderr field is required, not convenient. And
control-mode `%config-error` is reconstructed by a prose sniffer (`is_config_message` in
`control_mode.rs` pattern-matches warning text), so any rewording of config diagnostics
must either move to a typed marker or pin the wording with a test.

For an explicit `source-file` issued by a Command client, append invalid-line diagnostics to
stdout and return exit 1. Interactive clients keep warning events, while Control renders parser
diagnostics as `%config-error`. A glob miss without `-q` uses `No such file or directory: path` on
stderr and exits 1; a quiet miss produces no output and exits 0. A direct all-miss Control invocation
puts its diagnostics inside `%error` and stops the rest of that input line. If at least one direct
path matches, missing-path diagnostics remain inside `%end` and the input line continues. Mixed invalid input and a glob miss
populate both Command streams and exit 1. Preserve diagnostic order and duplicates. The zz-only
`skipped N unsupported tmux command(s): name, ...` summary stays on stderr and exits 0 so a
config with supported tmux behavior continues to load.

Add a stream-aware command result while preserving the current output-only client API.
Treat completed Command-client commands with exit 1 as successful protocol responses. Control uses
a response error for a direct all-miss source invocation so it can preserve tmux's same-line abort;
generic nonzero successes still continue. The CLI prints both streams and follows the pin's chain
rule above. CLI
coverage landed for the glob-resolved matched path plus
`:1: unknown command: wibble` on stdout with exit 1, the mixed-stream matrix, nested and
multiple inputs, the default-config path, Control `%config-error`, and the eleven-step
`smoke/source-file-diagnostics` scenario, whose fixture drives the plain CLI on both sides
from a `run-shell` job and diffs stdout, stderr, exit code, runtime errors, and outer propagation
byte-for-byte. `source-file -`
remains in the separate G6 streaming contract; it now refuses on stderr at rc 1.

The six-step `smoke/source-file-control` scenario separately preserves attached Control frame
boundaries. Protocol v76 now emits one tail-tag-47 `SourcedCommandGuard` for each replayed command
that survives command-name resolution. Unknown or ambiguous command names and malformed alias names
publish a located Warning that Control renders as `%config-error`, without a guard. Ordinary success
and quiet all-miss use an empty flags-1 `%end`; a mixed hit and miss keeps
its diagnostic inside `%end`; and all-miss, flag or arity failure, runtime failure, or depth refusal
ends `%error`. Runtime failures alone set `client_failure`. The writer defers these guards FIFO until
the direct outer frame closes, without leaking a guard into the next command. Existing source preflight
collects one command's path diagnostics before recursion. A focused daemon regression and the fifth
scenario check prove root missing-path guard, then middle missing-path guard, then leaf output guard,
each exactly once. That closes cross-depth ordering with no production change. A matched failed replay
still returns a completed nonzero success that the Control front end does not retain across every EOF
and detach order. `control-mode.source-file-exit-status` owns that client process-state matrix without
making source-command diagnostics globally sticky. The asynchronous
`run-shell` exit text itself is excluded from the slice because zz still emits it inside the completed
response where tmux prints it unframed after `%end`; `control-mode.async-command-output` tracks that
gap, and the scenario header names the exclusion. Its sixth step runs a second control client over
a 50-level chain so invocation 51 is loud. That step deliberately compares wording count,
outer-line continuation, reached depth, containing-file continuation, the leaf that never loads,
and the detached client's status instead of frame boundaries, because a 50-file chain would
otherwise be dominated by the tracked per-sourced-command guard difference.

The prose sniffers are pinned from both sides rather than retired:
`config_diagnostics_pin_the_control_mode_sniffer_wording` (zz-daemon) locks what the config
loader emits and `daemon_diagnostics_are_partitioned_between_config_and_source_channels`
(`control_mode.rs`) keeps parser causes on `%config-error` while source misses become plain error
frames. `route_config_source_errors` now marks grouped Control source diagnostics with the existing
Error kind, and the Control client routes that kind without inspecting text. Matched child read
failures such as invalid UTF-8, numeric OS errors, and colon-space paths use typed standalone Error
events. No-match, glob, and depth diagnostics now travel inside their protocol v76 sourced guard.
Both paths avoid prose classification. Config
summaries and lexer-owned diagnostics remain Warning events, so the active
`control-mode.diagnostic-typing` gap now contains only config identity. The known-family Warning
fallback remains for legacy producers, while the exact protocol handshake rejects v75 and v76
client-daemon skew before either event path can mix.

Protocol v72 later closed the top-level relative-path residue with a bounded local caller cwd,
glob-escaped cwd prefix, and declared-path diagnostics. The generated tracker keeps the related
residues separate: attached commands still use the client's process cwd instead of the session cwd;
deferred event hooks can still lose the current client used for cwd selection; startup replay starts
before the launching client registers; zz still applies valid commands after parser diagnostics
instead of aborting the whole file at the first cause; and non-UTF-8 cwd bytes are omitted rather
than preserved. Registered-client nested replay now carries the top-level selected base through each
recursive load. A sourced ordinary command still uses `ClientId::MAX`, but clearing its mutable
context cwd no longer changes the next nested source base. Sourcing the active default `zz/mux.conf`
forwards the snapshot through the ordinary runtime loader. A direct zz-native `reload-config`
forwards the same snapshot for registered clients. Startup keeps its separate clientless bootstrap
gap. Exact attached session-cwd selection
remains in `clients.attach-context`; the deferred event-hook and startup cases remain under
`source-file.event-hook-client-cwd` and `source-file.startup-client-cwd`. Hooks raised by ordinary
sourced commands also start from `ClientId::MAX`; `source-file.sourced-hook-client-cwd` owns that
client-identity path. Nested loud no-match and glob errors now use
the post-`-F` declared argument on the invoking client's diagnostic stream, and nested quiet
no-match stays silent. For Control, protocol v76 carries one sourced guard for each replayed command
that survives command-name resolution. Unknown or ambiguous command names and malformed alias names
publish a located Warning that Control renders as `%config-error`, without a guard. Ordinary and
quiet commands get an empty `%end`, nested partial matches retain diagnostics inside
`%end`, and all-miss or execution failures end `%error`. Guards stay in FIFO order after the direct
outer frame. The existing per-command preflight publishes the containing command's guards before
deeper replay; the strict six-step scenario and focused daemon regression prove that order with no
production change. Registered-client nested cwd rebasing is closed. Control process status for a
failed matched replay at EOF or detach remains under `control-mode.source-file-exit-status`.
The nesting limit is closed for depth wording, count, and continuation. Counting the initial
`source-file` as invocation 1, both sides run 50 concurrent source invocations and refuse invocation
51 before any of its paths are matched or loaded: Command stderr at rc 1, the same lowercase text on
the Control error channel while the outer typed line continues, and the capitalized `Too many nested
files` on an attached status line. `-q` does not suppress it, a refused command emits one diagnostic
rather than one per path, and the containing file keeps running its later physical lines. A
malformed invocation at that depth is diagnosed as malformed rather than as depth on both sides,
because the pin rejects it while parsing the containing file and never consults its depth guard;
that precedence, the stdout stream, and the rc-1 exit agree, while the differing malformed text
belongs to `mux.error-shapes` and the pin's abandonment of the rest of the containing file belongs
to `config.parser-edge-cases`. The refusal now appears inside the rejected nested command's own
flags-1 `%begin`/`%error` guard. Same-line replay grouping now matches the pin: synchronous
invalid/runtime failures, depth refusal, and loud zero-file source errors drop later siblings from
the same parser-owned source line while the next physical line runs. Matched sources do not
propagate child runtime, parser, or read failures into the parent group; zz retains matched child
read failures in `ConfigLoadReport`. Quiet zero-file misses, asynchronous commands, and unsupported
capability gaps retain continuation. Replayed target and set-option runtime failures now keep their
encounter order, use the invoking client's error channel, set the Command or Control status to 1,
capitalize attached warnings, and continue later physical lines through an outer source. Control
guard framing and cross-depth ordering are closed; source-file Control EOF and detach status plus
parser abort behavior remain open under their named gaps. Startup accounting
now matches the pin: one budget spans
every startup root, top-level roots do not count, and source commands after the first 50 retain their
declaring file and line. zz still discards those causes before Control or attached clients can read
them; `config.startup-diagnostic-delivery` owns that. Unix matching uses the
pin's `glob(3)` contract for backslashes, dotfiles, repeated stars, malformed patterns, and result
order.
One source-order residue stays explicit. `config.replayed-command-output` owns the fact that zz
routes the collected `-v` batch after replay rather than interleaving it with ordinary command
output. Runtime `source-file` now loads the active default config through the ordinary declared-order
path. Repeating default, after, and default files applies `DAD`; a loud miss returns status 1 while
later matches load; and ordinary diagnostics plus `-v` lines retain path and match order. Explicit
native `reload-config` keeps rediscovery, key-table and appearance reset, and stored override replay.
Startup first-existing discovery, ordered explicit `-f`, parse-only, and nested paths retain their
existing contracts. The focused CLI and daemon gates plus the 12-step diagnostics, 40-step format,
and six-step Control differential pass without skips or differences. This closure makes no
canonical-suite claim.
`source-file` no longer performs a second tilde rewrite after parsing, so parser-expanded tildes
stay absolute and literal tildes follow the selected relative-path base. SSH
omission is covered at the endpoint-facts helper but still lacks an end-to-end remote fixture. The
flag ledger is untouched at 127 pairs across 29 commands — diagnostics are behavior, not flags — and
`catalog.rs` has no diff this wave.

## Wave F - shared contracts and optional semantics

0. Shared parser and count foundation: give daemon and mux handlers one catalog-driven
   option and positional parser. Put reusable parsing beside `CommandSpec` in `zz-protocol`
   or export one deliberate mux API. Add positional minima and maxima, then route the four
   daemon parser exceptions through it. Add exact per-wave deltas; `BEHAVES` cannot measure
   flag work. **The machine-enforced unsupported-pair roster shipped early, with Wave D run 1**
   — `the_unsupported_flag_ledger_matches_the_catalog` in `crates/zz-mux/tests/catalog_floor.rs`
   holds all 122 pairs as a literal and cross-checks both directions against
   `COMMAND_SPECS` + `DAEMON_COMMAND_SPECS`, excluding the fourteen zz-native names derived
   against the pin's `cmd_table`. F0 inherits it rather than rebuilding it. (The roster's
   count is now 113: run 3 took `display-message -C -d` and the final run took
   `command-prompt -1 -C -e -i -k -N -T`.) **Also in F0's
   housekeeping: collapse the three near-identical deadline dispatcher threads**
   (`zz-display-panes`, `zz-monitor-silence`, `zz-client-message`, each ~70 lines differing
   only in key type and two closures) into one `run_deadline_dispatcher<K>` with token
   validation and weak-`Arc` shutdown baked in once. Deliberately not done inside Waves C or
   D — collapsing them there would have reopened three independently reviewed surfaces for no
   behavioral gain — but three is the threshold at which a fix in one silently fails to reach
   the others, and D1 or the F tranche may make it four.
1. Error contract, after F0: centralize pinned unknown-flag, missing-value, too-few and
   too-many argument, alias, and usage fallback shapes. Treat this as a command-semantics
   tranche rather than a text-only cleanup.
2. Key grammar and tables: add a fallible canonical key parser, reject malformed modifier
   tails and pin the supported stock copy-table metadata,
   repeat flags, and actions. Preserve zz-native product bindings. Bare `list-keys` alignment
   shipped 2026-08-22; the remaining selectors and stock copy-table repeat metadata shipped
   2026-08-24. Fixed-row top, middle, and bottom placement at x=0 now preserves the viewport across
   terminal revisions. The pinned action vocabulary contains 95 actions: 66 map to zz behavior and
   29 remain missing across seven categorical tracker items. The default-binding group owns only
   seven stock keys for five of those missing actions; the broader action group owns the other 24.
3. ~~`resize-window` and `window-size manual`~~: **SHIPPED 2026-08-22.** The practical absolute
   and relative forms own the durable layout extent, manual sizing outranks client measurements,
   PTYs follow the manual pane allocations, and a 16-step strict-geometry fixture covers output,
   formats, option transitions, bounds, and target/error precedence. Client-derived `-A`/`-a`
   remain loud.
4. ~~Prompt history, after A3 and D1~~: **SHIPPED 2026-08-22.**
   `show-prompt-history` and `clear-prompt-history [-T type]` now expose the existing typed rings
   with pin ordering, output, errors, selective clearing, and serialized persistence.
5. Lock and client process control: defer for a separate protocol and execution-policy
   approval together with `detach-client -E`. Settle target fanout, stale unlock rejection,
   reconnect cleanup, input gating, shell/cwd/environment, process failure, raw-mode restore,
   hooks, and `lock-after-time`. Keep the GUI behavior ledgered until it owns a lock flow.
6. Teach the differential harness to attach a client, restoring the copy-mode and chooser
   lane. This is a HARNESS fix, not a model change: zz's `copy-mode` exits 1 with `pane is not
   attached: %N` whenever no client is attached to the target pane — the mode lives on the
   per-client terminal view in zz and on the pane in tmux — and the choosers refuse with
   `choose-* requires an interactive client`. `compat/diff-scenario.sh` drives a bare CLI
   against a headless server, so every step of such a scenario diverges on exit class before
   any flag matters, and Wave D run 1's `copy-mode-flags` scenario was written and withdrawn
   for exactly that reason. Wave D run 1 proved the fix is achievable without touching zz's
   client model: nesting the run inside an outer multiplexer pane gives the inner server a
   real attached client and made every pin measurement in that wave possible, including
   reading mode screens that `capture-pane` cannot see because it reads `wp->base`. Do this
   before the copy-mode-adjacent tranches: D1, D3, and the F and G work all touch prompts,
   copy mode, or choosers, and each would otherwise land unit-tests-only. Note the reporting
   hazard while it stands — in the divergence matrix an absent row normally means "checked and
   matching", and here it means "unverifiable by the primary instrument", which is a much
   weaker claim than silence implies.

## Wave G - remaining server and engine flags

Complete F0 before these tranches. For each pair, change the catalog's unsupported flag to
an accepted flag or value and update the usage string in the same hunk. Add handler
behavior, delete its named refusal test, add differential coverage, and update the exact
unsupported-pair roster.

Tranches:

- G1 environment: `new-window` and `split-window` now carry repeated `-e KEY=VAL` overlays
  through `PaneCreated`, after the session environment and before forced pane identifiers.
  Malformed entries are ignored, later entries win, and the overlay is never stored on the
  session. Their `-E` forms create live empty panes without a child process. Creation-time
  `new-session -e` now persists last-wins overlays on the new session and reaches its first pane;
  creation-time `-E` suppresses the normal `update-environment` seed but retains explicit `-e`.
  `new-session -A` against an existing session ignores `-e`, matching the pin. The remaining G1
  work is client-sourced creation and attach-time reseeding: zz has no client environment to copy,
  accepted `new-session -E` has no existing-session reseed to suppress, and `attach-session -E`
  stays rejected until that wire contract exists.
- G2 pane input and marking: `last-pane -d/-e` shipped 2026-08-22 with input gating at the daemon's
  shared sink resolver. The remaining work is `select-pane -d -e -m -M -g -P`, the marked pane
  target and format facts, and per-pane style storage. Gate every daemon input route, keep one global
  marked pane, and clear it on pane relocation or death. Implement after C9.
- G3a placement: `new-window -b`, `join-pane -l`, `break-pane -a -b`, and `split-window -Z`
  shipped 2026-08-22. Join length accepts cells or percent, expands destination-pane formats,
  and uses the whole-window basis under `-f`. Break placement shuffles occupied window indexes,
  lets `-b` win over `-a`, and falls back from an unused indexed target to the current window.
  Successful `split-window -Z` zooms the post-spawn active pane; a plain split clears zoom even
  under `-d`.
- G3b spawn metadata and styles: `split-window -k -m -R -s -S -T`. Keep the C5-dependent
  subset parked until its terminal seam has approval; implement the independent subset only
  after C9 defines per-pane metadata.
- G3c wait lifecycle: implement `split-window -W` with command-queue ownership,
  cancellation, client disconnect, and daemon lifecycle tests.
- G4 keys: `unbind-key -a -q` and `list-keys -a -N -P` shipped 2026-08-22. The note listing
  reads `Binding::note`, limits its default view to `prefix` then `root`, computes facts after
  filtering, and treats `-P` as a literal label. **Complete 2026-08-24:** `list-keys -1 -O -r`
  and the positional key filter now share the pin's grammar, precedence, sorting, post-truncation
  facts, and attached-client presentation. Stock copy tables expose no false repeat bits. The pin's
  non-total comparator is retained as one bounded accepted divergence rather than copied into Rust.
- G5a chooser residue: `choose-buffer -F -k -y` and `choose-tree -F -h -k -y`.
- G5b output, format, and filter behavior: the relevant `display-message`, `display-panes`,
  `show-messages`, `source-file`, `capture-pane`, and `command-prompt` pairs.
- G5c client and process control: the remaining `detach-client`, `attach-session`,
  `new-session`, and `kill-*` pairs. Keep `detach-client -E` with F5's process-policy
  approval.
- G5d clipboard and client targeting: the remaining `set-buffer` and `load-buffer` pairs;
  `-w` targets a client clipboard rather than the current pane-based broadcast.
- G5e terminal engine and history: the remaining capture, copy-mode, clear-history,
  send-keys, and resize-pane pairs. Review pending-output, escape, hyperlink,
  alternate-history, reset, and target semantics before accepting each flag.
- The `resize-pane` direction amount metadata closed on 2026-08-25 without a runtime or wire change.
  Bare `-D`/`-L`/`-R`/`-U` already defaulted to one cell, and attached or separated amounts already
  worked. The catalog and manifest now model those four values as optional. The 16-step focused
  differential is clean, while `-M` and `-T` stay in G5e/G5f and their existing tracker groups.
- The first script-visible capture slice shipped 2026-08-22 without changing the unsupported-pair
  roster: `capture-pane -p` now owns stdout, no-`-p` capture owns named or automatic buffer bytes
  with the pin's final newline. The slice's 2026-08-23 extension brings its fixture to 23 clean
  steps and covers clustered value flags, inclusive/reversed `-S`/`-E` ranges, target-scoped format
  expansion, and invalid/out-of-range fallback. G5b/G5e still own `-C`/`-F`/`-H`/`-L`/`-P`/`-R`,
  inert `-T`, saved-alternate/raw transport, and the trailing-blank-row difference at fallback
  visible end.
- G5f mouse-driven behavior: `resize-pane -M` consumes an originating drag event and keeps
  its own input and geometry tranche.
- G6 binary stdin and stdout streaming: `split-window -I`, `load-buffer -`, `save-buffer -`,
  and `source-file -`. Only `split-window -I` is an unsupported flag pair; the other three
  are accepted positional grammar with missing behavior. Park the tranche until a dedicated
  protocol defines bounded binary chunks, EOF, backpressure, cancellation, disconnect,
  size limits, and binary stdout.
- Keep 23 pairs parked: `move-pane` x9, `break-pane -W -x -y -X -Y`, `new-session -t`,
  `kill-session -g`, `choose-tree -G`, `command-prompt -P`, `copy-mode -S`,
  `send-keys -M`, `display-message -I`, and `show-messages -T -t`.

The Wave D ledger held 113 unsupported flag pairs across 29 commands. The first 2026-08-22 alias
slice implemented three pairs, and the filtered kill-command slice implemented three more, leaving
107 across 26. Wave D is complete:
`attach-session -r` left with Wave B's read-only slice, `send-prefix -2` with Wave C run 2,
`copy-mode -H` plus `-K`/`-N` on both choosers with Wave D run 1, `display-message -C -d`
with run 3, and `command-prompt -1 -C -e -i -k -N -T` with the final run —
the plan's "leaving 114" was arithmetic drift, not a missed pair: 129 - 1 - 1 - 5 - 2 - 7
is 113. The original G list omitted seven chooser pairs, and later slices changed its environment
and spawn assignments. The unsupported-pair roster replaces prose arithmetic as the completion
proof. With the seven chooser pairs assigned to G5a and shipped `move-pane -l` counted only as an
implementation, the plan assigns 85 implementations and 28 parked pairs.

## Order and wave gates

The completed safe prelude is `0 -> A1 -> A2 -> B1-existing-wire`. After core approval, run
`A3 -> B with C3 title production -> C except C5 -> E -> D2 -> D4 -> D3 -> D1`. E proves the
new response streams without interactive state, and shipped 2026-08-22, as did D2 and D4. D3
establishes the freeze lifecycle before D1 reuses it for prompts.

Continue with `F0 -> F1 -> F2 -> F3 -> F4 -> G1 remainder -> G2 -> G3a -> independent G3b -> G3c ->
G4 -> G5a..G5f`. C5 and its G3b dependents wait for a terminal injection design. F5 plus
`detach-client -E` needs a later process-control bump, and G6 needs a binary-stream bump.
Each contract gets its own approval and review.

Each wave closes with:

- focused tests for each affected package and behavior
- `cargo test --workspace --all-features`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo fmt --all -- --check`
- `git diff --check`
- OKF validation
- `compat/run.sh --strict-geometry`, followed by a hard postcondition of at least the
  current 56 scenario rows (48 before Wave C run 1 added `command-alias` and
  `update-environment`, 50 before run 2 added `alerts` and `prefix2`, 52 before run 3
  added `display-panes-format` and `renderer-styles`, 54 before Wave E added
  `smoke/source-file-diagnostics`, 55 before Wave D run 3 added `display-message`),
  zero SKIPs, and no
  divergences outside the two documented geometry fixtures
- the `BEHAVES` assertion and option ledger updated
- the exact unsupported-pair roster and expected tranche delta updated
- `knowledge/tmux/divergences.md`, this plan, and `knowledge/log.md` updated
- an independent read-only Claude review against the pinned tmux source; Grok may substitute
  after its local CLI is authenticated

Give the independent reviewer the start commit, diff manifest, affected invariants, pinned
tmux anchors, and gate output. If review fixes land, rerun the focused and common gates and
obtain a clean follow-up verdict before closing the tranche.

The final campaign handoff also runs the CI surfaces outside those common gates: native and
nightly-wasm UI showcase checks, release-profile `zz-mux`, `just ios-test`, macOS CEF release
bundling, DMG packaging, and the hardware smokes below. Linux and Windows runtime and
packaging proof require remote CI. The workflow has no manual dispatch, so without later
commit and push authorization the final report records those platforms as pending rather
than claiming proof.

**Safe prelude common gates passed locally 2026-08-21.** The full workspace all-feature test
suite and all-target clippy pass, formatting and diff checks are clean, and OKF validation is
conformant with only the pre-existing stale-research warning. The strict compatibility run
completed all 48 scenarios with zero SKIPs and zero unexpected divergences; its two known
geometry fixtures matched their documented divergences, and all eight plugin smokes passed.

## Decision ledger for the unattended run

| Decision | Recommended disposition |
| --- | --- |
| Shared style parser | Move the style and tmux-color parser unit together to `zz-protocol`, then re-export it from `zz-mux`. |
| F5 TUI lock | Park. It adds new lock execution behavior and optional wire messages. |
| TUI status extras | Keep `PREFIX` and `COPY`; hide the detach hint after explicit status customization. |
| Unsupported config summary | Park until Command responses gain an approved stderr channel; keep stderr plus exit 0 and invalid lines at exit 1 in the eventual contract. |
| Client environment and stdin wire | Park `attach-session -E`, the attach-side reseeding half of `new-session -E`, and all G6 streaming until fabrico approves the contracts. Pane-local spawn `-e`/`-E` and creation-time `new-session -e`/`-E` need no new wire and have shipped. |
| Protocol v71 | Recommend the audited single bundle in A3, with per-client option publication, personalized status metadata, per-pane typed border colors, and Command-response stderr. Keep it parked until fabrico approves it. |
| Prefix scope | Keep both prefixes global in v71 to match the current shared key-table contract; treat tmux-style session scope as a separate core refactor. |
| Lock wire | Exclude it from v71; if F5 is approved, use a separate bump with an explicit client process-execution policy. |
| Timed messages and prompts | Add daemon-owned per-client presentation freeze, message identity, and explicit clear to v71. Route prompt special keys through the existing pane-targeted input and let the daemon return handled or pass. |
| `remain-on-exit-format` | Park C5 until terminal core owns a post-worker VT injection or frozen-view reconstruction seam. |
| F and G grouping | Use the parser foundation and semantic tranches above. Reject the old mechanical-single batch and enforce the 128-pair ledger in code. |
| Core semantic waves | Park daemon, mux, lifecycle, parser, target, geometry, and command-semantic changes under the unattended boundary. |

## Hardware smokes

The existing open smokes remain: Wave-B status, `allow-passthrough` image output,
DCS-filter benchmark A/B, and iTerm2 `-CC`. Add TUI status at the top with styles, GUI pane
border colors and inactive-window dimming, and the TUI lock flow if F5 later enters scope.

# Phases

Ordering rationale: everything-loud before anything-new (phase 0), the stock-binding blockers
before the grind because tmux's own default bindings hit them (phase 1), the differential
harness before the geometry rework it steers (phase 2 before 3), and `base-index` first in the
grind because index arithmetic touches everything (phase 4).

## Phase 0 — the floor (shipped 2026-08-16)

Catalog-driven unknown-flag rejection: every command rejects flags absent from its
`CommandSpec`, deleting the hand-rolled allowlists — 36 distinct sites in `command.rs` today
(6 `reject_flags` calls plus 30 inline allowlists), not the ~15 first estimated. The one-time
audit that catalog entries match handler-accepted flags **is** the work; the code change is
small. After this, every remaining gap is loud — the precondition for claiming compatibility
at all. Note: this makes currently-swallowed flags *louder* (by design); the fixes land in
phases 1 and 4.

## Phase 1 — superset rework + stock-binding blockers (shipped 2026-08-16)

Move every GUI-motivated divergence off tmux names (shipped: picker renamed `split-picker`,
key-bound `split-window` gives terminals, `select-window` bounds at zero positionals; the
stock-binding blockers below shipped the same day — `source-file` `-F`/`-n` moved to the
phase-4 grind, `-` stdin is a loud refusal):

- Stop routing key-bound `split-window` to the picker; zz's *default* bindings bind a zz verb,
  imported tmux bindings get pure tmux behavior. Also closes the TUI's bare-split-opens-picker
  gap.
- Rename the picker verb off `new-pane` — the pinned tmux owns that name for floating panes.
- Tighten the remaining zz-lax argument acceptance (`select-window` positionals; the
  `attach-session` half landed in PR #4).

Then the divergences tmux's **own default bindings** hit — a drop-in whose mouse wheel errors
is not a drop-in (all shipped 2026-08-16):

- `copy-mode -e`/`-M`/`-q` landed (stock `WheelUpPane`/`MouseDrag1Pane`/menu bindings use all
  three); `-k`/`-H`/`-S`/`-s` stay loud.
- `send-keys -N` with no keys arms the client's copy-mode count prefix (stock vi digit
  bindings work; the prefix is client-scoped where tmux's is pane-mode-scoped — see the
  divergence matrix).
- Window activation clears its panes' bells, so `next-window -a` steps instead of re-picking
  the same window, and the terminal bell latch is released on the same transition.
- `source-file` expands wildcard patterns for every declared path (`conf.d/*.conf` works), and
  Unix matching now follows tmux's `glob(3)` edge cases. `-` stdin is a loud refusal. `-F` expands
  paths in the resolved source target, `-n` parses without effects, `-t` selects the pane context,
  and `-v` preserves source order while staying suppressed for Control.
- `bind-key` payloads validate at bind time (names + flags; arity and targets still surface
  at keypress), and invalid config lines now reach the import report.

## Phase 2 — the differential harness (shipped 2026-08-16)

**Not control mode.** The harness is: one command script fed to zz and to the pinned tmux,
diff `list-sessions`/`list-windows`/`list-panes` output with *explicit `-F` formats* on both
sides — `-F` is already machine-readable and identical formats sidestep the default templates
(which only converge after the phase-4 formats grind). Prerequisite: geometry format
variables (`pane_width`/`pane_height`/`window_width`/`window_height`), readable today from
the measured cell geometry the daemon already feeds the engine (`pane_cells` via
`ResizeTerminal`) — so the harness diffs geometry *before* phase 3 lands and steers phase 3
to convergence, rather than being validated by it. Control mode itself moves to phase 6; a
control client is a worse differential tool (it streams `%output` for every pane and adds a
transport layer to debug).

Shipped as `compat/` (see [the compat harness playbook](/playbooks/compat-harness.md)): the
2a format vocabulary landed first (geometry, activity flags, tmux-style scope backfill), then
the per-step runner with strict TOPO/exit-class diffing, report-only GEO, a
`scenarios/known/` set for accepted divergences, and a Linux CI leg with the pin cached. The
seven-scenario corpus runs TOPO-clean against the pin; every GEO hunk is phase-3 steering
data.

## Phase 3 — cell-authoritative layout (shipped 2026-08-17)

Cells are the layout truth: `zz-mux/src/layout.rs` is an n-ary cell-tree port of the pin's
`layout.c` (splits, remove-gifts-space, window resize spread, resize-pane victim walks, all
seven presets, leaf-gated spread), owned per window by `model.rs`; the wire ratio tree is a
derived projection with stable divider ids, so no protocol, FFI, TUI, or iOS surface moved.
See [split-pane layout](/concepts/split-pane-layout.md) for the shipped architecture.

- Validated by 48 golden fixtures captured from the pin binary
  (`compat/gen-layout-fixtures.sh`) replayed in CI debug AND release, and by the harness
  running `--strict-geometry` clean across the corpus — strict geometry is now the CI
  contract, with each window's layout string structurally diffed at every step.
- Layout strings shipped both directions: `dump()` and `parse()` (case-insensitive
  checksum, optional leaf ids, 256-deep cap where the pin spins), `select-layout <string>`
  with the pin's exact bottom-right trim, and `#{window_layout}`.
- Windows are born at tmux's 80x24 headless, honor `new-session -x/-y`, and track a drawing
  client through a guarded measurement write-back (dead-band + repeat memo fixed point);
  divider drags stay smooth and commit the cell-snapped ratio on release (the feel decision
  landed on commit-on-release).
- The review rounds killed a daemon-aborting resolver recursion, a drag-override feedback
  loop into the write-back, unpublished mutations (generation-diff catch-alls now guard
  both daemon boundaries), and select-layout's missing unzoom-first; two upstream layout
  bugs (two-pane main-* presets, mixed-parent `-E` spread) are refused and documented in
  [the divergence matrix](/tmux/divergences.md) with `known/` harness scenarios.

## Phase 4 — the grind (shipped 2026-08-17: six waves, each reviewed to CONFIRMED-CLOSED)

Running wave-by-wave on [PR #6](https://github.com/demfabris/zz/pull/6) (`feat/tmux-grind`),
same loop as phases 0–3: settled plan → codex → full gates → adversarial pin review → close.

| Work | Scope | Status |
| --- | --- | --- |
| `base-index` / `pane-base-index` / `renumber-windows` | index arithmetic everywhere, plus the pin's full 248-entry option table routing every tmux name by declared scope (`setw -g base-index 1` works) | **shipped 2026-08-17** (wave 4a) |
| Target grammar | the full cmd-find pass order (fnmatch, `=`-exact, unique prefix, `{start}`/`{end}`/`{last}`/`^`/`$`/`!`/`+`/`-`), empty targets, tmux's exact `can't find …` error strings, cross-window `select-pane` focus semantics | **shipped 2026-08-17** (wave 4b) |
| Full formats engine | the 198-name registry, every scalar modifier + `S`/`W`/`P` loops + `e` math + `C` search, the daemon runtime-facts feed (proc cwd, pid, tty; OSC 7 → `pane_path`), `-c` as a format consumer with spawn.c's chdir chain, the `fmt:` differential channel | **shipped 2026-08-17** (wave 4c, four review rounds) |
| Options readback | `show-options`/`show-window-options` with the pin's quoting, `@user` options as pure storage (TPM), `set-`/`show-environment` + PTY env injection, the `out:` differential channel. The review round added: MRU session activity aligned to the pin (create/attach/key-input only — detached CLI traffic never bumps it), the `VISUAL`/`EDITOR` → `mode-keys` boot sniff, indexed `name[idx]` spellings, global-environ seeding + `update-environment` markers, name-sorted `list-sessions` (the `#{S:}` loop deliberately stays creation-ordered like the pin's), and harness env scrubbing (`TMUX_PANE`/`EDITOR` leak both poisoned local probes) | **shipped 2026-08-17** (wave 4d, one review round, CONFIRMED-CLOSED) |
| The gap commands | `move-window`/`swap-window` full flag surface, `find-window`, `list-clients`/`list-commands` (honest subset, usage strings show zz's accepted flags), `show-messages` (newest-first log, live `message-limit`, failing commands log both `command:` and `message:` lines), `start-server`, `refresh-client`, `list-windows -a` / `list-panes -a`/`-s` name-ordered (resurrect's save path). The range also carried the strftime-parity fix: display-message runs the pin's whole-string-per-level libc strftime (the workspace's only `unsafe` block), `%` accepted as modulo — root cause of the Linux-only CI divergence | **shipped 2026-08-17** (wave 4e, two review rounds, CONFIRMED-CLOSED) |
| Behavior options, semantics half | `mouse`, `escape-time`, `automatic-rename`, `automatic-rename-format`, `remain-on-exit`, `default-terminal`, `display-time`, and `repeat-time` typed storage/readback; active-pane tab-label gating and explicit-name pinning; retained dead facts plus stable-id `respawn-pane`/`respawn-window`; TERM, message/overlay timeout, and repeat-window consumers. `mouse` and `escape-time` stay storage-only for phase 8. The review round caught two falsified claims (a renamer that never fired; default-terminal correct in readback but not AT the default — the ledger's default-path hazard) and both are fixed and pin-verified; defaults come from the PIN BUILD's -DTMUX_MOUSE/-DTMUX_TERM, protocol v59, and the macOS zero-pgid panic behind the oldest flaky CI test died in validation | **shipped 2026-08-17** (wave 4f-1, one review round, CONFIRMED-CLOSED) |
| Behavior options, sizing/boot half | `aggressive-resize` stored at global-window/window scope; ON selects componentwise smallest rows and columns from clients actually viewing each window, while the existing zoom gate, active-pane writer, one-cell dead-band, and repeat memo remain unchanged (verified by positive control; seeded convergence sims pass on real sockets). Lazy-create boot parity: fresh daemons empty+unarmed, session 0 on the first default Interactive attach, ids aligned with tmux from the first `new-session` — the harness prologue's auto-session kill is gone and the GEO id-stripping is DELETED, so raw layout checksums and leaf pane ids byte-compare against the pin across all 25 scenarios | **shipped 2026-08-17** (wave 4f-2, one review round, CONFIRMED-CLOSED — phase 4 complete) |
| Daemon boot parity | CLI-spawned daemons boot empty; the first CLI `new-session` takes name `0` and ids `$0`/`@0`/`%0`, while an empty-target Interactive attach lazily materializes that next numeric session. The harness no longer kills zz session `0` in its prologue and now compares raw layout checksums and leaf ids | **shipped 2026-08-17** (wave 4f-2, phase 4 closed) |
| Styles (`#[…]`, `*-style`) | GUI titlebar strip and tabs render them since waves A/B (2026-08-20); zz-tui still drops them before the wire; the terminal-renderer styles (`pane-*border-style`, `window-style`, `mode-style`) are C3 | partial |
| `source-file -F` | format-expanded paths | **shipped 2026-08-22** |
| `source-file -n`/`-t`/`-v` | parse-only replay, pane-targeted context, and ordered verbose diagnostics | **shipped 2026-08-25** |

`switch-client` shipped as protocol v70 on 2026-08-20. A pane script can retarget the
Interactive client selected from its retained `ClientHello.origin`; the daemon reuses its
attachment path and sends the existing `ProtocolMessage::Attached` unsolicited to converge
the target client's presentation and command context.

## Phase 5 — the exec family (COMPLETE 2026-08-18)

All waves shipped, each reviewed to CONFIRMED-CLOSED against the pin:

- **Wave 5a-1** (`26c86d0`) — spawn argv parity: argc>=2 execs the argv directly
  (PATH search, no shell), argc==1 runs `default-shell -c`, argc==0 keeps zz's
  integrated login-shell path. `default-shell` is runtime-resolved at boot
  ($SHELL → passwd → /bin/sh), checkshell-validated at set time (`not a suitable
  shell:`), and reverts to `/bin/sh` on global unset; `default-command` wired at
  the spawn seam; `pane_start_command`/`pane_start_command_list` render with
  byte-exact `args_escape` / single-quote-per-element parity (52/52 adversarial
  quoting rows identical); respawn reuses creation-time argv AND shell; direct
  spawn failure dies status 1; dead panes serve their frozen frame to
  capture-pane.
- **Wave 5a-2** (`9f55f87`) — `run-shell`/`run` and `if-shell`/`if` execute for
  real: daemon job machinery (always `/bin/sh -c`, stdin = the output pipe, no
  timeout, own process group), foreground blocks the CLI with exit-code
  propagation (protocol v61: append-only `exit_code` on CommandResponse::Success),
  `'cmd' returned N` / `terminated by signal N` message shapes, four-sink output
  routing (resolved `-t` → pane overlay; CANFAIL fallback → client sink;
  session-less client → stdout; `-b` → MRU pane overlay), `-C` command insertion
  (expanded, no numeric vars, foreground waits through it), `-E`, `-d` strtod
  semantics (`''`→0, hex accepted, `invalid delay time:` on garbage, delay before
  empty-args check), `-c` verbatim with silent HOME fallback and verbatim child
  PWD, numeric `#{1}..#{n}` only on the non-`-C` string, `if-shell -F` first-byte
  truthiness, branches never expanded, brace blocks via the binding path,
  config-phase execution blocks boot with output dropped. The stray `-s` flag is
  accepted-and-ignored like the pin.

- **Wave 5b** (`081e88a`..`2d7a655`, CONFIRMED-CLOSED 2026-08-18) — `wait-for`/
  `wait` and `pipe-pane`/`pipep` execute for real. wait-for: channel registry
  with the pin's exact sticky-signal parity (a second `-S` destroys the channel
  — reproduced), FIFO lock handoff, locks deliberately leak across holder
  disconnect like the pin, kill-server flush, sticky signals survive the
  signaling client's disconnect; Command clients block faithfully, Interactive
  clients get the pin's clientless errors (accepted divergence — the GUI
  multiplexes one connection). pipe-pane: raw PTY output tap with pre-parse
  forwarding (tapped bytes reach the pipe child BEFORE VT parsing; a bounded
  4MiB ordered backlog feeds the parser in 16KiB turns — piped floods now
  drain FASTER than un-piped), bounded blocking tap = true backpressure with
  no drop path (8MB floods lossless), always-close-old-then-`-o`-toggle,
  strftime command expansion, `-I` injection, `#{pane_pipe}`/`#{pane_pipe_pid}`,
  pipe SURVIVES respawn-pane with the same child (pin-verified), receiver loss
  is loud (pipe fully closed, formats cleared), kill-server reaps job process
  groups, MAX_SHELL_JOBS raised to 256. Three probe-driven fix rounds; the
  final root cause was measured (VT parse was pacing the old serial path:
  55.9s of a 55.9s 2MB run), not guessed.

- **Wave 5c** (`e4e6602` + `40ddd63`, CONFIRMED-CLOSED 2026-08-18) — the hooks
  bus, both halves. 5c-1: all 68 hook names stored as array command options
  with pin scope (57 session / 11 window; `show-hooks -g`/`-gw` listings
  byte-identical incl. table order), set-time parsing, `-a` free-index
  allocation with reuse-after-unset, prefix matching, `-R` immediate fire
  (unknown silent), `@`-prefixed user hooks share the `@`-option slot exactly
  like the pin (set-hook overwrites the option, unlisted in show-hooks, `-R`
  parses-and-fires, parse failures swallowed), after-* fires only on success
  at the daemon boundary with hook_arguments/hook_argument_N/hook_flag_*
  formats, command-error on failures (hook output precedes the error text —
  protocol v62: append-only `output` on CommandResponse::Error), NOHOOKS
  one-level, hook output joins the TRIGGERING client. `set-hook -B` monitors
  rejected (ledger row: pin validates the spec instead). 5c-2: event hooks
  fire CLIENTLESS like the pin's deferred global queue (their output reaches
  no CLI; side effects land): session-created/closed/renamed/window-changed,
  window-linked/unlinked/renamed (incl. automatic-rename)/layout-changed/
  resized/pane-changed, pane-died/exited (pin's 4-cell remain-on-exit matrix
  matched)/mode-changed/title-changed, alert-bell,
  client-attached/detached/session-changed; 3-tree lookup with per-window
  isolation and session-shadows-window order; NOHOOKS full-drop; deferred
  tolerance of dead subjects; boot-ordering (config-armed hooks fire for the
  first session). Store-only (no zz seam yet; ledgered): alert-activity,
  alert-silence, client-active, client-focus-in/out, client-resized,
  client-light/dark-theme, pane-focus-in/out, pane-set-clipboard. Accepted
  divergence: window-layout-changed fires once on resize-pane/select-layout
  where the pin double-fires (under-fire, stable 3/3).

- **Wave 5d-1** (`2a0eb23` + `bbfc9aa`, CONFIRMED-CLOSED 2026-08-18) —
  `display-popup`/`popup` as a daemon-owned popup TerminalSession rendered by
  the GPUI client as a floating zz-design-language pane (maintainer decision:
  native visuals, one ledger row; zz-ui FloatingSurface hosts the terminal
  element, keys claimed above the prefix). Pin-exact behavior: client
  resolution (bare `no current client` byte-match, correct precedence),
  blocking CLI with the retval contract (exit status / raw signal / 129
  early-dismiss), size grammar (percent >100% errors `too large`), position
  grammar (popup_* variables, bottom-anchored -y, last-flag-wins), the
  command-shape matrix (default-command interplay, >=2-argv execvp,
  JOB_DEFAULTSHELL), -E/-EE/-k close matrix, -C clears any overlay,
  one-overlay-per-client with a dead-job-SAFE modify path (the pin's
  popup_modify NULL-deref is deliberately not replicated),
  popup-style/popup-border-style with pin `invalid style:` validation,
  popup-border-lines choices, sub-3x3 refusal, SIGTERM cleanup on
  detach/kill-server. Protocol v63 (append-only popup messages,
  structurally verified tail appends). Ledgered omissions: right-click
  context menu, border drag move/resize, to-pane transfer, TUI/control-mode
  rendering, mouse/status-line position variables. Hardware smoke pending
  (maintainer): blocking retvals, close matrix, input capture above the
  prefix, dead-job modify, -C cross-client, position letters, -e/-d live.

- **Wave 5d-2** (`01096c2` + `30d4daa`, CONFIRMED-CLOSED 2026-08-18; closes
  phase 5) — `display-menu`/`menu`, `confirm-before`/`confirm`, and the lock
  trio. Menus and confirm prompts render NATIVE on the 5d-1 FloatingSurface
  (behavior pin-exact, visuals native — same ledger row). Menu: exec order
  ported exactly (overlay silent-noop → -C validation → title → item build →
  empty/too-small silent-noops → -b), triplet grammar with separator
  slot-consumption/leading-and-double-separator drops/`not enough arguments`,
  build-time format expansion with empty-name drop, `-`-disabled items,
  unparsable keys never matchable, shortcut-beats-navigation selection,
  wrap-on-step but CLAMP-on-page (menu.c's PPage jump-to-0-without-skip wart
  and NPage clamp-then-walk-backward kept), Enter's chosen-block (no/invalid
  selection closes unless -O stay-open), cancel set exact, blocking CLI
  unblocks with cancel rc 0 and NO retval propagation (opposite of popups),
  chosen command queued on the menu's client with fire-time target
  re-validation, menu-style/menu-selected-style/menu-border-style/
  menu-border-lines window options with byte-identical theme-colour defaults
  — but inline -s/-H/-S styles pass through UNVALIDATED like menu_prepare's
  silent style_parse fallback (only the options-table path validates).
  Confirm: parse-up-front (parse error → no prompt), exact
  `Confirm '<name>'? (<key>/n) ` canonical-name prompt + `-p` one-trailing-
  space, printable-ASCII -c, blocking rc contract (reject/dismiss rc 1 —
  opposite convention from menu cancel), -b append-with-fresh-state,
  prompt-opening clears any overlay. Lock trio: storage + error parity only
  (lock-command default `lock -np`, lock-after-time 0 stored, readback;
  clientless shapes exact; after-lock-server fires via the 5c bus; NO lock
  process spawning over GUI surfaces — ledgered, revisit with the TUI).
  Protocol v63→v64 append-only (menu/confirm messages; popup tags
  structurally unchanged). GUI key-claim proven for a prefix-table key
  (armed `ctrl-a` with a menu focused sends nothing). Bonus fix found by
  probe: `zz kill-server; zz new-session -d` failed deterministically (the
  daemon's bound listener + socket file outlived the accept loop by seconds,
  EOF-ing backlog connects; pin 3/3 ok) — the daemon now drops listener +
  socket/identity guards immediately after the accept loop, the connect
  classifier treats same-version handshake-EOF as a dying daemon
  (ConnectionReset; socket-gone → ENOENT) with fake-daemon shapes untouched
  (`Socket operation on non-socket` byte-match), and prepare_socket waits
  out a still-connectable dying socket before AlreadyRunning. Ledgered
  hardening: SocketGuard's drop unlink is unconditional (no dev/inode
  ownership check) — window now microseconds, not airtight. Deferred:
  tmux mouse semantics on menus (GUI-native), MENU_STAYOPEN mouse paths.
  Hardware smoke pending (maintainer): menu keyboard walk + shortcut fire,
  confirm prompt accept/reject live, paging clamp feel on a long menu.

Phase 7a (binary surface) shipped in parallel (`a054c38` + `34d9d60`,
autostart CONFIRMED-CLOSED): tmux argv (-V `tmux 3.8-zz`, -L/-S/-f, -c, -N,
-l; tmux-shaped usage + `unknown option`/`option requires an argument`
lines), daemon autostart gated to the pin's five CMD_STARTSERVER commands
(the `ls || new-session -d` idiom restored; bare `error connecting to
<path> (<errno>)`; distinct stale-socket `no server running on <path>`),
no intermediate -L label dirs, and $TMUX=<socket>,<pid>,<session> +
TMUX_PANE=%N in panes plus $TMUX (no TMUX_PANE) in exec-family jobs —
closing the tpm-breaker ledger row. FULLY CLOSED 2026-08-18 with `64fd9a6` +
`05a5258` + `4184b80` (reviewer rounds 9-11): the native `zz attach` grammar
is a tmux superset (`attach`/`attach-session -t <session>` both spellings,
`-d` wired, engine-identical rejections), and attach orders like the pin —
daemon connect WITH autostart first (attach is CMD_STARTSERVER; `-f` config
reaches the spawned daemon), session resolution second (`attach -t bogus` →
`can't find session: bogus` rc 1 headless; untargeted empty server → `no
sessions`), the TTY interactivity check LAST. Style validator carries the
full style_parse token set (align/fill/us/list/range/push-default/
pop-default families; `range=session`/`hyperlink` rejected like the pin);
`-L <notadir>/x` says `error connecting to` (connect-first ordering). The
`zz attach: ` stderr prefix is a second wrapper-class shape (rc +
post-prefix text exact) — phase-7 error-shape scope. Two items were
deferred to phase 8 with the attach contract: the no-tty `new-session`
divergence (pin: `open terminal failed: not a terminal` rc 1; zz: detached
create rc 0), CLOSED 2026-08-20; and the pty-gated nested-session refusal
probe, which stayed open and is now ledgered in the divergence matrix (it
needs the client's tty compared against this server's pane ttys — `$TMUX`
alone is the wrong signal). Accepted wart adopted: `-L
<nested/label> new-session` prints `error creating <path>` and exits 0 like
the pin.

Original phase-5 tiering, kept for the record:

- `run-shell`, `if-shell`, `wait-for`, `pipe-pane` — genuinely `sh -c` + effects (~1 week).
  `if-shell` is already parsed and kept (only `%if` is skipped at parse time); the upgrade is
  executing the stored branches.
- `set-hook`/`show-hooks` — an event bus, not a spawn: tmux has **68** hook points at the pin,
  and control-mode notifications (phase 6) are fed by the same events (~1 week).
- `display-popup`, `display-menu`, `confirm-before`, the `lock-*` trio — UI on both the GUI
  and TUI surfaces; `display-popup` is load-bearing for tmux-fzf and `fzf-tmux -p` (~1 week+).

**The consent gate, rebuilt on verified facts.** Config already executes shell ungated today:
`#()` in status strings spawns `/bin/sh -c` every `status-interval`, and the
`[shell-command]` positional on the three creation commands spawns the same way. And the
daemon sources config at initialize, before any client connects — there is nobody to prompt.
So:

- **Your own `mux.conf` is trusted**, like `.bashrc` — tmux itself runs `run-shell` from
  `.tmux.conf` without ceremony, and gating only `run-shell` while `#()` runs free would be
  theater. Exec lines in the user's own config just run, including at daemon start.
- **The gate guards the import flow**: when zz copies a foreign `tmux.conf` in, the importing
  client is present — prompt once per import (never per line), show the exec lines, persist
  the decision into the imported result.
- **Remote hosts need no per-host consent plumbing**: a remote daemon sources *that host's*
  own config (nothing travels over ssh; fleet config writes `host-*` lines only), so each
  host's config sits in that host's trust domain.
- Interactively-typed commands (`prefix :`) always run without ceremony.

## Phase 6 — control mode (COMPLETE 2026-08-18)

`-C`/`-CC` for iTerm2 and control-mode scripts. The transport, verified against iTerm2's
`TmuxGateway.m`: iTerm2 launches `tmux -CC` **in a PTY and parses the `%begin`/`%output` text
protocol from the process's stdio** — it never opens tmux's socket. So control mode is a zz
*front-end*, like `zz-tui`: the zz process speaks the CC text protocol on its own
stdin/stdout and talks postcard to the daemon behind it. Daemon-side needs: a raw pane-output
tap (the wire ships rendered grids today; `%output` carries raw bytes), notification events
(from phase 5's hook bus), and `refresh-client -C`/`-B`/`-A` (client size, subscriptions,
pane visibility). The harness (phase 2) does not wait for this.

- **Wave 6a** (`c4206f0` + `4c7bfa5`, CONFIRMED-CLOSED 2026-08-18) — the
  skeleton + framing. Protocol v65: `ClientKind::Control` appended (attach
  rights + event subscription + Command-style output routing, no frames, no
  input, no color scheme; never-wedge on interactive-only commands).
  `crates/zz/src/control_mode.rs`: -C/-CC argv counting, CMD_STARTSERVER-
  gated autostart (pin-probed: `-C ls` neither starts nor frames; bare `-C`
  = new-session and does), connect failure = bare stderr with no framing,
  `%begin/%end/%error <t> <n> <f>` (f=0 argv / f=1 stdin; per-client
  monotonic n = ledgered safe subset of the pin's server-global sparse
  counter), the attach rule (non-attaching commands exit rc 0 with a bare
  `%exit` after their block; `new-session -d` never reads stdin; stdin
  gated until attached), argv parse failures unframed, stdin `parse error:`
  blocks, empty-line detach, whitespace/comment no-block, `;` chains one
  block per command with abort-on-error per line (pin-probed), all %exit
  paths, -CC near-raw termios + `\x1bP1000p`/`\x1b\\` envelope (both
  RAII-guaranteed on unwind), and a block-state writer that defers
  notification lines while a block is open (the 6b seam). Fix round: bare
  `list-sessions`/`list-windows`/`list-panes` now render the pin's default
  templates through the format engine — a pre-existing phase-4 gap (the
  harness always diffed with -F; the legacy `(id $N)` shapes reached no
  other surface). Live stream differ 10/10 vs the pin with a re-proven
  positive control. Ledgered: `history_size`/`history_bytes` render honest
  zeros (shape exact, needs a history-stats seam);
  `session_grouped`/`session_group`/`pane_floating_flag` render empty
  through conditionals; zz blocks are COMPLETE where the pin's WAIT
  commands emit late bare lines; zz emits ONE block per stdin command where
  the pin adds a flags-0 block per after-hook; no `default-client-command`
  option (new-session hardcoded, the pin's default).

- **Wave 6b** (`cbb34a2`, stamped `0dc46aa`, CONFIRMED-CLOSED 2026-08-18) —
  notifications + layout strings + basic %output. Protocol v66:
  `EventPayload::HookEvent {name, variables}` (tag 40) exposes the 5c hook
  bus to Control subscribers only, `PaneOutput {pane, bytes}` (tag 41), a
  typed overflow exit (tag 39), and `WindowSnapshot` gained trailing
  `layout_dump`/`visible_layout_dump` (tmux layout strings, checksummed).
  Daemon: paste-buffer-changed/deleted seams added; a daemon-owned
  raw-output multiplexer owns the pane tap and feeds BOTH pipe-pane and
  Control subscribers — ownership transfers (rearm), never evicts; verified
  live in both orders (pipe-then-control and control-then-pipe). Front-end:
  the full notification inventory rendered through the block-deferral seam
  (one FIFO for notifications AND %output → nothing ever interleaves into
  an open block, arrival order preserved), %output with the pin's exact
  escaping (\NNN for 0x00-0x1F + backslash, 8-bit raw — byte-identical
  concatenated streams), %message, %config-error, `%exit too far behind` on
  the overflow disconnect. window-unlinked ALWAYS renders
  %unlinked-window-close (the pin's deferred callback runs post-unlink, so
  plain %window-close is unreachable without linked windows — probe-caught,
  reviewer-verified against control-notify.c). Live two-client mutation
  probe: notification streams line-identical INCLUDING ordering, modulo the
  ledgered automatic-rename class (the pin's 500ms sniffer emits transient
  `tmux`/`kernel_task` names; zz single-fires the settled name). Ledgered:
  the A5 overflow trigger divergence (count/size vs the pin's 5-minute age
  model — same %exit text, different trigger); the tap handoff is
  replace-then-rearm, not atomic (flood-test in 6c); startup notification
  ordering verified empirically, not proven structurally (re-probe if
  publish ordering changes); client-name spellings in %client-* lines
  unverified vs the pin's c->name.

- **Waves 6c + 6d** (`0e5ea00` + `4e69882` + `ed7d3c5`, combined
  CONFIRMED-CLOSED 2026-08-18 — closes phase 6) — flow control, sizing,
  subscriptions. 6c: protocol v67 (PaneOutputState/PaneOutputAged/
  ControlFlags); per-(Control client, pane) output state with off/paused
  DISCARDING AT QUEUE ENTRY; auto-pause on oldest-chunk age under
  pause-after; AGE-KILL at the pin's 300s without pause-after (closes the
  A5 divergence — the mailbox count/size cap is now only a backstop);
  pacing with the pin constants (8192/512/32, headroom÷panes÷3, message-
  count gate at half the mailbox cap); refresh-client -A on/off/continue/
  pause with silent-malformed; -f/-F no-output (resets offsets),
  pause-after[=N], wait-exit (empty-line/EOF release); %extended-output
  `%N <age> : ` + %pause/%continue. The load-bearing 6c lesson: flow
  control requires REAL backpressure — the front-end reads through a
  bounded sync_channel(32) so a stalled consumer reaches the daemon (an
  unbounded channel silently absorbed floods and pause-after could never
  fire), and detach/EOF DRAIN queued events before %exit (the pin's
  control_all_done flush; the daemon acks a Control self-detach with
  Detached as the FIFO flush marker). Hook delivery uses the pin's exact
  per-name session guards (window-layout-changed/linked/unlinked/renamed +
  client-session-changed are attached-only; sessions/paste-buffer/
  pane-mode/client-detached reach session-less clients), the departing
  client is excluded from its own client-detached, and the front-end
  renders hooks only once attached (pin CLIENT_EXIT analog: `-C
  new-session -d` shows zero notifications). 6d: protocol v68
  (SubscriptionChanged); refresh-client -C whole-client + @w:WxH
  per-window sizing with pin error shapes and 1-10000 bounds — a sized
  Control client legitimately drives window sizing exactly like the pin
  (pin-probed: 150x40 -> 200x60 during attach, persists after detach) and
  feeds menu/popup geometry gating; -B subscriptions (session/%pane/%*/
  @window/@* scopes, first-two-colons split, fewer = REMOVE, 1s
  change-only evaluation with initial report and entity sweeps —
  kill-window probe shows no phantom reports and no stale state);
  client_flags format. Client-name spelling unified: device-{N} is
  canonical everywhere (generation + all print surfaces), client-{N} kept
  as a resolver-only alias — the resolvers accept exactly what
  list-clients prints. Live probes: -A matrix semantics, -C matrix,
  %extended-output, no-output, and all three %subscription-changed shapes
  byte-identical to the pin (subscription probes must hold the control
  client's stdin OPEN across the pin's 1s timer). Ledgered: %pause/
  %continue placement (pin writes them INSIDE the triggering block via
  synchronous control_write; zz after it — blocks-complete family,
  reviewer-endorsed); zz-lax %-word parsing on the control stdin (pin:
  `parse error: syntax error` for unquoted %0:pause); stdin commands share
  the 32-slot channel with %output (a flood can delay a new command by up
  to 32 events — bounded, thin-client property); pipe_pane_has_no_gap +
  default_shell_rejects join the load-flake set (VT-throughput root
  cause). Hardware smoke pending (maintainer): zz -CC under REAL iTerm2 —
  attach, pane content, window sizing via -C, detach.

## Phase 7 — the binary surface — **PHASE COMPLETE 2026-08-18**

Closed by 7a (argv surface, `$TMUX` shape, `-V`), 7b (error-output shapes), and 7d
(the alias smoke suite, `e45f0dd`). The only phase-7 residue is the optional 7c
appendix: arity/flag rejection wording, the `usage:` fallback, and the
`MissingTarget` inner texts — all ledgered, none script-facing.

- tmux argv on the zz binary: `-L` (name → socket path), `-S`, `-f`, `-2`, `-u`, plus `-C`/
  `-CC` (front-end from phase 6) and `-V`.
- `$TMUX` exported in panes alongside `ZZ_*`, in tmux's **exact shape**
  `socket_path,server_pid,session_id` — resurrect `cut`s field 1 for the socket, continuum
  field 2 for the pid; `[ -n "$TMUX" ]` alone is not the contract. `$TMUX_PANE` = `%id`.
- `tmux -V` answers `tmux 3.8-zz`: the pin `d77c9dc6` is `next-3.8` (`AC_INIT([tmux],
  next-3.8)`), not 3.5. TPM's version check digit-strips either to `38`; handle other
  version-gating fallout case by case.
- Exit codes and error-output shapes matched where scripts grep them. **SHIPPED
  2026-08-18** (wave 7b, `b350414`): the CLI renders the pin's bare stderr — the
  `ServerError` render is lifted from control mode with an `InvalidCommand`-only strip
  (`UnsupportedCommand` keeps its `unsupported command:` noun on every surface, a
  reviewer-caught over-strip). All twelve `regress/options-values.sh` strings plus
  `can't find session/window/pane:` and `unknown command:` byte-match the pin (live
  probe 27/27 with positive control); no-tty attach says `open terminal failed: not a
  terminal`; show-messages records pin-shaped `message:`/`command:` pairs; config
  errors compose `%config-error <file>:<line>: <text>` exactly as the pin regress
  greps. Deferred to a 7c-if-wanted: `command <name>:` arity/flag shapes and the
  `usage:` fallback (need per-command arity metadata), the ~24 remaining
  `needs a value` sites, key-string strictness.
- **SHIPPED 2026-08-18 (wave 7d):** the alias smoke suite runs real plugin configs through
  PATH-carried `tmux` exec shims against zz and the pin. The harness stages a scratch HOME,
  sources each config through control mode, compares stdout and stderr independently, checks
  per-key `list-keys -F` facts, and requires both warning signals: the `%config-error` line set
  and the source-file block's `%end`/`%error` terminator. "Zero warnings" therefore means no
  invalid config causes and no skipped-command summary; skip-only summaries became visible in
  control mode in this wave. A missing plugin cache is a visible SKIP, never a pass.

  | Scenario | Scope |
  | --- | --- |
  | `tpm-init` | TPM bootstrap, plugin environment, and install/update bindings |
  | `sensible` | Supported option application plus the two pinned unsupported-option skips |
  | `vim-tmux-navigator` | Root navigation bindings and a non-vim focus move |
  | `yank` | Copy-mode-vi yank bindings |
  | `resurrect-init` | Save/restore bindings; the restore flow remains out of scope |
  | `continuum-init` | Bootstrap through `display-message -p -F` |
  | `fpp-init` | Binding and note registration; pane runtime remains out of scope |
  | `own-conf` | Frozen first-party `~/.tmux.conf` snapshot and exact skip summary |
  | `fixture-conf` | The in-tree parser fixture promoted to an end-to-end smoke |
  | `oh-my-tmux` | Oh My Tmux's full boot including its shell half (added 2026-08-20), seven stock bindings, six option readbacks |

  The corpus pins TPM, tmux-sensible, vim-tmux-navigator, tmux-yank, tmux-resurrect,
  tmux-continuum, tmux-fpp, and — since 2026-08-20 — Oh My Tmux (`gpakosz/.tmux` at
  `58a3dcc`). Oh My Tmux's `.tmux.conf` pipes *itself* through `sh` and locates itself as
  `~/.tmux.conf`, so the harness stages the corpus file verbatim (`conf: ~/…` resolves
  against the scratch HOME) with its stock `.tmux.conf.local` beside it (`stage:`). Its zz
  warning line — zero since Wave C run 2 landed `send-prefix -2` (2026-08-21) — was the
  campaign's baseline to drive to zero. Adding it flushed out two real defects on
  first contact: shell jobs never received the `set-environment` overlay (so Oh My Tmux's
  `$TMUX_PROGRAM`-chained bootstrap silently never ran), and every stored-command renderer
  ignored the pin's `args_print` shape (flag grouping and order, canonical names,
  `args_escape` quoting) — both fixed and pin-matched the same day, the latter by giving the
  19 daemon-side catalog specs their real flag arity.

  The corpus forced three capability fixes on first contact, each hit by a real config
  (all reviewer-swept against the pin): **command prefix resolution** — the pin's
  `cmd_find` contract (exact alias wins, unique prefix over the alphabetical table
  resolves, `ambiguous command: <name>, could be: <list>` byte-exact) implemented across
  engine and daemon dispatch, because tmux-sensible and tmux-continuum call
  `tmux show-option` (a prefix, not an alias) everywhere; **the argv word grammar** —
  `cmd_parse_from_arguments`' trailing-`;` rule (word-trailing `;` splits, `\;` keeps a
  literal, empty segments drop) shared between the CLI chain and bind payloads, because
  tpm's `start-server\;` reaches argv as an attached `start-server;`; and **parse-time
  `~` expansion** for unquoted and just-inside-double-quote leading tildes, because
  stored bindings are `list-keys`-visible and the pin stores absolute paths.

## Phase 8 — the attach contract (closed 2026-08-20; empty-daemon regression repaired 2026-08-22)

The four invocations the alias lives on (rows 3-4 largely closed by 7a
2026-08-18, row 1 by the launcher wave 2026-08-19):

| Invocation | tmux | zz today |
| --- | --- | --- |
| `tmux` | new session + attach this TTY | works — the installed launcher rewrites bare argv to `new-session -A`, so an empty daemon atomically creates and attaches session zero while a live daemon attaches its current session; the GUI lives behind `zz app`. |
| `tmux new -s foo` | create **and** attach this process | CLOSED 2026-08-20 — the CLI routes attaching forms through the TUI on an Interactive connection and refuses off a TTY without creating anything; nested duplicate-name error precedence is the loud exception ledgered below |
| `tmux attach -t foo` | attach this TTY | works — full `-t`/`-d` grammar, TUI attach on a TTY, engine-identical `can't find session:` headless (7a) |
| `tmux attach` | attach, starting the server if needed | works — autostarts the daemon (CMD_STARTSERVER) and returns `no sessions`, exit 1, on an empty server, preserving `attach || new-session`; TTY check last (7a) |

The 2026-08-19 launcher wave moved bare `zz` into the TUI, per the
[TUI client](/designs/tui-client.md) design), the GUI
lives behind the exact verb `zz app` (Launch Services carries the caller's cwd via
`ZZ_APP_STARTUP_DIRECTORY`), `$TMUX` without `ZZ_SOCKET` is refused instead of dialed as a
zz endpoint (decision 4's boundary made loud), and startup config re-enters through a
private `tmux` PATH shim gated by the `ZZ_STARTUP_REENTRY` capability so `run-shell`/
`if-shell`/TPM lines work while the daemon is still sourcing its config. Linux packages
ship the CLI launcher as `cli`, `/usr/bin/zz` points at it, and the desktop entry runs
`zz app`. A later TUI target preflight exposed the distinction between bare product launch and the
tmux `attach` verb. The 2026-08-22 repair routes only empty launcher argv through existing
`new-session -A`, keeps explicit attach's `no sessions` contract, and adds deterministic first-run
race plus `attach || new-session` coverage, so rows 1 and 4 are both closed.

The 2026-08-20 client-seam wave closed row 2. `zz new -s foo` on a TTY now creates the
session and attaches the calling process; off a TTY it refuses exactly like the pin and
creates nothing.

The shape, deliberately: **no connection upgrade and no protocol bump.** `zz attach`
already bypasses the Command client (`run_command_mode` intercepts it and runs the TUI on
an Interactive connection), so `new-session` rides the same precedent — the CLI routes any
*attaching* invocation of the chain to the TUI, and the whole `\;` chain executes on that
Interactive connection. The engine gained tmux's `CLIENT_TERMINAL` as a three-state
`ClientTerminal { NoClient, Absent, Present }` on the execution context, and the client
declares its terminal through a `client-terminal-v1` token in `ClientHello.capabilities` —
a free-form `Vec<String>`, so the encoding is unchanged and `PROTOCOL_VERSION` stays 69.

On its creation path, the pin checks a `-t` target combined with a command or `-n`, validates the
window and session names, tries `-A` delegation, checks duplicate sessions, validates an unresolved
`-t` as a session-group name, then checks nesting, terminal presence, and `-x`/`-y`
(`cmd-new-session.c:97-238`). zz differs at one loud edge: its atomic daemon-side nesting guard runs
before mux execution, so a nested attaching command reports the nesting refusal before any of those
creation validations. Neither server mutates state. Three states, not two, because the pin distinguishes a NULL
client from a client without a terminal: `if (c == NULL) detached = 1` (`:164-167`) makes a
config's bare `new-session` create detached, and `if (c == NULL) return CMD_RETURN_NORMAL`
(`cmd-attach-session.c:71-72`) — placed *above* target resolution — makes a config's
`attach-session`, `new-session -A`, and even `attach-session -t bogus` silent successes.
Hooks run as NULL clients too. Four review rounds were needed to get those three states
right; the two-state version silently broke both config and hooks.

Also closed with the wave: `-P`/`-F` output (default template `#{session_name}:`), the
`width/height too small|too large|invalid` family, literal `-x -`/`-y -`, and the
`duplicate session:` string (it had carried a stray `name` word since before the campaign).

Terminal size now reaches `new-session -x -/-y -`. Since the 2026-08-25 nested-signal repair,
attaching `new-session` and `attach-session` issue the pinned refusal before any mux mutation only
when the hello carries `client-nested-v1` and its independently retained tty matches one of this
daemon's pane ttys. Unsetting `$TMUX` omits the marker, not the tty, and forces either attach path
without weakening client targeting. Local Control uses that same two-fact refusal only when stdin
is a tty; fresh creation and `-A` misses remain allowed. The creation-validation precedence
above, including the pin's `invalid session group name` check, is ledgered in the divergence matrix.
Protocol v70 closed client-exit notices and
`switch-client` retargeting on 2026-08-20.

# Acceptance

**Config/script drop-in (phases 0–7):**

- The differential harness passes a shared command-script corpus against real tmux, including
  geometry (via `-F` formats).
- A real-world `tmux.conf` corpus imports with zero skipped lines (exec lines prompt at
  import, once per config) — and zero *deferred* failures: commands inside `bind-key`
  payloads validate at import time (phase 1), not at keypress.
- TPM boots — this spans `run-shell` (5), `show-options`/`set-environment`/`start-server`
  (4), and `$TMUX`/`-V` (7); it is not a single-phase criterion.
- A resurrect-style save/restore round-trips via layout strings, **except grouped sessions**:
  resurrect's `restore.sh` recreates them with `new-session -t`, which stays a loud error
  under decision 3. The carve-out is accepted, not accidental.
- `bench/run.sh` shows no regression after phase 3.

**Full drop-in (phase 8):** the four attach-contract invocations behave on a TTY. **Met
2026-08-20** — verified live against the pin on a pty (`new -s x \; split-window -h` →
alt screen, panes `0` and `1` on both) and byte-exact headless for every error row.

# Out of scope, permanently

- Linked windows and session groups — decision 3. `new-session -t` stays a loud rejection.
  Two named consequences: resurrect's grouped-session restores error loudly (above), and
  `break-pane` on a single-pane window keeps refusing (tmux *relinks* the window into the
  destination — that is linked-window machinery).
- Speaking tmux's private client-server socket protocol — decision 4. iTerm2 does not need
  it (phase 6).
- Fleet broadcast (`--all`) — unchanged from the superset roadmap: composition over features.

# The 100% ledger — consciously parked

Everything below was seen, weighed, and deliberately not done during phase 4. This is the
checklist for a future 100%-compat assessment: each row is either an accepted divergence to
re-confirm, a deferred mechanic with an owner phase, or an open question. The operational
divergence matrix ([divergences](/tmux/divergences.md)) carries the per-command detail;
this list is the campaign-level index of it plus the items that never got a matrix row.

**Accepted divergences (documented, revisit only deliberately):**

- The CLI error prefix: CLOSED by wave 7b (2026-08-18) — both wrapper shapes
  (`zz: mux command failed: …` and `zz attach: …`) are gone; stderr is the pin's bare
  text. Deliberate residue: `unsupported command: <name>` for catalogued-but-
  unimplemented commands/options (zz-only condition, legible on CLI and CC alike), and
  `zz: ` retained for zz-only daemon errors (handshake, protocol mismatch).
- `history-limit` default stays 10000 (pin: 2000) — product choice, fenced by a drift test
  whose allowlist is exactly this one name.
- `list-commands` is the honest implemented subset, and usage strings show zz's accepted
  flags rather than the pin's verbatim strings (4e review decision: never advertise a flag
  that errors).
- Default (no `-F`) listing line formats (`list-panes`/`list-windows`/`list-sessions`)
  keep zz's own shapes; the harness and scripts compare through `-F`.
- Non-UTF-8 argv: pin VIS-octal-escapes (`a\377b`), zz replacement-chars (U+FFFD) —
  `to_string_lossy` at the CLI boundary; OsString plumbing judged not worth it.
- `update-environment` markers at session create honor the stored array (Wave C run 1) but
  source from the daemon's environment, not the attaching client's (the wire carries no
  client environ); diverges when the daemon outlives the shell that started it. Creation-time
  `new-session -e/-E` is implemented, but the same missing field leaves attach re-seeding absent,
  `attach-session -E` rejected, and `fnmatch` value patterns unexpanded.
- Two upstream layout bugs refused rather than reproduced (two-pane `main-*` preset,
  mixed-parent `-E` spread) — `known/` scenarios pin them.
- Grouped sessions / linked windows / socket interop / fleet broadcast — the permanent
  out-of-scope list above; resurrect's grouped-session restore errors loudly by design.

**Deferred mechanics (owner in parentheses):**

- Array options as a category (`terminal-features`, `terminal-overrides`, `user-keys`,
  `pane-colours`, `codepoint-widths`): all eight store with indexed semantics since the
  Lane-2 sweep (2026-08-20); three are now consumed — `status-format[]` (B1) plus
  `command-alias[]` and `update-environment[]` (Wave C run 1, 2026-08-21). The remaining
  five still drive nothing (TUI phases).
- Styles (`#[…]`, `*-style` options), which remain TUI-meaningful. The `source-file -F/-n/-t/-v`
  path and replay flags have shipped.
- `#()` job bodies: both sides strftime the whole string first (pinned by test), but the
  pin also format-expands `#{…}` *inside* the body before running it; zz hands the shell
  hook the body raw (phase 5/6 — status-seam surface).
- `#{S:}` loop ordering follows the pin's global sort criteria default (index); if zz ever
  grows choose-tree sort commands, the loop default must track the mutable criteria
  (choose-tree work).
- Positional-arity validation is unguarded and the daemon buffer family hand-rolls its
  parsing (phase-0 leftovers). zz now rejects tmux-invalid `move-pane -p` and accepts the pin's
  `move-pane -l` grammar.
- The TTY attach contract closed 2026-08-20 (phase 8); protocol v70 then closed
  `switch-client` and the client-exit notice seam. Control mode is phase 6; `tmux -V`/`$TMUX`
  shape is phase 7.
- Exec family, hooks bus, popups/menus (phase 5) — see that section's tiering.

- ~~Spawn argv semantics~~ CLOSED by wave 5a-1 (`26c86d0`): argc>=2 direct exec,
  argc==1 default-shell `-c`, both pin-verified. Residual accepted divergences:
  argv0 for argc==1 is the full shell path, not the basename (portable-pty cannot
  override argv0); argc==0 DOES get the pin's `-basename` login argv0 via
  portable-pty default-prog, EXCEPT when shell integration rewrites the builder
  (bash at the default `detect` setting — pre-existing, not a 5a regression);
  argc>=2 exec failure is detected pre-fork but surfaces as the pin's death class
  (pane_dead=1, status 1).
- Exec-family job divergences (wave 5a-2, reviewer-CONFIRMED, accepted): `-t`
  pane output goes to zz's command-output overlay, not view-mode-in-the-pane, and
  is dropped when no interactive subscriber exists; `-b` no-`-t` output routes to
  the MRU session's active pane overlay; jobs receive `$TMUX` without `$TMUX_PANE`,
  but inherit the daemon environment instead of the pin's clean global/session overlay
  and do not synthesize the TERM family; shell jobs are capped (a runaway
  backstop the pin does not have — raised from 16 in the 5b fix round; over-cap
  `-b` jobs fail with a background message like the pin's job_run failure);
  Interactive clients cannot park on blocking `wait-for` (they get the pin's
  clientless error; zz's GUI multiplexes one connection — scripts are faithful).
- Wave-5b ledger (reviewer-CONFIRMED, non-blocking): the raw-output tap now
  leads the screen transiently under flood (bounded by the 4MiB backlog,
  exactly convergent — harmless for pipe-pane, but phase-6 `%output` consumers
  that correlate output against concurrently-queried screen state will see the
  output lead where tmux keeps them in lockstep); the VT-parser-throughput gap
  this row flagged got its wave and CLOSED 2026-08-19: the 93s-vs-1s flood delta
  was the Zig `Debug` build of the vendored engine (dev builds now compile it
  `ReleaseSafe`, and a 1ms wall-time bound caps each drain turn), so the
  mid-flood capture-pane timeout class is gone — see the load-flake entry below
  and `knowledge/terminal/pty-drain.md`.
- Error-text surface: the grep-facing classes CLOSED by wave 7b (2026-08-18) —
  bare pin-exact stderr for option-value/target/unknown-command errors (twelve
  regress strings byte-verified), `already set:` respelled to the pin,
  no-tty attach = `open terminal failed: not a terminal`. Still zz-shaped by
  sequencing, not oversight: arity/flag rejections (`command <name>: too
  few/too many arguments`, `unknown flag -X`, `-X expects an argument`), the
  per-command `usage:` fallback, ~24 `needs a value` sites. Companion ledger
  rows live in the divergence matrix: the command prefix-matching capability
  gap, and `set prefix` silently accepting unresolvable C-/M- keys.
- Wave-5d-2 ledger (reviewer-CONFIRMED, non-blocking): `SocketGuard`'s drop
  unlink is unconditional — it cannot distinguish its own socket from a
  successor daemon's at the same path. The early guard drop moved the unlink
  to the correct side of a successor's bind (window is now microseconds), but
  a dev/inode ownership check captured at bind time would make it airtight.
- Build-define-derived option defaults: the pin build's Makefile overrides source
  fallbacks (`-DTMUX_MOUSE=1`, `-DTMUX_TERM=tmux-256color` — both now matched), and
  three unimplemented options carry the same hazard when they land: `editor`
  (platform `_PATH_VI`), `default-shell` (runtime-resolved to the invoking user's
  shell, NOT the compile-time default), `lock-command` (`TMUX_LOCK_CMD`). Defaults
  must be probed from the pin binary or resolved at runtime, never transcribed from
  tmux.h.
- The default-path hazard (named by the 4f-1 review): aligning a default *constant*
  is not wiring the default *path* — any option whose effect flows through an
  `Option<T>` that is `None` at default can read back correctly while behaving
  divergently. When implementing an option, test the effect AT the default, not
  only after an explicit set.

- Wire discipline (from the v59 audit): postcard structs serialize positionally, so
  struct fields must be APPENDED, never inserted — v59's `WindowSnapshot::automatic_rename`
  went in mid-struct and is safe only because every frame's envelope version-gates
  before deserialization (framing.rs). Two future changes would turn that into silent
  corruption: version negotiation accepting N−1, or any frame path skipping the
  envelope check. Keep the gate strict; append from now on.

**Open questions (investigate before declaring 100%):**

- The lazy-create two-client WIRE race: the in-process concurrent-attach test covers
  the Shared-level interleaving (which per-connection handler threads share), but a
  literal two-InteractiveClients-over-sockets race was never constructed (reviewer:
  CANNOT-VERIFY). Low risk; probe before the full-compat claim.

- The zz-client simulator hang: one 93-minute wedge in `Simulation::boot` (blocking socket
  read, daemon silent) under full-workspace load; passes solo, immediate rerun green. Four
  structural suspects cleared; the per-command trail's added lock contention is the
  surviving hypothesis. PLAUSIBLE, unreproduced.
- The VT-throughput flake root cause is fixed (2026-08-19): dev/test builds compiled the
  VT engine at Zig `Debug` (~6x slower parse), blowing the 2s command budgets under load.
  Dev builds now default to `ReleaseSafe`, a 1ms wall-time bound caps each drain turn
  regardless of parse rate, the silent control-tap arm timeout now logs and retries once,
  and the pipe-rearm timeout logs before tearing down — see
  `knowledge/terminal/pty-drain.md`. The `pipe_pane_has_no_gap…` "extra bytes" flake that
  dominated under load was hunted same-day and was a TEST bug, not a daemon bug: the
  readiness gate matched `ZZ_HANDOFF_READY` inside the echoed setup command itself, so
  under load the pipe opened before `stty raw -echo` ran and the tap faithfully captured
  echo noise (plus, separately, bash's `child setpgid … Operation not permitted`
  job-control warning). Fixed with the self-match-proof `printf 'ZZ_HANDOFF_%s\n' READY`
  idiom (the sibling burst test already used it — that's why it never flaked), `set +m`,
  and eval stderr silenced; 10/10 clean under the double-suite load that failed 7/8
  before. The daemon's tap handoff was verified byte-exact. The cli_binary control-mode
  exit-code-1 flakes (three sightings incl. two CI runs) were a real defect hunted the
  same day: nothing waited for connection writer threads before the stopping daemon's
  process exit, so the `kill-server` response and `ServerStopping` could die unflushed in
  mailboxes and the `-C` client saw a raw disconnect (exit 1). Fixed with a bounded
  post-`ServerStopping` drain (`drain_subscribers_for_shutdown`, tmux's `control_all_done`
  contract). Residual occasional load-flakes (pre-existing, pass solo):
  `copy_pipe_timeout…` (pgid-recycle EPERM), `kill_server_reaps…`, `history_request…`,
  `default_shell_rejects…`, and a `control_new_session…` startup-rename-transient
  event-ordering assert — all under double-suite load only.
- macOS-vs-glibc strftime quirks are now load-bearing (the daemon calls libc strftime —
  the workspace's only `unsafe` block): any future platform (musl, Windows) needs its own
  parity probe of unknown-`%` handling.

# The options residue — three lanes (decided 2026-08-19)

*Count correction, 2026-08-20:* the wave stamps below ("72/180", "78/180", "78 behave")
are the campaign's running tallies of options given a typed home in the honest-knobs and
status structs. A consumer trace on 2026-08-20 found that twelve of those are never read —
the true split was **66 behave / 114 store-only**; protocol v70's `detach-on-destroy`
consumer moves the current split to **67 behave / 113 store-only**, and the divergence
matrix carries both rosters. The stamps are left as written; the matrix is the number to quote.

The 2026-08-19 full inventory: 38 of the pin's 180 named options are implemented, 142 are
recognized-but-unimplemented, 7 of 8 array options have no storage, and `set-option
<hook-name>` plus indexed array spellings **silently succeed doing nothing** (the
`is_array` early return) — a silent no-op, worse than the loud skip. Every missing option
now belongs to exactly one lane:

**Status-bar family wave A SHIPPED 2026-08-20 (engine/daemon/wire; GUI titlebar = wave
B, pending):** all 17 status/window-status options store with pin-probed byte-exact
defaults (`themegreen` IS the next-3.8 spelling — the pin grew theme-palette colours),
`#{`-bearing style values defer validation like the pin (options.c:1177), `-a` joins
styles with commas, a real `parse_style`/`parse_styled_segments` replaced the
discard-everything validator (valid_style = is_some), the formats engine preserves
`#[…]` markers (inner `#{}`/`#()` expand per format.c), the renderer wraps halves in
the pin's default-stack order and left-trims BOTH halves to the length budgets (the
pin template left-trims status-right too — probed), and `WindowSnapshot.status_label`
(v69) ships per-window expanded `window-status-format` labels. Guards: 66→72-step
status-options differential scenario incl. rejections + bare-listing steps. Ledgered:
`#()` inside window-status-format renders empty (no job cache on the label path),
`#{window_flags}` lacks `# ~ M` (pre-existing), `#[ignore]` later-marker parsing
unused until wave B, unknown `range=` carried as `Other`, status-justify/-position/
status 2..5 stored-not-honored (titlebar is top, single-line, tabs own the centre).
own-conf's skip warning dropped 9 → 8 (`status-position` now stores).

**Wave B SHIPPED 2026-08-20 (GUI titlebar consumption; visual smoke PENDING —
maintainer):** the daemon pre-styles each `status_label` with the pin's overlay chain
(current-unless-exactly-`default` or base, then last-style, then bell-style —
additive `style_parse` tokens in one marker, probe-matched), the styled-run builder
maps `parse_styled_segments` onto gpui highlight runs (byte-range math UTF-8-safe,
reverse swaps resolved colors, dim fades, theme slots via the `tmux_style_colour`
conventions), both status surfaces (titlebar strip + expanded-sidebar footer) render
literal styles with NO trimming, and tab pills consume labels while keeping widget
chrome — an EXPLICIT `bg=` lifts into a subtle pill tint that inherits hover;
reverse-synthesized backgrounds stay on the glyph runs (the default bell `reverse`
renders as an inverted readable label, reviewer-caught blocker). Ledgered: empty-TEXT
labels fall back to `index:name` (pin renders empty — deliberate), unclosed `#[` in
a window name drops the tail (same class as the pin's draw stop), curly underscore
renders non-wavy, STRIP pill constants duplicated from zz-ui (fork discipline).
Hardware smoke pending (maintainer): bell-tab invert readable + hover, current-tab
underscore, last/current style washes, styled halves in BOTH chrome modes with
padding preserved, default `[#S]` beside the badge (accepted duplication — UX call
open), `#[reverse]` on status halves, empty-vs-blank status halves.

**GUI status replacement SHIPPED 2026-08-23:** the Wave B tab pills, titlebar halves,
and sidebar footer were deleted. One native GPUI status surface now consumes every
semantic status region: `status-left`, the attached session's snapshot windows, and
`status-right`. The GUI no longer paints `status-format[]` terminal cells or any tmux
style. Powerline glyphs divide left/right content into borderless theme-owned chunks,
recognized glyphs become zz-ui icons, arbitrary text and `#()` output survive, and
snapshot index/name/focus/zoom/bell state drives the window controls. Those controls
retain stable-id selection, rename/close menus, focused overflow, and one filled active
surface; window-format strings and the full style family remain TUI-only visually. The surface is full-width bottom in sidebar mode
and full-width top in titlebar mode, regardless of `status-position`. Settings and
layout remain one top-chrome cluster in both modes and take width from the titlebar
rail; the bottom status bar is tmux-only, has no agent rollup, and disappears when
`status off` leaves it empty.

**Honest-knobs wave C1 SHIPPED 2026-08-20 (17 more options; 72/180):** `focus-events`
(delivery gated at the daemon funnel — pin default OFF is a user-visible change: apps
stop receiving focus events until `set -g focus-events on`), `bell-action` +
`visual-bell` on the pin's alerts.c model (flag-vs-hook-vs-ring-vs-message gating,
per-client current-window evaluation at fan-out, control clients skipped, byte-exact
message text), `key-table`/`prefix-timeout` in the shared key engine (custom tables
dispatch; timeout disarms lazily on the next key — documented edge),
`prompt-history-limit` + `history-file` (saves on submission vs the pin's
at-shutdown — durability divergence), `display-panes-time` (closes the display-time
reuse divergence), the five layout knobs as percentage-aware strings resolved at
apply time (pin else-chains ported; 48 golden fixtures untouched), `default-size`
(detached new-session, 1..10000 clamps), `window-size` latest|largest|smallest with
`manual` stored-as-latest (no `resize-window` yet) — and the aggressive-resize
COMPOSITION now matches resize.c: ON is a candidate filter, window-size aggregates;
ON no longer forces smallest — and `allow-set-title` gating OSC 0/2 adoption
(`allow-rename` storage-only, no ESC-k scanner). `list-keys -T <nonexistent>` now
errors byte-exactly like the pin (a zz-invented exemption for key-table-named tables
was removed). Bare `list-keys` flags-column padding subsequently closed on 2026-08-22;
sensible/own-conf skip counts dropped to 1 and 6. Guards: four C1 differential
scenarios (defaults/errors/layout/readback, all live-probed).

**Honest-knobs wave C2 SHIPPED 2026-08-20 (terminal-worker knobs; 78/180) — closes
the passthrough hazard:** a shared chunk-boundary-safe DCS filter ahead of all three
PTY→VT feed sites unwraps `\ePtmux;…\e\\` (ESC un-doubling per input.c) when
`allow-passthrough` allows — `off` consumes like stock tmux, `on` behaves as `all`
(worker lacks visibility state; ledgered), payloads cap at 1 MiB (input-buffer-size
default) then discard-until-ST, non-tmux DCS (sixel, zz's own `\eP1000p` control
framing) passes untouched (split-at-every-byte regression tests), and the no-escape
fast path stays one-scan zero-copy (live bench A/B pending — needs real windows).
`wrap-search off` clamps at the ends like the pin; window moves now push worker
knobs intra-session too (`TerminalKnobsChanged` on join/break/swap — PaneRelocated
only ever fired cross-session, reviewer-caught). `cursor-style`/`cursor-colour`
bridge to per-pane appearance clones (DECSCUSR still outranks until reset; blink
half yields to an explicit zz `cursor-blink` config — ledgered).
`alternate-screen`/`scroll-on-clear` store-only. Hardware-pending: bench A/B for
the filter; a live image.nvim/kitty-icat smoke with `allow-passthrough on`.

**Lane-2 sweep SHIPPED 2026-08-20 — every option in the table now has a home (180/180
store, 78 behave):** the remaining ~90 names gained pin-exact defaults, validation, scope,
inheritance, and listing shapes; the eight array options gained real indexed storage with
the pin's separators, hole-reuse, and `name[N]`/`-u name[N]` semantics; and `set-option
<hook-name>` writes the hook table instead of silently succeeding. Both silent-success
paths are dead. Bare `show-options -s`/`-g`/`-gw` byte-match the pin, and both smoke
configs — including a real user `tmux.conf` — import with **zero skipped lines**, which is
the phase-8 acceptance criterion for configs. The wave also flushed out two latent bugs
from earlier waves (`popup-style`/`popup-border-style` were session-scoped where the pin
makes them window-scoped, resolved from the target session's current window) and one in
the harness itself (`diff-scenario.sh` leaked the developer's `EDITOR`/`VISUAL` into the
pin server on non-smoke runs, which had been read as a zz divergence).

**Lane 1 — GUI-effect (~48; implement, wired to real behavior).** The status-bar family
rendered in the collapsed-sidebar titlebar strip, which is already a proto-status-bar
(session badge = `status-left`'s `[#S]`, tab row = window list, right corner = the default
left+right concatenated): `status-style/-bg/-fg/-justify`, `status-left/right-style/-length`,
the eight `window-status-*` formats/styles and `-separator` — content renders literal
`#[…]` styles (user content, like terminal cells; the strip's frame stays theme chrome),
tabs stay interactive widgets fed format-expanded labels. All nine alert options
(`monitor-*`, `*-action`, `visual-*`) on the shipped bell plumbing. Titles (`set-titles`,
`set-titles-string`, `allow-rename`, `allow-set-title`). Terminal engine: `focus-events`
(behavior already unconditionally on — store the flag and gate), `alternate-screen`,
`scroll-on-clear`, `cursor-style/-colour` (bridge to the existing appearance config), and
`allow-passthrough` (see the hazard below). Keys and prompt: `prefix2`, `key-table`,
`prefix-timeout`, `status-keys`, `wrap-search`, `prompt-history-limit`, `history-file`.
Layout: `main-pane-width/height`, `other-pane-width/height`, `tiled-layout-max-columns`
(today five hardcoded constants in `layout.rs`), `window-size`, `default-size`. Overlays:
`display-panes-time/-format`, `remain-on-exit-format`. Shared: `command-alias[]` (shipped
2026-08-21). Maintainer correction 2026-08-24: the raw TUI honors explicit
`pane-border-style`/`pane-active-border-style` colors, while the GPUI client keeps its
pane borders under the zz chrome theme (attributes beyond color remain ignored, one
divergence row); `window-style`/`window-active-style` (inactive-pane dimming) and the
`mode-style`/`copy-mode-*-style` selection/match/mark styles ALL render in the GUI
terminal renderer — content styling, no chrome-doctrine conflict. The remaining lifecycle
flags (`exit-empty`, `exit-unattached`, `destroy-unattached`) are honored when a config
EXPLICITLY sets them, defaults keeping zz's persistent-daemon behavior — shipped
2026-08-21. `should_shutdown_if_empty` still reads armed ∧ zero sessions ∧ zero
subscribers with nothing set; explicit writes swap in the pin's `server_loop` rule, and the
"attached client keeps the daemon alive" guard stays as a documented divergence that no
policy can override. The
Settings → Advanced `QuitDaemonOnExit` key is a DIFFERENT trigger (app quit, not
sessions-drained); both axes coexist, the settings description should cross-reference.
`detach-on-destroy` shipped separately in protocol v70 and follows its pinned default and
all four explicit survivor policies.

**Lane 2 — store-only (~40; accept + store silently, divergence row each, the TUI
consumes later).** `terminal-features[]`, `terminal-overrides[]`, `extended-keys(-format)`,
`backspace`, `user-keys[]`, `xterm-keys`, `input-buffer-size`, `get-clipboard`,
`codepoint-widths[]`, `variation-selector-always-wide`, `status-format[]`,
`status-position` (the titlebar bar is top-only), `message-line`, `message-style`,
`message-command-style`, `message-format` (toasts and the command prompt keep zz's design
language — the popup/menu precedent), `fill-character`, `pane-border-lines/-indicators`,
`pane-scrollbars*` (4), `pane-colours[]`, `clock-mode-*`, `prompt-cursor-*` (4),
`assume-paste-time`, `editor`, `default-client-command`, `session-status-*`,
`pane-status-*`, `window-pane-*-status-format`, the `copy-mode-line-number*` family.
Landing this lane also REPLACES the silent no-ops with real storage — arrays store and
read back, `set-option <hook-name>` routes to the hook table.

**Lane 3 — N/A-native, documented.** `tree-mode-*` (native choosers, rationale exists),
and the 21 pin-3.6 theme-palette options (`theme`, `dark-theme-*`, `light-theme-*`) —
zero corpus/real-world hits; parked entirely until demand exists.

**Hazard (correctness, not compat — probed 2026-08-19):** since 7a put the exact
`$TMUX` shape and `TERM=tmux-256color` in panes, tmux-aware programs (image.nvim,
kitty icat and friends) WRAP passthrough sequences in `\ePtmux;…\e\\`. zz-terminal has
zero unwrap code, so the VT consumes the DCS silently. That MATCHES stock tmux's
`allow-passthrough off` default — but it is a regression against pre-7a zz, where the
same programs saw no `$TMUX`, emitted raw kitty sequences, and images worked; and the
opt-in is impossible because `allow-passthrough` is unimplemented and refuses to set.
Fix = Lane 1 `allow-passthrough` (on|off|all per the pin): store the option, and on a
`tmux;`-prefixed DCS with the option enabled, un-double the escaped ESCs and refeed the
payload through the VT parser.

**Grammar wave — SHIPPED 2026-08-19 (codex implement → adversarial review → two fix
rounds; reviewer seat moved to grok-4.6 xhigh mid-wave, verdict MERGE-READY):** the
main config grammar surface landed — `$VAR`/`${VAR}` expansion (charset, `${9}` vs `$9`,
`\$`, undefined→empty), the full escape set (`\NNN`/`\u`/`\U`/singles, invalid forms
error byte-exact), `NAME=value` + `%hidden` assignments (applied at parse time BEFORE
the file's commands, visible same-line, with assignment side effects surviving parser
diagnostics — all pin-probed), and
`%if`/`%elif`/`%else`/`%endif` EVALUATION (engine format expansion at server/global
scope, `FORMAT_NOJOBS` — `#()` renders empty and never spawns, pin `format_true`,
same-line + nested forms, balanced-through-whitespace `#{…}` conditions). The parser emits
the pin's `syntax error` strings for individual diagnostics, but end-to-end `source-file`
abort behavior is not compatible: the pin discards the file's command list at the first cause,
while zz can report multiple diagnostics and still apply later valid commands. The five re-parse
sites (bind `{}` bodies, set-hook, if-shell, confirm-before, command-prompt) expand
against the LIVE engine global environment (hidden included). Guards: 25 parser unit
tests, daemon readback regressions, and the 15-step `smoke/config-grammar` harness
scenario byte-diffed against the pin. Ledgered divergences: control-mode stdin keeps
`$VAR` LITERAL (pin expands server-side; non-expanding entry point, test-pinned);
re-parse sites expand at bind/execution time vs the pin's config-tokenization time
(comment at `bound_commands`); `\377`→U+00FF and `\000`-retained vs the pin's raw
byte/NUL-truncation (String storage, test-pinned); the pin's `%elif`/`%else`
assignment-leak quirk is NOT reproduced (zz keeps single-branch assignment scope);
`%if c ; cmd ; %endif` is zz-lax (pin rejects the `;`). Oh My Tmux joined the smoke
corpus on 2026-08-20. The later source-file diagnostics wave fixed command-stream delivery and
exit status for the file's own parser diagnostics. Replayed target and set-option runtime failures
now use the invoking error channel and nonzero status as well. First-diagnostic whole-file abort
remains live in `config.parser-edge-cases`; successful replay output remains under
`config.replayed-command-output`.

# Risks

- `tmux -V` gating: any answer is a small lie; plugins may exercise version-specific paths.
- Cell-stepped dividers may feel worse than today's smooth drag — mitigated by the
  commit-on-release option.
- Formats/styles are tmux's largest maintenance surface; the pinned-commit discipline
  (verify against `d77c9dc6`, never guess) is what keeps the grind honest.
- The Interactive/Command client split is load-bearing (attach effects apply only to
  Interactive clients). Phase 8 cut first (2026-08-20), then protocol v70 extended the
  same attachment path for `switch-client` and destroy-policy survivor switches; there is
  still no connection upgrade or parallel focus mechanism.
- Consent scope: trusting the user's own config matches tmux and current zz behavior, but it
  means an imported-then-edited config never re-prompts; acceptable, worth stating.

# Related

- [tmux superset roadmap](/designs/tmux-superset-roadmap.md) — tiers 1–3 (landed) and the
  doctrine this plan amends.
- [tmux divergence matrix](/tmux/divergences.md) — the current deltas, row by row.
- [tmux compatibility philosophy](/tmux/tmux-compat.md) — the subset contract that holds
  until each phase lands.
- [TUI client](/designs/tui-client.md) — the attach surface phase 8 rides on.
