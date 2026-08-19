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
timestamp: 2026-08-18T00:00:00Z
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
| Styles (`#[…]`, `*-style`) | meaningful on the TUI surface; GUI maps to theme | later |
| `source-file -F`/`-n`/`-v` | format-expanded paths, parse-only, verbose printing — deferred from phase 1 | later |

`switch-client` is **not** mechanical: a pane script's `switch-client` must retarget some
*other* Interactive client's attachment, and the only pane→client seam today is
`ClientHello.origin`, sent by Command clients only. It rides the same client-seam work as
phase 8.

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
post-prefix text exact) — phase-7 error-shape scope. Deferred to phase 8
with the attach contract: no-tty `new-session` divergence (pin: `open
terminal failed: not a terminal` rc 1; zz: detached create rc 0) and the
pty-gated nested-session refusal probe. Accepted wart adopted: `-L
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
  unverified vs the pin's c->name. NEXT: 6c = -A pane states +
  no-output/pause-after flags + the pin's pacing constants (pacing
  implemented faithfully even while the kill trigger stays divergent;
  pause must gate queue ENTRY, not drain), then 6d sizing/subscriptions.

## Phase 7 — the binary surface (~1 week) — **PHASE COMPLETE 2026-08-18**

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

  The corpus pins TPM, tmux-sensible, vim-tmux-navigator, tmux-yank, tmux-resurrect,
  tmux-continuum, and tmux-fpp. Oh My Tmux remains gated on `%if` evaluation and is not part of
  this wave.

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

## Phase 8 — the attach contract (gated on the TUI design)

The four invocations the alias lives on (rows 3-4 largely closed by 7a,
2026-08-18):

| Invocation | tmux | zz today |
| --- | --- | --- |
| `tmux` | new session + attach this TTY | boots the GUI, no TTY/headless check |
| `tmux new -s foo` | create **and** attach this process | creates, exits — the daemon applies `MuxEffect::Attach` only for Interactive clients |
| `tmux attach -t foo` | attach this TTY | works — full `-t`/`-d` grammar, TUI attach on a TTY, engine-identical `can't find session:` headless (7a) |
| `tmux attach` | attach, starting the server if needed | works — autostarts the daemon (CMD_STARTSERVER), `no sessions` on an empty server, TTY check last (7a) |

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
- `update-environment` markers at session create source from the daemon's environment, not
  the attaching client's (the wire carries no client environ); diverges when the daemon
  outlives the shell that started it.
- Two upstream layout bugs refused rather than reproduced (two-pane `main-*` preset,
  mixed-parent `-E` spread) — `known/` scenarios pin them.
- Grouped sessions / linked windows / socket interop / fleet broadcast — the permanent
  out-of-scope list above; resurrect's grouped-session restore errors loudly by design.

**Deferred mechanics (owner in parentheses):**

- Array options as a category (`status-format[N]`, `command-alias`, `terminal-features`):
  indexed spellings parse and answer silently; storage/rendering unimplemented (styles
  wave / TUI phases).
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
- `switch-client` and the TTY attach contract ride the client-seam design (phase 8);
  control mode is phase 6; `tmux -V`/`$TMUX` shape is phase 7.
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
  output lead where tmux keeps them in lockstep); VT parser throughput is the
  largest pin-adjacent gap (debug-build measurement: 8MB un-piped flood 93s vs
  the pin's 1s — it is the direct cause of the mid-flood capture-pane timeout
  and the copy_pipe_timeout load-flake; deserves its own wave, not more
  per-test margin raising).
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
- The three documented load-flaky `zz-daemon` tests (`terminal_process_exit_…` and
  friends) still fail under full-workspace parallel load on CI occasionally — flake, not
  behavior, but noise in every assessment.
- macOS-vs-glibc strftime quirks are now load-bearing (the daemon calls libc strftime —
  the workspace's only `unsafe` block): any future platform (musl, Windows) needs its own
  parity probe of unknown-`%` handling.

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
