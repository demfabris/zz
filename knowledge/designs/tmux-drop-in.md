---
type: Design Plan
title: tmux drop-in plan
description: "The alias-tmux=zz plan: 100% of tmux's command grammar, options, formats, and geometry on tmux names, zz power moved to superset verbs, exec commands behind a consent gate — six phases, one permanent skip (linked windows), one explicit non-goal (real-tmux socket interop)."
status: Approved
tags:
- tmux
- compatibility
- drop-in
- layout
- control-mode
- roadmap
timestamp: 2026-08-16T00:00:00Z
---

# Overview

Goal: `alias tmux=zz` works — a tmux user's binary invocations, config, scripts, and muscle
memory all behave identically, while zz-only power lives in superset verbs that never collide
with tmux names. This supersedes the never-list of the
[tmux superset roadmap](/designs/tmux-superset-roadmap.md); the current deltas being closed are
enumerated in the [divergence matrix](/tmux/divergences.md).

**Decisions (2026-08-16, fabrico):**

1. The exec-family refusal (`run-shell` etc.) is lifted. It ships behind a consent gate for
   imported/pasted configs — a UX safeguard, not doctrine.
2. Cell-authoritative layout is approved. Verified to have no rendering or benchmark
   performance impact; the only visible GUI change is dividers stepping by cells (with a
   smooth-drag/commit-on-release option).
3. Linked windows and session groups (`new-session -t`) are **skipped permanently** — the
   single "100% minus" item. One window belongs to one session; the rejection stays loud.
4. Interop with a real tmux binary over its private socket protocol is a non-goal. The alias
   means zz handles tmux's argv everywhere; it never speaks tmux's client-server wire format.

# Phases

Ordering rationale: everything-loud before anything-new (phase 0), the differential harness
before the geometry rework it validates (phase 2 before 3), and `base-index` first in the
grind because index arithmetic touches everything (phase 4).

## Phase 0 — the floor (in flight)

- Land [PR #4](https://github.com/demfabris/zz/pull/4) (hunt-claim corrections).
- Catalog-driven unknown-flag rejection: every command rejects flags absent from its
  `CommandSpec`, deleting the ~15 hand-rolled allowlists. Requires a one-time audit that
  catalog entries match handler-accepted flags. After this, every remaining gap is loud —
  the precondition for claiming compatibility at all. ~2–3 days.

## Phase 1 — the superset rework (~1 week)

Move every GUI-motivated divergence off tmux names:

- Stop routing key-bound `split-window` to the picker; zz's *default* bindings bind a zz verb,
  imported tmux bindings get pure tmux behavior.
- Rename the picker verb off `new-pane` — the pinned tmux now owns that name for floating
  panes. Frees the name for a real floating-pane implementation later.
- Tighten the remaining zz-lax argument acceptance (`select-window`/`attach-session`
  positionals), mirroring the PR #4 kill-command fix.

## Phase 2 — control mode as the harness (~2–3 weeks)

Implement tmux's control mode (`-C`/`-CC`, the `%begin`/`%end`/notification text protocol) as
a new daemon client kind. Buys three things: iTerm2 integration, the machine-readable surface
scripts expect, and — the reason it comes early — the **differential test harness**: feed the
same command script to zz and a real tmux, diff `list-sessions`/`list-windows`/`list-panes`
output until they agree. Phase 3 is validated with it.

## Phase 3 — cell-authoritative layout (~2–3 weeks)

Make absolute cells the layout truth in `zz-mux` (`model.rs` split/resize/preset math), with
ratios derived for rendering — reversing today's direction. Closes the entire silent-geometry
block of the divergence matrix at once: nested-resize sibling drift, the 10–90% clamp vs
`PANE_MINIMUM` (1 cell), `-f` pane numbering, tmux's integer spread on window resize. Enables
faithful serialized layout strings (`select-layout` compat, resurrect-style save/restore).

- No rendering/bench impact: layout math is a handful of nodes on state change; the draw
  pipeline consumes the same pixel rects; `bench/` never touches layout.
- GUI feel decision: dividers step by whole cells (tmux-like), or the drag stays smooth and
  commits the rounded cell count on release. Decide by trying both.
- The daemon geometry feed (`pane_cells` from `ResizeTerminal`) already exists.

## Phase 4 — the grind (~2 months, parallelizable)

| Work | Scope | Estimate |
| --- | --- | --- |
| `base-index` / `pane-base-index` / `renumber-windows` | index arithmetic everywhere — first | with options below |
| Remaining ~150 options | mechanical; unknown options keep report-and-skip until implemented | 3–4 weeks |
| Full formats engine | tmux's ~200 `format.c` variables + modifiers, replacing the subset | 2 weeks |
| Styles (`#[…]`, `*-style`) | meaningful on the TUI surface; GUI maps to theme | 2 weeks |
| The 18 gap commands | `switch-client`, `show-options`/`show-window-options`, `move-window`, `swap-window`, `set-environment`/`show-environment`, `respawn-pane`/`respawn-window`, `find-window`, `resize-window`, `list-clients`, `list-commands`, `show-messages`, `clear-`/`show-prompt-history` | 2–3 weeks |

## Phase 5 — the exec family (~1 week code; gate design is the work)

`run-shell`, `if-shell`, `set-hook`/`show-hooks`, `wait-for`, `pipe-pane`, `display-popup`,
`display-menu`, `confirm-before`, the `lock-*` trio. Code is trivial (`sh -c` + effects). The
design surface is the consent gate:

- Interactively-typed commands (`prefix :`) run without ceremony — the user is present.
- Imported/sourced configs prompt on first exec, with a persisted per-config allowlist.
- ssh-host scoping: a consented config does not silently execute on every fleet host; consent
  is per (config, host).
- `if-shell` upgrade path: parse-and-skip today becomes full execution under the same gate.

## Phase 6 — the binary surface (~days)

- tmux argv on the zz binary: `-L` (maps a name to a socket path), `-S`, `-f`, `-2`, `-u`.
- `$TMUX` and `$TMUX_PANE` exported in panes alongside `ZZ_*`, shaped so `[ -n "$TMUX" ]` and
  `$TMUX_PANE` targeting work.
- Exit codes and error-output shapes matched where scripts grep them.
- `tmux -V` answer: emit `tmux 3.5-zz` (or the pinned version + suffix); handle plugin
  version-gating fallout case by case.
- An alias smoke suite: a corpus of real-world `tmux.conf` files and scripts run under
  `alias tmux=zz`, asserting zero warnings and matching behavior.

# Acceptance

- The differential harness (phase 2) passes a shared command-script corpus against real tmux,
  including geometry.
- A real-world `tmux.conf` corpus imports with zero skipped lines (exec lines prompt instead
  of skip).
- TPM boots under the consent gate; a session save/restore plugin round-trips via layout
  strings.
- `bench/run.sh` shows no regression after phase 3.

# Out of scope, permanently

- Linked windows and session groups — decision 3. `new-session -t` stays a loud rejection.
- Speaking tmux's private client-server socket protocol — decision 4.
- Fleet broadcast (`--all`) — unchanged from the superset roadmap: composition over features.
- TUI-as-default-surface on GUI-less hosts is required for the alias to fully pay off but is
  its own design: [TUI client](/designs/tui-client.md).

# Risks

- `tmux -V` gating: any answer is a small lie; plugins may exercise version-specific paths.
- Cell-stepped dividers may feel worse than today's smooth drag — mitigated by the
  commit-on-release option.
- Formats/styles are tmux's largest maintenance surface; the pinned-commit discipline
  (verify against `d77c9dc6`, never guess) is what keeps the grind honest.
- Consent fatigue: a config with many `run-shell` lines must prompt once per config, not once
  per line.

# Related

- [tmux superset roadmap](/designs/tmux-superset-roadmap.md) — tiers 1–3 (landed) and the
  doctrine this plan amends.
- [tmux divergence matrix](/tmux/divergences.md) — the current deltas, row by row.
- [tmux compatibility philosophy](/tmux/tmux-compat.md) — the subset contract that holds
  until each phase lands.
- [TUI client](/designs/tui-client.md) — the attach surface the alias eventually rides on.
