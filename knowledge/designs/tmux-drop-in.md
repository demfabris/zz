---
type: Design Plan
title: tmux drop-in plan
description: "The alias-tmux=zz plan: 100% of tmux's command grammar, options, formats, and geometry on tmux names, zz power moved to superset verbs, exec commands behind an import-time consent gate — nine phases ending at the TTY attach contract, one permanent skip (linked windows), one explicit non-goal (real-tmux socket interop)."
status: Original nine phases shipped 2026-08-20; campaign planned against 57ae502, safe prelude shipped, core campaign approved 2026-08-21 and in execution
tags:
- tmux
- compatibility
- drop-in
- layout
- control-mode
- roadmap
timestamp: 2026-08-20T00:00:00Z
---

# Overview

Goal: `alias tmux=zz` works — a tmux user's binary invocations, config, scripts, and muscle
memory all behave identically, while zz-only power lives in superset verbs that never collide
with tmux names. This supersedes the never-list of the
[tmux superset roadmap](/designs/tmux-superset-roadmap.md); the current deltas being closed are
enumerated in the [divergence matrix](/tmux/divergences.md).

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

# Where this stands (2026-08-20)

**All nine original phases are shipped, and the human attach half of the target is met:** a human
typing `tmux new -s foo` lands inside the session. Strict config/script indistinguishability remains
the end-state rather than a current claim: the live ledger below still has unsupported flags and
documented semantic divergences.

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
| 8 — the attach contract (all four rows) | **closed 2026-08-20** |

Acceptance, measured rather than asserted: the differential corpus runs byte-clean against
pinned tmux `d77c9dc6` across ~40 scenarios; a real user `tmux.conf` and tmux-sensible
import with **zero skipped lines**; TPM installs end-to-end under a PATH shim; twelve
strings from the pin's own regress suite match byte-exact; and the attach contract was
verified live on a pty (`new -s x \; split-window -h` → alt screen, panes `0` and `1` on
both binaries).

**Options: all 180 of the pin's named options store; 67 behave.** The remaining 113 are
honest storage — they accept, validate, scope, inherit, and read back exactly like the pin,
and the [divergence matrix](/tmux/divergences.md) carries both rosters. (The running
"78/180" tallies in the residue section below counted options given a typed home in the
honest-knobs structs; a 2026-08-20 consumer trace found twelve of those unread. No table
marker or drift test separates the two yet.) Nothing silently succeeds doing nothing any
more.

**Commands: 80 of the pin's 92 verbs run; 12 are absent** (3 buildable, 4 native-chrome
superseded, 4 parked by decision or by the missing floating-pane model, `server-access`).
After Waves 2a and 2b, **30 of the 80 still reject tmux flags** — exactly 129
catalog-declared `unsupported` pairs, inventoried in the matrix — so "100% of the command
grammar" holds at the verb level, not yet at the flag level.

**What is genuinely left**, in the order real configs hit it (re-ranked 2026-08-20 on
corpus data: Oh My Tmux, fzf-tmux, tmux-sessionizer, and the seven pinned plugins):

0. ~~**Oh My Tmux into the smoke corpus**~~ — SHIPPED 2026-08-20 (scenario green: 15 steps,
   zero divergences; after Wave 2a, zz's baseline is one skip: `send-prefix -2`).
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
4. **The C3 knob batch** (v71) — `monitor-activity`/`monitor-silence` with the `#`/`~`
   window flags and `activity-action`/`visual-activity`, the `set-titles` pair, `prefix2`
   (Oh My Tmux's `send-prefix -2`), `display-panes-format`, `remain-on-exit-format`,
   parse-time `command-alias`, honoring a user's `update-environment` list at seeding
   (today hardcoded), the remaining lifecycle trio when explicitly set, and the renderer styles
   fabrico already decided (`pane-*border-style` colors, `window-style`/
   `window-active-style` dimming, `mode-style`). Add the `BEHAVES` list + drift test.
5. **`source-file` diagnostics on the CLI** — exit 1 with the pin's `path:line: message`
   where today the plain CLI exits 0 silent. Revalidation found that parse and glob failures
   fit the current response types, but the zz-only unsupported summary cannot reach stderr
   with exit 0 because Command responses have no stderr channel. Keep the whole item parked
   until that response contract is approved rather than splitting one command's diagnostics.
6. **Optional waves** — the 7c error-wording appendix (25 `needs a value` sites, `unknown
   flag -X`, the `usage:` fallback), key-string parity (`list-keys` padding, `C-zz` prefix
   strictness), `resize-window` (also unparks `window-size manual`), the prompt-history
   pair, lock-program spawning on the TUI.
7. **Parked by decision** — `status-keys` vi prompt (half-vi is worse than none), linked
   windows and session groups (decision 3), the floating-pane family (`new-pane`,
   `switch-mode`, `move-pane` placement flags), real-tmux socket interop (decision 4), the
   21 theme-palette options and `tree-mode-*` (no demand).

Hardware-pending items that need fabrico rather than code: the wave-B status-bar visual
smoke, a live `allow-passthrough` image smoke, the DCS-filter bench A/B, and an iTerm2
`-CC` run.

# Final compatibility campaign (planned at `57ae502`)

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

The starting ledger has 129 unsupported command-and-flag pairs. Waves B through D remove
15, leaving 114. The revised G plan assigns 85 implementations and names 29 parked pairs,
including the four client-environment flags and `split-window -I` omitted from the first
count.

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
The three new mux keys are publication-only — `from_config_key` deliberately omits
`mouse`/`escape-time`/`prefix2` until their consuming waves open the config surface.

The approval audit completed read-only on 2026-08-21. Append the following fields and
variants as one bump. Postcard structs and enums stay append-only; the manually encoded
terminal lane retains its explicit byte layout.

| Contract | Exact shape | Consumer and ownership |
| --- | --- | --- |
| Mux options | Append `MuxOptionKey::{Mouse, EscapeTime, Prefix2}` with tags 14 through 16. Keep `Prefix` and `Prefix2` global in v71 because the shared `KeyTables` own one prefix pair; tmux-style session-scoped prefixes require a separate core refactor. | B2, B3, C2. `Mouse` publishes the attached session's effective value; `EscapeTime` and `Prefix2` publish the global values. |
| Status | Append `StatusLine.{title: String, base_style: String, rows: Vec<String>, position: StatusPosition, message_line: u8, customized: bool}`. Keep the existing `left` and `right` first for v70 layout. Move `StatusPosition` into `zz-protocol` and re-export it from `zz-mux`; keep justify inside the expanded row formats. Cap rows at five and every string at 4 KiB. Reject a sixth row before allocation, require `base_style` to parse as a style, and require `message_line == 0` for no rows or less than `rows.len()` otherwise. | B1 and B4, C3. `rows` is the authoritative personalized status block and drives `is_empty`: zero rows means off, blank rows still consume geometry, and sparse `status-format` indices do not compact. `base_style` paints blank rows. `customized` controls zz-native hints even when an explicit value equals the default. Title still publishes while status is off. |
| Display panes | Append `PaneIndicator.label: String`, bounded to 1 KiB; remove its current `Copy` shape and borrow it in helpers. | C4. The daemon expands `display-panes-format` separately in each pane context and both clients paint the result. |
| Pane borders | Append `PaneSnapshot.{border_colour, active_border_colour}: Option<TmuxColour>` and give `TmuxColour` validated wire serialization. `None` means theme fallback. | C9. Pane-scoped fields preserve pane to window to global inheritance; one pair on `WindowSnapshot` cannot represent distinct pane overrides. Keep non-color attributes ledgered. |
| Prompt | Keep `CommandPromptKind::{Command, Value}`. Append `prompt_type: CommandPromptType`, `mode: CommandPromptMode`, and `no_freeze: bool`; types are `Command|Search` and `Text|Single|Numeric|Incremental|Key|BackspaceExit`. Do not add a prompt-key action. | D1. `kind` remains presentation state, while `-T` is independent. Resolve mode flags with pinned priority `-1`, `-N`, `-i`, `-k`, `-e`; preserve `-C` independently. Route raw special-mode keys through the existing pane-targeted `InputMessage::Key`. The daemon prompt state machine returns handled or pass so `-N` can submit digits and then feed the first non-digit into normal key processing. |
| Copy mode | Append `hide_position: bool` after `TerminalMode::Copy.total`; encode Copy as tag plus two `u64`s plus one canonical bool byte. Append `TerminalViewAction::EnterCopyModeWith { scroll_exit, hide_position }` at tag 27 while preserving both old variants. | D2. The action shape lets `-e` and `-H` compose; both clients suppress only the position text. |
| Choosers | Append bounded `key: String` to `ChooseTreeItem` and `ChooseBufferItem`; cap it at 64 bytes and use empty for no shortcut. | D4. Existing actions already carry `KeyInput`. Default rows use `0..9`, then `M-a..M-z`; invalid keys become empty and duplicate keys select the first row before navigation. `-N` preview state remains internal. |
| Command stderr | Append `stderr: String` after `CommandResponse::Success.exit_code`, using the existing frame bound. Preserve the current output-only client API and add a stream-aware result for the CLI. | Wave E. v71 initializes it empty; Wave E later populates exact stdout and stderr independently, including stderr with exit 0. |
| Timed-message lifecycle | Append `message_id: u64` to `EventPayload::TimedClientMessage` and append `EventPayload::TimedClientMessageCleared { message_id }` at tag 46. The daemon assigns identities and owns timers. | D3 and D1. The daemon freezes terminal publication per client for an ordinary message or prompt, keeps PTY parsing live, then publishes one full latest viewport before patches resume. `display-message -C` and incremental or `-C` prompts skip the freeze. Identity prevents an old timer from clearing a replacement; an explicit clear makes duration-zero and TUI behavior converge. |
| Client terminal facts | Introduce `client-tty-v1:` and `client-size-v1:` as new value tokens in `ClientHello.capabilities` (none exist today; `zz-client` currently sends empty capabilities, and the existing value-token precedent is `zz-startup-reentry=`); append `InputMessage::ClientTerminalSize { columns, rows }` at tag 17 for later `SIGWINCH` updates. | B5. The daemon uses current per-client facts for nested-session checks, dash dimensions, and client width/height formats. Clients collect the initial values before connect, then publish size changes. |

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
off, pin-exact. Ledgered bounds: status-row window-option scoping (fix scheduled into
Wave C via the `Expander::lookup` loop-item seam), the status-block suppression
threshold, the empty-title expansion edge, read-only decoupled from `ignore-size`, and
`new-session` inside a pane not yet nested-refusing.

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
   - Collect tty and size before the connection. `client-tty-v1:/dev/ttysN` lets the daemon
     compare the caller tty with pane ttys and issue the pinned nested-session refusal.
   - `client-size-v1:COLSxROWS` supplies the initial `-x -` and `-y -` creation dimensions
     and `ClientFormatFacts.width` and `height`; publish later `SIGWINCH` changes through the
     v71 client-size input.
   - Define tty discovery for Command clients rather than limiting the check to TUI attach.
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

1. Alerts: generalize `raise_pane_bell` into an alert path for bell, activity, and silence.
   Track per-window activity and silence flags, reset silence deadlines on output, aggregate
   pane state, and clear flags on selection. Generalize the client-keyed display-panes
   deadline code or add a pane/window timer owner. Expose `#` and `~` through formats, apply
   `window-status-activity-style`, and fire the alert hooks. Move `monitor-activity`,
   `monitor-silence`, `monitor-bell`, `activity-action`, `silence-action`, `visual-activity`,
   `visual-silence`, and `window-status-activity-style` into `BEHAVES` and add deterministic
   differential scenarios.
2. `prefix2` and `send-prefix -2`: store a second prefix in the shared key tables, arm and
   re-arm on either prefix, publish it through `MuxOptionKey::Prefix2`, and send nothing when
   the second prefix is unset. Remove Oh My Tmux's last expected warning.
3. `set-titles` and `set-titles-string`: execute this source half before B4's client sinks.
   Expand the title per client and publish it independently of status visibility.
4. `display-panes-format`: expand the format per pane into `PaneIndicator.label`. Both
   clients parse styled segments before painting, including the default `#[align=right]`;
   honor overlay alignment and clip the resulting row to the pane indicator width.
5. `remain-on-exit-format`: park until the terminal actor has an approved post-worker VT
   injection or frozen-view reconstruction seam. The current retained-pane path marks the
   pane dead after the live PTY/VT actor exits, so the proposed feed path does not exist.
6. `command-alias`: **shipped 2026-08-21.** `MuxEngine::expand_command_alias` expands one
   layer through the config tokenizer, appends caller arguments, and never recurses (the
   pin's `CMD_PARSE_NOALIAS`). It runs at both dispatch chokepoints — the daemon's
   preemption fork before `DAEMON_COMMAND_NAMES`, so aliases reach daemon-owned commands,
   and `execute_with_shell_validator` before canonical lookup — plus bind-key, set-hook,
   and option-command validation. The six stored defaults match the pin; no catalog alias
   deletion was needed. Ledgered: control mode's client-side parse pre-check rejects
   user alias names before dispatch (no wire carries server options to that client).
7. `update-environment`: **shipped 2026-08-21.** `seed_session_environment` and
   `global_tmux_option_value` both read the stored array; the frozen constant is gone from
   `command.rs`. The client-environment flags (`-E`, attach re-seeding, `fnmatch` value
   patterns) stay ledgered because the wire carries no client environment.
8. Lifecycle trio: **shipped 2026-08-21.** A dedicated `scalar_option_explicit` accessor
   over the stored-scalar maps gives presence-means-set semantics at each option's pin
   scope; `should_shutdown_if_empty` consults `exit_empty_explicit` /
   `exit_unattached_explicit` and otherwise keeps the latch rule byte-identical, and
   `enforce_destroy_unattached` reproduces `server_check_unattached` after attach, detach,
   switch, and unregister. The `subscribers.is_empty()` conjunct survives every policy, and
   policies are dormant inside the startup bracket. `keep-last`/`keep-group` follow their
   ungrouped-session reading while linked session groups remain the permanent skip. All of
   it is covered by in-process daemon tests, never compat scenarios.
9. Renderer styles:
   - Feed `window-style`, `window-active-style`, `mode-style`, and the three copy-mode styles
     through the existing per-pane appearance bridge.
   - Carry typed `pane-border-style` and `pane-active-border-style` colors on each v71
     `PaneSnapshot`. Clients fall back to theme colors when the fields are `None`. Keep
     non-color attributes as a documented divergence.
   - Resolve border colors per pane during personalized daemon snapshot stamping. Refresh
     active and inactive appearance after pane/window selection and relocation as well as
     option writes.

The full B and C target moves `BEHAVES` from 67 to 105: 12 Wave B consumers and 26 Wave C
consumers. The C tranche, if approved without `remain-on-exit-format`, stops at 104. Wave C
run 1 (items 6, 7, 8) took it 81 to 86.
Regenerate the option rosters in `knowledge/tmux/divergences.md` and enforce both expected
deltas in tests.

## Wave D - daemon-owned interactive state

1. Implement `command-prompt -1 -k -N -T -i -C -e` in the daemon-owned prompt state
   machine. Clients render state and forward raw keys. Keep Command and Search histories
   separate; run special modes through the pane-targeted key path; let numeric mode submit
   and pass its first non-digit into normal input. Plain prompts use the shared per-client
   freeze lifecycle, while incremental and `-C` prompts keep publishing terminal frames.
2. Implement `copy-mode -H` through a `hide_position` field that suppresses the copy
   position in both clients.
3. Implement `display-message -C` as tmux's `no_freeze` behavior: keep terminal updates
   flowing while the status message is displayed. Plain `display-message` freezes
   presentation for that client, and clear resumes it with a full latest viewport. `-C`
   does not clear the message. Cover positive timers, duration zero and key clear,
   replacement-message timer safety, and TUI clearing. Audit facts: today the client owns
   the timer and the TUI drops `duration_ms` entirely (messages pin forever), so D3
   includes TUI timer/clear consumption; the daemon-owned `DisplayPanesDeadline`
   dispatcher is the working precedent for daemon-side deadlines, but it stores one
   deadline per client, so per-message identity still needs its own state.
4. Implement `choose-tree` and `choose-buffer -K -N`: the daemon expands a key for each row,
   the key selects the row, and both clients show it. One `-N` disables preview; repeated
   `-NN` selects tmux's large-preview mode. zz can accept the no-preview case and must ledger
   large preview until it has a matching presentation.

Implement and review D2 and D4 before the shared freeze work. Implement D3 next, then reuse
its lifecycle in D1. Use control-mode and PTY tests for interactive prompt behavior, then
delete the matching refusal assertions.

## Wave E - `source-file` CLI diagnostics

**Parked after source and live-behavior revalidation 2026-08-21.** A3 adds the response
stderr channel needed for mixed output and for the zz-only unsupported summary. CLI text
classification remains rejected because it would leak daemon semantics into presentation
code.

Two audit facts shape this wave: Command clients are never subscribed to the event stream
at all (`daemon.rs` subscribes Interactive and Control only), so the CLI cannot receive
warning events even in principle — the stderr field is required, not convenient. And
control-mode `%config-error` is reconstructed by a prose sniffer (`is_config_message` in
`control_mode.rs` pattern-matches warning text), so any rewording of config diagnostics
must either move to a typed marker or pin the wording with a test.

For an explicit `source-file` issued by a Command client, append invalid-line diagnostics to
stdout and return exit 1. Interactive and Control clients keep warning events and
`%config-error`. A glob miss without `-q` uses `No such file or directory: path` on stderr
and exits 1; a quiet miss produces no output and exits 0. Mixed invalid input and a glob miss
populate both streams and exit 1. Preserve diagnostic order and duplicates. The zz-only
`skipped N unsupported tmux command(s): name, ...` summary stays on stderr and exits 0 so a
config with supported tmux behavior continues to load.

Add a stream-aware command result while preserving the current output-only client API.
Treat completed commands with exit 1 as successful protocol responses; reserve response
errors for dispatch, transport, and server failures. The CLI prints both streams and stops
a command chain on nonzero. Add CLI coverage for the glob-resolved matched path plus
`:1: unknown command: wibble` on stdout with exit 1, the mixed-stream matrix, nested and
multiple inputs, the default-config path, Control `%config-error`, and a smoke scenario that
stages its fixture. `source-file -` remains in the separate G6 streaming contract.

## Wave F - shared contracts and optional semantics

0. Shared parser and count foundation: give daemon and mux handlers one catalog-driven
   option and positional parser. Put reusable parsing beside `CommandSpec` in `zz-protocol`
   or export one deliberate mux API. Add positional minima and maxima, then route the four
   daemon parser exceptions through it. Add a machine-enforced unsupported-pair roster and
   exact per-wave deltas; `BEHAVES` cannot measure flag work.
1. Error contract, after F0: centralize pinned unknown-flag, missing-value, too-few and
   too-many argument, alias, and usage fallback shapes. Treat this as a command-semantics
   tranche rather than a text-only cleanup.
2. Key grammar and tables: add a fallible canonical key parser, reject malformed modifier
   tails, align bare `list-keys` output, and pin the supported stock copy-table metadata,
   repeat flags, and actions. Preserve zz-native product bindings. Review missing actions
   before importing more pinned rows. Complete this tranche before G4.
3. `resize-window` and `window-size manual`: approve geometry policy on its own. Specify
   absolute and relative precedence, ranges, persistent manual extent across viewer changes,
   layout effects, PTY resize behavior, and differential fixtures. Stop resolving Manual as
   Latest only after those invariants have tests.
4. Prompt history, after A3 and D1: add typed Command and Search storage plus
   `show-prompt-history` and `clear-prompt-history [-T type]`. Pin ordering, output, errors,
   and prompt-to-history routing. Clearing rewrites persisted state so old entries do not
   return after restart. Keep save-on-submit as the documented durability choice.
5. Lock and client process control: defer for a separate protocol and execution-policy
   approval together with `detach-client -E`. Settle target fanout, stale unlock rejection,
   reconnect cleanup, input gating, shell/cwd/environment, process failure, raw-mode restore,
   hooks, and `lock-after-time`. Keep the GUI behavior ledgered until it owns a lock flow.

## Wave G - remaining server and engine flags

Complete F0 before these tranches. For each pair, change the catalog's unsupported flag to
an accepted flag or value and update the usage string in the same hunk. Add handler
behavior, delete its named refusal test, add differential coverage, and update the exact
unsupported-pair roster.

Tranches:

- G1 environment: carry repeated creation-command `-e KEY=VAL` through the internal spawn
  effects for new session, window, and pane paths, with pinned validation and precedence
  against session and global environment. Park all four `-E` pairs for `attach-session`,
  `new-session`, `new-window`, and `split-window` until the client environment has an
  approved wire contract.
- G2 pane input and marking: `select-pane -d -e -m -M -g -P`, `last-pane -d -e`, the marked
  pane target and format facts, and per-pane style storage. Gate every daemon input route,
  keep one global marked pane, and clear it on pane relocation or death. Implement after C9.
- G3a placement: `break-pane -a -b`, `new-window -b`, `join-pane -l`, and
  `split-window -Z`.
- G3b spawn metadata and styles: `split-window -k -m -R -s -S -T`. Keep the C5-dependent
  subset parked until its terminal seam has approval; implement the independent subset only
  after C9 defines per-pane metadata.
- G3c wait lifecycle: implement `split-window -W` with command-queue ownership,
  cancellation, client disconnect, and daemon lifecycle tests.
- G4 keys: `list-keys -1 -a -N -O -P -r` and `unbind-key -a -q`. `Binding` already carries
  `note: Option<String>` and its wire snapshot preserves it, so `list-keys -N` has backing
  state. Implement after F2 and pin filtering, sort order, prefix formatting, widths,
  missing targets, notes, and repeat metadata.
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
- G5f mouse-driven behavior: `resize-pane -M` consumes an originating drag event and keeps
  its own input and geometry tranche.
- G6 binary stdin and stdout streaming: `split-window -I`, `load-buffer -`, `save-buffer -`,
  and `source-file -`. Only `split-window -I` is an unsupported flag pair; the other three
  are accepted positional grammar with missing behavior. Park the tranche until a dedicated
  protocol defines bounded binary chunks, EOF, backpressure, cancellation, disconnect,
  size limits, and binary stdout.
- Keep 24 pairs parked: `move-pane` x10, `break-pane -W -x -y -X -Y`, `new-session -t`,
  `kill-session -g`, `choose-tree -G`, `command-prompt -P`, `copy-mode -S`,
  `send-keys -M`, `display-message -I`, and `show-messages -T -t`.

The current ledger has 128 unsupported flag pairs across 30 commands (`attach-session -r`
left it with Wave B's read-only slice). Waves B through D
remove 15, leaving 114 for G and the parked contracts. The original G list omitted seven
chooser pairs and four parked `-E` pairs; the unsupported-pair roster replaces prose
arithmetic as the completion proof. With the seven chooser pairs assigned to G5a and the
current `-E`, streaming, and explicit parked sets unchanged, the planned result is 85
implemented and 29 parked.

## Order and wave gates

The completed safe prelude is `0 -> A1 -> A2 -> B1-existing-wire`. After core approval, run
`A3 -> B with C3 title production -> C except C5 -> E -> D2 -> D4 -> D3 -> D1`. E proves the
new response streams without interactive state. D3 establishes the freeze lifecycle before
D1 reuses it for prompts.

Continue with `F0 -> F1 -> F2 -> F3 -> F4 -> G1 -> G2 -> G3a -> independent G3b -> G3c ->
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
  current 50 scenario rows (48 before Wave C run 1 added `command-alias` and
  `update-environment`), zero SKIPs, and no divergences outside the two documented
  geometry fixtures
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
| Client environment and stdin wire | Park G1 `-E` and all G6 streaming until fabrico approves the contracts. |
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
- `source-file` globs every path (`conf.d/*.conf` works); `-` stdin is a loud refusal;
  `-F`/`-n`/`-v` are deferred to the phase-4 grind (options table row below).
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
| `source-file -F`/`-n`/`-v` | format-expanded paths, parse-only, verbose printing — deferred from phase 1 | later |

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
  warning line — now one skip, `send-prefix -2` — is the campaign's
  baseline to drive to zero. Adding it flushed out two real defects on
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

## Phase 8 — the attach contract (CLOSED 2026-08-20; all four rows)

The four invocations the alias lives on (rows 3-4 largely closed by 7a
2026-08-18, row 1 by the launcher wave 2026-08-19):

| Invocation | tmux | zz today |
| --- | --- | --- |
| `tmux` | new session + attach this TTY | CLOSED 2026-08-19 — the installed launcher rewrites bare argv to `attach`: TUI on a TTY, lazy first-session create. A re-invocation attaches to the live session where tmux would stack a new one (deliberate); the GUI moved behind the exact verb `zz app` |
| `tmux new -s foo` | create **and** attach this process | CLOSED 2026-08-20 — the CLI routes attaching forms through the TUI on an Interactive connection; the engine runs the pin's check order and refuses off a TTY without creating anything |
| `tmux attach -t foo` | attach this TTY | works — full `-t`/`-d` grammar, TUI attach on a TTY, engine-identical `can't find session:` headless (7a) |
| `tmux attach` | attach, starting the server if needed | works — autostarts the daemon (CMD_STARTSERVER), `no sessions` on an empty server, TTY check last (7a) |

The 2026-08-19 launcher wave closed row 1 and the alias boundary around it: bare `zz`
rewrites to `attach` (TUI, per the [TUI client](/designs/tui-client.md) design), the GUI
lives behind the exact verb `zz app` (Launch Services carries the caller's cwd via
`ZZ_APP_STARTUP_DIRECTORY`), `$TMUX` without `ZZ_SOCKET` is refused instead of dialed as a
zz endpoint (decision 4's boundary made loud), and startup config re-enters through a
private `tmux` PATH shim gated by the `ZZ_STARTUP_REENTRY` capability so `run-shell`/
`if-shell`/TPM lines work while the daemon is still sourcing its config. Linux packages
ship the CLI launcher as `cli`, `/usr/bin/zz` points at it, and the desktop entry runs
`zz app`.

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

The pin's check order is reproduced exactly, and **no failing check creates a session**:
`-A` delegate (which ignores `-d`) → duplicate → nested → terminal → `-x`/`-y`
(`cmd-new-session.c:122-238`). Three states, not two, because the pin distinguishes a NULL
client from a client without a terminal: `if (c == NULL) detached = 1` (`:164-167`) makes a
config's bare `new-session` create detached, and `if (c == NULL) return CMD_RETURN_NORMAL`
(`cmd-attach-session.c:71-72`) — placed *above* target resolution — makes a config's
`attach-session`, `new-session -A`, and even `attach-session -t bogus` silent successes.
Hooks run as NULL clients too. Four review rounds were needed to get those three states
right; the two-state version silently broke both config and hooks.

Also closed with the wave: `-P`/`-F` output (default template `#{session_name}:`), the
`width/height too small|too large|invalid` family, literal `-x -`/`-y -`, and the
`duplicate session:` string (it had carried a stray `name` word since before the campaign).

Still open on this surface, ledgered rather than done: `-x -`/`-y -` use 80×24 instead of
the *client's* size (the pin reads `c->tty.sx/sy`, which needs the client's terminal size
plumbed to the engine); the nested-session check (`server_client_check_nested` compares the
client's tty against this server's pane ttys — keying off `$TMUX` alone is wrong, since a
fake `$TMUX` on a non-pane pty still attaches on the pin). Protocol v70 closed the
client-exit notices and `switch-client` retargeting on 2026-08-20.

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
  client environ); diverges when the daemon outlives the shell that started it. The same
  missing field keeps `-E` rejected, attach re-seeding absent, and `fnmatch` value patterns
  unexpanded.
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
- Styles (`#[…]`, `*-style` options) and `source-file -F/-n/-v` (marked *later* in the
  phase-4 table; styles are TUI-meaningful).
- `#()` job bodies: both sides strftime the whole string first (pinned by test), but the
  pin also format-expands `#{…}` *inside* the body before running it; zz hands the shell
  hook the body raw (phase 5/6 — status-seam surface).
- `#{S:}` loop ordering follows the pin's global sort criteria default (index); if zz ever
  grows choose-tree sort commands, the loop default must track the mutable criteria
  (choose-tree work).
- Positional-arity validation is unguarded and the daemon buffer family hand-rolls its
  parsing (phase-0 leftovers); `move-pane -p` is zz-lax.
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
was removed). Ledgered: bare `list-keys` flags-column padding (key-string wave),
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
2026-08-21). Maintainer decisions folded in: `pane-border-style`/`pane-active-border-style` HONOR
explicitly-set colors over the theme (attributes beyond color ignored, one divergence
row); `window-style`/`window-active-style` (inactive-pane dimming) and the
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
pin's config grammar landed whole — `$VAR`/`${VAR}` expansion (charset, `${9}` vs `$9`,
`\$`, undefined→empty), the full escape set (`\NNN`/`\u`/`\U`/singles, invalid forms
error byte-exact), `NAME=value` + `%hidden` assignments (applied at parse time BEFORE
the file's commands, visible same-line, SURVIVING a parse abort — all pin-probed), and
`%if`/`%elif`/`%else`/`%endif` EVALUATION (engine format expansion at server/global
scope, `FORMAT_NOJOBS` — `#()` renders empty and never spawns, pin `format_true`,
same-line + nested forms, balanced-through-whitespace `#{…}` conditions). Whole-file
abort on the first diagnostic with the pin's `syntax error` strings. The five re-parse
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
corpus on 2026-08-20. NEW TICKET exposed with a live repro: zz's
`source-file` swallows parse diagnostics and exits 0 where the pin prints
`path:line: message` rc 1 / `%config-error` — pre-existing, documented asymmetrically
in the config-grammar scenario's warn expectations.

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
