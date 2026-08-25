---
type: Reference
title: tmux divergence matrix
description: "Dated rationale and source evidence for measured tmux divergences, including command, flag, behavior, option, format, hook, and protocol differences."
resource: third_party/tmux-reference/UPSTREAM.md
tags: [tmux, compatibility, divergences, gaps, reference]
timestamp: 2026-08-25T00:00:00-03:00
---

# Overview

This page preserves rationale and source evidence from the compatibility sweeps against tmux commit
`d77c9dc6aa021e4bc61f0da128c591af695e6466`. Four independent passes produced the original
2026-08-16 record; later dated paragraphs capture follow-up measurements. The
[compatibility philosophy](/tmux/tmux-compat.md) defines the contract, and the
[superset roadmap](/designs/tmux-superset-roadmap.md) records delivery sequence.

`compat/tmux-gaps.json` owns live TODOs, classifications, and status.
Read the generated [gap report](/tmux/gaps.md) for the current inventory. Treat every count and
roster below as a dated measurement. Add or close work in the registry, then regenerate the report;
keep this page for the detailed reasoning that a compact tracker row cannot carry.

**State anchor:** the "implemented surface" section reflects
[PR #4](https://github.com/demfabris/zz/pull/4) (`fix/tmux-compat-hunt-v2`, merged 2026-08-16
as `53b523e`) plus the phase 3d layout-string and phase 4a–4f-2 source dated 2026-08-17. PR #4 corrected the
hunt-claim regressions, including two implemented backwards
(`new-window -t` bare-target order, positional targets on the kill commands) plus the
`kill-session -C`, `new-session -A`, `resize-pane` attached-adjustment, `send-keys -H`/`-l`,
`copy-mode -du`, window-step-error, and boolean-case fixes.

The counts and flag ledger were refreshed against the live source after Waves 2a and 2b on
2026-08-20. The pre-wave catalog held 159 unsupported pairs across 38 tmux commands; those
waves removed 30 pairs, Wave B's read-only slice removed `attach-session -r` on
2026-08-21, Wave C run 2 removed `send-prefix -2` the same day, and Wave D run 1 removed
`copy-mode -H` plus `-K`/`-N` on both choosers on 2026-08-22, Wave D run 3 removed
`display-message -C`/`-d` the same day, and Wave D's final run removed the seven
`command-prompt` mode flags, leaving 113 across 29. The 2026-08-22 alias tranche then implemented
`new-window -b` and `unbind-key -a/-q`, then the filtered `kill-session -a`, `kill-window -a`, and
`kill-pane -a` forms, leaving 107 across 26. Implementing `resize-window` with client-derived
`-A`/`-a` still loud brought the ledger to 109 across 27; `display-panes -N/-t` and `join-pane -l`
then reduced it to 106 across 25. Pane-local `-e` and empty-pane `-E` on `new-window` and
`split-window` removed four more pairs, and `last-pane -d/-e` removed two more. Four micro flags,
three `list-keys` selectors, and creation-time `new-session -e/-E` removed the next nine pairs.
`set-buffer -n`, `source-file -F`, `split-window -Z`, and `break-pane -a/-b` removed five more;
adding `move-pane -l` removed one more, leaving 85 across 23. The zz-only `split-picker` contributes another 19 markers to a raw catalog
grep and is deliberately excluded from tmux compatibility counts.

The doctrine requires an explicit decision for every silent difference because tmux syntax must
keep tmux meaning or fail loudly. Loud refusals can remain accepted exclusions. The registry records
that decision and whether the difference still blocks a supported workload.

# Recognized but unimplemented commands: 2026-08-22 snapshot

Counted 2026-08-20 against the pin's `cmd_table[]` (92 entries, 78 with an alias, 170
spellings). zz's shared catalog (`crates/zz-protocol/src/catalog.rs`) holds 102 verbs: 19
zz-native and 83 tmux. The split between `COMMAND_SPECS` and `DAEMON_COMMAND_SPECS` is ownership,
not discoverability: both feed resolution, `list-commands`, completion, and stored-command
rendering. `MuxEngine` runs 59 of the tmux verbs; the daemon
intercepts `list-clients`, `refresh-client`, `show-messages`, and `switch-client` ahead of it. Every
pin alias resolves to the same command it does in tmux. Nine of tmux's 92 commands have no handler
(`UNIMPLEMENTED_TMUX_COMMAND_TABLE`, 13 spellings): they resolve as recognized-unimplemented, answer
`unsupported command: <name>` rc 1, count as skips in config reports, and stay absent from
`list-commands` so feature probes take their fallback path. No pinned spelling resolves as unknown.
Three deliberate groups:

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
are rejected. Ten names have no automatic producer: `after-queue`,
`client-active`, `client-focus-in`, `client-focus-out`,
`client-resized`, `client-light-theme`, `client-dark-theme`, `pane-focus-in`,
`pane-focus-out`, and `pane-set-clipboard`. `window-layout-changed` single-fires
where the pin double-fires on resize/select-layout.

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

## Parked by decision or by model (4)

| Command | Why it stays out |
| --- | --- |
| `link-window` / `unlink-window` | Linked windows and session groups are skipped permanently (drop-in plan decision 3). One window belongs to one session. |
| `new-pane` | Creates a tmux floating pane by default. zz has no floating-pane mux model; the phase-1 picker verb was renamed off `new-pane` so the name stays tmux's. |
| `switch-mode` | Targets an ordinary pane and installs tmux's switch mode. zz's native mode ownership has no equivalent command transition. |

# Flag-level gaps on implemented commands: 2026-08-22 snapshot

Being cataloged is not the whole contract: 23 of the 83 implemented tmux commands still
reject 85 flags the pin accepts, answering `unsupported command: <cmd> -X` rc 1 (and
counting as config skips). The catalog declares the unsupported pairs and the mux/daemon
parsers enforce them. Inventory as of 2026-08-22 (flags in pin spelling; flags marked † are
gated by decision 3 or by missing context/model support, the rest are plain work):

| Command | Rejected flags |
| --- | --- |
| `attach-session` | `-c -f -x`; `-E` † |
| `break-pane` | `-W -x -y -X -Y` † |
| `capture-pane` | `-C -F -H -L -P -R` |
| `choose-buffer` | `-F -k -y` |
| `choose-tree` | `-F -h -k -y`; `-G` † |
| `clear-history` | `-H` |
| `command-prompt` | `-F -l -t`; `-P` † |
| `copy-mode` | `-k -s`; `-S` † |
| `detach-client` | `-t -P`; `-E` † |
| `display-message` | `-a -c -N -v`; `-I` † |
| `kill-session` | `-g` † |
| `list-keys` | `-1 -O -r` |
| `load-buffer` | `-w` |
| `move-pane` | `-D -L -P -R -U -X -Y -z -M` (floating-pane placement) † |
| `new-session` | `-X -f`; `-t` † |
| `resize-pane` | `-M -T` |
| `resize-window` | `-a -A` |
| `select-pane` | `-d -e -g -M -m -P` |
| `send-keys` | `-c -K -R`; `-M` † |
| `set-buffer` | `-w` |
| `show-messages` | `-J`; `-T -t` † |
| `source-file` | `-t -n -v` |
| `split-window` | `-k -m -R -s -S -T -W`; `-I` † |

`list-commands` prints each command's usage with zz's accepted flags, so the rows above are
the exact catalog-declared places its output differs from the pin's.

This table preserves the 2026-08-22 roster. Do not update it as a live ledger.
`python3 compat/tmux-tracker.py check` validates the registry and evidence;
`cargo test -p zz-mux compat_manifest` checks current Rust structural gaps against it.
`python3 compat/tmux-tracker.py write-report` publishes the current roster in the generated gap
report. `just compat-check` runs the full gate.

## Accepted grammar divergence evidence

The catalog count does not include syntax zz accepts or parses before diverging:

- The default prefix table has 60 bindings against the pin's 92, with 59 overlapping keys. zz
  adds `e -> send-last-output`, omits 33 stock keys, and deliberately maps `%`/`"` to
  `split-picker` plus `s`/`w` to `focus-sidebar`. Explicit imported commands retain tmux meaning;
  the exact default delta lives in [key tables](/tmux/key-tables.md).
- `refresh-client` rejects bare redraw, `-S`, `-c -D -L -R -U -l -r`, and the optional
  adjustment positional. `-A -B -C -f -F -t` behave.
- `load-buffer -` rejects stdin, `save-buffer -` rejects stdout, and `source-file -` rejects
  stdin.
- Relative `load-buffer` and `save-buffer` paths use the persistent daemon's cwd. tmux routes file
  access through the invoking client, using a command client's cwd or an attached session cwd. The
  remote and attached-client close remains under `buffers.client-file-context`.
- `show-buffer` refuses non-UTF-8 buffer bytes.
- `capture-pane` now routes like the pin: `-p` prints and ignores `-b`; without `-p`, `-b` selects
  a named paste buffer and bare capture creates an automatic one, including the final newline in
  stored bytes. Clustered value flags such as `-pS -3`, `-pS0`, `-pb name`, and `-pE5` follow
  tmux: a value option consumes the rest of its cluster. Numeric `-S`/`-E`, raw `-`, inclusive and
  reversed bounds, target-scoped format expansion, and tmux's silent invalid/out-of-range fallback
  match.
  One text residue remains: when `-E` falls back to visible end and the viewport has trailing blank
  rows, tmux emits those rows as newlines while zz stops after the last retained content row. `-T`
  is inert. Capture stays UTF-8 text/VT output; there is no retained saved-alternate grid, pending
  raw-byte stream, raw-grid dump, hyperlink list, line-flag prefix, or line-number prefix. The last
  six forms stay loudly rejected as `-P`/`-C`/`-R`/`-H`/`-F`/`-L` rather than being approximated.
- `copy-mode` (every flag, including bare) exits 1 with `pane is not attached: %N` when no
  client is attached to the target pane, where the pin sets the mode regardless — the mode
  lives on the pane in tmux and on the per-client terminal view in zz
  (`MuxEffect::TerminalView` resolves a target client and errors on an empty set). The same
  gate keeps `choose-tree`/`choose-buffer` at `choose-* requires an interactive client`. This
  is why no copy-mode or chooser behavior can ride the differential corpus, which drives both
  sides through a bare CLI against a headless server: every step would diverge on exit class
  before any flag mattered. Found 2026-08-22 while trying to add a `copy-mode -H` scenario.
- `choose-tree`/`choose-buffer` accept one `-N` as a no-op — zz's choosers are native
  surfaces with no preview pane, so "no preview" is already their only layout — and reject a
  repeated `-N` as `unsupported command: <cmd> -NN`, the pin's `MODE_TREE_PREVIEW_BIG`
  (`args_has(args, 'N') > 1` in `mode_tree_start`), which has no zz presentation. `-K` expands
  per row, and the two sides discard different expansions: the pin drops what
  `key_string_lookup_string` cannot PARSE (`KEYC_UNKNOWN` -> `KEYC_NONE`), never testing
  whether anything can press the result, while zz drops what falls outside its own input
  vocabulary (`zz_protocol::is_key_name`, defined as exactly the grammar `input_key_name`
  emits). zz's gate is strictly the narrower one, so a spelling tmux parses but zz has no
  keystroke for — `M-C-a` and other orderings, `Space`, key names zz does not model — is
  drawn by the pin and blank in zz. Conservative by choice: a key zz could never deliver
  would be a dead shortcut.
- `command-prompt` never comma-splits `-p` or `-I`, so `-p 'a,b'` raises ONE prompt labelled
  `a,b` where the pin chains two and feeds their answers to `%1` and `%2`. zz's behaviour is
  exactly the pin's `-l`, which is why `-l` stays REJECTED rather than accepted as a no-op:
  accepting it would advertise an opt-out from a chain zz cannot run. `cmd_command_prompt_exec`
  builds `cdata->prompts[]` by `strsep(&next_prompt, ",")` and
  `cmd_command_prompt_callback` walks `cdata->current` toward `cdata->count`; zz has neither.
  `-F` stays rejected for the same honesty reason: the pin expands the template through
  `format_single_from_target`, and zz's only prompt-side expander (`expand_prompt_input`)
  understands `#S` and `#W` and nothing else, so accepting `-F` would silently drop every
  other format. `-t` stays rejected with the rest of the client-fanout contract.
- `command-prompt` draws a different LABEL from the pin in two of three cases, found while
  measuring the mode flags and left alone because D1's brief required zero visible change for
  prompts using none of the new flags. `cmd_command_prompt_exec` appends a trailing space to
  every label it builds (`xasprintf(&tmp, "%s ", prompt)`) EXCEPT the bare `:` default, and
  when there is no `-p` but there IS a template it labels the prompt `(<first command name>) `
  rather than `:`. Measured: `-p lbl` then `q` drew `lbl q`, a bare `command-prompt
  'display-message ...'` drew `(display-message) q`, and a bare `command-prompt` drew `:q`. zz
  draws `lblq`, `:q` and `:q`. Cosmetic, three lines to fix in `MuxEngine::command_prompt`, and
  a natural pickup for the F error/label tranche.
- `command-prompt -N`'s pass-through runs in the opposite order to the pin. Both sides submit
  the collected digits AND process the non-digit key normally; the pin's cmdq runs the passed
  key's binding first (measured: a `-N` prompt fed `1`, `2`, `z` with `z` bound logged
  `ZBOUND` before `NUM[12]`, because `cmd_command_prompt_callback` uses
  `cmdq_insert_after` on the already-waiting item while the key callback is appended), and zz
  submits first because `input_command_prompt_key` completes before it answers "pass". zz has
  no command queue to reproduce the interleave, and the observable contract — both commands
  run — holds.
- `command-prompt -k` answers with `input_key_name`, which spells a space as `" "` where the
  pin's `key_string_lookup_key` spells it `Space`. Same narrow-vocabulary rule as the chooser
  `-K` gutter above.
- `command-prompt` edits with emacs keys only. The pin routes every prompt key through
  `prompt_translate_key` first when `status-keys` is `vi`, and `tmux.c:543-554` rewrites
  `status-keys` AND `mode-keys` to `vi` at startup whenever the basename of `$VISUAL`/`$EDITOR`
  contains `vi` — so a developer box with `EDITOR=nvim` silently runs a vi prompt even though
  the options table default is emacs. zz's prompt has no vi mode. Pre-existing; recorded here
  because it is why every pin measurement in this row had to `set -g status-keys emacs` first,
  and because it makes `status-keys`/`mode-keys` defaults environment-dependent in a way zz
  does not reproduce at all. The sharpest symptom: in vi mode Escape enters
  `PROMPT_COMMANDMODE` instead of closing the prompt, so `command-prompt` on such a box has no
  one-key cancel at all. Note also that `prompt_draw` branches only on `PROMPT_COMMANDMODE`
  and `PROMPT_QUOTENEXT` — none of D1's mode flags change how a prompt is DRAWN, which is why
  the TUI's status-line prompt needed no rendering change to stay faithful and only the GPUI
  palette, a zz-native modal, adjusted its affordances.
- The freeze a message raises is DERIVED in zz and LATCHED in the pin, and they part company in
  one place. `status_message_clear` only drops `TTY_FREEZE` `if (c->prompt == NULL)`, so
  clearing a message while ANY prompt is open leaves the client frozen — including a
  `command-prompt -C` or `-i` prompt that explicitly asked not to freeze. Measured on the
  nested rig: with a `-C` prompt open the view ticked `TICK62` to `TICK77`, a `display-message
  -d 0` stalled it, the dismissing key delivered exactly one catch-up frame (`TICK91`) and then
  NOTHING until the prompt closed (`TICK144`). zz's `client_terminal_publication_frozen` reads
  `message.freeze || prompt.freeze` fresh on every publication, so retiring the message
  resumes the client immediately. This is the prompt path's analogue of the
  `display-message -N` stickiness recorded for Wave D run 3, and zz diverges deliberately:
  reproducing it would mean latching a flag zz has no other reason to keep.
- `list-keys <key>` rejects the positional key filter.
- `send-keys -H` rejects bytes `80` through `ff`.
- Bind-time validation checks shared names and catalog flags, including daemon-native long options,
  but still misses complete positional arity and target validation.

## `send-keys` copy-action grammar

The outer `send-keys` parser rejects `-C`, `-P`, and `-o` with the pin's `command send-keys:
unknown flag -X` shape. The window-copy parser owns those names after the action. It recognizes `-C`
and `-P` on the pin's 14 copy-family grammar entries, recognizes `-o` on `next-prompt` and
`previous-prompt`, splits a `-CP` cluster, and honors `--`. A local parse failure returns success
without running an action and resets the copy-mode repeat prefix to 1. The catalog, mux tests, and
`micro-flags` scenario cover the two parser boundaries. `terminal.key-control` retains execution for
`copy-line`, `copy-line-and-cancel`, `copy-pipe-line`, and `copy-pipe-line-and-cancel` under
`semantic:send-keys-copy-command-shape`. The pin also redraws the first copy-mode line after a local
parser failure. zz emits the repeat reset without a no-op redraw action, so the same item owns that
presentation residue.

Read-only authorization checks whether `-X` was present before full option validation, so a
non-copy request such as unsupported `-M` answers `client is read-only` instead of exposing the later
parser error. Pin-recognized unsafe commands that zz does not implement, including the copy-line,
selection-mode, scroll-exit, and unsupported search forms, still reject at authorization. Empty and
genuinely unknown `-X` commands remain safe at this layer and follow their ordinary no-op or no-mode
path. A direct request authorizes only that invocation; a stored binding list is preflighted as one
all-or-nothing chain before any effect, matching the pinned command-list check.

## Former Wave 2e ownership (27 pairs)

The old plan grouped these commands under a TUI tranche. Source tracing shows that most of
the work belongs to the server:

| Command family | Server/core | Client/presentation | Parked on missing context/model |
| --- | --- | --- | --- |
| `command-prompt` | `-F -l -t` | ~~`-1 -C -e -i -k -N -T`~~ shipped | `-P` pane-rendered prompt |
| `copy-mode` | `-k -s` | . | `-S` bound mouse-slider context |
| `send-keys` | `-c -K -R` | . | `-M` originating mouse event |
| `display-message` | `-a -c -N -v` | . | `-I` CLI stdin/protocol stream |
| chooser residue | tree `-F -h -k -y`; buffer `-F -k -y` | . | tree `-G` session groups |
| `display-panes` | ~~`-N -t`~~ shipped | . | . |
| `show-messages` | `-J` | . | `-T -t` TTY capability model |

That leaves **20 server/core**, **0 client/presentation**, and **7 parked** pairs, and the three
numbers reconcile against the enforced roster above command by command. Wave D run 1 shipped
`-K`/`-N` on both choosers out of the middle column, run 3 shipped `display-message -d` out
of the first and `-C` out of the second, and the final run cleared `command-prompt`'s whole
middle column — the mode flags turned out to be daemon-owned rather than client work, which
is why they landed with a state machine in `zz-daemon` and only a key-relay branch in each
client. **The middle column is now empty**, so the TUI queue this table existed to define is
closed and everything left in it belongs to F and G. Moving the first column into a client
queue would duplicate daemon-owned command semantics in a client.

# Implemented-surface measurements through 2026-08-24

| Where | Divergence | Loud or silent? |
| --- | --- | --- |
| `find-window` | Detached CLI calls validate the target and return success with no output, including for zero matches. zz does not open tmux's attached-client window-tree chooser. | **silent**, bounded |
| `list-commands` | zz lists implemented commands in tmux's line format. Each usage string reports zz's accepted flags, so affected rows differ from the pin. Unimplemented commands stay absent so feature probes can take their fallback path. | **silent**, deliberate |
| `list-keys` selection and formatting | `list-keys -F` expands the pin's `notes_only`, `key_repeat`, `key_note`, `key_prefix`, `key_table`, canonical `key_string`, quoted `key_command`, repeat-set, and width facts. Literal stored space bases render as `Space` or `C-Space`; widths use that spelling, and positional filtering compares base, type, and modifiers while excluding stored spelling and flags. Bare output uses the pin's global padding. `-N` chooses `prefix` then `root` unless `-T` names one table, filters on note presence, and uses command text when the note value is empty; `-a` disables that filter and `-P` supplies a literal displayed prefix. The positional key filter, `-1`, `-O`, and `-r` follow the pinned ordering and error precedence, including per-table `-N` sorting and attached-client status routing for `-1`. Stock `copy-mode` and `copy-mode-vi` bindings now carry no repeat bits; copy-mode repetition remains runtime state. | none outside the bounded sort ties below and the separately tracked long key-modifier aliases |
| Copy-mode vi numeric prefix | The default `1` through `9` bindings keep zz's per-client `copy-mode-repeat` command shape rather than opening tmux's pane-cell `command-prompt -NP`. Digits, including a following `0`, accumulate to 9,999. The first `send` or `send-keys` command whose option prefix contains `-X` consumes the count. Its own `-N` wins; otherwise zz inserts separate `-N <count>` arguments immediately before the option argument containing `-X`. The engine does not scan onward after a stored `-N`, and a binding with no qualifying `-X` leaves the count armed. Prefix-consuming movements, jumps, matching brackets, and repeat-search run N times; `other-end` swaps only for odd N; `select-line` spans N lines; the copy-end-of-line family spans N rows and copies once; other toggles, selection, copy, clear-selection, cancel, and later actions run once. Bare `0` remains start-of-line. Direct terminal `send-keys -N` accepts the pin's full 1 through UINT_MAX range and stops on input backpressure. Native browser sinks cap repeats at 9,999 because tmux has no browser pane. | behavior closed; the visible and `list-keys` command-shape difference plus the buffered-prefix and browser caps are accepted under `keys.copy-mode-native-numeric-prefix`, `keys.copy-mode-action-and-repeat-fidelity`, and `terminal.key-control` |
| Copy-mode fixed viewport rows | `top-line`, `middle-line`, and `bottom-line` set column zero and place the cursor at the current frozen viewport's top, middle, or bottom row without moving that viewport. Targets clamp to the retained revision. | closed for these three placements; `history-bottom`, logical lines, wrapping, scrolling, and wider action semantics remain under `copy-mode.action-fidelity` |
| Copy-mode action vocabulary | The pinned `window-copy` table contains 95 action names. zz maps 66 to typed mux and terminal behavior. The remaining 29 stay classified under seven semantic items: action vocabulary, cursor geometry, logical-line and mode-key behavior, goto-line, selection lifecycle, jump/page/prompt actions, and copy formatting and destination effects. Seven absent default keys depend on five of those actions. | tracked under `copy-mode.action-fidelity`; `keys.copy-mode-unsupported-default-actions` owns only the seven default keys |
| `list-keys` sort ties | Pinned tmux's comparator truncates key identity and returns non-total results for equal-base modifier/type ties, cross-table ties, and fields that do not apply to bindings, so libc `qsort` may reorder those rows. zz keeps a total deterministic order: `-O key` compares tmux's low-32-bit base identity first, including packed two- and three-byte UTF-8, then type, modifier bits, flags, canonical spelling, table, and original traversal index. Four-byte Unicode uses its scalar value as a stable fallback because tmux's packed value does not fit the retained low 32 bits. Stable distinct-base `key`, `order`, reverse, and per-table `-N` cases remain differential-tested. | **silent**, deliberate and bounded to comparator ties plus four-byte Unicode |
| Long key-modifier aliases | zz accepts `Ctrl-` and `Alt-` anywhere its shared key parser accepts `C-` and `M-`, then canonicalizes them to the short spelling. Pinned tmux accepts case-insensitive short spellings such as `c-b`, but rejects the long aliases with `bad key`. This affects key options such as `prefix2` plus bind/unbind and other key-taking commands. | loud on the pin, permissive in zz |
| `#{config_files}` default discovery | Explicit startup `-f` paths are retained in order and comma-joined like the pin, and later `source-file` calls do not append. `reload-config` selects zz's current default candidate and replaces the retained fact with that path or empty. Without `-f`, pinned tmux lists every expanded default candidate whether or not it exists and does not canonicalize it; zz lists only the first existing zz-owned mux config, or empty when none exists. | **silent**, deliberate config-ownership boundary |
| `refresh-client` | `-A`/`-B`/`-C`/`-f`/`-F`/`-t` behave (phase 6: flow control, subscriptions, control-client sizing). `-C` is Control's explicit geometry path; Control does not emit the TUI-only `ClientTerminalSize` message. Bare redraw, `-S`, and the attached-client redraw/scroll family (`-c -D -L -R -U -l -r` plus the optional positional adjustment) answer `unsupported command: refresh-client interactive behavior`; detached command clients with no target get the pin's exact `no current client`. | loud |
| Supported client-selector targets | Every implemented tmux command flag that selects an attached client uses one matcher: `detach-client -t`, `switch-client -c`, `display-message -c`, `display-panes -t`, `display-popup -c`, `display-menu -c`, `confirm-before -t`, `refresh-client -t`, `lock-client -t`, and `load-buffer -t`. It accepts an exact client name, full tty, or tty after removing exactly one leading `/dev/` prefix, with exactly one optional trailing colon. It does not accept a final pathname basename, so `/dev/pts/3` admits `pts/3` but not `3` unless that is the exact client name. Collisions choose the globally oldest attached client by creation id, independent of session switches. The shared `device-N` alias remains a zz extension; popup, menu, confirm, refresh, and lock also retain numeric `N` and `client-N` aliases. A local terminal surface or Command client publishes `client-tty-v1:` whenever the tty is discoverable, independently from the additive `client-nested-v1` marker that a nonempty `$TMUX` enables. A local Control client now publishes the same identity from terminal stdin only; piped stdin and remote endpoints omit the caller-host tty. The daemon keeps this tty as internal selector and nested-attach state; it does not expose `#{client_tty}` through `ClientFormatFacts`. Unsupported `command-prompt -t`, `show-messages -t`, `send-keys -c`, and `suspend-client -t`, plus inert `set-buffer -t`, are outside this closure. | none on the common tmux-compatible selector shapes; native aliases remain zz extensions, and the unsupported or inert command flags keep their existing owners |
| Local Control terminal identity | Closed 2026-08-25 without a wire bump. A local Control hello carries its bounded cwd, `client-tty-v1:` only when stdin has a discoverable tty, and `client-nested-v1` only for a nonempty `$TMUX`. It never samples size, sends `client-size-v1:` or `ClientTerminalSize`, infers geometry, retains TERM or terminal-name facts, or exposes its tty through `ClientFormatFacts`; `refresh-client -C` remains the explicit Control geometry path. The established `attach-session`, `new-session -A`, and `new-session -Ad` refusal paths require the marker plus an exact pane-tty match when they would attach an existing session. Fresh `new-session` and `-A` misses still create and attach, while duplicate and validation errors keep their existing precedence. Piped stdin is not nested-refused merely because `$TMUX` is set. Registration cleanup already removes both retained facts. The sequential daemon suite passed 600/600, the focused Control CLI suite passed 30/30, and debug build, strict clippy, and fmt passed. The complete attached-client differential passed for zz and pinned tmux, including piped stdin, and independent review found the fresh-marker harness sound. This is not a canonical-suite claim. | none for local Control tty identity, nested intent, refusal gating, or fresh-session behavior; broader attach sizing remains under `clients.attach-context`, and `clients.context-formats` remains open |
| Read-only clients (`attach-session -r`, `switch-client -r`) | The daemon accounts a raw terminal `Key` and resolves it through the client's ordinary key tables, allowing the pin's `CMD_READONLY` command roster (`attach-session`, `copy-mode`, `detach-client`, `list-clients`, `send-keys`, `switch-client`) while still dropping PTY forwarding; other commands answer the pin's `client is read-only`. `send-keys` keeps the pin's second authorization layer: absence of `-X` is decided before full option and repeat parsing, so even unsupported `-M` reports read-only first. With `-X`, typed read-only-safe movement, history, line, word, paragraph, prompt, bracket, goto-line, set-mark, jump-to-mark, and cancel actions work. Selection, copying, search, rectangle, jump capture, and the pin-recognized but zz-unimplemented unsafe copy-line, selection-mode, scroll-exit, and search names reject; genuinely unknown and empty actions retain the pin's later no-op or no-mode path. A direct request authorizes its one invocation. A stored binding list expands one alias layer and preflights the whole list as one all-or-nothing chain before any effect, matching the pin. A read-only local view effect cannot fan into another session's clients. Raw keys bypass retained choosers, command prompts, and `display-panes`, as tmux's writable-only prequeue does. Direct local scrolling and copy-mode entry/navigation work, update activity plus latest geometry once, and preserve bells. Paste, clear-history, raw mouse, mixed wheel, and application pane Focus remain blocked; rejected non-focus native actions, including mouse, still account once, retain the modal, and preserve the bell. Standalone terminal `Text` accounts once without writing the PTY or clearing the bell, while matching text after a key adds no second update. Browser key/text, divider resize, popup/menu/confirm actions, uploads, and agent prompts remain dropped. `client_flags` reports `read-only` without the pin's coupled `ignore-size` because zz sizes every client individually, so the pin's `CLIENT_IGNORESIZE` half of `-r` has no zz meaning. The pin's same-uid check on re-marking a read-only client is skipped by the single-user daemon. | **silent** for native dropped-input feedback; unsafe commands and bindings are loud; uncoupled ignore-size and same-uid policy are deliberate |
| Session activity core | `session_activity` retains Unix seconds, starts at `session_created`, and refreshes through the shared same- or other-session attach funnel and queued terminal input. Ordinary read-only `Key` messages and rejected read-only terminal-view input, including raw mouse motion, refresh before rejection and advance latest geometry without clearing bells. Writable chooser raw keys, dedicated actions, and terminal-view input refresh activity and advance latest geometry exactly once without clearing bells; activating another session then records the target attach as a second boundary. Read-only-safe local view actions bypass a retained chooser or `display-panes` overlay, reach the pane, and use the same once-only accounting. One bounded ordered queue per client correlates pane-and-lane Key-plus-Text pairs, so a match uses the Key result with no second update. Standalone writable terminal or browser text accounts after modal consumption; standalone read-only terminal text accounts without PTY input or a bell clear. Writable command-prompt consumption and valid `display-panes` selection do not refresh activity. An unmatched display-panes key, Escape, non-hover mouse action, or wheel closes the overlay and falls through ordinary input. Bare buttonless hover Motion remains consumed as a native presentation choice, and timeout fabricates no activity. Native client-theme notifications, resize, `switch-client -T`, and detached commands do not refresh activity. `S/t` and `list-sessions -O activity` use a separate logical MRU counter, so same-second activity still reorders deterministically with session name as the exact-tie break. | none for the closed core, chooser routing, committed text, modal accounting, and display-panes fallthrough edges; native browser input outside modal consumption retains zz's deliberate superset activity behavior |
| Session client focus | The `ClientFocus` shape introduced in protocol v73 separates client-window focus from pane/application focus. GPUI seeds desired focus only when construction finds an active window, leaves inactive construction unset until the first activation callback, and replays the latest value once after every successful attachment epoch; pane and sidebar transitions do not update it. The TUI assumes its outer terminal is initially foregrounded, caches focus changes while attachment is pending, and sends the latest client focus once after every successful `Attached` event. Real outer focus events additionally emit pane Focus only for an active terminal; attachment never synthesizes pane focus. iOS sends its current scene state after the initial attach and every successful session or recovery attachment request, without replaying pane focus; scene transitions pair client focus with pane Focus when it retains a terminal input owner. Every successful attach independently advances latest geometry and recalculates affected panes, even with `focus-events` off. When that option is on, both client-focus directions update session and client activity exactly once, including read-only clients. FocusIn also becomes the geometry owner: `window-size latest` takes its rows, columns, and cell metrics, while manual, largest, and smallest keep their mode-correct rows and columns but refresh its cell metrics. FocusOut preserves the owner. Writable pane Focus alone still forwards to the application but changes neither activity nor geometry, so a paired client and pane signal touches activity once. Read-only pane Focus is rejected; its client-window transition travels through `ClientFocus` instead. Neither focus signal clears bells; the client-focus path is inert while the server option is off. A zz-side two-client regression with different retained geometries proves same-session attach ownership without focus events and mirrors the pinned FocusIn/latest rows-and-columns rule. `ClientFocus` is not CLI-drivable, so that half is not a differential-harness proof. The separate read-only fixture proves that zz accepts the notification and updates activity; it does not prove tmux `attach -r` resize behavior because tmux couples read-only with `ignore-size` and zz does not. | none for attach latest, the client signal, writable FocusIn/latest, or writable modal routing; the uncoupled read-only/ignore-size model remains under `clients.read-only-and-focus` |
| Session focus through writable overlays | With `focus-events` on, the daemon runs `ClientFocus` through the pinned writable prequeue before activity accounting. It dismisses the active status message and resumes frozen terminal publication, then closes `display-panes` and cancels its deadline. Key prompts submit `FocusIn` or `FocusOut` text and consume the transition. Numeric prompts submit without recording history and pass it; Text, Single, Incremental, and BackspaceExit prompts consume it and stay open. Choose-tree and choose-buffer keep their pane-mode routing. Read-only clients retain every modal and message while accounting both directions. FocusIn alone advances latest geometry; when that also changes an activity-sorted chooser, the daemon publishes the snapshot and independently refreshes the chooser. Neither direction clears bells. After accounting, writable focus dispatches synthetic `Any` through choose-tree, choose-buffer, active copy or command-output mode, then effective root. A transient binding wins; an unbound transient table falls back without retiring the mode. Read-only focus authorizes the complete selected binding before any effect. Disabled focus bypasses both accounting and dispatch. Exact `FocusIn` and `FocusOut` stay invalid key names. | behavior closed in focused daemon and protocol tests; pane `command-prompt -P` remains under `prompt.pane-rendered`; `ClientFocus` is not CLI-drivable, so there is no differential or canonical-suite claim |
| Session activity text edge | Closed 2026-08-25. The daemon keeps one 32-entry ordered queue per client for validated press or repeat keys whose `text_follows` bit is set. Each entry records its pane and Terminal or BrowserSurface lane. Text scans forward to the first same-pane, same-lane entry, retires only the skipped prefix, and consumes the match while preserving its suffix. Empty matching Text is inert and retires linked suppression; a no-match Text leaves the queue intact and is standalone. A two-browser-pane regression proves a skipped entry cannot retire the later bound key's suppression debt before its Text arrives, and bounded eviction retires debt on the evicted entry. Terminal command-output text accounts before it is swallowed; browser command-output text is consumed before activity. Detach, unregister or reconnect, and successful wire Attach clear the queue; a synchronous binding-driven switch preserves it. GPUI terminal standalone text and GPUI browser key-plus-text emission are source-tested. TUI keys remain unpaired; FFI exposes the explicit pair bit, and iOS uses standalone text plus unpaired key calls. | closed; a pair contributes at most one update, while writable modal consumption may contribute zero and read-only browser text retains its native silent drop |
| Session activity wake lifecycle | Pinned tmux refreshes activity when a suspended tty client receives `MSG_WAKEUP` or `MSG_UNLOCK`. zz has no suspended attached-client state or corresponding protocol message; reconnect and reattach use the ordinary attach seam. | accepted native lifecycle difference under `formats.session-activity-wake-lifecycle` |
| `switch-client -E` | Accepted as a no-op because zz does not retain the attaching client's environment; the same missing model already bounds session `update-environment` seeding. | **silent**, bounded |
| Session current window across clients | tmux stores one current window on the session, so `select-window` or `switch-client -t session:window` moves every client attached there. zz keeps `focused_windows` per client by design. One client can change windows without moving its peers, and peer rows from `list-clients -F '#{window_index}'` can differ from the pin. | **silent**, deliberate zz extension |
| `#{client_flags}` unmodeled flags | zz emits `attached` and every modeled control/read-only flag in the pin's sequence. Protocol v73 consumes client-focus notifications for activity and geometry ownership but does not retain a current focused boolean, so `focused` remains absent. It also omits the pin's `UTF-8` client flag because UTF-8 is a fixed protocol contract rather than client state. | **silent**, bounded |
| `copy-mode` | `-k -S -s` rejected (`-e`/`-q`/`-M` and `-H` are implemented). | loud |
| `capture-pane` residue | Stdout versus named/automatic-buffer routing, stored trailing newlines, clustered value flags, and inclusive/reversed `-S`/`-E` ranges are differential-clean since 2026-08-23. Bounds expand in the target pane's format context; invalid or out-of-range values silently fall back to visible start/end like the pin. When an invalid `-E` reaches trailing blank viewport rows, tmux emits one newline per row while zz stops at the last retained content row. `-T` remains inert; saved-alternate capture, raw pending/grid bytes, hyperlinks, line flags, and line numbers remain outside the model. | **silent**, bounded for `-T` and trailing blank rows; loud for the six rejected transports |
| Top-level `source-file` paths and CLI diagnostics | Since protocol v72, each eligible local caller publishes one bounded daemon-host cwd; SSH callers publish none, and a non-UTF-8 or oversized cwd is omitted so the client can still connect. `-F` expands each declared path independently in the command's resolved pane context, then top-level relative paths are prefixed with a glob-escaped caller cwd before globbing. `-t` resolves that pane once and supplies it to both `-F` and replayed commands without changing the source cwd; a missing target follows `CMD_FIND_CANFAIL` and loads with an empty target context. `-n` parses the complete file without applying environment assignments or commands, while retaining lexer diagnostics and optional verbose output. zz does not yet perform tmux's full command-name, flag, and arity validation during that parse. `-v` emits canonical `path:line: command` groups in declared-path, glob, and physical-line order, inherits through nested sources, and stays suppressed for Control clients. Command clients receive verbose stdout. Interactive clients receive Info events, not tmux's exact attached view-mode presentation. On Unix, zz quotes the cwd bytewise and calls `glob(3)` with flags zero like the pin. Backslash escaping, leading-dot exclusion, ordinary repeated-star behavior, malformed-pattern handling, and C-locale per-pattern order therefore agree. Declared paths retain caller order, and a quiet miss does not stop later paths. This matches an unattached tmux command client with a representable cwd, including literal glob metacharacters in the cwd. `source-file` also matches the pin's tilde boundary: the config parser may expand a leading tilde before command execution, but a tilde that reaches path resolution literally remains cwd-relative even when daemon HOME contains glob metacharacters. For commands issued by an attached client, pinned tmux selects the session cwd while zz currently selects the cwd from that client's hello; the common case agrees because both point at the attach directory, but a deliberately different client and session cwd remains tracked under `clients.attach-context`. Invalid-line diagnostics append to STDOUT as `path:line: message` in encounter order with duplicates kept and exit 1; a loud glob miss writes `No such file or directory: <declared path>` to STDERR and exits 1; a quiet miss stays silent at rc 0; mixed input populates both streams at rc 1. The zz-only `skipped N unsupported tmux command(s): …` summary goes to STDERR at rc 0. Interactive clients receive the declared-path warning. A direct all-miss Control invocation prints its diagnostics inside `%error` and stops the rest of that input line. If at least one declared path matches, the direct errors stay inside a `%end` frame and the line continues. Matched parser diagnostics use `%config-error` and also let the line continue. Native Windows keeps the Rust matcher: recursive `**` and its escaping rules are a zz platform extension because tmux has no native Windows oracle. `-` stdin is refused loudly on stderr at rc 1. | none for Unix glob matching, ordinary representable cwd prefixing, cwd escaping, literal tilde handling, declared-path order, no-effect parsing, pane targeting, verbose Command output, Control suppression, CLI-stream wording, and direct Control framing; native Windows intentionally keeps its own glob dialect; full parse-time command validation, exact attached verbose presentation, non-UTF-8 cwd omission, and attached client-versus-session cwd selection remain bounded; stdin remains loud |
| Nested `source-file` base | Pinned tmux repeats `server_client_get_cwd` for each nested `source-file`, so `a/entry.conf` containing `source-file leaf.conf` reads `<client cwd>/leaf.conf`. zz now snapshots the base selected for a registered client's top-level source and passes it through recursive replay. The nested source keeps that base after an ordinary sourced command executes through `ClientId::MAX` and clears the mutable execution-context cwd. Runtime `source-file` forwards the snapshot when it loads the active default `zz/mux.conf` through the ordinary path; direct zz-native `reload-config` forwards it through the separate native reset path. Startup keeps its separate clientless bootstrap gap. The differential fixture includes a containing-file decoy. CLI fixtures use a caller cwd with spaces and glob metacharacters and cover ordinary replay, active-default ordinary loading, and direct native reload with a decoy beside `mux.conf`. Command, Control, and Interactive clients share this daemon path. Exact attached session-cwd selection remains under `clients.attach-context`; this close proves nested reuse of the selected base, not equality between the client hello cwd and the session cwd. Ordinary replay still runs through the sentinel client. Successful Command and attached output are tracked under `config.replayed-command-output`; hooks that source again are tracked under `source-file.sourced-hook-client-cwd`; the closed nested queue proof covers cross-depth Control ordering. | none for registered-client nested rebasing, active-default ordinary loading, direct native reload, or per-command sourced guards; startup bootstrap, attached base selection, successful replay output, and sourced-hook cwd remain in those named groups |
| Sourced-command hook `source-file` base | Pinned tmux copies the original queue client onto each command loaded from a file. A hook raised by one of those commands inherits that client and its cwd. zz executes the sourced command as `ClientId::MAX`; the hook starts a new sentinel-client source invocation and cannot use the stable base carried by its containing replay. | **silent**, tracked under `source-file.sourced-hook-client-cwd` |
| Event-hook `source-file` base | Command and immediate hooks retain the invoking client on both sides. Deferred event hooks differ: zz executes them with its sentinel client and can fall back to home, while tmux selects the current or best attached client and uses that client's session cwd. | **silent**, tracked under `source-file.event-hook-client-cwd` after `clients.attach-context` |
| Startup `source-file` base | Pinned tmux keeps `cfg_client` available until startup configuration finishes, so nested relative sources use the launching client's cwd. zz loads startup configuration before a client registers, then keeps its existing containing-file fallback through that clientless replay. | **silent**, tracked under the separate `source-file.startup-client-cwd` bootstrap gap |
| Control frames during sourced config replay | Protocol v76 carries one tail-tag-47 `SourcedCommandGuard` for each replayed command that survives command-name resolution. Unknown or ambiguous command names and malformed alias names publish a located Warning that Control renders as `%config-error`, without a guard. Control writes each guard as a flags-1 frame after the direct outer frame: ordinary success and quiet all-miss use an empty `%end`, mixed hit and miss keeps its diagnostic inside `%end`, and all-miss, flag or arity failure, runtime failure, or depth refusal ends `%error`. Runtime failures alone set `client_failure`, so a later clean detach cannot erase exit 1. Guards defer FIFO without leaking into the next command. Matched read failures follow as typed standalone Error events; other config and lexer Warning prose still uses the config classifier. The existing loader preflights every declared path for one source command before recursion. A focused regression and strict six-step differential prove the root missing-path guard, middle missing-path guard, then leaf output guard order, each exactly once, with no production change. | guard shape, termination, runtime-failure stickiness, FIFO deferral, and cross-depth containing-before-child order behave for resolved command names; source-file Control process status remains under `control-mode.source-file-exit-status` |
| Control source-file process exit status | A matched source replay can complete with a nonzero `CommandResponse::Success` while source-command guards correctly leave `client_failure` false. The Control front end renders that result but does not retain it in every long-lived stdin order. Pinned behavior is exit 1 when failed replay completion is followed by EOF, exit 0 when an explicit `detach-client` is read after replay has completed, and exit 1 when `detach-client` plus EOF were already queued while replay waited. This is source-file result and EOF ordering, not globally sticky diagnostics. | loud process-status mismatch, tracked under `control-mode.source-file-exit-status` |
| Nested `source-file` depth | Measured 2026-08-24 and guard placement closed 2026-08-25. Counting the initial `source-file` as invocation 1, both sides run 50 concurrent source invocations and refuse invocation 51 before any of its paths are matched or loaded. Command stderr is `too many nested files` at rc 1, an attached tty shows the capitalized `Too many nested files`, and Control carries the same lowercase text inside the rejected nested command's own flags-1 `%begin`/`%error` guard while the outer typed line continues. `-q` does not suppress the refusal, one diagnostic covers a refused command rather than each of its paths, the refused paths are never globbed or loaded, and the containing file keeps executing its later physical lines. The refused source's own same-line `;` sibling is dropped on both sides, while the matched parent `source-file` stays on the asynchronous wait path and therefore runs its own same-line sibling on both sides. A malformed invocation at the refused depth is diagnosed as malformed rather than as depth on both sides: the pin rejects it while parsing the containing file and never consults its depth guard, and zz reaches the same precedence by running the depth guard after the command's own flag and positional validation. Only that precedence, the stdout stream, and the rc-1 exit are closed there: the pin prints `command source-file: too few arguments (need at least 1)` and `command source-file: unknown flag -Z` where zz prints `source-file needs a path` and `source-file does not support -Z`, and the pin then abandons the rest of the containing file where zz continues it. | the depth wording, count, `-q`, per-command granularity, guard placement, later-line continuation, and same-line removal behave; malformed text remains under `mux.error-shapes`, file abort remains under `config.parser-edge-cases` |
| Startup `source-file` depth accounting and causes | Both sides share one cumulative 50-command source budget across every startup root. Top-level roots do not count; quiet misses consume slots; one command with many paths consumes one slot; command 51 and later retain `<file>:<line>: too many nested files`; later ordinary commands continue. Runtime sequential source commands stay unbounded. zz's native `reload-config` replays the whole root under one fresh startup budget of its own, so a reload lands the state a fresh start would; the pin has no reload command and its `cfg_finished` gate never re-opens the cumulative budget after startup. zz records the located cause in its startup report and log, then discards the report. tmux retains those clientless causes: initial Control prints `%config-error` before its first `%begin`, a later Control attach receives them inside its attach frame, and a normal attached client opens the cause view. | accounting behaves; client delivery and placement are tracked in `config.startup-diagnostic-delivery` |
| Control-mode asynchronous exit diagnostics | For `run-shell 'exit 3'`, both sides close the command with `%end` and continue the same input line. tmux prints `'exit 3' returned 3` unframed after `%end`; zz carries that text inside the completed response frame. | loud, tracked in `control-mode.async-command-output` |
| Control-mode diagnostic identity | Protocol v76 puts source-command diagnostics into the command's `SourcedCommandGuard`; termination and `client_failure` are explicit fields rather than prose classifications. Matched source-read failures follow the parent guard as typed standalone Error events, so invalid UTF-8, numeric OS errors, and colon-space paths do not depend on wording or pathname shape. Background inserted-command failures addressed to a Control client use the same Error channel with raw daemon error text; only Interactive status messages capitalize the first character. Gesture, prompt, paste, and command-output Error producers remain Interactive-only. Copy-pipe worker failures notify Interactive clients but stay silent on Control because they carry no request identity; exact asynchronous delivery awaits a pinned probe. Config summaries and lexer-owned diagnostics still travel as generic Warning events and use the `%config-error` prose classifier. The known-family Warning fallback remains for legacy producers, while the exact protocol handshake rejects v75 and v76 client-daemon skew before either event path can mix. | source typing and sourced-command guards behave; config identity remains under `control-mode.diagnostic-typing`; copy-pipe delivery remains under `control-mode.async-copy-pipe-errors` |
| Config parse-abort semantics | Pinned tmux aborts a sourced file at the first parser diagnostic, discards that file's command list, and emits one cause. zz records every invalid line and continues applying valid commands from the file. CLI stdout/stderr routing is exact for the diagnostics zz produces, but the number of diagnostics and resulting mutations can differ. | **silent** when later valid commands apply; output differs when a file has multiple invalid lines |
| Same-line command groups in sourced files | Measured 2026-08-24. Both sides key replay groups by the parser-owned source and physical line. A synchronous invalid or runtime command error, a depth-refused nested source, and a loud `source-file` no-match or glob error with zero files drop only the later `;` siblings from that group; the next physical line still runs, and a quiet no-match is success. A matched `source-file` takes the asynchronous wait path, so child runtime, parser, and read failures plus a mixed missing-and-matched invocation do not prune the parent line; zz retains a child read failure in its load report. A pinned directory-read probe returned rc 1 with `Input/output error: <path>` while both the parent same-line and next-line markers ran. An asynchronously failing `run-shell` also leaves its sibling. Both sides keep the same-line sibling for a `-` path, while zz's stdin transport gap remains under `protocol.binary-streams`. Equal line numbers from separate files do not collide. zz-classified unsupported capability gaps now skip and continue later same-line siblings; before this slice they pruned those siblings. That new continuation is desirable for zz import capability gaps, but it has no pinned proof because the corresponding commands are unsupported in zz. Control prepares a complete input line and aborts a preparation error before effects. Against an already-running compatible daemon, the local CLI scans its complete prepared vector before stdin capture, attach or TUI routing, and execution. A later unknown name has the pinned error shape and prevents every earlier effect. A malformed alias body also prevents effects, but its loud unknown-command shape is zz-selected while `aliases.command-bodies` remains open. Cold or failed preparation falls open to static routing, so an autospawn verb can still run before a later unknown command. Runtime failures keep earlier effects and prune later commands. Local flag, arity, and other argument validation plus config and source-file replay remain dispatch-at-a-time under `mux.chain-parse-abort`; replay alias observation remains under `aliases.config-parse-unit`. | pinned continuation behavior is proved for the supported error cases above and live-daemon unknown-name CLI preparation; cold CLI chains, local argument validation, and replay groups remain open under `mux.chain-parse-abort`; UnsupportedCommand continuation is new and pin-unproven; sourced guard placement behaves; matched-read wording and parser abort behavior remain under `mux.error-shapes` and `config.parser-edge-cases`; cross-depth order closed separately |
| Runtime failures of replayed commands | Closed 2026-08-25 against pinned tmux. Config replay records runtime failures in encounter order. A missing `kill-session` target and a well-formed `set-option` with an unknown name emit the pin's bare text on Command stderr at rc 1, inside the replayed command's Control `%error` guard with `client_failure` set, and as capitalized attached warnings. Later physical lines still run, and an outer `source-file` propagates the inner error and nonzero status without blocking inner or outer continuation. Unknown command names and malformed set-option syntax keep the existing file-prefixed parse-diagnostic path. Clientless startup remains separate. | none for the adopted error channel, exit status, sourced guard, ordering, and continuation; startup delivery remains under `config.startup-diagnostic-delivery` |
| Output of successful replayed commands | Measured 2026-08-24; Control guard output closed 2026-08-25. Pinned tmux copies the invoking item state onto every command a file loads, so `cmdq_print` output from a sourced `display-message -p` or `list-sessions` reaches the invoking client in file order on Command stdout, on the Control output channel, and in the attached view, while a clientless startup load builds a fresh empty state and prints nothing. zz captures successful output inside each protocol v76 sourced guard for Control. It still drops that output for Command and attached clients, and collects every `-v` line before replay instead of preserving physical interleaving. | Control guard output and frame boundaries behave; Command stdout, attached presentation, and `-v` interleaving remain under `config.replayed-command-output` |
| Active default config in a multi-file `source-file` | Closed 2026-08-25. Runtime `source-file` loads every active-default match through the ordinary declared-order loader instead of entering native reload. Declared default, after, and default paths apply as `DAD`; a loud miss returns status 1 without stopping later matches; and ordinary diagnostics plus `-v` lines retain declared path and glob order. Explicit zz-native `reload-config` still rediscovers the first existing candidate, replaces `#{config_files}`, resets key tables, rebuilds appearance, and reapplies stored mux overrides. Startup first-existing discovery and ordered explicit `-f` roots remain intentional; parse-only and nested paths are unchanged. Focused CLI and daemon tests, strict clippy, fmt, and the 12-step diagnostics, 40-step format, and six-step Control differential pass with zero differences and no skips. This makes no canonical-suite claim. | none on runtime active-default ordering; native reload and startup retain their documented zz-owned behavior |
| `mouse` / `escape-time` | Behaving since 2026-08-21 (Wave B2/B3). zz-tui gates the outer-terminal mouse modes (`?1003h`/`?1006h`/`?1016h`) on the session-effective `Mouse` value from the v71 publication, emits/retracts them live on `MuxOptionsChanged` (the pin's default is on: the reference builds with `-DTMUX_MOUSE=1`), and the daemon drops mouse-originated `TerminalView` input from terminal-surface clients when the effective value is off; the GUI's native mouse stays ungated per decision 6. With the option off, an application inside a pane can still use the mouse exactly as the pin documents (`options-table.c` mouse help; `server-client.c` forward_key): the outer modes also follow the active pane's own `mouse_tracking`, events forward straight to the tracking pane under the cursor with every chrome branch skipped, and the daemon admits them for panes whose app requested tracking. Chrome mouse (status clicks, sidebar, dividers, focus clicks) remains available only while the option is on — matching the pin, whose mouse key bindings also fire only then. `escape-time` replaces the TUI's old 25 ms escape fold timeout (pin default 10 ms, 0 clamps to 1 like `tty_keys_next`). Both keys are config-writable through `MuxOptionKey::from_config_key` with the standard reload-reapply semantics. | none — behaving |
| `set-titles` empty expansion | With `set-titles on` and a `set-titles-string` that expands to the empty string, zz publishes an empty `StatusLine.title`: the GUI reverts to its native title and zz-tui writes no OSC, where the pin's `server_client_set_title` would set an empty terminal title. Empty doubles as the "option off" wire state, so this narrow edge is deliberate. | **silent**, narrow and deliberate |
| `automatic-rename` / `automatic-rename-format` | Runtime command changes update `Window.name` while automatic rename is on, and the configured format is expanded with pane facts. Explicit `rename-window`, `new-window -n`, or a named first window pins a window-local `off`. zz refreshes when its runtime fact changes rather than on tmux's 500 ms timer, so a process transition that the sampler has not observed can lag differently. | **silent**, bounded timing residue |
| `aggressive-resize` + `window-size` | Since 2026-08-20 `aggressive-resize` is a candidate FILTER (ON = clients focused on the window; OFF = zz's viewer set, a per-client-focus stand-in for the pin's linked-window `session_has`) and `window-size` is the AGGREGATION policy. `latest` picks the most-recent-input owner, while `largest`/`smallest` aggregate componentwise. Manual sizing now has higher precedence than either: `resize-window` stores the durable layout extent, selects a local `window-size manual`, and client measurements cannot overwrite it. Switching an already resized window away from manual and back uses its then-current layout in zz; tmux retains a separate last-manual extent. | **silent**, bounded only across a manual → automatic → manual transition |
| `resize-window` client-derived and out-of-range forms | `-x`/`-y`, `-L`/`-R`/`-U`/`-D`, their shared positive adjustment, target precedence, manual formats, and effective geometry match the pin for the supported 1..=10,000 cell surface. `-A` and `-a` are loud because they require deriving the largest or smallest size from attached clients. For a relative adjustment that drives the requested manual size outside 1..=10,000, both clamp the effective window geometry, but tmux's separate `window_manual_width`/`window_manual_height` fact can expose the unclamped request while zz reports the durable effective layout extent. | loud on `-A`/`-a`; **silent**, bounded outside practical cell limits |
| Client targeting and requested detach | Closed 2026-08-25 against pinned `cmd-find.c`, `cmd-detach-client.c`, and `server-client.c`. One daemon resolver serves `detach-client` and `switch-client`: explicit targets use the supported matcher above before any `-s` lookup. Targetless Interactive and Control commands select themselves; a Command client first uses the best client on its origin pane's session, then the best client on the most recently active attached session. `detach-client -s` wins over `-a` and a missing source session quietly does nothing; `-a` detaches every peer except the resolved target. Explicit `-t` resolves before `-s`, including its error. Read-only clients may detach only themselves. Requested detach carries the existing Requested reason without a by-client; `attach-session -d` keeps Evicted. Local terminal surfaces publish their real tty even outside nested tmux, while SSH clients omit the caller-host tty. The later `clients.tty-basename-targeting` closure aligned every supported selector caller, removed final-basename matching, and fixed global creation-order collision precedence. The later local-Control closure extended only stdin-backed tty identity and nonempty-`$TMUX` intent to that client kind. Sequential daemon coverage passed 598/598. Focused selector tests, a debug build, strict daemon clippy, and fmt passed. Scoped zz and pinned-tmux tty guards passed, but the full attached-client harness later blocked on unrelated nested-attach interleaving, so this is not a full-harness or canonical-suite claim. `detach-client -E` and the parent-HUP actions `attach-session -x`, attaching `new-session -X`, and `detach-client -P` remain separate gaps. | none on bare, `-a`, `-s`, supported selectors, requested event classification, or eviction classification |
| `display-message` client selection and format fallback | Closed 2026-08-25 against pinned `cmd-display-message.c` and `cmd-find.c`. A nonprinting call sends its message to the attached client selected by the common matcher above; a missing destination stays quiet. The separate `-t` or default pane owns pane, window, session, and non-client format facts, but not inherited duration. CANFAIL retains every resolved component before a miss: a missing session leaves client and target facts empty, a valid session with a bad window falls forward to its current window and active pane, and a valid window with a bad pane falls forward to its active pane. Client facts use the `-c` destination only when it belongs to that retained target session. Otherwise an attached target session uses its most-active client. For a valid unattached target, an absent `-c`, a destination attached to another session, or an unresolved `-c` selector widens to the globally most-active attached client, with the oldest-created client winning an activity tie. `client_session` comes from that selected client's actual attachment; session, window, and pane facts remain target-scoped. Zero attached clients leave client facts empty. An attached target with no `-c` still leaves client facts empty under `clients.context-formats`. Nonprinting stays quiet and successful, while `-p` expands the retained or empty context through the caller and never arms message state. Delivery, duration selection, printing lifecycle, buffer-path context, and Command-client selection do not use the global fallback. Interactive destinations still own replacement, deadline, freeze, and sticky `-N` state. Control destinations receive the event without that state, and read-only destinations can receive it; read-only callers still fail before execution. Sequential daemon tests pass 599/599. Focused activity and zero-client tests plus scoped fallback probes for zz and pinned tmux pass. One independent run completed the attached-client harness, but later current runs passed the scoped fallback probes and then failed at unrelated nested-attach terminal-query interleaving. The full harness is not stably green, and this close makes no canonical-suite claim. Bare `-t =` and `{mouse}` still need bound mouse context, and relative or special pane targets remain incomplete. The mux carries the selector only in its internal effect, so this adds no wire field. | none on the supported `-c` matcher, componentwise CANFAIL surface, or valid-unattached global format fallback; attached-target facts without `-c`, mouse target context, and relative or special grammar remain tracked |
| `display-time` | Status-message toasts consume the configured milliseconds, and since Wave D run 3 (2026-08-22) the daemon owns that timer: `display-message` without `-d` reads the destination client's attached session value exactly like the pin's `status_message_set` with `delay == -1`, independently from the pane session selected by `-t`; `-d` overrides it per invocation. A zero installs no deadline and waits for writable input. A non-release key press, a nonempty bulk `Text` packet that survives bound-key suppression, an explicit paste, non-hover mouse or wheel input, or enabled writable `ClientFocus` retires the active message before downstream dispatch. Bare hover deliberately remains a zz presentation-only no-op, and every read-only input leaves the message armed. Suppressed trailing text from a binding cannot erase the message that binding raised. `display-message -N` now matches the pin's sticky client flag: a positive-effective-duration ordinary Interactive message writes it, with `-N` setting the bit and a plain message clearing it. A positive-duration Interactive `PrintOrMessage` producer such as `list-keys -1` also clears it. Explicit or inherited zero duration, clear, expiry, printing, Control clients, and a missing destination leave it unchanged. While the bit and an active message coexist, writable terminal Key, standalone or paired `Text`, Paste, non-hover mouse and wheel input, and `ClientFocus` stop before message dismissal, display-panes teardown, prompt handling, dispatch, or activity accounting. An ignored release retires a swallowed press decision without forwarding. The committed-text queue matches the first entry with the same pane and input lane, uses the committed character from the key, retires the skipped queue prefix and its linked debt, then retires the matched debt while preserving the later suffix. Browser-before-terminal ordering therefore preserves the later terminal debt, while terminal-before-browser ordering retires the skipped terminal debt as stale. Read-only input and native browser-surface input keep their prior paths. Alert-produced visual messages still publish client-timed events without a daemon record, so zz does not freeze terminal publication, reset the sticky bit, or share replacement, expiry, zero-duration, and input-dismissal behavior. The pin passes `no_freeze = 0` and `ignore_keys = 0` from `alerts.c` into `status_message_set`, which sets `TTY_FREEZE` and makes the alert dismissible. Since 2026-08-20 the omitted `display-panes -d` duration comes from `display-panes-time` like the pin. | ordinary `display-message` lifecycle and `-N` behave; bare hover is an intentional native adaptation; alert messages remain **silent** under `alerts.message-lifecycle` |
| `respawn-pane` / `respawn-window` | Dead panes revive with stable pane identity; `respawn-window` keeps its first pane and removes the rest. `-E`, `-k`, `-c`, repeated `-e NAME=VALUE`, and stored command/cwd reuse are implemented. | none known on the cataloged surface |
| Array options | Since the 2026-08-20 Lane-2 sweep all eight real array options (`command-alias`, `codepoint-widths`, `user-keys`, `terminal-overrides`, `terminal-features`, `status-format`, `pane-colours`, `update-environment`) store with the pin's separators, hole reuse, and `name[N]`/`-u name[N]` semantics, and the 68 hook names route to the hook table. Since the B1 server slice (2026-08-21) `status-format[]` drives the daemon's personalized `StatusLine.rows` production (sparse indices publish blank rows, a session array overrides the global one whole, scoped writes refresh that session's attached clients). Wave C added two more consumers (2026-08-21): `command-alias[]` expands one layer before canonical lookup at both dispatch chokepoints (`MuxEngine::resolve_command_alias`), and, like the pin's parse-time expansion, `bind-key`/`set-hook`/`default-client-command` STORE the expansion, so `list-keys` and `show-hooks` print `list-windows` for an aliased `lsw` on both servers (differentially pinned). Aliases nested inside a `{ … }` argument of a stored command expand at execution instead of at store time, so their stored text keeps the alias name. Writable stored bindings resolve each command immediately before dispatch, so an earlier command may change the alias seen by the next; a failure uses the ordinary command-output and `key_command_failed` warning path. Read-only clients instead resolve and authorize the whole stored chain before any effect. Protocol v74 Control and the local ordinary CLI prepare each complete argv unit under one daemon lock and freeze that one alias layer. The CLI uses the returned canonical identity and alias-match bit for attach, stdin, and kill recovery routing and carries the vector unchanged across a TUI reconnect. Every prepared command is reauthorized during execution, so alias shadowing is not an authorization control. Remote `--host` preparation remains under `aliases.remote-client-preflight`. Only SINGLE-command alias bodies expand: the pin also accepts a multi-command body (`x=cmd1 ; cmd2`, caller arguments appended to the last, `cmd-parse.c:2317`) and an empty body (silent rc 0), where zz refuses both with `unknown command: <alias>` rc 1 — loud rather than silent, per doctrine, because zz's dispatch chokepoint executes exactly one command. The resolver distinguishes no exact alias, one supported expansion, and a matched empty, multi-command, or unparsable body. Every mux and daemon caller refuses the matched-unsupported case before canonical lookup, catalog-alias lookup, read-only authorization, or stored-binding dispatch, so shadows such as `kill-server=`, `list-windows=cmd1 ; cmd2`, and `lsw=cmd1 ; cmd2` cannot fall through to the command they hide. Alias lookup is exact on the typed name in both (a command prefix like `ls` never reaches the alias table). `update-environment[]` drives `seed_session_environment` plus its own readback. The remaining five still drive nothing. Indexed `@`/table scalars follow tmux (`not an array` on set; indexed show reads the scalar). | **silent**, store-only except `status-format[]`, `command-alias[]`, and `update-environment[]`; multi/empty execution remains under `aliases.command-bodies` |
| Status-row window-option scoping | **Closed 2026-08-22 (Wave C run 3).** `StatusRowVariables` now layers each window's explicit overrides (the `WindowStatusOption` set plus the window-scoped renderer styles) and each session's explicit session-scoped values over the global map, and `DaemonFormatHooks::variable` consults the loop item's `session_id`/`window_id` context before the flat map — exactly the loop-item seam `Expander::lookup` already fed. Both surfaces now agree: `per_window_status_overrides_reach_the_label_surface_and_row_variables` pins the engine seam, `per_window_status_overrides_style_the_rendered_row` pins the rendered rows, the `cli_binary` PTY smoke is back to its original `setw -t styled:0` shape end-to-end, and the `renderer-styles` differential scenario pins `setw -t w mode-style` reaching `display-message -p '#{mode-style}'` in the target window's context like the pin's `format_expand` option walk. Command-path expansion (`display-message -p`, the execute-time hooks) resolves the same injected option names; option names outside the injected status/renderer set still do not resolve as formats. | none — behaving (residue: only the injected names resolve as formats) |
| Renderer-style residue (C9) | Only the COLOUR halves behave: `window-style`/`window-active-style` patch each pane's default fg/bg (attributes, `dim`, and the styles' `#()` shell branches stay inert; the appearance seam expands conditionals with context-only hooks), and `pane-border-style`/`pane-active-border-style` publish one fg colour per pane for the raw TUI (`None` selects its normal fallback; non-colour border attributes and `bg` fills stay ledgered per the v71 contract). The GPUI client ignores those border fields and derives pane chrome from its local theme. `mode-style` colours the copy-mode selection (the pin's `copy-mode-selection-style` default chain) and the copy-mode match styles colour the GUI's search overlays through the published appearance. That appearance channel carries one global value, so `setw -t` per-window copy-mode/mode styles store but do not recolour. zz's copy-mode position indicator keeps its theme chrome (`copy-mode-position-style`/`-format` are store-only), the TUI flattens all overlays to reverse video, and `copy-mode-mark-style` resolves but paints nothing because zz renders no mark element. | **silent**, bounded |
| Border style owner granularity | The pin resolves the border style per BORDER CELL SPAN (`redraw_get_pane_for_border_style`, `screen-redraw.c:1108-1131`): an explicit owner, else the active pane **only when it is adjacent to that span** (`redraw_data_has_pane`), else `top`→`bottom`→`left`→`right`, else the window-scoped default. The zz TUI resolves one colour per whole divider and attributes the active style when the active pane is anywhere in the split's SUBTREE (`Divider.style_pane`, `crates/zz-tui/src/layout.rs`). They agree for flat two-pane splits and diverge from three panes up: with `A | (B / C)` and C active, tmux paints the outer divider's top half with the inactive style (neighbours A and B) and only its bottom half active, where zz paints the whole divider active. Inert until a config sets `pane-border-style`/`pane-active-border-style` (both publish `None` at defaults). Closing it needs per-segment dividers in the TUI layout model. | **silent**, bounded, opt-in |
| `display-panes` label presentation | The pin paints big numerals plus the expanded `display-panes-format` across the pane's top row in the `display-panes-colour` cell. zz expands the same format per pane into `PaneIndicator.label` (1 KiB cap) and paints it through the shared styled-segment path: the TUI composes it across the pane header row right of the selection-key badge (alignment and exact-width clipping via `compose_status_row`), the GUI as an alignment-bucketed top strip inside the indicator overlay clipped at the pane edge. The label's base colours stay theme-derived — `display-panes-colour`/`display-panes-active-colour` remain store-only — and zz keeps its native badge/card instead of the pin's numerals. | **silent**, bounded |
| `display-panes` queue blocking | With no `-b`, tmux returns `CMD_RETURN_WAIT` and resumes that client's command queue when the overlay closes. zz accepts `-b` but always returns immediately, so a command sequence continues while the overlay is still open. `-N`, client targeting, duration, and key fallthrough do not change this retained difference. | **silent**, bounded |
| Status-block suppression threshold | tmux hides the status line when `tty.sy <= statuslines` (resize.c `CLIENT_STATUSOFF`), so a 3-row terminal with `status 2` still shows both status rows plus one window row. zz panes carry a header row, so the TUI suppresses the block when `rows < statuslines + 2` (one header plus one content row must survive) — in that same 3-row terminal zz shows no status block and gives all rows to the pane. The GUI mirrors the rule against its measured canvas in line-height units. | **silent**, bounded |
| `history-limit` default | zz keeps 10,000 lines for its product default; the pin keeps 2,000. `show-options -g history-limit` prints the effective 10,000 value. | **silent**, deliberate |
| Plain option listings | No-argument listings contain tmux table names and `@` user names. The six zz-native settings stay available through explicit-name queries and never appear as unknown words in tmux-parsing scripts. | **silent**, zz extension hidden from tmux listings |
| Nested `new-session` validation precedence | On the pin's creation path, `cmd-new-session.c` checks a `-t` target combined with a command or `-n`, then validates the window name and session name, tries `-A` delegation, checks for a duplicate session, and validates an unresolved `-t` as a session-group name before reaching its nested-client guard. That last failure is `invalid session group name: <target>`. zz preflights every non-detached creation-plus-attach path before mux parsing or mutation and reports `sessions should be nested with care, unset $TMUX to force`; both reject without changing state, but the first error and wording differ. Every detached `-Ad` path is deferred until the mux either creates a detached session or returns a mutation-free Attach effect. That post-effect check restores command context on refusal, catches an existing expanded target, and avoids refusing a formatted miss merely because its raw format text names a literal session. | loud wording and precedence difference |
| Session environment updates | Both servers seed their global environment at boot. Since Wave C (2026-08-21) zz honors the stored `update-environment` array when creating a session, including unset markers for missing names. Creation-time `new-session -e NAME=VALUE` overlays that seed, persists on the session, reaches the first pane, and is last-wins; malformed values without `=` are ignored. Like the pin, an empty-name `=VALUE` entry remains visible in `show-environment` but is discarded at terminal spawn, and pane-local `new-window`/`split-window` overlays discard it at the same boundary. Creation-time `-E` skips the `update-environment` seed while retaining explicit `-e` values. On an existing `new-session -A` path, tmux and zz both ignore `-e`. The remaining gap is client context: tmux seeds from the creating or attaching client's environment (`cmd-new-session.c:282`, `cmd-attach-session.c:135`), while zz has no client-environment field and uses the daemon's boot environment. zz also never re-seeds an existing session on attach, so accepted `new-session -E` has no attach-path reseed to suppress and `attach-session -E` remains rejected. The pin's session-scoped `update-environment` value matters only on that missing attach-time path. zz matches the pin's glob-free names; the pin's `fnmatch` patterns are not expanded. | **silent**, bounded to client-sourced creation and attach-time reseeding |
| Lifecycle trio | `exit-empty`, `exit-unattached`, and `destroy-unattached` are inert until a config EXPLICITLY sets them (presence in the stored-scalar map, not the effective value): unset, zz keeps its persistent-daemon rule, `armed ∧ zero sessions ∧ zero subscribers`. Explicitly set, the pin's `server_loop` (`server.c:281-292`, whose client loop at `:289-292` is the check the subscriber clause below contrasts against) and `server_check_unattached` (`server-fn.c:481`) policies take over — enforced on client departure and command execution, where the pin re-evaluates every loop iteration — with one permanent divergence: the `zero subscribers` conjunct is LOAD-BEARING and survives every policy, because a zz GUI/TUI client can outlive its session where a tmux client cannot, so an attached client must never have the daemon die under it. "Attached" means present in `ServerState::attached` (a client bound to a session); "subscriber" means an Interactive or Control client holding an outbound mailbox. `exit-unattached on` therefore exits when no client is bound to a session AND no client is subscribed, where the pin needs only the former. Policies are also dormant inside the startup bracket so a boot config cannot kill the daemon it is configuring. `destroy-unattached=keep-last`/`keep-group` are decided by linked session groups in the pin (`session_group_contains`); zz has no session groups, so `keep-last` never destroys (every session is effectively the last of its group) and `keep-group` always destroys — both are exact for the ungrouped case, which is every zz session. Session groups stay the permanent compatibility skip. | **silent**, bounded, opt-in |
| Client-exit notices | Closed for zz-tui in protocol v70: requested/evicted detaches print `[detached (from session X)]` rc 0, a destroyed session with no survivor prints `[exited]` rc 0, shutdown prints `[server exited]` rc 1, and a lost connection prints `[server exited unexpectedly]` rc 1, all after terminal restoration. Native GUI and control-mode surfaces keep their existing presentation. | closed |
| In-UI error text width | Command errors surfaced inside the TUI render in the sidebar's 28-column status row, so a long tmux message truncates (`can't find window: 99` shows as `can't find win`). The message text itself is now the pin's, via one shared renderer. Collapsed-sidebar mode uses the full width. | **silent**, cosmetic |
| `#()` job environment | Closed by wave 7d: status jobs receive `TMUX=socket,pid,-1`, the pane working directory as `PWD`, and no `TMUX_PANE`, matching the pin's session-null status-job shape. | closed |
| Shell job environment overlay | Closed on the overlay half 2026-08-20: `run-shell`/`if-shell` jobs receive the global `set-environment` overlay and, when the job has a session, the session overlay — hidden entries withheld, child-unset markers removed — matching `environ_for_session`; this is what lets Oh My Tmux's `$TMUX_PROGRAM`-chained bootstrap run at all. Still divergent: jobs start from the daemon's environment rather than a clean one, the TERM family (`TERM`, `TERM_PROGRAM`, `COLORTERM`) is not synthesized, and status `#()` jobs get only `TMUX`/`PWD`. The smoke harness injects a canary so scenarios cannot accidentally depend on inherited host state. | **silent**, bounded |
| `#{version}` | zz reports `3.8-zz`, sharing the compatibility-version source used by `zz -V` (`tmux 3.8-zz`); the pin reports `next-3.8`. The suffix is deliberate so scripts can identify the compatible implementation without confusing it with upstream tmux. | **silent**, deliberate |
| Non-UTF-8 command arguments | tmux prints a byte such as `a\377b` with octal vis escaping. zz converts argv with `to_string_lossy` before escaping and prints `a<U+FFFD>b`. | **silent**, accepted edge |
| Config `~` expansion | Leading `~` of unquoted words and a `~` just inside an opening double quote expand to `$HOME` at parse time, matching the pin. Tildes inside single quotes, escaped tildes, and ordinary mid-word tildes stay literal on both sides. At one quote boundary they differ: the pin expands a tilde immediately after a closing single or double quote, while zz leaves it literal. `~user` forms also stay literal where the pin resolves them via `getpwnam`, and an unset/empty `HOME` leaves the `~` literal where the pin fails the line with a parse error. | **silent** edge |
| Command-name abbreviation | Closed 2026-08-24. Exact canonical names and aliases resolve first. Non-exact lookup searches the pinned tmux canonical namespace before the guarded 19-name native roster, so every pinned prefix keeps its tmux result while native names stay exact and noncolliding native abbreviations such as `capture-b` remain available. The manifest gate derives the roster from catalog minus oracle and checks every prefix of all 92 pinned names. A strict 29-step differential scenario covers the 25 prefixes that native names had changed, exact tmux aliases, a user `command-alias` named `split`, and ambiguous `list-commands` exit parity. The daemon resolves one alias layer before read-only authorization and reuses that invocation through dispatch and hooks. Writable stored bindings prepare each command immediately before dispatch, while read-only clients prepare and authorize the whole chain before any effect. Matched unsupported bodies refuse without falling through to canonical or catalog-alias lookup. Protocol v74 closes Control's former static unknown-name precheck: the client asks the daemon to prepare the whole initial argv unit or LF line under one lock before allocating frames, then executes the returned invocations without a second alias lookup and with normal authorization. Local ordinary CLI commands now use the same prepared canonical identity and alias-match state for attach, new-session, stdin, and kill recovery routing, including immutable TUI handoff. Remote `--host` preprocessing remains static under `aliases.remote-client-preflight`. Prefixes resolving to catalogued-but-unimplemented commands still answer `unsupported command: <canonical>`. | closed |
| `set prefix` key validation | zz rejects unresolvable bare keys with the pin's `bad key: <value>` but silently accepts unresolvable `C-`/`M-` keys the pin rejects (`C-zz`): a typo'd prefix is accepted and never fires. Full strictness needs the pin's `key_string_table` breadth (`^a` caret form, `BTab`, the KP family) — a partial tightening would loudly reject pin-valid keys instead, so this waits for a key-string parity wave. | **silent** edge |
| `resize-pane` direction amount metadata | Closed 2026-08-25 as a catalog-only reconciliation. Runtime already accepted bare `-D`/`-L`/`-R`/`-U` with amount 1 plus attached and separated integer amounts. The four catalog entries now mark their values optional, and the manifest compares that shape with the pin. No handler, effect, or wire contract changed. The strict 16-step `resize-directions` differential is clean with no skips. `-M` and `-T` remain open under their existing owners. | closed metadata gap; runtime unchanged |
| Error-shape residue (post-7b) | Grep-facing error classes are pin-bare and byte-exact since wave 7b (2026-08-18): the twelve `options-values.sh` regress strings, `can't find session/window/pane:`, `unknown command:`, `already set:`, `open terminal failed: not a terminal`, show-messages pairs, `%config-error <file>:<line>:`. Catalogued-but-unimplemented commands/options answer `unsupported command: <name>` — a zz-only condition the pin would instead run. Arity/flag rejections and usage fallbacks keep zz wording (`<cmd> does not support -X` vs the pin's `command <cmd>: unknown flag -X`; no `usage:` fallback) pending per-command arity metadata (7c). | loud |
| Alerts | Full alerts.c state behavior since the 2026-08-24 edge closure: `monitor-bell` gates the bell path, `monitor-activity` raises the activity flag from PTY output, and `monitor-silence` arms per-window daemon deadlines that re-arm on output and expiry. Every successful silence-option write, including a same-value write or repeated global reset, resets every live window timer; a missing local `-u` and a rejected `-o` do not. Window selection clears alerts and requeues activity like `session_set_current`. Attach clears bell, activity, and silence only on the session's active window and releases every terminal bell latch there before snapshotting. Alert action and label gating is evaluated once against that active window, then the same ring/message decision fans to every eligible Interactive client; zz's wider per-client focus model remains deliberate elsewhere. Flags surface through `#{window_flags}` (`#`/`!`/`~` in pin order), the window flag formats, `session_alert`/`session_alerts`, and `session_activity_flag`/`session_silence_flag`, whose misleading names mirror the resolved target window. Pinned alert messages freeze terminal publication through `status_message_set(..., no_freeze = 0)`. zz publishes them outside `ActiveClientMessage`, so it does not arm that freeze or share the rest of the daemon-owned lifecycle. | state behavior matches; message lifetime remains **silent** under `alerts.message-lifecycle` |
| `select-layout main-*` with 2 panes | The pin never sizes the lone "other" pane (layout-set.c:264-269, :458-463), leaving stale geometry that fails tmux's own `layout_check`; zz sizes it (80x24 → main 80x22 + other 80x1). Deliberate: zz refuses to reproduce an upstream bug. | **silent**, zz more correct |
| `select-layout -E` on a mixed parent | The pin spreads only leaf children (layout.c `layout_cell_is_tiled`) but divides the parent's full extent among them, so a parent mixing leaves with nested nodes gets corrupt sums (observed: 40+42+39 in an 80-wide window, last pane at xoff 84). Every later operation on that corrupted window keeps diverging: one `-E` produced four geometry divergences, three downstream, so the known scenario has one causal step but the divergence is not bounded to it. zz refuses that spread and stops the walk where the pin stops. All-leaf parents are exact (48 pin fixtures + `known/known-spread-mixed.txt`). | **silent**, zz more correct |
| `select-layout` strings with zero-sized leaves | The pin accepts a leaf with width or height zero. zz rejects it to preserve the `PANE_MINIMUM` invariant. | **loud**, zz more correct |
| `select-layout` strings with extents above `u16::MAX` | The pin accepts `70000x70000` through its `u_int` geometry. zz rejects it; `PANE_MAXIMUM` is 10000. | **loud** |
| `select-layout` strings with single-child nodes | The pin accepts a node with one child. zz requires every node to have at least two children. | **loud**, zz more correct |
| `select-layout` string validation order and depth | zz validates the whole string before trimming cells to the current pane count. The pin trims first and runs `layout_check` afterward, so a sum violation confined to a deleted cell fails only in zz. zz caps parsing at 256 levels and rejects a 300-deep string in bounded time; the live pin held 100% CPU for minutes on input around 100 levels deep. This validation-order edge is one-directional: zz never accepts a string that the pin rejects. | **loud** |
| Lone-pane `select-layout` strings | On an 80x24 one-pane window, the pin accepts `a8fd,120x30,0,0,0`, keeps `window_width=80`, and dumps `120x30` from its new root. zz adopts the encoded extent for both the window and layout. | **silent**, zz more correct |
| Detached `split-window -Z` hidden-pane width | Both servers zoom the post-spawn active pane, including the existing pane under `-d`, and a successful split without `-Z` clears zoom. Immediately after `split-window -dZ`, the pin also reports the newly created hidden pane at the full window width until the window unzooms (`layout_assign_pane` receives `SPAWN_ZOOM` before `window_pop_zoom`). zz reports that hidden pane's saved layout allocation while reporting only the active zoomed pane at full size. Active-pane and zoom flags match. | **silent**, bounded |
| `move-pane` on tiled panes | The pin reserves `move-pane` for floating targets and returns `pane is not floating` for a tiled target (cmd-join-pane.c:428-431). zz has no floating panes and keeps `move-pane` as an alias of `join-pane`. The accepted `-l` grammar, format expansion, and `-f` sizing basis match the shared pinned tiled-layout path, but zz still performs the move where the pin refuses it. | loud |
| Attached-GUI `#{pane_width}` | Formats report the engine's cell allocation while PTYs are still sized by client pixel measurement, so a drawn pane's format can drift a cell from `tput cols` until the client-reported window size lands. Headless is exact. | **silent**, bounded |
| `#{window_flags}` | zz emits `#` activity, `!` bell, `~` silence, `*` current, `-` last, and `Z` zoomed in tmux order (Wave C run 2 backed activity and silence). `M` marked remains absent because zz does not model the marked pane. | **silent**, `M` only |
| `#{command}` in command items | Closed 2026-08-24. Both sides treat `#{command}` as a command-queue-item fact rather than a list-row fact. tmux adds it in `cmdq_merge_formats` from the running item's `cmd_entry` name and then merges the item state over it; zz carries the canonical entry after alias and prefix resolution through the mux dispatch hooks and daemon-owned expansion, consulting explicit item state first. The daemon path covers run-shell shell and string `-C`, if-shell conditions, capture and pipe arguments, client and buffer list formats and filters, popup and menu presentation values, confirm string preparation, and the post-spawn `new-window`/`split-window -P -F` pass that adds live pane facts. Typed blocks skip the parent expansion, hook bodies report their own command beside the trigger `#{hook}`, and confirm prompts plus delayed Control subscriptions remain outside an item with an empty value. Popup argv and environment assignments remain raw. | closed; pinned by `command-item-format.txt` and the 24-step `daemon-command-item-format.txt` |
| Command-argument format and name expansion | The 2026-08-24 close covers five target-sensitive paths: `rename-session` and `rename-window` positional names, direct `show-options` and `show-window-options` optional names including `show-hooks` forwarding, and directional `select-pane -T`. The 2026-08-25 follow-ups close both `new-session` names, `new-window -n`, and both buffer file paths. `new-session -n` expands, validates, and vis-cleans before `-s`; `new-window -n` expands once in its destination session context with session format type before the same validation and cleaning; both rename paths use their resolved active pane and pane format type before applying that helper after one expansion. `break-pane -n` follows the pin's different rule: it stays literal, then validates and cleans before placement. Rust hooks and pinned source establish ASCII-control rejection and exact ordering, while the strict differential covers valid Unicode, empty expansion, backslash cleaning, cleaned collision or reuse, literal break-pane tokens, and rename parity. An unindexed `new-window -S` repeats format expansion over the cleaned first-pass value for lookup while preserving that first-pass value for creation, and an explicit `break-pane -n` disables window-local `automatic-rename` on both placement paths. `load-buffer` and `save-buffer` expand their path once before `~/` handling and I/O, with canonical command identity, explicit item-state precedence, and no second pass over replacement text. A valid `load-buffer -t` supplies its attached session, focused window, and active pane; a miss is quiet and falls back to the most-recent mux context. Targetless buffer paths select the invoking or best attached client before the same context fallback. Raw or produced `-` streams remain unsupported, and relative, attached-session, and remote file ownership remains under `buffers.client-file-context`. | ten expanded paths, literal break-pane naming, and both creation-name edges closed; client file context active |
| `send-keys -N` (no keys) | Arms the **invoking client's** count prefix; tmux stores it on the pane mode, so another client's (or a Command client's) `-N` is a silent no-op in zz. | **silent** edge |
| `send-keys -X` | The action-local `-C`/`-P`/`-o` grammar, `--`, parser-failure behavior, and repeat-prefix reset match the pin. zz still has no pane-owned mode entry, so it cannot return the pin's `not in a mode` error when no client copy view exists. | **silent** |
| `send-keys -H` | Bytes `80`–`ff` refused; tmux writes the raw byte (`KeyToken::Literal` carries UTF-8). | loud |
| Unguarded commands | Closed by the [drop-in plan](/designs/tmux-drop-in.md)'s phase 0: every engine command rejects options centrally from its catalog `CommandSpec` — flags tmux has at the pin but zz lacks error as unsupported (and count in config-import skip reports); flags tmux doesn't have error as invalid. The daemon-side `capture-pane`/buffer family shares the clustered value/flag parser, and since 2026-08-20 its 19 catalog specs carry the pin's full flag arity (accepted and `unsupported` alike), so renderers and hook variables read the right shapes. | loud |
| `bind-key` payloads | Bind-time validation covers shared names and flags, including daemon-native long options. Positional arity and target errors can still surface at keypress. tmux validates the full argument template at bind time. | **silent** edge |
| Empty-daemon command-query startup | zz autostarts its persistent daemon and `list-sessions` succeeds with empty output, while tmux reports `no server running on ...` when its server is absent. Once either server exists without sessions, explicit targetless `attach`/`attach-session` return `no sessions`, exit 1, and the first `new-session` gets name `0` and ids `$0`/`@0`/`%0` on both. | **silent** lifecycle difference |
| Bare launcher with a non-empty server | tmux with empty argv defaults to `new-session`, so it creates and attaches another numbered session. The installed zz launcher maps empty argv to `new-session -A`, so it attaches the current session instead. Empty-server behavior matches because both create and attach session zero. | **silent**, deliberate product-friendly launcher behavior |

## Format variables: 2026-08-22 snapshot

These names are registered so parsing matches the pinned 198-name table. Thirty-two are always
unavailable: 31 lack a backing seam and `client_termname` has a seam whose retained value is always
empty. Thirteen more have usable backing only with a client/list context and may be empty in an
ordinary pane expansion. The other 153 names have general state, runtime, hook, or defined default backing.
Each gap is separate on purpose.

| Variable | Missing backing | Loud or silent? |
| --- | --- | --- |
| `buffer_mode_format` | No tmux buffer-mode row formatter; zz's buffer chooser is native. | **silent** |
| `client_activity` | `list-clients -F` supplies the daemon's retained activity time; ordinary and status expansion do not inject it. | **silent**, context-only |
| `client_colours` | The attaching client's terminal color count is not fed into format expansion. | **silent** |
| `client_created` | Client creation time is not retained as a format fact. | **silent** |
| `client_flags` | `list-clients -F` and per-recipient status/title expansion supply modeled flags; an ordinary pane expansion has no client row. | **silent**, context-only |
| `client_height` | `list-clients -F` and per-recipient status/title expansion supply the client's current row count; an ordinary pane expansion has no client row. | **silent**, context-only |
| `client_key_table` | `list-clients -F` supplies the active per-client table; ordinary and status expansion do not inject it. | **silent**, context-only |
| `client_last_session` | `list-clients -F` supplies the previous live session; ordinary and status expansion do not inject it. | **silent**, context-only |
| `client_mode_format` | No tmux client-mode row formatter; zz's client surfaces are native. | **silent** |
| `client_name` | `list-clients` and per-recipient status/title expansion supply the registry name; an ordinary pane expansion has no client row. | **silent**, context-only |
| `client_readonly` | `list-clients -F` supplies whether the row's client is read-only; ordinary and status expansion do not inject it. | **silent**, context-only |
| `client_session` | `list-clients` and per-recipient status/title expansion supply the attachment; an ordinary pane expansion has no client row. | **silent**, context-only |
| `client_termfeatures` | Terminal feature negotiation is not represented as a tmux format string. | **silent** |
| `client_termname` | A client hook exists for list/status contexts, but the attaching client's `TERM` name is not retained, so its value is always empty. | **silent** |
| `client_termtype` | tmux's terminal-type classification has no zz equivalent. | **silent** |
| `client_theme` | `list-clients` and per-recipient status/title expansion supply the retained light/dark theme; an ordinary pane expansion has no client row. | **silent**, context-only |
| `client_tty` | Eligible local terminal surfaces and Command clients retain a tty internally for client selection and nested-attach checks. A local Control client retains stdin's tty when discoverable; remote endpoints and piped Control stdin omit it. `ClientFormatFacts` does not carry that retained value, so `#{client_tty}` remains unavailable in list, status, title, and ordinary format expansion. | **silent**, missing format plumbing rather than missing local selector state |
| `client_uid` | `list-clients -F` and per-recipient status/title expansion supply the daemon user's uid; an ordinary pane expansion has no client row. | **silent**, context-only and bounded |
| `client_user` | `list-clients` and per-recipient status/title expansion use the daemon user because the socket does not retain a separate attach-client user. | **silent**, context-only and bounded |
| `client_width` | `list-clients -F` and per-recipient status/title expansion supply the client's current column count; an ordinary pane expansion has no client row. | **silent**, context-only |
| `cursor_character` | The glyph under the terminal cursor is not mirrored into mux facts. | **silent** |
| `cursor_colour` | Cursor color is not mirrored into mux facts. | **silent** |
| `mouse_hyperlink` | Command formats do not receive tmux's mouse-event hyperlink record. | **silent** |
| `mouse_line` | Command formats do not receive tmux's mouse-event line text. | **silent** |
| `mouse_pane` | Command formats do not receive tmux's mouse-event pane id. | **silent** |
| `mouse_status_line` | Command formats do not receive tmux's mouse-event status-line index. | **silent** |
| `mouse_status_range` | Command formats do not receive tmux's mouse-event status range. | **silent** |
| `mouse_word` | Command formats do not receive tmux's mouse-event word text. | **silent** |
| `pane_bg` | The terminal cell background at the cursor is not mirrored into mux facts. | **silent** |
| `pane_fg` | The terminal cell foreground at the cursor is not mirrored into mux facts. | **silent** |
| `pane_key_mode` | Native copy/view mode is not projected as tmux's pane key mode. | **silent** |
| `pane_mode` | Native pane mode is not projected as tmux's mode name. | **silent** |
| `pane_pb_state` | Terminal progress-bar state is not mirrored into mux facts. | **silent** |
| `pane_search_string` | Native per-view search text is not mirrored into mux facts. | **silent** |
| `pane_tabs` | Terminal tab stops are not mirrored into mux facts. | **silent** |
| `session_group` | Session groups are unsupported, so no group name exists. | **silent** |
| `session_group_attached_list` | Session groups are unsupported, so no grouped attachment list exists. | **silent** |
| `session_group_list` | Session groups are unsupported, so no member list exists. | **silent** |
| `session_last_attached` | `list-clients -F` supplies the retained time for the row's session; ordinary and status expansion do not inject it. | **silent**, context-only |
| `session_path` | zz has no separate per-session working directory fact. | **silent** |
| `tree_mode_format` | No tmux tree-mode row formatter; zz's tree chooser is native. | **silent** |
| `window_activity` | Mux state tracks a monotonic activity point for `-O activity` list and chooser sorting, but does not expose a timestamp through this format variable; `W/t` retains window-index order. | **silent**, bounded |
| `window_offset_x` | Client viewport X offset is not fed into window formats. | **silent** |
| `window_offset_y` | Client viewport Y offset is not fed into window formats. | **silent** |

# Options coverage: 2026-08-22 snapshot

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

**105 behave**, meaning a value change is consumed somewhere outside set/show/inherit/
readback (consumer-traced 2026-08-20; nine status-production names joined in the B1 server
slice on 2026-08-21, `status-position` joined with B1's client half the same day,
`mouse`, `escape-time`, `set-titles`, and `set-titles-string` joined with the B2/B3/title
slice, and `command-alias`, `update-environment`, `exit-empty`, `exit-unattached`, and
`destroy-unattached` joined with the Wave C alias/environment/lifecycle slice, the
nine alert/prefix2 names joined with Wave C run 2, all 2026-08-21; `display-panes-format`
and the eight renderer styles joined with Wave C run 3 on 2026-08-22; and
`window-status-separator` joined through the daemon status-row renderer on 2026-08-24).
**75 are store-only.**
The earlier "78 behave" counted
options given a typed home in the honest-knobs/status structs, twelve of which nothing
read. `tmux_options::BEHAVES` distinguishes the consumer-traced names from storage-only
options and test-pins its count, uniqueness, and membership in the option catalog.
`tmux_stored_scalar` and `tmux_stored_array` storage is store-only by construction until a
consumer wave wires a name up and moves it into `BEHAVES` (B1 moved six stored scalars and
the `status-format` array).

**Behaving (105):**

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
  `visual-bell`; since Wave C run 2 (2026-08-21) the full alert set: `monitor-bell`,
  `monitor-activity`, `monitor-silence` (per-window daemon deadlines),
  `activity-action`/`silence-action`, `visual-activity`/`visual-silence`, and
  `window-status-activity-style` (see the Alerts row).
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
  `StatusLine` per client; the TUI paints the authoritative rows, while the GUI uses
  their list alignment as one input to native snapshot-backed window controls; see the
  Presentation row): `status-format[]` (sparse indices publish blank rows, session arrays
  override whole), `status-justify` (resolved inside the expanded row formats),
  `message-line` (published clamped to the row count; selects the row messages and the TUI
  prompt replace), `status-position` (the TUI shifts or shrinks its canvas for the block;
  GUI placement follows the app's chrome mode), `pane-status-style`,
  `pane-status-current-style`, `session-status-style`, `session-status-current-style`,
  `window-pane-status-format`, `window-pane-current-status-format` (all six resolve
  inside the default pane/session list rows), and `window-status-separator` (each nonfinal
  window item resolves its separator in item scope; exact row output remains TUI-only).
- Terminal surface and titles (B2/B3 + the C3 title source, 2026-08-21): `mouse` and
  `escape-time` (zz-tui consumes the session-effective values; the daemon rejects mouse
  input from terminal-surface clients when off — see the `mouse` / `escape-time` row),
  `set-titles` and `set-titles-string` (the daemon expands the title per client into
  `StatusLine.title`, publishing even with `status off`; zz-tui writes OSC 2 on non-empty
  changes and the GUI adopts the window title only when the option is explicitly on).
- Command aliasing, environment, and lifecycle (Wave C, 2026-08-21): `command-alias`
  (one layer before canonical lookup in standalone mux execution; daemon execution prepares
  one immutable layer before authorization and reuses it through dispatch and hooks; stored
  bindings prepare each command when it is about to run; bind-key/set-hook/option-command
  validation uses the same non-recursive rule as the pin's `CMD_PARSE_NOALIAS`),
  `update-environment` (drives `seed_session_environment` and its
  own readback), and the lifecycle trio `exit-empty`, `exit-unattached`,
  `destroy-unattached` — all three inert until EXPLICITLY set, and the
  "zero subscribers" guard survives every policy (see the Lifecycle trio row).
- Keys (Wave C run 2, 2026-08-21): `prefix2` — stored as a global-session scalar,
  synced into the shared `KeyTables` so either prefix arms the prefix table, published
  through `MuxOptionKey::Prefix2` and config-writable via `from_config_key`;
  `send-prefix -2` sends it and is a silent success while it is unset, like the pin.
- Renderer styles and the display-panes label (Wave C run 3, 2026-08-22):
  `display-panes-format` (the daemon expands it separately in each pane's context into
  `PaneIndicator.label`, format-expanded but not strftime-expanded like the pin's
  `format_single`; both clients parse the styled segments, honor `#[align=…]`, and clip),
  `window-style`/`window-active-style` (colour halves feed the per-pane appearance bridge:
  the daemon patches each pane's terminal fg/bg defaults with the pin's per-channel
  active-over-base fallback, re-resolving on option writes, selection, and relocation),
  `pane-border-style`/`pane-active-border-style` (explicit colours resolve pane → window →
  global during personalized snapshot stamping into the v71 `PaneSnapshot` fields; the TUI
  colours dividers and pane headers, while the GPUI client keeps its pane chrome under the
  zz theme), `mode-style` (copy-mode selection colours,
  matching the pin's `copy-mode-selection-style` default chain), and
  `copy-mode-match-style`/`copy-mode-current-match-style` (search overlay colours through
  the published appearance) — plus `copy-mode-mark-style`, resolved at the same seam but
  visually inert because zz renders no mark (see the Renderer-styles row). All nine also
  resolve as option-name formats and per-window/per-session layers in status rows and
  `display-message -p` (see the closed scoping row).

**Store-only (75):**

- Typed storage that nothing reads (31): `lock-after-time`,
  `lock-command` (the lock commands are no-ops); `allow-rename`, `alternate-screen`,
  `scroll-on-clear`, `extended-keys`, `extended-keys-format`, `xterm-keys`, `backspace`,
  `editor`, `assume-paste-time`, `input-buffer-size`, `get-clipboard`,
  `default-client-command`, `fill-character`, `variation-selector-always-wide`;
  `message-style`, `message-command-style`, `message-format`;
  `pane-border-lines`, `pane-border-indicators`, the four `pane-scrollbars*`; the four
  `prompt-*cursor-*`; `clock-mode-colour`, `clock-mode-style`.
- Generic scalar storage (39 of the 63 scalar-backed names) plus five of the eight
  arrays: everything else in the table,
  including `remain-on-exit-format`, `status-keys`,
  `copy-mode-selection-style`, `copy-mode-position-style`,
  `display-panes-colour`/`display-panes-active-colour`,
  `terminal-overrides[]`, `terminal-features[]`, `user-keys[]`,
  `pane-colours[]`, `codepoint-widths[]`, the 21 theme-palette options, and the
  four `tree-mode-*` options. Lane assignments live in the drop-in plan's "options residue"
  section.

The index trio follows tmux's session/window inheritance, allocation, targeting, format,
and close-triggered renumbering behavior. `set-option` also accepts six zz-native names —
the agent/editor/history-trickle keys — which don't count toward tmux coverage and never
appear in the no-argument listings (those contain tmux table names and `@` names only).
`show-options` and `show-window-options` expose values with tmux's exact string escaping,
value-only and inherited forms. A no-name `show-options -H` keeps those rows and appends the
scope's stored hook arrays in option-table order; named hook queries do not require `-H`.
Free-form `@` names are pure string storage at server,
global-session, session, global-window, window, and pane scope, including append and unset;
this is the storage seam TPM and plugins use. Global and per-session environment overlays
have `set-environment`/`show-environment` readback and are merged into new terminal PTYs,
including hidden and child-unset entries; the daemon seeds the global map from its process
environment, and `new-session` copies the currently stored global `update-environment[]` names,
whose initial value is the pin default, or writes unset markers; creation-time `-e` overlays that
session map and `-E` skips the array seed. `automatic-rename` gates the desktop's active-pane label and explicit
window names install the pin's window-local `off`; its format string is evaluated by the
daemon-side label path only. `remain-on-exit` retains a frozen dead pane with live
`pane_dead` and normal-exit `pane_dead_status` facts, and the respawn commands revive that
stable pane slot. `default-terminal`, `display-time`, and `repeat-time` feed new PTYs,
client message/overlay timers, and each attached session's repeat-key window.
`list-keys` now matches the pin's selectors, key filtering, aggregate facts, stock copy-table repeat
metadata, global flags, table, and key-column padding. Only the deterministic comparator boundary
described above and the separately tracked long key-modifier spelling overacceptance remain.

# Protocol and process level

| Area | tmux | zz |
| --- | --- | --- |
| Env contract | `$TMUX`, `$TMUX_PANE`, plus server-seeded global and client-updated session overlays | Panes get `$TMUX` in tmux's exact `socket,pid,session` shape plus `TMUX_PANE=%N`; exec-family jobs get `$TMUX` without `TMUX_PANE`; wave 7d added status-job `TMUX=socket,pid,-1`, `PWD`, and no `TMUX_PANE`. `ZZ_PANE`/`ZZ_SESSION`/`ZZ_SOCKET` ride alongside panes. The remaining clean/session job-overlay divergence is listed above. |
| Binary argv | `-L -S -f -2 -C -u -V -N -c -l` | Closed by 7a (2026-08-18): `-V` (`tmux 3.8-zz`), `-L`/`-S`/`-f`/`-c`/`-N`/`-l`/`-2`/`-u`, tmux-shaped usage and unknown-option lines, pin CMD_STARTSERVER autostart. `-C`/`-CC` are the phase-6 control-mode front-end (row below). |
| Control mode `-CC` | What iTerm2 integration speaks. | SHIPPED (phase 6 complete 2026-08-18): a stdio front-end speaking the full CC protocol — framing, notifications, `%output` with flow control (pause/age-kill/pacing), `refresh-client -A/-B/-C/-f`. Deliberate divergences, all reviewer-endorsed: blocks are COMPLETE (WAIT commands keep output in-block; after-hooks add no extra block; `%pause`/`%continue` land after the triggering block, not inside); per-client monotonic `n`; zz-lax unquoted `%`-words on the control stdin; automatic-rename transients single-fire. |
| Session groups | `new-session -t`. | Cataloged, rejected. |
| `StatusLine.customized` | No equivalent — tmux has no wire and no explicit-write ledger. | zz-native v71 field: true while any explicit `status`, `status-*`, or `status-format` write is in force for the recipient's scope (even when the value equals the default); scalar and whole-array unsets clear their mark, an indexed `status-format[N]` unset keeps it. It gates only the TUI's `Ctrl-\ detach` hint. GUI visibility instead follows whether the native status model is empty, so `customized` has no GUI appearance effect. |
| Presentation | Status line, prompts, choosers drawn as terminal escapes. | The TUI renders the daemon's personalized `status-format[]` rows through the shared `zz-client` compositor that reproduces `format-draw.c` alignment sections, `fill=`, list focus/truncation, blank-row base style, and hit ranges. It places that authoritative block at `status-position`, replaces the selected `message_line` row with messages or a prompt, and routes window-range clicks. The GUI never paints those rows as tmux-authored cells: its always-native surface uses `status.left`/`status.right`, snapshot-backed window controls, and only the row list-alignment directive. Its top or bottom placement follows the app chrome mode, visibility does not depend on `customized`, and `status-position` has no GUI placement authority. Prompts and choosers stay native on both. |

# Related

- [live tmux compatibility gaps](/tmux/gaps.md) — generated TODO, decision, and status report.
- [tmux drop-in plan](/designs/tmux-drop-in.md) — the 2026-08-16 campaign plan and delivery record.
- [tmux compatibility philosophy](/tmux/tmux-compat.md) — the contract these divergences are
  measured against.
- [tmux superset roadmap](/designs/tmux-superset-roadmap.md) — the tier ladder and the amended
  never-list.
- [commands](/tmux/commands.md) — the implemented verb-by-verb reference.
