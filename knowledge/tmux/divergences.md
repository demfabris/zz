---
type: Reference
title: tmux divergence matrix
description: "Every known divergence from tmux at the pinned reference commit: the 12 missing commands and why, the 30 implemented commands that still reject tmux flags, behavioral gaps on the implemented surface, the options coverage (all 180 store, 86 behave), and the protocol-level differences."
resource: third_party/tmux-reference/UPSTREAM.md
tags: [tmux, compatibility, divergences, gaps, reference]
timestamp: 2026-08-20T00:00:00-03:00
---

# Overview

This is the exhaustive inventory of where zz differs from tmux at the pinned reference commit
`d77c9dc6aa021e4bc61f0da128c591af695e6466`, produced by the 2026-08-16 verification sweep: four
independent passes compared each implemented behavior against the fetched tmux C sources. It
complements the [compatibility philosophy](/tmux/tmux-compat.md) (the contract) and the
[superset roadmap](/designs/tmux-superset-roadmap.md) (the plan) by enumerating the actual deltas.

**State anchor:** the "implemented surface" section reflects
[PR #4](https://github.com/demfabris/zz/pull/4) (`fix/tmux-compat-hunt-v2`, merged 2026-08-16
as `53b523e`) plus the phase 3d layout-string and phase 4a–4f-2 source dated 2026-08-17. PR #4 corrected the
hunt-claim regressions, including two implemented backwards
(`new-window -t` bare-target order, positional targets on the kill commands) plus the
`kill-session -C`, `new-session -A`, `resize-pane` attached-adjustment, `send-keys -H`/`-l`,
`copy-mode -du`, window-step-error, and boolean-case fixes.

The counts and flag ledger were refreshed against the live source after Waves 2a and 2b on
2026-08-20. The pre-wave catalog held 159 unsupported pairs across 38 tmux commands; those
waves removed 30 pairs, and Wave B's read-only slice removed `attach-session -r` on
2026-08-21, leaving 128 across 30. The zz-only `split-picker` contributes another
19 markers to a raw catalog grep and is deliberately excluded from tmux compatibility counts.

The one-line read: everything marked **silent** below is a bug by zz's own doctrine (tmux syntax
must mean what tmux means, or error loudly); everything loud is a choice; the "genuine gaps"
command block plus the remaining options grind is the actual drop-in backlog.

# Missing commands (12 of 92)

Counted 2026-08-20 against the pin's `cmd_table[]` (92 entries, 78 with an alias, 170
spellings). zz's shared catalog (`crates/zz-protocol/src/catalog.rs`) holds 94 verbs: 14
zz-native and 80 tmux — 61 in `COMMAND_SPECS` plus 19 in `DAEMON_COMMAND_SPECS` for the verbs
that need client, job, buffer, or overlay state (`capture-pane`, the seven `*-buffer` commands,
`run-shell`, `if-shell`, `wait-for`, `pipe-pane`, `display-popup`, `display-menu`,
`confirm-before`, `switch-client`, and the lock trio). `MuxEngine` runs 58 of the tmux verbs; the daemon
intercepts `list-clients`, `refresh-client`, `show-messages`, and `switch-client` ahead of it. Every pin alias
resolves to the same command it does in tmux. That leaves 12 of tmux's 92 commands absent
(`UNIMPLEMENTED_TMUX_COMMAND_TABLE`, 19 spellings), all answering `unsupported command: <name>`
rc 1, counted as skips in config reports, and absent from `list-commands` so feature probes
take their fallback path. Four deliberate groups:

## Exec commands (doctrine revised by phase 5, 2026-08-18)

The old "config must never execute programs" doctrine died with phase 5's verified
facts: `#()` and creation-command positionals already spawned `/bin/sh` ungated, so
gating only the exec commands was theater. The user's own `mux.conf` is trusted like
`.bashrc`; the consent gate moves to the foreign-config *import flow*. `run-shell` /
`run` and `if-shell` / `if` now execute for real (wave 5a-2, `9f55f87`) with
pin-parity semantics — see the drop-in plan's phase-5 section for the accepted
divergences (overlay output routing, inherited job environment, job cap backstop).
`wait-for` / `wait` and `pipe-pane` / `pipep` shipped in wave 5b (`2d7a655`)
with pin-parity semantics (sticky signals, lock leak-on-disconnect, pre-parse
output tap, pipe survives respawn); Interactive clients cannot park on blocking
waits — they get the pin's clientless errors (scripts are faithful).

`set-hook` / `show-hooks` shipped in wave 5c (`40ddd63`): full 68-name storage
with pin scope, after-* + command-error + event hooks fire (events clientless
like the pin), `@`-user hooks share the option slot. Ledgered: `-B` monitors
rejected; 11 event names store-only (no zz seam); `window-layout-changed`
single-fires where the pin double-fires on resize/select-layout.

The lock trio shipped in wave 5d-2 (`01096c2`) as storage + error parity:
`lock-command` (default `lock -np`) and `lock-after-time` (default 0) store
and read back, all three commands and aliases parse with pin-exact clientless
error shapes, and `after-lock-server` fires through the hook bus — but zz
never spawns a lock program over GUI surfaces (a locked GPUI window running
`lock -np` is meaningless; revisit with the TUI client), and `lock-after-time`
drives no timer. **silent**, deliberate.

Still unimplemented and skipped from config:

| Command | What it does in tmux |
| --- | --- |
| `server-access` | Per-user ACLs for a shared server socket. |

## Superseded by native GUI chrome (4)

`display-popup`, `display-menu`, and `confirm-before` left this table in waves
5d-1/5d-2: behavior is pin-exact, but the surfaces render as native
zz-design-language floating panes (FloatingSurface) instead of cell-drawn
overlays — one deliberate visual divergence, keyboard semantics ported from
`menu.c`/`popup.c` exactly (menus clamp on paging, wrap on step; tmux mouse
semantics on menus stay GUI-native).

| Command | What it does in tmux |
| --- | --- |
| `customize-mode` | Interactive options browser. |
| `choose-client` | Chooser listing attached clients. |
| `clock-mode` | Full-pane clock. |
| `suspend-client` | Ctrl-Z the attaching client. |

## Genuine gaps — buildable, nothing blocks them (3)

| Command | What it does | Weight |
| --- | --- | --- |
| `resize-window` | Manual window sizing decoupled from clients. Also what makes `window-size manual` behave as `latest` today. | low |
| `clear-prompt-history` / `show-prompt-history` | Prompt history management (`prompt-history-limit` and `history-file` already behave). | low |

## Parked by decision or by model (4)

| Command | Why it stays out |
| --- | --- |
| `link-window` / `unlink-window` | Linked windows and session groups are skipped permanently (drop-in plan decision 3). One window belongs to one session. |
| `new-pane` / `switch-mode` | The pin's floating-pane family (new in next-3.8). zz has no floating-pane model; the phase-1 picker verb was renamed off `new-pane` so the name stays tmux's. Unassessed beyond that. |

# Flag-level gaps on implemented commands (30 of 80; 128 pairs)

Being cataloged is not the whole contract: 30 of the 80 implemented tmux commands still
reject 128 flags the pin accepts, answering `unsupported command: <cmd> -X` rc 1 (and
counting as config skips). The catalog declares the unsupported pairs and the mux/daemon
parsers enforce them. Inventory as of 2026-08-20 (flags in pin spelling; flags marked † are
gated by decision 3 or by missing context/model support, the rest are plain work):

| Command | Rejected flags |
| --- | --- |
| `attach-session` | `-c -E -f -x` |
| `break-pane` | `-a -b`; `-W -x -y -X -Y` † |
| `capture-pane` | `-C -F -H -L -P -R` |
| `choose-buffer` | `-F -K -k -N -y` |
| `choose-tree` | `-F -h -K -k -N -y`; `-G` † |
| `clear-history` | `-H` |
| `command-prompt` | `-1 -C -e -F -i -k -l -N -t -T`; `-P` † |
| `copy-mode` | `-k -H -s`; `-S` † |
| `detach-client` | `-E -t -P` |
| `display-message` | `-a -C -c -d -l -N -v`; `-I` † |
| `display-panes` | `-N -t` |
| `join-pane` | `-l` |
| `kill-pane` | `-f` |
| `kill-session` | `-f`; `-g` † |
| `kill-window` | `-f` |
| `last-pane` | `-d -e` |
| `list-keys` | `-1 -a -N -O -P -r` |
| `load-buffer` | `-t -w` |
| `move-pane` | `-D -L -P -R -U -X -Y -l -z -M` (floating-pane placement) † |
| `new-session` | `-E -X -e -f`; `-t` † |
| `new-window` | `-b -E -e` |
| `resize-pane` | `-M -T` |
| `select-pane` | `-d -e -g -M -m -P` |
| `send-keys` | `-c -F -K -R`; `-M` † |
| `send-prefix` | `-2` |
| `set-buffer` | `-t -n -w` |
| `show-messages` | `-J`; `-T -t` † |
| `source-file` | `-t -F -n -v` |
| `split-window` | `-e -E -I -k -m -R -s -S -T -W -Z` |
| `unbind-key` | `-a -q` |

`list-commands` prints each command's usage with zz's accepted flags, so the rows above are
the exact catalog-declared places its output differs from the pin's.

## Accepted grammar that still diverges

The catalog count does not include syntax zz accepts or parses before diverging:

- `refresh-client` rejects bare redraw, `-S`, `-c -D -L -R -U -l -r`, and the optional
  adjustment positional. `-A -B -C -f -F -t` behave.
- `load-buffer -` rejects stdin, `save-buffer -` rejects stdout, and `source-file -` rejects
  stdin.
- `show-buffer` refuses non-UTF-8 buffer bytes.
- `capture-pane -p` and `-T` parse but do not affect behavior. Routing looks only at whether
  `-b` is present: bare capture prints instead of filling the default buffer, while
  `-b name -p` stores instead of printing.
- `list-keys <key>` rejects the positional key filter.
- `send-keys -H` rejects bytes `80` through `ff`.
- Bind-time validation checks names and catalog flags but still misses complete positional
  arity/target validation and daemon-command payload validation.

## Catalog overacceptance

Four flags are zz extensions on tmux command names even though the pin rejects them:
`move-pane -p` and `send-keys -C`/`-P`/`-o`. They are compatibility debt, not progress
against the 128 unsupported pairs.

## Former Wave 2e ownership (45 pairs)

The old plan grouped these commands under a TUI tranche. Source tracing shows that most of
the work belongs to the server:

| Command family | Server/core | Client/presentation | Parked on missing context/model |
| --- | --- | --- | --- |
| `command-prompt` | `-F -l -t` | `-1 -C -e -i -k -N -T` | `-P` pane-rendered prompt |
| `copy-mode` | `-k -s` | `-H` indicator visibility | `-S` bound mouse-slider context |
| `send-keys` | `-c -F -K -R` | . | `-M` originating mouse event |
| `display-message` | `-a -c -d -l -N -v` | `-C` | `-I` CLI stdin/protocol stream |
| chooser residue | tree `-F -h -k -y`; buffer `-F -k -y` | `-K -N` on both | tree `-G` session groups |
| `display-panes` | `-N -t` | . | . |
| `show-messages` | `-J` | . | `-T -t` TTY capability model |

That is **25 server/core**, **13 client/presentation**, and **7 parked** pairs. The TUI
queue should contain only the middle column; moving the first column there would duplicate
daemon-owned command semantics in a client.

# Divergences on the implemented surface

| Where | Divergence | Loud or silent? |
| --- | --- | --- |
| `find-window` | Detached CLI calls validate the target and return success with no output, including for zero matches. zz does not open tmux's attached-client window-tree chooser. | **silent**, bounded |
| `list-commands` | zz lists implemented commands in tmux's line format. Each usage string reports zz's accepted flags, so affected rows differ from the pin. Unimplemented commands stay absent so feature probes can take their fallback path. | **silent**, deliberate |
| `list-keys` default formatting | `list-keys -F` expands the pin's per-binding `key_repeat`, `key_note`, `key_prefix`, `key_table`, raw `key_string`, and quoted `key_command` facts. Since 2026-08-20 every stored command renders through the pin's `args_print` model — `#{key_command}`, the bare `list-keys` form, and `show-hooks` alike: canonical names, value-less flags merged first in flag order, valued flags in flag order, `args_escape` quoting (seven pin vectors plus three hook shapes byte-matched). Still missing: the bare form's list-width-aware padding, and zz's default copy-mode tables mark 35 (`copy-mode`) / 76 (`copy-mode-vi`) stock bindings `-r` where the pin marks none — zz's copy-mode model uses the repeat flag, so `#{key_repeat}` and bare listings differ for stock bindings only. | **silent**, deferred |
| `refresh-client` | `-A`/`-B`/`-C`/`-f`/`-F`/`-t` behave (phase 6: flow control, subscriptions, control-client sizing). Bare redraw, `-S`, and the attached-client redraw/scroll family (`-c -D -L -R -U -l -r` plus the optional positional adjustment) answer `unsupported command: refresh-client interactive behavior`; detached command clients with no target get the pin's exact `no current client`. | loud |
| `switch-client -c` TTY targets | Client names and zz's synthetic `device-N` ids resolve, including one trailing `:` trimmed like the pin. Since 2026-08-21 terminal-surface clients send `client-tty-v1:` in the hello only when `$TMUX` marks a nested run (the fact exists for the nested-attach check), so the pin's full-tty and `/dev/`-stripped aliases still do not resolve in general. | loud for a TTY-only target |
| Read-only clients (`attach-session -r`, `switch-client -r`) | Enforced at the daemon input funnel since 2026-08-21: terminal and browser key/text, paste, mouse, divider resize, prompt/chooser/popup/menu/confirm actions, uploads, and agent prompts are dropped for a read-only client, while output, resize, detach, and the pin's `CMD_READONLY` command roster (`attach-session`, `copy-mode`, `detach-client`, `list-clients`, `send-keys`, `switch-client`) still work; other commands answer the pin's `client is read-only`. `client_flags` reports `read-only` without the pin's coupled `ignore-size` — zz sizes every client individually, so the pin's `CLIENT_IGNORESIZE` half of `-r` has no zz meaning. The pin's same-uid check on re-marking a read-only client is skipped (single-user daemon). | **silent** only on the dropped-input feedback (tmux also drops silently) |
| `switch-client -E` | Accepted as a no-op because zz does not retain the attaching client's environment; the same missing model already bounds session `update-environment` seeding. | **silent**, bounded |
| Session current window across clients | tmux stores one current window on the session, so `select-window` or `switch-client -t session:window` moves every client attached there. zz keeps `focused_windows` per client by design. One client can change windows without moving its peers, and peer rows from `list-clients -F '#{window_index}'` can differ from the pin. | **silent**, deliberate zz extension |
| `#{client_flags}` unmodeled flags | zz emits `attached` and every modeled control/read-only flag in the pin's sequence. It omits `focused` because the daemon does not track terminal focus, and it omits the pin's `UTF-8` client flag because UTF-8 is a fixed protocol contract rather than client state. | **silent**, bounded |
| `copy-mode` | `-k -H -S -s` rejected (`-e`/`-q`/`-M` — the stock-binding trio — are implemented). | loud |
| Native chooser status | `choose-tree` and `choose-buffer` implement `-f`/`-O`/`-r`, including tmux's default orders, hierarchy pruning, and zero-match fallback to the unfiltered chooser. The native overlay does not show tmux's `filter: no matches` status after that fallback. | **silent**, cosmetic |
| `capture-pane` routing | `-p` and `-T` are accepted but inert. A bare capture prints instead of filling the default buffer, while `-b name -p` stores instead of printing. | **silent** |
| `source-file` | Parse diagnostics are shaped like the pin's (`path:line: message`) but travel as client-event warnings, so control-mode and GUI clients see them (`%config-error`) while the plain CLI exits 0 with empty stderr where the pin prints the line and exits 1. Closing it requires changing the CLI event-routing behavior and adding direct diagnostic coverage. `-` stdin refused loudly; `-t`/`-F`/`-n`/`-v` rejected as unsupported. Globbing works. | **silent** on the CLI, loud elsewhere |
| `mouse` / `escape-time` | Behaving since 2026-08-21 (Wave B2/B3). zz-tui gates the outer-terminal mouse modes (`?1003h`/`?1006h`/`?1016h`) on the session-effective `Mouse` value from the v71 publication, emits/retracts them live on `MuxOptionsChanged` (the pin's default is on: the reference builds with `-DTMUX_MOUSE=1`), and the daemon drops mouse-originated `TerminalView` input from terminal-surface clients when the effective value is off; the GUI's native mouse stays ungated per decision 6. With the option off, an application inside a pane can still use the mouse exactly as the pin documents (`options-table.c` mouse help; `server-client.c` forward_key): the outer modes also follow the active pane's own `mouse_tracking`, events forward straight to the tracking pane under the cursor with every chrome branch skipped, and the daemon admits them for panes whose app requested tracking. Chrome mouse (status clicks, sidebar, dividers, focus clicks) remains available only while the option is on — matching the pin, whose mouse key bindings also fire only then. `escape-time` replaces the TUI's old 25 ms escape fold timeout (pin default 10 ms, 0 clamps to 1 like `tty_keys_next`). Both keys are config-writable through `MuxOptionKey::from_config_key` with the standard reload-reapply semantics. | none — behaving |
| `set-titles` empty expansion | With `set-titles on` and a `set-titles-string` that expands to the empty string, zz publishes an empty `StatusLine.title`: the GUI reverts to its native title and zz-tui writes no OSC, where the pin's `server_client_set_title` would set an empty terminal title. Empty doubles as the "option off" wire state, so this narrow edge is deliberate. | **silent**, narrow and deliberate |
| `automatic-rename` / `automatic-rename-format` | `automatic-rename` gates the desktop's active-pane-derived tab label, and explicit `rename-window`, `new-window -n`, or the first-window name pins a window-local `off`. zz does not mutate `Window.name` every 500 ms, so `#{window_name}` remains the explicit model name, and the stored format string is not evaluated by the presentation-only renamer. | **silent**, bounded |
| `aggressive-resize` + `window-size` | Since 2026-08-20 both compose like the pin (`resize.c:366-376`): `aggressive-resize` is a candidate FILTER (ON = clients focused on the window; OFF = zz's viewer set, a per-client-focus stand-in for the pin's linked-window `session_has`), and `window-size` is the AGGREGATION policy — `latest` (default) picks the most-recent-input owner, `largest`/`smallest` aggregate componentwise. ON no longer forces `smallest`; configs relying on that must also set `window-size smallest`. `manual` is stored but behaves as `latest` until `resize-window` exists. | **silent**, bounded |
| `display-time` | Status-message toasts consume the configured milliseconds. Since 2026-08-20 the omitted `display-panes -d` duration comes from `display-panes-time` like the pin (the old reuse divergence is closed). A zero toast remains until manual dismissal, while tmux dismisses its zero-duration status message on a key. | **silent**, deliberate |
| `respawn-pane` / `respawn-window` | Dead panes revive with stable pane identity; `respawn-window` keeps its first pane and removes the rest. `-k`, `-c`, repeated `-e NAME=VALUE`, and stored command/cwd reuse are implemented. The pin's `-E` empty-environment flag is cataloged but rejected. | loud for `-E` |
| Array options | Since the 2026-08-20 Lane-2 sweep all eight real array options (`command-alias`, `codepoint-widths`, `user-keys`, `terminal-overrides`, `terminal-features`, `status-format`, `pane-colours`, `update-environment`) store with the pin's separators, hole reuse, and `name[N]`/`-u name[N]` semantics, and the 68 hook names route to the hook table. Since the B1 server slice (2026-08-21) `status-format[]` drives the daemon's personalized `StatusLine.rows` production (sparse indices publish blank rows, a session array overrides the global one whole, scoped writes refresh that session's attached clients). Wave C added two more consumers (2026-08-21): `command-alias[]` expands one layer before canonical lookup at both dispatch chokepoints (`MuxEngine::expand_command_alias`), and, like the pin's parse-time expansion, `bind-key`/`set-hook`/`default-client-command` STORE the expansion, so `list-keys` and `show-hooks` print `list-windows` for an aliased `lsw` on both servers (differentially pinned). Aliases nested inside a `{ … }` argument of a stored command expand at execution instead of at store time, so their stored text keeps the alias name. Only SINGLE-command alias bodies expand: the pin also accepts a multi-command body (`x=cmd1 ; cmd2`, caller arguments appended to the last, `cmd-parse.c:2317`) and an empty body (silent rc 0), where zz refuses both with `unknown command: <alias>` rc 1 — loud rather than silent, per doctrine, because zz's dispatch chokepoint executes exactly one command. Alias lookup is exact on the typed name in both (a command prefix like `ls` never reaches the alias table). `update-environment[]` drives `seed_session_environment` plus its own readback. The remaining five still drive nothing. Indexed `@`/table scalars follow tmux (`not an array` on set; indexed show reads the scalar). | **silent**, store-only except `status-format[]`, `command-alias[]`, and `update-environment[]` |
| Status-row window-option scoping | tmux resolves `window-status-*`, `pane-status-*`, and `window-pane-*-status-format` per loop item during `status-format` expansion, so a per-window override (`set -w -t work:2 window-status-style 'fg=red'`) styles that window's entry in the row. zz's B1 row expansion resolves those names at global/session-effective scope through a per-client variables map, while the `status_label` path keeps honoring per-window overrides — since B1's client half renders the rows (2026-08-21), the two surfaces can visibly disagree for windows with local overrides: the rendered status block shows the global/session style while the zz-native label surfaces show the override. Resolution path: per-window variable resolution through the loop-item hook seam (`Expander::lookup` already passes the item context), scheduled for Wave C (the `Expander::lookup` loop-item seam). | **silent**, bounded |
| Status-block suppression threshold | tmux hides the status line when `tty.sy <= statuslines` (resize.c `CLIENT_STATUSOFF`), so a 3-row terminal with `status 2` still shows both status rows plus one window row. zz panes carry a header row, so the TUI suppresses the block when `rows < statuslines + 2` (one header plus one content row must survive) — in that same 3-row terminal zz shows no status block and gives all rows to the pane. The GUI mirrors the rule against its measured canvas in line-height units. | **silent**, bounded |
| `history-limit` default | zz keeps 10,000 lines for its product default; the pin keeps 2,000. `show-options -g history-limit` prints the effective 10,000 value. | **silent**, deliberate |
| Plain option listings | No-argument listings contain tmux table names and `@` user names. The six zz-native settings stay available through explicit-name queries and never appear as unknown words in tmux-parsing scripts. | **silent**, zz extension hidden from tmux listings |
| Session environment updates | Both servers seed their global environment at boot. Since Wave C (2026-08-21) zz honors the stored `update-environment` array at session creation like the pin's `environ_update` (`environ.c:186`), including unset markers for names with no value. What still differs is the SOURCE: tmux copies each name from the creating client's environment (`cmd-new-session.c:282`), zz has no client-environment field and copies from the daemon's boot environment. They differ when the daemon outlives the shell that started it. zz reads the global `update-environment` array only; the pin's session-scoped value would bite solely at the attach-time re-seed zz does not perform, so the scope gap is subsumed by the no-re-seed bound. Two consequences of that missing field stay ledgered: `new-session -E`/`attach-session -E` remain cataloged-but-rejected, and zz never re-seeds on attach (the pin re-runs `environ_update` in `cmd-attach-session.c:135`). zz matches the pin's glob-free names; the pin's `fnmatch` patterns in `update-environment` values are not expanded. | **silent**, bounded |
| Lifecycle trio | `exit-empty`, `exit-unattached`, and `destroy-unattached` are inert until a config EXPLICITLY sets them (presence in the stored-scalar map, not the effective value): unset, zz keeps its persistent-daemon rule, `armed ∧ zero sessions ∧ zero subscribers`. Explicitly set, the pin's `server_loop` (`server.c:281-292`, whose client loop at `:289-292` is the check the subscriber clause below contrasts against) and `server_check_unattached` (`server-fn.c:481`) policies take over — enforced on client departure and command execution, where the pin re-evaluates every loop iteration — with one permanent divergence: the `zero subscribers` conjunct is LOAD-BEARING and survives every policy, because a zz GUI/TUI client can outlive its session where a tmux client cannot, so an attached client must never have the daemon die under it. "Attached" means present in `ServerState::attached` (a client bound to a session); "subscriber" means an Interactive or Control client holding an outbound mailbox. `exit-unattached on` therefore exits when no client is bound to a session AND no client is subscribed, where the pin needs only the former. Policies are also dormant inside the startup bracket so a boot config cannot kill the daemon it is configuring. `destroy-unattached=keep-last`/`keep-group` are decided by linked session groups in the pin (`session_group_contains`); zz has no session groups, so `keep-last` never destroys (every session is effectively the last of its group) and `keep-group` always destroys — both are exact for the ungrouped case, which is every zz session. Session groups stay the permanent compatibility skip. | **silent**, bounded, opt-in |
| `new-session` attach contract | Closed 2026-08-20. `zz new -s foo` creates and attaches on a TTY, refuses off one with `open terminal failed: not a terminal` and creates nothing, and reproduces the pin's full check order (`-A` delegate ignoring `-d` → duplicate → nested → terminal → `-x`/`-y`). NULL-client callers (config files, hooks) force detached and treat attach as a silent success, matching `cmd-new-session.c:164-167` and `cmd-attach-session.c:71-72`. | closed |
| `new-session -x -` / `-y -` | The literal `-` is accepted and no longer an error, but it resolves to the 80x24 default instead of the *client's* terminal size (the pin reads `c->tty.sx/sy`, `cmd-new-session.c:216-238`). Detached-with-no-client is identical to the pin; only the with-a-client case differs. Needs the client's terminal size plumbed into the engine. | **silent**, bounded |
| Nested `new-session` / `attach-session` | The pin refuses with `sessions should be nested with care, unset $TMUX to force` when the calling client's tty is a pane of this server (`server_client_check_nested`, gated at `cmd-new-session.c:191-198`); zz runs a nested TUI instead. `$TMUX` alone is the wrong signal — a fake `$TMUX` on a non-pane pty still attaches on the pin, so this needs the client's tty compared against pane ttys. | loud on the pin, permissive in zz |
| Client-exit notices | Closed for zz-tui in protocol v70: requested/evicted detaches print `[detached (from session X)]` rc 0, a destroyed session with no survivor prints `[exited]` rc 0, shutdown prints `[server exited]` rc 1, and a lost connection prints `[server exited unexpectedly]` rc 1, all after terminal restoration. Native GUI and control-mode surfaces keep their existing presentation. | closed |
| In-UI error text width | Command errors surfaced inside the TUI render in the sidebar's 28-column status row, so a long tmux message truncates (`can't find window: 99` shows as `can't find win`). The message text itself is now the pin's, via one shared renderer. Collapsed-sidebar mode uses the full width. | **silent**, cosmetic |
| `#()` job environment | Closed by wave 7d: status jobs receive `TMUX=socket,pid,-1`, the pane working directory as `PWD`, and no `TMUX_PANE`, matching the pin's session-null status-job shape. | closed |
| Shell job environment overlay | Closed on the overlay half 2026-08-20: `run-shell`/`if-shell` jobs receive the global `set-environment` overlay and, when the job has a session, the session overlay — hidden entries withheld, child-unset markers removed — matching `environ_for_session`; this is what lets Oh My Tmux's `$TMUX_PROGRAM`-chained bootstrap run at all. Still divergent: jobs start from the daemon's environment rather than a clean one, the TERM family (`TERM`, `TERM_PROGRAM`, `COLORTERM`) is not synthesized, and status `#()` jobs get only `TMUX`/`PWD`. The smoke harness injects a canary so scenarios cannot accidentally depend on inherited host state. | **silent**, bounded |
| `#{version}` | zz reports `3.8-zz`, sharing the compatibility-version source used by `zz -V` (`tmux 3.8-zz`); the pin reports `next-3.8`. The suffix is deliberate so scripts can identify the compatible implementation without confusing it with upstream tmux. | **silent**, deliberate |
| Non-UTF-8 command arguments | tmux prints a byte such as `a\377b` with octal vis escaping. zz converts argv with `to_string_lossy` before escaping and prints `a<U+FFFD>b`. | **silent**, accepted edge |
| Config `~` expansion | Leading `~` of unquoted words and a `~` just inside an opening double quote expand to `$HOME` at parse time, matching the pin (single-quoted, escaped, and mid-word tildes stay literal on both sides — probe-verified 7/7). Deliberate residue: `~user` forms stay literal where the pin resolves them via `getpwnam`, and an unset/empty `HOME` leaves the `~` literal where the pin fails the line with a parse error. | **silent** edge |
| Command-name abbreviation | CLOSED by wave 7d (2026-08-18): zz implements the pin's `cmd_find` contract (cmd.c:470-508) — exact alias wins outright, a unique prefix over the alphabetical name table resolves (engine and daemon dispatch alike), several matches answer the pin's byte-exact `ambiguous command: <name>, could be: <list>`. Reviewer-swept every 2..N prefix of all 92 pin names: resolution classes match; remaining textual differences are the ledgered arity/flag wording (7c). Prefixes resolving to catalogued-but-unimplemented commands answer `unsupported command: <canonical>`. | closed |
| `set prefix` key validation | zz rejects unresolvable bare keys with the pin's `bad key: <value>` but silently accepts unresolvable `C-`/`M-` keys the pin rejects (`C-zz`): a typo'd prefix is accepted and never fires. Full strictness needs the pin's `key_string_table` breadth (`^a` caret form, `BTab`, the KP family) — a partial tightening would loudly reject pin-valid keys instead, so this waits for a key-string parity wave. | **silent** edge |
| Error-shape residue (post-7b) | Grep-facing error classes are pin-bare and byte-exact since wave 7b (2026-08-18): the twelve `options-values.sh` regress strings, `can't find session/window/pane:`, `unknown command:`, `already set:`, `open terminal failed: not a terminal`, show-messages pairs, `%config-error <file>:<line>:`. Catalogued-but-unimplemented commands/options answer `unsupported command: <name>` — a zz-only condition the pin would instead run. Arity/flag rejections and usage fallbacks keep zz wording (`<cmd> does not support -X` vs the pin's `command <cmd>: unknown flag -X`; no `usage:` fallback) pending per-command arity metadata (7c). | loud |
| Alerts | Bell path only: `bell-action` and `visual-bell` behave on the pin's alerts.c model (C1, 2026-08-20). `monitor-activity`/`monitor-silence`/`monitor-bell`, `activity-action`/`silence-action`, and `visual-activity`/`visual-silence` store and read back but drive nothing — the bell path stays unconditional (`honest_knobs.rs:764`) and no activity/silence timers exist. Matches tmux defaults; an explicit `monitor-activity on` is silently inert. | **silent**, store-only |
| `select-layout main-*` with 2 panes | The pin never sizes the lone "other" pane (layout-set.c:264-269, :458-463), leaving stale geometry that fails tmux's own `layout_check`; zz sizes it (80x24 → main 80x22 + other 80x1). Deliberate: zz refuses to reproduce an upstream bug. | **silent**, zz more correct |
| `select-layout -E` on a mixed parent | The pin spreads only leaf children (layout.c `layout_cell_is_tiled`) but divides the parent's full extent among them, so a parent mixing leaves with nested nodes gets corrupt sums (observed: 40+42+39 in an 80-wide window, last pane at xoff 84). Every later operation on that corrupted window keeps diverging: one `-E` produced four geometry divergences, three downstream, so the known scenario has one causal step but the divergence is not bounded to it. zz refuses that spread and stops the walk where the pin stops. All-leaf parents are exact (48 pin fixtures + `known/known-spread-mixed.txt`). | **silent**, zz more correct |
| `select-layout` strings with zero-sized leaves | The pin accepts a leaf with width or height zero. zz rejects it to preserve the `PANE_MINIMUM` invariant. | **loud**, zz more correct |
| `select-layout` strings with extents above `u16::MAX` | The pin accepts `70000x70000` through its `u_int` geometry. zz rejects it; `PANE_MAXIMUM` is 10000. | **loud** |
| `select-layout` strings with single-child nodes | The pin accepts a node with one child. zz requires every node to have at least two children. | **loud**, zz more correct |
| `select-layout` string validation order and depth | zz validates the whole string before trimming cells to the current pane count. The pin trims first and runs `layout_check` afterward, so a sum violation confined to a deleted cell fails only in zz. zz caps parsing at 256 levels and rejects a 300-deep string in bounded time; the live pin held 100% CPU for minutes on input around 100 levels deep. This validation-order edge is one-directional: zz never accepts a string that the pin rejects. | **loud** |
| Lone-pane `select-layout` strings | On an 80x24 one-pane window, the pin accepts `a8fd,120x30,0,0,0`, keeps `window_width=80`, and dumps `120x30` from its new root. zz adopts the encoded extent for both the window and layout. | **silent**, zz more correct |
| Detached `split-window` while zoomed | tmux pops zoom before the split (cmd-split-window.c:239). zz preserves zoom for `split-window -d` while it changes the hidden layout. A focused split and every non-`-Z` `resize-pane` unzoom first on both sides. | **silent** |
| `move-pane` on tiled panes | The pin reserves `move-pane` for floating targets and returns `pane is not floating` for a tiled target (cmd-join-pane.c:428-431). zz has no floating panes and keeps `move-pane` as an alias of `join-pane`. | loud |
| Attached-GUI `#{pane_width}` | Formats report the engine's cell allocation while PTYs are still sized by client pixel measurement, so a drawn pane's format can drift a cell from `tput cols` until the client-reported window size lands. Headless is exact. | **silent**, bounded |
| `#{window_flags}` | zz emits `!` bell, `*` current, `-` last, and `Z` zoomed in tmux order. `#` activity, `~` silence, and `M` marked remain absent because zz does not model those states. | **silent** |
| `send-keys -N` (no keys) | Arms the **invoking client's** count prefix; tmux stores it on the pane mode, so another client's (or a Command client's) `-N` is a silent no-op in zz. | **silent** edge |
| `send-keys -X` | `select-line`/`copy-end-of-line` ignore counts; flags written after the verb (`-X copy-selection -C`) parse as positionals; no "not in a mode" error. | **silent** |
| `send-keys -H` | Bytes `80`–`ff` refused; tmux writes the raw byte (`KeyToken::Literal` carries UTF-8). | loud |
| Unguarded commands | Closed by the [drop-in plan](/designs/tmux-drop-in.md)'s phase 0: every engine command rejects options centrally from its catalog `CommandSpec` — flags tmux has at the pin but zz lacks error as unsupported (and count in config-import skip reports); flags tmux doesn't have error as invalid. Residual: the daemon-side `capture-pane`/buffer family still hand-rolls parsing, though since 2026-08-20 its 19 catalog specs carry the pin's full flag arity (accepted and `unsupported` alike), so renderers and hook variables read the right shapes. | loud |
| `bind-key` payloads | Bind-time validation covers names and flags only; positional arity and target errors still surface at keypress, and daemon-side verbs (`capture-pane`, the buffer family) bind with no validation at all. tmux validates the full argument template at bind time. | **silent** edge |
| Empty-daemon listing and attach | Both servers now begin with empty session/window/pane sets, so the first `new-session` gets name `0` and ids `$0`/`@0`/`%0`. zz's CLI connection path auto-starts a missing daemon and `list-sessions` succeeds with empty output, while tmux's missing-server path reports `no server running on ...`. A default Interactive attach to an empty zz daemon lazily creates the next numeric session; registration, background fleet probes, explicit missing targets, and Command clients do not. | **silent**, native-client accommodation |

## Format variables that remain unbacked

These names are registered so parsing matches the pinned 198-name table, but zz still returns an
empty string where tmux can return data. Each gap is separate on purpose: none of them is hidden
inside a generic “unsupported formats” claim.

| Variable | Missing backing | Loud or silent? |
| --- | --- | --- |
| `buffer_mode_format` | No tmux buffer-mode row formatter; zz's buffer chooser is native. | **silent** |
| `client_activity` | `list-clients -F` supplies the daemon's retained activity time; status-line expansion still lacks a client row context. | **silent**, bounded |
| `client_colours` | The attaching client's terminal color count is not fed into format expansion. | **silent** |
| `client_created` | Client creation time is not retained as a format fact. | **silent** |
| `client_flags` | `list-clients -F` supplies `attached` plus modeled read-only and control flags in pin order; status-line expansion still lacks a client row context. | **silent**, bounded |
| `client_key_table` | `list-clients -F` supplies the active per-client table; status-line expansion still lacks a client row context. | **silent**, bounded |
| `client_last_session` | `list-clients -F` supplies the previous live session; status-line expansion still lacks a client row context. | **silent**, bounded |
| `client_mode_format` | No tmux client-mode row formatter; zz's client surfaces are native. | **silent** |
| `client_name` | `list-clients` supplies the registry name; status-line expansion still lacks a client row context. | **silent** |
| `client_session` | `list-clients` supplies the attached session name; status-line expansion still lacks a client row context. | **silent** |
| `client_termfeatures` | Terminal feature negotiation is not represented as a tmux format string. | **silent** |
| `client_termname` | The attaching client's `TERM` name is not retained as a format fact. | **silent** |
| `client_termtype` | tmux's terminal-type classification has no zz equivalent. | **silent** |
| `client_theme` | `list-clients` supplies the retained light/dark theme; status-line expansion still lacks a client row context. | **silent** |
| `client_tty` | Native and remote zz clients do not provide a tmux attach-client TTY path. | **silent** |
| `client_user` | `list-clients` uses the daemon user because the local socket does not retain a separate attach-client user. | **silent**, bounded |
| `config_files` | The config loader does not retain tmux's printable loaded-file list. | **silent** |
| `cursor_character` | The glyph under the terminal cursor is not mirrored into mux facts. | **silent** |
| `cursor_colour` | Cursor color is not mirrored into mux facts. | **silent** |
| `mouse_hyperlink` | Command formats do not receive tmux's mouse-event hyperlink record. | **silent** |
| `mouse_line` | Command formats do not receive tmux's mouse-event line text. | **silent** |
| `mouse_pane` | Command formats do not receive tmux's mouse-event pane id. | **silent** |
| `mouse_status_line` | Command formats do not receive tmux's mouse-event status-line index. | **silent** |
| `mouse_status_range` | Command formats do not receive tmux's mouse-event status range. | **silent** |
| `mouse_word` | Command formats do not receive tmux's mouse-event word text. | **silent** |
| `pane_bg` | The terminal cell background at the cursor is not mirrored into mux facts. | **silent** |
| `pane_dead_signal` | Retained dead panes do not publish the terminating signal as a format fact. | **silent** |
| `pane_dead_time` | Retained dead panes do not publish their exit timestamp. | **silent** |
| `pane_fg` | The terminal cell foreground at the cursor is not mirrored into mux facts. | **silent** |
| `pane_key_mode` | Native copy/view mode is not projected as tmux's pane key mode. | **silent** |
| `pane_mode` | Native pane mode is not projected as tmux's mode name. | **silent** |
| `pane_pb_state` | Terminal progress-bar state is not mirrored into mux facts. | **silent** |
| `pane_search_string` | Native per-view search text is not mirrored into mux facts. | **silent** |
| `pane_tabs` | Terminal tab stops are not mirrored into mux facts. | **silent** |
| `session_activity` | The daemon tracks activity to choose `detach-on-destroy off` survivors, but it is not exposed as a format fact; `S/t` still sorts by creation time. | **silent** |
| `session_group` | Session groups are unsupported, so no group name exists. | **silent** |
| `session_group_attached_list` | Session groups are unsupported, so no grouped attachment list exists. | **silent** |
| `session_group_list` | Session groups are unsupported, so no member list exists. | **silent** |
| `session_last_attached` | `list-clients -F` supplies the retained time for the row's session; status-line expansion still lacks a client row context. | **silent**, bounded |
| `session_path` | zz has no separate per-session working directory fact. | **silent** |
| `tree_mode_format` | No tmux tree-mode row formatter; zz's tree chooser is native. | **silent** |
| `window_activity` | Mux state tracks a monotonic activity point for `-O activity` list and chooser sorting, but does not expose a timestamp through this format variable; `W/t` retains window-index order. | **silent**, bounded |
| `window_offset_x` | Client viewport X offset is not fed into window formats. | **silent** |
| `window_offset_y` | Client viewport Y offset is not fed into window formats. | **silent** |

# Options: all 180 store, 86 behave

tmux's `options-table.c` holds 180 named options (plus 68 hook entries) at the pin. Since
the 2026-08-20 Lane-2 sweep **every one of the 180 stores** with the pin's exact default,
type validation, scope, inheritance, `-a`/`-u`/toggle semantics, and listing shape, so bare
`show-options -s`/`-g`/`-gw` byte-match the pin and a real `tmux.conf` imports with **zero
skipped lines**. That is test-enforced: `tmux_options.rs`
(`listing_order_covers_every_non_hook_table_option_once`,
`every_named_option_has_storage_metadata`) and `command.rs`
(`every_remaining_named_scalar_stores_and_bare_listings_cover_the_pin_table`) pin the 180,
the eight array options store with indexed semantics, and `set-option <hook-name>` writes
the hook table — the two paths that used to silently succeed while doing nothing are dead.

**86 behave**, meaning a value change is consumed somewhere outside set/show/inherit/
readback (consumer-traced 2026-08-20; nine status-production names joined in the B1 server
slice on 2026-08-21, `status-position` joined with B1's client half the same day,
`mouse`, `escape-time`, `set-titles`, and `set-titles-string` joined with the B2/B3/title
slice, and `command-alias`, `update-environment`, `exit-empty`, `exit-unattached`, and
`destroy-unattached` joined with the Wave C alias/environment/lifecycle slice, all
2026-08-21). **94 are store-only.** The earlier "78 behave" counted
options given a typed home in the honest-knobs/status structs, twelve of which nothing
read. `tmux_options::BEHAVES` distinguishes the consumer-traced names from storage-only
options and test-pins its count, uniqueness, and membership in the option catalog.
`tmux_stored_scalar` and `tmux_stored_array` storage is store-only by construction until a
consumer wave wires a name up and moves it into `BEHAVES` (B1 moved six stored scalars and
the `status-format` array).

**Behaving (86):**

- Indexing and sessions: `base-index`, `pane-base-index`, `renumber-windows`,
  `default-size`, `window-size`, `aggressive-resize`, `history-limit` (10,000 product
  default; the pin keeps 2,000 — the one fenced default divergence), `detach-on-destroy`.
- Keys and prompts: `prefix`, `mode-keys`, `key-table`, `prefix-timeout`, `repeat-time`,
  `initial-repeat-time`, `prompt-history-limit`, `history-file` (saved on submission, not
  at shutdown), `word-separators`, `wrap-search`.
- Spawn and terminal: `default-shell`, `default-command`, `default-terminal`,
  `remain-on-exit`, `focus-events`, `allow-passthrough` (`on` behaves as `all` — the
  worker lacks visibility state; payloads cap at 1 MiB then discard-until-ST; nested
  `\ePtmux;` is not recursively unwrapped), `allow-set-title`, `cursor-style`/
  `cursor-colour` (per-pane appearance clones; a zz-config `cursor-blink` override still
  outranks the blink half), `synchronize-panes`.
- Names and alerts: `automatic-rename`, `automatic-rename-format`, `bell-action`,
  `visual-bell`.
- Overlays and buffers: `display-time`, `display-panes-time`, `message-limit`,
  `buffer-limit`, `set-clipboard`, `copy-command`, and the seven `menu-*`/`popup-*`
  style and border options.
- Layout: `main-pane-width`/`-height`, `other-pane-width`/`-height`,
  `tiled-layout-max-columns`.
- Status bar (GUI titlebar strip and tabs; see the Presentation row): `status`,
  `status-interval`, `status-left`/`-right`, `status-left-length`/`-right-length`,
  `status-left-style`/`-right-style`, `status-style`, `status-bg`, `status-fg`,
  `window-status-format`, `window-status-current-format`, `window-status-style`,
  `window-status-current-style`, `window-status-last-style`, `window-status-bell-style`.
- Status rows (B1, 2026-08-21 — the daemon expands these into the personalized v71
  `StatusLine` per client, and since the client half both clients render the result
  through the shared `zz-client` status-row compositor; see the Presentation row):
  `status-format[]` (sparse indices publish blank rows, session arrays override whole),
  `status-justify` (resolved inside the expanded row formats), `message-line` (published
  clamped to the row count; selects the row messages and the TUI prompt replace),
  `status-position` (the TUI shifts or shrinks its canvas for the block; the GUI places
  its customized-gated block container top or bottom), `pane-status-style`,
  `pane-status-current-style`, `session-status-style`, `session-status-current-style`,
  `window-pane-status-format`, `window-pane-current-status-format` (all six resolve
  inside the default pane/session list rows).
- Terminal surface and titles (B2/B3 + the C3 title source, 2026-08-21): `mouse` and
  `escape-time` (zz-tui consumes the session-effective values; the daemon rejects mouse
  input from terminal-surface clients when off — see the `mouse` / `escape-time` row),
  `set-titles` and `set-titles-string` (the daemon expands the title per client into
  `StatusLine.title`, publishing even with `status off`; zz-tui writes OSC 2 on non-empty
  changes and the GUI adopts the window title only when the option is explicitly on).
- Command aliasing, environment, and lifecycle (Wave C, 2026-08-21): `command-alias`
  (one expansion layer before canonical lookup at both dispatch chokepoints, and at
  bind-key/set-hook/option-command validation; non-recursive like the pin's
  `CMD_PARSE_NOALIAS`), `update-environment` (drives `seed_session_environment` and its
  own readback), and the lifecycle trio `exit-empty`, `exit-unattached`,
  `destroy-unattached` — all three inert until EXPLICITLY set, and the
  "zero subscribers" guard survives every policy (see the Lifecycle trio row).

**Store-only (94):**

- Typed storage that nothing reads (38): `lock-after-time`,
  `lock-command` (the lock commands are no-ops); `monitor-activity`, `monitor-silence`,
  `monitor-bell`, `activity-action`, `silence-action`; `allow-rename`, `alternate-screen`,
  `scroll-on-clear`, `extended-keys`, `extended-keys-format`, `xterm-keys`, `backspace`,
  `editor`, `assume-paste-time`, `input-buffer-size`, `get-clipboard`,
  `default-client-command`, `fill-character`, `variation-selector-always-wide`;
  `message-style`, `message-command-style`, `message-format`;
  `pane-border-lines`, `pane-border-indicators`, the four `pane-scrollbars*`; the four
  `prompt-*cursor-*`; `clock-mode-colour`, `clock-mode-style`;
  `window-status-separator`, `window-status-activity-style`.
- Generic scalar storage (51 of the 63 scalar-backed names) plus five of the eight
  arrays: everything else in the table,
  including `prefix2`, `display-panes-format`,
  `remain-on-exit-format`, `visual-activity`/`visual-silence`, `status-keys`,
  `pane-border-style`/`pane-active-border-style`,
  `window-style`/`window-active-style`, `mode-style` and the `copy-mode-*` styles,
  `terminal-overrides[]`, `terminal-features[]`, `user-keys[]`,
  `pane-colours[]`, `codepoint-widths[]`, the 21 theme-palette options, and the
  `tree-mode-*` trio. Lane assignments live in the drop-in plan's "options residue"
  section.

The index trio follows tmux's session/window inheritance, allocation, targeting, format,
and close-triggered renumbering behavior. `set-option` also accepts six zz-native names —
the agent/editor/history-trickle keys — which don't count toward tmux coverage and never
appear in the no-argument listings (those contain tmux table names and `@` names only).
`show-options` and `show-window-options` expose values with tmux's exact string escaping,
value-only and inherited forms. Free-form `@` names are pure string storage at server,
global-session, session, global-window, window, and pane scope, including append and unset;
this is the storage seam TPM and plugins use. Global and per-session environment overlays
have `set-environment`/`show-environment` readback and are merged into new terminal PTYs,
including hidden and child-unset entries; the daemon seeds the global map from its process
environment, and `new-session` copies the fixed default `update-environment` names or
writes unset markers. `automatic-rename` gates the desktop's active-pane label and explicit
window names install the pin's window-local `off`; its format string is evaluated by the
daemon-side label path only. `remain-on-exit` retains a frozen dead pane with live
`pane_dead` and normal-exit `pane_dead_status` facts, and the respawn commands revive that
stable pane slot. `default-terminal`, `display-time`, and `repeat-time` feed new PTYs,
client message/overlay timers, and each attached session's repeat-key window.
Bare `list-keys` output lacks the pin's flags-column padding (`bind-key  -T` two-space
form) — ledgered for the key-string wave.

# Protocol and process level

| Area | tmux | zz |
| --- | --- | --- |
| Env contract | `$TMUX`, `$TMUX_PANE`, plus server-seeded global and client-updated session overlays | Panes get `$TMUX` in tmux's exact `socket,pid,session` shape plus `TMUX_PANE=%N`; exec-family jobs get `$TMUX` without `TMUX_PANE`; wave 7d added status-job `TMUX=socket,pid,-1`, `PWD`, and no `TMUX_PANE`. `ZZ_PANE`/`ZZ_SESSION`/`ZZ_SOCKET` ride alongside panes. The remaining clean/session job-overlay divergence is listed above. |
| Binary argv | `-L -S -f -2 -C -u -V -N -c -l` | Closed by 7a (2026-08-18): `-V` (`tmux 3.8-zz`), `-L`/`-S`/`-f`/`-c`/`-N`/`-l`/`-2`/`-u`, tmux-shaped usage and unknown-option lines, pin CMD_STARTSERVER autostart. `-C`/`-CC` are the phase-6 control-mode front-end (row below). |
| Control mode `-CC` | What iTerm2 integration speaks. | SHIPPED (phase 6 complete 2026-08-18): a stdio front-end speaking the full CC protocol — framing, notifications, `%output` with flow control (pause/age-kill/pacing), `refresh-client -A/-B/-C/-f`. Deliberate divergences, all reviewer-endorsed: blocks are COMPLETE (WAIT commands keep output in-block; after-hooks add no extra block; `%pause`/`%continue` land after the triggering block, not inside); per-client monotonic `n`; zz-lax unquoted `%`-words on the control stdin; automatic-rename transients single-fire. |
| Session groups | `new-session -t`. | Cataloged, rejected. |
| `StatusLine.customized` | No equivalent — tmux has no wire and no explicit-write ledger. | zz-native v71 field: true while any explicit `status`, `status-*`, or `status-format` write is in force for the recipient's scope (even when the value equals the default); scalar and whole-array unsets clear their mark, an indexed `status-format[N]` unset keeps it. Gates the TUI's `Ctrl-\ detach` hint (dropped when customized) and the GUI's tmux status-block container (shown only when customized). |
| Presentation | Status line, prompts, choosers drawn as terminal escapes. | Since B1's client half (2026-08-21) both clients render the daemon's personalized `status-format[]` rows through one shared compositor in `zz-client` that reproduces `format-draw.c` — alignment sections, `fill=`, list focus/truncation with `<`/`>` markers, `base_style` on blank rows (an empty or unparseable `base_style` means theme default), and window/pane/session hit ranges. The TUI renders the authoritative block across the main columns at the published `status-position` (top shifts the canvas, bottom shrinks it; the block is suppressed when the terminal cannot keep one pane-content row), keeps its three-row zz-native sidebar consuming `left`/`right`/`status_label` beside it, replaces the `message_line` row with client messages and the prompt (one virtual row at the configured position when status is off), keeps PREFIX/COPY indicators as a right-aligned overlay, drops the `Ctrl-\ detach` hint once `customized`, and routes status-row window-range clicks to `select-window`. The GUI keeps its native titlebar strip and sidebar at defaults (decision 6) and adds a top-or-bottom monospace row container across the main content area only when `customized` is set. Prompts and choosers stay native on both. |

# Related

- [tmux drop-in plan](/designs/tmux-drop-in.md) — the 2026-08-16 plan and current work queue.
  Linked windows/session groups and real-tmux socket interop are the only permanent exclusions;
  the other ledgered rows remain compatibility work or bounded native-surface divergences.
- [tmux compatibility philosophy](/tmux/tmux-compat.md) — the contract these divergences are
  measured against.
- [tmux superset roadmap](/designs/tmux-superset-roadmap.md) — the tier ladder and the amended
  never-list.
- [commands](/tmux/commands.md) — the implemented verb-by-verb reference.
