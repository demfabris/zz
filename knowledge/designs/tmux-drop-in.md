---
type: Design Plan
title: tmux drop-in plan
description: "The alias-tmux=zz plan: 100% of tmux's command grammar, options, formats, and geometry on tmux names, zz power moved to superset verbs, exec commands behind an import-time consent gate — nine phases ending at the TTY attach contract, one permanent skip (linked windows), one explicit non-goal (real-tmux socket interop)."
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

Revised same day after an adversarial review, every claim of which was verified against the
tree: the differential harness moved off control mode onto `list-* -F`, the exec/consent story
was rebuilt around the fact that config already executes shell ungated, and the TTY attach
contract — the part of the alias the original six phases never scheduled — became phase 8.

The target splits in two:

- **Config/script drop-in** (phases 0–7): configs import and behave, scripts that create,
  query, target, and kill sessions work, TPM and resurrect-class plugins run. This is the bulk
  of the alias and none of it needs a TTY-attaching client.
- **Full drop-in** (phase 8): bare `tmux`, `tmux new -s foo`, and `tmux attach` attach the
  calling terminal. Gated on the [TUI client](/designs/tui-client.md) design's open rungs.

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

## Phase 4 — the grind (~2–3 months, parallelizable)

Running wave-by-wave on [PR #6](https://github.com/demfabris/zz/pull/6) (`feat/tmux-grind`),
same loop as phases 0–3: settled plan → codex → full gates → adversarial pin review → close.

| Work | Scope | Status |
| --- | --- | --- |
| `base-index` / `pane-base-index` / `renumber-windows` | index arithmetic everywhere, plus the pin's full 248-entry option table routing every tmux name by declared scope (`setw -g base-index 1` works) | **shipped 2026-08-17** (wave 4a) |
| Target grammar | the full cmd-find pass order (fnmatch, `=`-exact, unique prefix, `{start}`/`{end}`/`{last}`/`^`/`$`/`!`/`+`/`-`), empty targets, tmux's exact `can't find …` error strings, cross-window `select-pane` focus semantics | **shipped 2026-08-17** (wave 4b) |
| Full formats engine | the 198-name registry, every scalar modifier + `S`/`W`/`P` loops + `e` math + `C` search, the daemon runtime-facts feed (proc cwd, pid, tty; OSC 7 → `pane_path`), `-c` as a format consumer with spawn.c's chdir chain, the `fmt:` differential channel | **shipped 2026-08-17** (wave 4c, four review rounds) |
| Options readback | `show-options`/`show-window-options` with the pin's quoting, `@user` options as pure storage (TPM), `set-`/`show-environment` + PTY env injection, the `out:` differential channel | in flight (wave 4d) |
| The gap commands | the buildable missing commands minus `respawn-*` (held for the behavior wave) and minus `link-`/`unlink-window` (decision 3): `move-window`/`swap-window`, `find-window`, `list-clients`/`list-commands`, `show-messages`, `start-server` as a no-op (TPM's bootstrap), basic `refresh-client` | next (wave 4e) |
| Behavior options | **all greenlit 2026-08-17 (fabrico)**: `automatic-rename` (tabs track the running program, explicit names win), `remain-on-exit` shipping TOGETHER with `respawn-pane`/`respawn-window`, `aggressive-resize` (per-window multi-client sizing through the measurement write-back), and the mechanical trio `default-terminal`/`display-time`/`repeat-time`. `mouse` + `escape-time`: accepted and stored, semantics scoped to the TUI attach client (phase 8); the GUI ignores them | wave 4f |
| Daemon boot parity | **decided 2026-08-17 (fabrico)**: lazy-create the auto-session — CLI-spawned daemons boot empty like tmux; session `0` materializes only when a GUI client attaches with no session. Un-shifts `$N`/`@N`/`%N` for scripts; revisit the harness prologue's `kill-session -t 0` and the layout-diff id-stripping after | with 4f |
| Styles (`#[…]`, `*-style`) | meaningful on the TUI surface; GUI maps to theme | later |
| `source-file -F`/`-n`/`-v` | format-expanded paths, parse-only, verbose printing — deferred from phase 1 | later |

`switch-client` is **not** mechanical: a pane script's `switch-client` must retarget some
*other* Interactive client's attachment, and the only pane→client seam today is
`ClientHello.origin`, sent by Command clients only. It rides the same client-seam work as
phase 8.

## Phase 5 — the exec family (~3 weeks)

Three tiers, not one:

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

## Phase 6 — control mode (~2–3 weeks)

`-C`/`-CC` for iTerm2 and control-mode scripts. The transport, verified against iTerm2's
`TmuxGateway.m`: iTerm2 launches `tmux -CC` **in a PTY and parses the `%begin`/`%output` text
protocol from the process's stdio** — it never opens tmux's socket. So control mode is a zz
*front-end*, like `zz-tui`: the zz process speaks the CC text protocol on its own
stdin/stdout and talks postcard to the daemon behind it. Daemon-side needs: a raw pane-output
tap (the wire ships rendered grids today; `%output` carries raw bytes), notification events
(from phase 5's hook bus), and `refresh-client -C`/`-B`/`-A` (client size, subscriptions,
pane visibility). The harness (phase 2) does not wait for this.

## Phase 7 — the binary surface (~1 week)

- tmux argv on the zz binary: `-L` (name → socket path), `-S`, `-f`, `-2`, `-u`, plus `-C`/
  `-CC` (front-end from phase 6) and `-V`.
- `$TMUX` exported in panes alongside `ZZ_*`, in tmux's **exact shape**
  `socket_path,server_pid,session_id` — resurrect `cut`s field 1 for the socket, continuum
  field 2 for the pid; `[ -n "$TMUX" ]` alone is not the contract. `$TMUX_PANE` = `%id`.
- `tmux -V` answers `tmux 3.8-zz`: the pin `d77c9dc6` is `next-3.8` (`AC_INIT([tmux],
  next-3.8)`), not 3.5. TPM's version check digit-strips either to `38`; handle other
  version-gating fallout case by case.
- Exit codes and error-output shapes matched where scripts grep them.
- An alias smoke suite: a corpus of real-world `tmux.conf` files and scripts run under
  `alias tmux=zz`, asserting zero warnings and matching behavior.

## Phase 8 — the attach contract (gated on the TUI design)

The four invocations the alias lives on, none of which work today:

| Invocation | tmux | zz today |
| --- | --- | --- |
| `tmux` | new session + attach this TTY | boots the GUI, no TTY/headless check |
| `tmux new -s foo` | create **and** attach this process | creates, exits — the daemon applies `MuxEffect::Attach` only for Interactive clients |
| `tmux attach -t foo` | attach this TTY | usage error — the attach subcommand takes only a bare positional |
| `tmux attach` | attach, starting the server if needed | closest match, but refuses to spawn a missing daemon (command mode does auto-spawn; attach doesn't) |

Needs: TUI-as-default on a TTY (the [TUI client](/designs/tui-client.md) design's open
rungs — this is why the phase is gated, not estimated), `attach -t`/flag parsing,
daemon-spawn on attach, and a story for `new-session` attaching the calling process (run the
TUI client, or a Command→Interactive upgrade). Shares the client-seam work with
`switch-client` (phase 4).

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

**Full drop-in (phase 8):** the four attach-contract invocations behave on a TTY.

# Out of scope, permanently

- Linked windows and session groups — decision 3. `new-session -t` stays a loud rejection.
  Two named consequences: resurrect's grouped-session restores error loudly (above), and
  `break-pane` on a single-pane window keeps refusing (tmux *relinks* the window into the
  destination — that is linked-window machinery).
- Speaking tmux's private client-server socket protocol — decision 4. iTerm2 does not need
  it (phase 6).
- Fleet broadcast (`--all`) — unchanged from the superset roadmap: composition over features.

# Risks

- `tmux -V` gating: any answer is a small lie; plugins may exercise version-specific paths.
- Cell-stepped dividers may feel worse than today's smooth drag — mitigated by the
  commit-on-release option.
- Formats/styles are tmux's largest maintenance surface; the pinned-commit discipline
  (verify against `d77c9dc6`, never guess) is what keeps the grind honest.
- The Interactive/Command client split is load-bearing (attach effects apply only to
  Interactive clients); `switch-client` and phase 8 both cut into that seam — do the seam
  design once, not twice.
- Consent scope: trusting the user's own config matches tmux and current zz behavior, but it
  means an imported-then-edited config never re-prompts; acceptable, worth stating.

# Related

- [tmux superset roadmap](/designs/tmux-superset-roadmap.md) — tiers 1–3 (landed) and the
  doctrine this plan amends.
- [tmux divergence matrix](/tmux/divergences.md) — the current deltas, row by row.
- [tmux compatibility philosophy](/tmux/tmux-compat.md) — the subset contract that holds
  until each phase lands.
- [TUI client](/designs/tui-client.md) — the attach surface phase 8 rides on.
