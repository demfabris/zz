# tmux-compat campaign review (cycles 1 to 13)

Written 2026-09-04 from eight lens reports and their long-form notes in this directory, with the disputed claims re-checked against the registry, the pinned C, the scenarios and the zz source at HEAD. Read-only: no build, no harness run, no board writes. Numbers below are stated only where I verified them myself; counts a reviewer produced by census are attributed to that lens.

## Headline

The registry is closed against its own list: `compat/progress.py` reports 99.7% (303 of 304 frozen items), 64 of 65 groups done, one open group (`clients.byte-clean-consumers`, 3 items), 173 closed records, 215 scenarios (100 plain, 115 smoke). The method held: a pinned tmux as the truth, a differential harness, a registry whose acceptance clauses are the contract, adversarial reviewers who re-probe the pin, and hard per-group budgets. Keep all of that.

The list itself was drawn after argv parsing and below the screen, and that is where a switcher breaks. The desktop app throws the tmux status line away (`crates/zz/src/mux/client.rs:3841`), so every theme and status plugin a tmux user installs first does nothing in the app they were sold. `~/.tmux.conf` is never read at boot; the import copies one file once and the copy drifts from what tpm and `bind r source-file` keep editing. Pane processes have no `tmux` on PATH, so the plugin proofs hold only under the harness's own PATH wrapper, which the product does not ship. The default prefix table kills without `confirm-before`, pastes without `-p`, and has no `d`; `S-Left` and its siblings can never fire from any table. 453 items sit in 42 accepted groups, 18 of them on a one-sentence reason, and two reasons name a reopen condition that the campaign's own corpus already meets (tmux-resurrect reads `#{history_size}`; oh-my-tmux runs `save-buffer -`).

Some of the biggest gaps are not lane-shaped. The harness compares daemon facts and cannot see a rendered row or the GPUI client at all; its attached-client fixture is red on this box while `--check-summary` passes on a stored `PASS` footer; the handoff and the campaign log are two cycles behind the registry. Fix those instruments first, then run three more two-lane cycles.

## State of the record (verified)

| Fact | Value | Source |
| --- | --- | --- |
| Meter | 99.7%, 303/304 items, 64/65 groups | `python3 compat/progress.py` |
| Baseline freeze | 2026-08-31 | `compat/progress-baseline.json` |
| Live groups | 43: 42 accepted (453 items), 1 open (3 items) | `compat/tmux-gaps.json` |
| Live items by kind | key 93, native-key 85, option 62, format 55, binding 45, semantic 45, flag 32, native-command 22, command 9, presentation 6, protocol 2 | same |
| Closed records | 173 | same |
| Scenarios | 100 plain + 115 smoke = 215 | `ls compat/scenarios` |
| Handoff | says "cycle 12 is the last one", meter 99.0%, three open groups | `compat/orchestration/HANDOFF.md` |
| Campaign log | last section is cycle 11 | `compat/orchestration/CAMPAIGN-LOG.md` |
| Attached fixture | `Status: PASS` footer stored; closed `rendering.geometry-residue` records the fixture red at `559fd8a` on this box | `compat/results/summary.md`, registry |

The meter arithmetic the oracle lens raised is right and worth publishing beside the percentage: the 304-item list is the frozen agreed scope, but the 42 accepted groups hold 453 more identified items. A reader who sees 99.7% should also see 304 of 757 identified.

## Top findings

Ranked by who breaks and how loudly, not by how interesting the bug is. Silent wrong answers rank above visible errors; popular plugins and universal dotfile idioms rank above rare idioms.

| Rank | Gap | Who breaks | Evidence (verified unless marked) | Next step |
| --- | --- | --- | --- | --- |
| 1 | The desktop app discards the tmux status line. No registry item names it; the two knowledge pages contradict each other. | Every desktop user of catppuccin, dracula, powerline, tmux-battery, tmux-cpu, prefix-highlight, mode-indicator, oh-my-tmux's theme, continuum's indicator. Silent: the config parses, the options store, the meter counts them done, the bar never appears. | `crates/zz/src/mux/client.rs:3841` `CoreEvent::StatusChanged` in a no-op arm; `knowledge/tmux/status-line.md:18-19` "GUI clients ignore StatusLine"; `knowledge/tmux/tmux-compat.md:538` claims a GUI row; daemon still expands rows per client (`status.rs` sampler). Ecosystem lens verified these plugins write nothing but status options. | Product decision recorded as an item either way. If yes: draw `status-format[]` rows in a GUI row when `StatusLine.customized` is set (`zz_client::compose_status_row` is client-core code). If no: write the reason naming the plugins and fix tmux-compat.md:538. |
| 2 | Config discovery: zz boots from `zz/mux.conf` only, never `~/.tmux.conf`; import copies the first of three candidates (no `/etc/tmux.conf`) verbatim, and the copy drifts from the file tpm and `bind r source-file ~/.tmux.conf` keep editing. The harness injects every config with `-C source-file`, so discovery was never measured. | Every `alias tmux=zz` user on a CLI-first box (unconfigured server, silently); every tpm user after import (plugins installed against one file, zz booting the other); `/etc/tmux.conf` sites; XDG mid-migration setups with two files (pin loads both, probed by the accepted-groups lens). | `crates/zz-daemon/src/paths.rs` `default_mux_config` (zz/mux.conf only), `tmux_config_candidates_for` (three paths, first existing), used only by import (`crates/zz/src/config/import.rs:84-92`); `daemon.rs:1160` `startup_mux_config_files`; pin `Makefile.am:14` four paths, `cfg.c` loads each; `compat/diff-scenario.sh` `conf:` lines become `-C source-file` on both sides; registry: `semantic:config-files-native-discovery` inside `presentation.native-status`, whose whole reason is one sentence that never mentions config. Site docs document the copy (`site/src/content/docs/docs/tmux.md:16`). | Own group `config.discovery`. Source every existing candidate in pin order including `/etc/tmux.conf` when no `-f` is given (or source the discovered file in place after mux.conf); a scenario that starts both servers with HOME holding `.tmux.conf` and no `-f`, then diffs `show-options -g` and `list-keys -T prefix`. |
| 3 | Pane processes get no `tmux` on PATH. The daemon's private `tmux` wrapper reaches only jobs (run-shell, if-shell, status `#()`); the harness puts its own wrapper first on PATH for the whole zz side, so the plugin corpus is proven under a PATH real users never have. | vim-tmux-navigator's vim half (`tmux -S ... select-pane` from the pane), fzf `--tmux` and fzf-tmux, tmux-thumbs, tmux-fingers, extrakto, tmux-fzf, tmux-sessionizer in a `new-window`, tmux-floax. With real tmux installed beside zz they silently drive a real tmux server; without one, command not found. | `crates/zz-daemon/src/lib.rs:133-177` `configure_shell_job_environment` is the only caller of `configure_tmux_shim`, invoked from `status.rs:1601` and `daemon.rs:32235/32366/32544` (jobs); pane spawn env at `daemon.rs:7290-7310` sets TMUX, TMUX_PANE, ZZ_* and no PATH; daemon test at 63824-63844 asserts a pane has no `ZZ_TMUX_EXECUTABLE`; `compat/diff-scenario.sh:129` prepends `$ZZ_SHIM_DIR` to PATH and `:239-246` writes the wrapper; packaging installs only `zz` (entrypoint lens). | Decision: a pane PATH entry, a `tmux` launcher symlink in packaging, or documented interactive-only. Either way a harness mode whose zz side runs the installed layout with no `--socket` and the pin tmux first on PATH, plus one scenario running a corpus script from a pane shell. |
| 4 | The default prefix table omits 33 stock keys and changes four: `x` and `&` kill without `confirm-before`, `]` pastes without `-p`, `?` drops `-N`, `d` is unbound (the raw TUI detaches on a chrome chord, `ui C-d`). Accepted under a one-sentence reason that argues only the picker and sidebar keys. | Every tmux user's muscle memory. Destructive: an accidental `prefix x` kills a pane with no prompt. `prefix ]` into vim gives the auto-indent staircase. `prefix d` over ssh does nothing. | `crates/zz-protocol/src/key.rs:53-90`; `knowledge/tmux/key-tables.md:190-206` lists the 33 omitted keys and the four changed commands (60 zz bindings vs the pin's 92); registry `keys.default-prefix` reason: "Picker and sidebar bindings are part of the zz GUI experience." (61 items). `confirm-before` and `paste-buffer -p` are implemented (accepted-groups lens). Across all tables the oracle lens counted 138 of 303 pin bindings accepted native. `crates/zz-client/src/chrome.rs:692` binds detach to `ui C-d`. | Split the group: adopt the pin's exact command for `x & ] ? d PPage f . ( ) L m M i ~ # - ' M-n M-p`; keep native only where a native verb genuinely replaces (`% " s w r e t D C C-z Tab BTab * @ g < >`). Add a scenario diffing bare `list-keys -T prefix` so the accepted remainder is enumerated, not implied. |
| 5 | Shift on special keys is never named, so `bind -n S-Left previous-window` (and every `S-Up/S-Down/S-Right/BTab` binding) is accepted, listed, and can never fire from any table. | The window-switch idiom from the tmux wiki and countless dotfiles; `S-Up/S-Down` resize idioms; copy-mode-vi `S-` bindings. Silent: the bind loads clean. None of the 8 corpus plugins uses one, so the harness never saw it. | `key.rs` `input_key_name` folds shift only for character keys (lines 1218-1246, no `S-` branch); mux stores the spelling; pin `key-string.c:351`, default binds at `key-bindings.c:457-460`; closed `choosers.command-flags` says "S-Up and S-Down cannot be delivered by zz at all ... recorded rather than built". | New open group `keys.shift-modifier`: emit `S-` for special keys in the shared key contract, deliver from TUI and GPUI, `Tab` as `BTab`; attached scenario on both binaries. |
| 6 | `#{history_size}`, `#{cursor_x}`, `#{cursor_y}`, `#{alternate_on}` answer 0. tmux-resurrect reads the first two to size its capture; the accepted reason says "reopen once a workload asks". | Every resurrect user with `@resurrect-capture-pane-contents 'on'` (the README option): zz captures `-S -0`, the visible screen only, so restored panes silently lose their scrollback; panes with at most one non-empty screen line are skipped. The `PageUp` / `alternate_on` dotfile idiom drops less and vim into copy mode. tmux-jump's overlay cursor lands at 0,0. | `crates/zz-mux/src/formats.rs:496,540,541,545` (`Zero`); `compat/.cache/plugins/tmux-resurrect/scripts/save.sh:126-131` reads both, `:143` runs `capture-pane -epJ -S "-$history_size"`; pin probe after `seq 1 100`: `79 23` (ecosystem lens); `smoke/resurrect-init.txt:2` excludes save and restore; precedent: `pane_pb_state` already flows from the worker's byte filter, and libghostty exposes cursor and scrollback getters (accepted-groups lens). | Reopen those four names only (not the 28) through the worker channel the reason already names; a smoke scenario that runs resurrect's `save.sh` on both binaries and diffs the saved pane-content files. |
| 7 | `save-buffer -` and `load-buffer -` are refused. The reason's reopen condition ("a named workload") is met by the campaign's own corpus. | oh-my-tmux's `prefix y` (`.tmux.conf:131-139`, in the corpus); the wiki clipboard bindings `tmux save-buffer - \| xclip` and `xclip -o \| tmux load-buffer - ; tmux paste-buffer`. Loud error, but on the key people press most. | `crates/zz-daemon/src/daemon.rs:14359` and `:14412` `UnsupportedCommand`; pin `cmd-save-buffer.c:115` writes to the command client's stdout (the path `show-buffer` already uses); probed on the pin from `run-shell` (accepted-groups lens); v98 already carries `RawText` arguments. | Bounded reopen: `save-buffer -` as show-buffer's byte-clean stdout path; `load-buffer -` via the CLI's existing bounded `read_stdin_payload` as a `RawText` argument. Keep `display-message -I`, `split-window -I`, `source-file -` refused. Smoke scenario through `run-shell`. |
| 8 | Mouse-key bindings are stored and never fire, with no diagnostic; zz's Linux drag selection lands in PRIMARY only. | tmux-yank on Linux with `mouse on`: drag-select, Ctrl+V in a browser, nothing. `bind -n WheelUpPane ...` and `MouseDown3Pane display-menu` idioms load clean and do nothing. Silent. | `mouse.bound-context` reason (measured: pin runs the binding, zz never); `compat/.cache/plugins/tmux-yank/yank.tmux:51,59` bind `MouseDragEnd1Pane copy-pipe-and-cancel`; `crates/zz/src/mux/client.rs:3724-3728` writes PRIMARY on linux (accepted-groups lens). | Keep the mouse tables accepted but: warn once at bind time; on Linux mirror drag selections to CLIPBOARD (or honour `set-clipboard`), or run a stored `MouseDragEnd1Pane copy-pipe*` command with the native selection text. |
| 9 | Status `#()` runs synchronously under the status mutex with a 2 s cap; the pin runs it as a background job and keeps the last output. | Every `status-right` with battery, cpu, weather, git, oh-my-tmux helpers: attach stalls up to 2 s; a segment slower than 2 s renders blank forever where tmux shows the last value. The detached corpus can never trigger it. | `crates/zz-daemon/src/status.rs:29-30` (2 s, 10 ms poll), `:1631-1643` blocking `try_wait` loop; `daemon.rs:4609/4832` render under the lock on the interval thread, `:5047` on every attach (harness lens); pin `format.c` `job_run(JOB_NOWAIT)` with cached `fj->out`, probed `[#(sleep 3; echo slow)]` answers `[]` in 4 ms. | Move shell jobs off-thread, publish on completion, cache as the pin's `fj->out`; attached fixture with `status-interval 1` and a 3 s segment, read the row after 4 s on both. |
| 10 | A custom key table is never left after a bound key fires, and unbound keys are swallowed while parked (after `switch-client -T root` too). | `switch-client -T <table>` one-shot mode idioms (tmux-modal, the wiki's resize and pane tables): after the first bound key the keyboard appears dead. | `key.rs:957-984`: unbound key with a table set returns `Ignore` and keeps the table; only `prefix` resets; pin `server-client.c:1490-1497, 1536-1556`; measured on the pin with a control client (prose lens). Lives only inside closed `terminal.key-control`. | Slug `semantic:key-table-reset-after-dispatch`: reset to root after a non-repeat dispatch, retry an unbound key in root; attached scenario. |
| 11 | The attached-client fixture is red on this box, the gate passes on a stored `PASS` footer, and the handoff and log are two cycles behind. | Every close whose only proof is the attached fixture (copy mode, prompts, choosers, menus, popups, mouse, paste, focus) is unverified on this box; the next orchestrator starts from a handoff that says three groups are open when one is. | Closed `rendering.geometry-residue`: "the fixture is already red at origin/main 559fd8a ... --check-summary passes on that stale line"; `compat/run.sh:311` only re-reads the footer, `:555-560` writes it only from a passing full run; `compat/results/summary.md` ends `Status: PASS`; `HANDOFF.md:63` "attached-client PASS"; CAMPAIGN-LOG's last section is cycle 11; progress.py says 303/304 against the handoff's 301/304. | Give the fixture an owner: fix `probe_command_output_navigation` and `probe_command_prompt`; make `--check-summary` refuse a footer without a commit stamp; write the cycle 12 and 13 log entries and refresh HANDOFF. Hours, not a lane. |
| 12 | Control-mode notifications are never diffed against the pin (every transcript strips `%` lines); `zz -CC` was never run under iTerm2; the deliberate divergences live in a doc, not the registry; `%layout-change` after `refresh-client -C` is rendered from the client's last snapshot. | iTerm2 `tmux -CC` users, the one mass consumer of `%output`, `%layout-change`, `%window-add`. | `smoke/fixtures/source-file-control.sh:189-259` awk drops `^%`; census: only 6 of ~25 kinds appear anywhere (oracle lens); `knowledge/designs/tmux-drop-in.md:2214` "Hardware smoke pending (maintainer)"; `knowledge/tmux/divergences.md:1160`; 0 registry hits for `%pause`; `crates/zz/src/control_mode.rs:1085-1093`. The scripting lens recorded the pin's bytes for the whole iTerm2 command list. | A `control-notify` fixture that keeps `%` lines, normalizes ids and block numbers, and replays the recorded command list on both binaries; register the divergences with the bytes already measured; one hour on a Mac under real iTerm2. |
| 13 | `refresh-client -S` and bare `refresh-client` error with "interactive behavior"; the pin only forces a status job refresh and a repaint. | Status-updating scripts (`tmux set -g @x ...; tmux refresh-client -S`), `bind r source-file ... \; refresh-client -S`, status plugins. Loud. | `daemon.rs:13094-13099`; pin `cmd-refresh-client.c:282-287` (accepted-groups lens). | `-S` forces an immediate `#()` re-run and returns 0; bare does the same plus a no-op redraw; keep the pan family refused. |
| 14 | The raw TUI at 80 columns keeps a 29-column sidebar and three chrome rows: `ssh -t host zz attach` on 80x24 gets roughly a 51x21 pane where tmux gives 80x23. | Every remote user on a laptop-sized terminal, mosh, serial consoles. Accepted only by implication. | `crates/zz-tui/src/sidebar.rs:11-15` (`WIDTH 28`, `BORDER_WIDTH 1`, `AUTO_HIDE_COLUMNS 80`, `STATUS_ROWS 3`); fixture comment `format-window-bigger.sh:55-57`. | Product decision: raise the auto-hide threshold or start hidden under the tmux alias; a fixture asserting `tput cols` inside the pane at 80x24. |
| 15 | The CLI output writer changes bytes: `display -p ''` prints nothing, `show-buffer` gains a trailing newline. Two scenarios were written around it instead of a slug. | The `bind y run "tmux show-buffer \| xclip -selection clipboard"` idiom (a copied command gains a newline and executes when pasted into a shell); `tmux show-buffer > file`; line-oriented readers of `display -p`. | `crates/zz/src/lib.rs:1853-1863` (early return on empty, appends `\n`); pin probes: one `\n` for the empty case, 5 raw bytes for show-buffer (prose lens). | Slug `semantic:cli-output-byte-fidelity`: write bytes as the pin's command client does. Same file as finding 7, same lane. |

## Second tier

Real gaps with a named victim, smaller or louder than the table above.

- Plugin corpus proofs are parse, `list-keys` and option readback; no plugin runtime path (resurrect save and restore, tpm `prefix I`, yank `copy_line.sh`, continuum status, fpp) runs on either binary. Finding 6 is the first concrete break behind a clean corpus row. (proofs, ecosystem)
- Registered by name, never fired: the oracle lens counted 17 consumed options with no scenario (including `synchronize-panes` and `default-command`), 39 of 68 hooks never triggered (including `window-layout-changed`, `session-closed`, `pane-title-changed`), 33 formats plus `#H #h #F #P #T` and the `e` and `p` modifiers never expanded. Cheap scenarios.
- Harness proof holes (proofs lens): stderr is compared only in smoke mode, so a plain scenario is clean whenever both sides are nonzero; 18 closed records carry only `resource:` pointers; `history.hyperlink-reset` was closed on a check that cannot observe a reset; `format-listing.sh` answers the pin's theme query so both sides say `client_theme=dark`; four stdout-shape fixtures would stay clean on a shared environmental failure; `attached-client.sh:673-700` asserts different observables per side; popup underlay focus pairs are waited for on tmux only (`:939-941`, `:1448-1450`) and the divergence has no item, which hits vim `autoread` users after a `display-popup`.
- Detached panes spawn at 80x24 whatever `new-session -x/-y` says (`daemon.rs:7320`, `:7476` `initial_size: None`; pin `stty size` 6 40). Headless TUI test harnesses and pre-sized scripts wrap wrong. (prose)
- Initial `#{pane_title}` is `terminal`, the pin's is the hostname (`crates/zz-mux/src/model.rs:3849-3852`). `pane-border-status` with the default format shows "terminal". (prose)
- Menu-driven plugins (tmux-which-key, tmux-menus, tmux-fzf) expose `clock-mode`, `choose-client`, `customize-mode`, `show-messages -T`, `link/unlink-window` as broken default entries (`crates/zz-protocol/src/catalog.rs:624-637`). (ecosystem)
- `new-session -t` is refused (`catalog.rs:1253`) under a one-sentence reason, though zz already gives every client an independent current window, which is what the two-monitor idiom wants. (accepted-groups)
- `history-limit` above 1,000,000 is refused (`crates/zz-terminal/src/model.rs:20`; pin accepts INT_MAX): a cargo-cult `set -g history-limit 5000000` errors at every boot. Loud. (harness)
- Server signals: no SIGUSR1 socket recreate, SIGHUP kills the daemon (`daemon.rs:1351` handles Term and Int only). (oracle)
- Resize storms: the pin coalesces on a 250 ms timer (4 SIGWINCH of 31, probed), zz resizes on every event. (harness)
- Timing knobs (`repeat-time`, `escape-time`, `display-panes-time`) are proved as option readbacks, never as clocks. (harness)
- tmuxp, tmuxinator, libtmux and byobu were never composed against zz. (scripting)
- No process is named `tmux` and the alias does not reach non-interactive scripts: tmux-sessionizer's `pgrep tmux` branch, `pkill tmux` habits. (ecosystem, scripting)
- `zz <cmd>` inside a real tmux pane refuses every command with exit 1 (`crates/zz/src/lib.rs:106`), the pin uses `$TMUX`'s socket. The migration moment itself; loud, unrecorded. (entrypoint)
- The `tmux` command line is not an oracle section: `-D` refused, `--version` accepted, default socket ignores `TMUX_TMPDIR`, `zz -L x` and `tmux -L x` share one socket file. (entrypoint)
- 18 of 42 accepted groups carry a one-sentence reason; a dozen divergences live only in closed-record prose with no slug (control-mode `$NAME`, GPUI overlay residues, retained-pane history in the GUI, `display-panes` template output, copy-mode search-string scope, control window geometry, the `set-option -t :nope` residue, and more). (accepted-groups, prose)

## Where reviewers disagreed, and who was right

- Pane environment identity. The entrypoint lens listed `TERM_PROGRAM=tmux` for panes under "checked and fine"; the oracle lens said panes get `TERM_PROGRAM=zz`. The oracle lens is right: `crates/zz-terminal/src/session.rs:4822` sets `zz` and `CARGO_PKG_VERSION`, while `daemon.rs:355` strips the TERM family before spawn; jobs (`lib.rs:172`) and popups (`daemon.rs:13346`) get `tmux` and `3.8-zz`. Nobody named breaks; the inconsistency stays in the tail.
- `tmux -V` = `tmux 3.8-zz`. The entrypoint lens said tmux-yank's `10#8-zz` aborts under `set -u`. tmux-yank sets `-u` only in its CI script (`citest:3`), not in `helpers.sh` or `yank.tmux`, so the case does not occur. The ecosystem, accepted-groups and scripting lenses are right that every surveyed parser survives; the suffix is a registered accepted item (`semantic:version-suffix`). Tail.
- Config discovery. Four lenses described it from different ends; they agree at the code level. The oracle lens's framing is the accurate one for a user: startup never reads `~/.tmux.conf`; the "first of three" rule the accepted-groups lens described is the import path.
- The default prefix table. The oracle lens's 138 of 303 counts every table (root mouse, copy-mode mouse, numeric prefix, move table); the accepted-groups lens's 33 omitted keys is the prefix table alone, matching `key-tables.md:190`. Different measures, both right.
- tmux-resurrect. The prose lens said the plugin "degrades gracefully via its screen-line fallback". Partly: `pane_has_any_content` does fall through to a line count, but `capture_pane_contents` still runs `-S "-$history_size"`, which is `-S -0` on zz (`save.sh:143`), so the scrollback loss is real. The other four lenses are right.
- `load-buffer -`. The scripting lens rated it inferred (wiki idiom from memory); the accepted-groups lens measured it against the pin and found the workload in the corpus (oh-my-tmux). Kept on the stronger evidence.
- Record counts. The registry has 173 closed records (prose lens), not 172 (proofs and harness lenses); the campaign log ends at cycle 11 (prose lens is right); the handoff's 99.0% and three open groups are stale against the registry's 99.7% and one.

## The lens nobody took

Each reviewer took a surface: the oracle, the accepted groups, the ecosystem, the harness, the proofs, the entrypoint, the scripting tools, the prose residue. Nobody took **the switcher's first hour as a single walk**: install zz, add the alias while real tmux sessions are still running, type `tmux`, get the import prompt (or not, on a CLI-first box), open the first pane, press the first plugin key. Pieces of that walk appear in five reports (findings 2, 3, 4, 14, the `$TMUX` refusal, the `-L` socket collision) but no scenario starts from an empty HOME with the installed layout and no `--socket`, and no reviewer sequenced the pieces. That lens would find, in order: the refusal inside the old tmux pane; the one-time GUI import prompt that CLI-first users never see; `import-tmux-config` overwriting an already customized `zz/mux.conf` (`paths.rs:193` writes the target with `atomic_write` and no existence check that I can see; the CLI path was not checked further); the unconfigured server; the dead `prefix d`; the missing status bar; and, on the second day, the daemon that outlives the app and keeps running the old binary after an upgrade.

Two sub-lenses fold into it:

- **Destructive edges.** What can lose a user's work: `x` and `&` without `confirm-before`, resurrect's silent scrollback loss, the import overwrite above, a daemon crash leaving the terminal in raw mode (unproven), `remain-on-exit` scrollback unreadable from the GUI. Cycle 11's silent destructive kill in the desktop chooser was caught by a reviewer whose scope happened to include it. Nobody enumerated the class.
- **What the product promises against what the registry accepted.** `site/src/pages/index.astro:21` sells "one tmux-compatible layout, one set of keys"; `docs/tmux.md:16` and `configuration.md:19` document the verbatim copy honestly; `tmux-compat.md:538` promises a GUI status row the desktop does not draw. Nobody read the site and the bundle against the 42 accepted groups.

## Next cycles

The campaign's shape stays: two lanes per cycle, Opus 5 at xhigh for workers, reviewers and gate, hard per-group budgets in minutes, one lane owning any protocol bump, pairwise-disjoint board zones (`python3 compat/board.py zones`: mux-command, mux-model, mux-formats, mux-options, config-parser, daemon-core, daemon-status, control-client, client-core, raw-tui, terminal-engine, protocol-message, protocol-key, protocol-catalog, desktop-gpui). Where a lane needs a few lines outside its zones it declares them one by one in notes, as cycle 13 did for `daemon.rs` regions.

### Before cycle 14: an instrument pass, not a cycle

The three biggest structural gaps are not lane-shaped. One owner, hours each, no reviewers:

1. Fix the attached-client fixture on this box (`probe_command_output_navigation`, `probe_command_prompt`) and make `--check-summary` refuse a `PASS` footer that carries no commit stamp or predates the tip. Until this is done every attached-only close is unverified here.
2. A launcher mode in `compat/diff-scenario.sh`: the zz side runs the installed layout (`cli` beside `zz`), no `--socket`, `ZZ_SOCKET` unset, a scrubbed PATH with the pin tmux first. Finding 3 cannot be proved without it; `compat/packaged-cli.sh` exits 2 off macOS and is not called.
3. A row-level TUI differential: run zz-tui below 50 columns or with the sidebar toggled off, `status off` on the recorder, and diff the last row's `capture-pane -p -e` bytes over a small format corpus. Without it no status rendering claim has a differential guard.
4. Write the cycle 12 and 13 entries into CAMPAIGN-LOG.md and refresh HANDOFF.md from the registry.

### Cycle 14: the first hour

- Lane A, branch `campaign/batch-keys-contract`, zones protocol-key, raw-tui, client-core; declared excursion: `daemon.rs:12659` (`switch-client -T` storage). Budgets: split `keys.default-prefix` 90 min, `keys.shift-modifier` 120 min, `keys.table-lifecycle` 90 min. Closes findings 4, 5, 10.
- Lane B, branch `campaign/batch-buffers-vt-facts`, zones daemon-core, terminal-engine, mux-formats, desktop-gpui limited to `crates/zz/src/lib.rs` CLI read and print paths, protocol-message only if a bump is unavoidable. Budgets: `save-buffer -` and `load-buffer -` 120 min, the four terminal facts 150 min, CLI output bytes 45 min, `refresh-client -S` 45 min, resurrect save fixture 60 min. Closes findings 6, 7, 13, 15 and lands the resurrect save scenario.

### Cycle 15: entrypoint and status

- Lane A, branch `campaign/batch-config-discovery`, zones daemon-core (`paths.rs`, `lib.rs`, the startup and import paths of `daemon.rs`), protocol-catalog, desktop-gpui limited to `crates/zz/src/config/import*.rs` and `lib.rs` argv. Budgets: `config.discovery` 150 min, pane PATH or launcher symlink decision 90 min, CLI oracle section with the `$TMUX` refusal and `-V` recorded 60 min, import overwrite guard 30 min. Closes finding 2, finding 3 (product side), the CLI surface, the migration refusal.
- Lane B, branch `campaign/batch-status-jobs-control-notify`, zones daemon-status, control-client. Budgets: background `#()` with cached output 150 min, control-notify fixture and registered divergences 120 min, `%layout-change` from the live layout 60 min. Closes findings 9 and 12 (the fixture; the iTerm2 hour stays a maintainer task).

### Cycle 16: the desktop as a tmux client, and the proof debt

- Lane A, branch `campaign/batch-desktop-status-row`, zones desktop-gpui, client-core, raw-tui. Budgets: GUI status row when `StatusLine.customized` 180 min (after the product decision is written into the registry first), Linux drag-to-CLIPBOARD 60 min, a `#[gpui::test]` matrix feeding every daemon overlay payload kind into the workspace 90 min, sidebar auto-hide threshold 45 min. Closes findings 1, 8, 14 and the cycle-11 class of "daemon state with no desktop consumer".
- Lane B, branch `campaign/batch-proof-debt`, zones none (compat/, the registry, knowledge/ only; declared). Budgets: per-plugin runtime fixtures 120 min, the census scenarios for options, hooks and formats 90 min, the harness holes (`err:` query kind, stdout-shape fixtures, underlay focus item, evidence drift rule in `compat/check.sh`) 90 min, slugs for the prose-only divergences and pin citations for the 18 thin reasons 90 min. Closes the second tier's proof items and finding 11's registry half.

Left over after cycle 16, each hours in one zone: detached pane geometry (daemon-core, terminal-engine), initial `pane_title` (mux-model), `clock-mode` and `choose-client` (desktop-gpui, raw-tui), `new-session -t` attach form (protocol-catalog, daemon-core), `history-limit` cap, signals, resize coalescing. Fold them into whichever lane has budget left, or a fourth cycle if the first three land clean.

## The tail

Nobody I can name breaks; recorded so the next campaign does not rediscover them.

- `tmux -V` prints `tmux 3.8-zz`, a shape no tmux ever printed. Every surveyed parser survives; registered as `semantic:version-suffix`. Decide whether `zz --version` alone carries the suffix.
- Panes get `TERM_PROGRAM=zz`, jobs and popups get `tmux`. Pick one identity.
- The style grammar (`range=`, `list=`, `push-default`) is implemented with unit tests and sits outside the oracle, the registry and the harness.
- Control-mode `$NAME` is not expanded (the pin expands from the global environment); called an accepted divergence in two pages, no group carries it.
- zz never emits `%window-close`; the pin does when a window still linked in the client's session is unlinked elsewhere. zz's answer is friendlier for iTerm2; record it.
- `display-panes` template output and exit status never reach the invoking CLI; an overlay does not make a second `display-panes` a no-op.
- The remembered copy-mode search string dies with the mode (pin: pane-scoped).
- GPUI overlay residues: popup pointer policy is the raw TUI's, `M-+` undeliverable, `display-menu -c` over an open chooser.
- A retained (`remain-on-exit`) pane's scrollback is unreadable from a GUI client (`history()` answers `ActorStopped`).
- Nested attach inside a `display-popup` (tmux-floax, the scratch-popup idiom) is allowed by rule and proven by nobody. Inferred, one lens.
- Keypress latency has no instrument; abrupt daemon death leaves terminal-mode restoration unproven. Inferred, one lens each.
- `options.native-overlay-styles` and `native-mode-styles` accept 25 theme options store-only without stating what the raw TUI does with them. Inferred.
- The daemon's job wrapper execs the bundled `zz`, not the launcher, so a bare `tmux` from a job opens the desktop GUI.
- Scale is untested: 22 windows in one scenario at most, 2 clients at most.
- `history.hyperlink-reset` should move from closed to accepted; its observable is vacuous.

## What held up

Name them so the next campaign keeps them.

1. **Adversarial reviewers who re-probe the pin.** Cycle 11's choosers reviewer caught a silent destructive kill in the desktop chooser that no test could see; the formats reviewer caught `expand_client_loop` built with `trace: None` against what the closed record asserted. The log's own words: "The reviews were worth more than the tests this cycle." The review prompt's rule that a test asserting zz's current behaviour without pin derivation is a defect is what made the closes trustworthy.
2. **Hard per-group budgets.** Cycle 10's open-budget lane ran 4h15m and made the cycle six hours; cycles 11 and 13 finished inside their budgets (CAMPAIGN-LOG:334-335). Keep the ceiling and the rule that an unprovable clause is a finding written into the reason, not a failure.
3. **The registry grammar.** Typed slugs, acceptance clauses as the contract, the relocation pattern that requires the measured pin behaviour and the product stance in the accepted reason, `compat_manifest_tests.rs` refusing an upstream flag that is neither implemented nor tracked, and `tmux-tracker.py`'s path-reference check (the proofs lens found 1235 of 1235 evidence pointers resolve). This grammar is why eight reviewers could audit 42 accepted groups and 173 closed records in an afternoon. Its weakness is not the grammar but 18 reasons that used one sentence where the grammar asked for a measurement.
4. **Pin-derived proofs where they were pin-derived.** The oracle was extracted from the pin's C tables and independently re-derived by two lenses at 92/92 commands, 180/180 options, 68/68 hooks, 303/303 default bindings. `lane2-store.txt` dumps bare `show-options -s`, `-g`, `-gw` on both binaries. `smoke/command-flag-errors` pins usage errors from the pin's own usage strings. Fixtures carry side canaries (`zz-side-only`, `tmux-side-only`) and a `clean:N` readback so both-sides-broken cannot read clean. The proofs lens graded 46 of 57 sampled closes as resting on a differential row or a pin-measured literal.
5. **The frozen meter.** `progress-baseline.json` frozen 2026-08-31 with scope additions tracked separately, so the percentage could not be moved by re-scoping. Keep it, and publish the 304-of-757 number beside it.
6. **Gate recovery and merge tooling.** Cycle 11's gate died on a network timeout after merging one lane; the recovery order written into HANDOFF.md ("push the rebased tip as `campaign/<name>-gated` first, verify must-fixes in the code, re-run stages whose inputs changed") rebuilt nothing and took an hour. `gaps-merge.py` merges registry records by id and refuses a record both sides changed.
7. **Zone discipline with declared regions.** Cycle 13 had both lanes in `daemon.rs` in declared disjoint regions; the rebase kept both hunks and the workspace run judged. Keep declaring regions rather than widening zones.
8. **The FOREGROUND rule.** Cycle 10 dropped a lane when a reviewer ended its turn on a background task; every prompt since carries the rule and no lane has been lost that way.

## Appendix: every raw finding by lens, with disposition

F-numbers refer to the top table (F1 to F15) and the second tier (S), tail (T). "Merged" means the same gap seen from another side; the row named first carries the evidence.

### oracle-coverage

1. Config discovery never measured, `~/.tmux.conf` not read at startup: kept as F2 (primary framing).
2. `history_size` and `cursor_y` zeroed, resurrect reads them: merged into F6.
3. 138 of 303 default bindings accepted native; x/&/]/?/r changed: merged into F4 (the all-tables count kept as context).
4. Control-mode messages asserted for only 6 kinds, divergences in a doc: merged into F12.
5. 17 consumed options never in a scenario: kept in the second tier.
6. 39 of 68 hooks never fired: kept in the second tier.
7. 33 formats and five aliases never in a scenario: kept in the second tier.
8. SIGUSR1 absent, SIGHUP kills: kept in the second tier.
9. Pane vs job TERM_PROGRAM identity: tail (verified; entrypoint's opposite claim was wrong).
10. Style grammar outside the oracle: tail.
Checked-and-fine list: accepted; the oracle name-level completeness is cited under "what held up".

### accepted-groups

1. `save-buffer -` / `load-buffer -` refused: kept as F7 (measured; the workload is in the corpus).
2. `alternate_on`, `cursor_y`, `history_size` zeroed: merged into F6 (its `pane_pb_state` precedent and libghostty getters kept as the fix shape).
3. Prefix table drops 33 keys and de-fangs three: kept as F4 (primary framing; `prefix d` detail kept, detach chord corrected to `ui C-d` per chrome.rs:692).
4. `refresh-client -S` refused: kept as F13.
5. Config discovery first-of-three, no `/etc`: merged into F2.
6. Mouse bindings dead, Linux PRIMARY only: kept as F8.
7. `new-session -t` refused: second tier.
8. 18 one-sentence reasons: second tier (process), folded into cycle 16 lane B.
9. Overlay and mode style reasons thin: tail (inferred, one lens).
Checked-and-fine: accepted; the `-V` and `session_grouped` items agree with my checks.

### ecosystem

1. Status-line plugin family has no desktop consumer: kept as F1 (merged with harness 1; the plugin roster is this lens's contribution).
2. terminal-runtime zeroes: merged into F6 (the tmux-jump case and the 79/23 probe kept).
3. Alias never reaches pane-hosted runtimes: merged into F3.
4. Import copy drift: merged into F2.
5. Resurrect save/restore never runs: second tier (runtime proofs), scenario scheduled in cycle 14 lane B and cycle 16 lane B.
6. Mouse bindings never fire: merged into F8.
7. Menu plugins expose unimplemented commands: second tier.
8. Nested attach inside a popup: tail (inferred, one lens).
9. Script-driven copy mode has no scenario: second tier (runtime proofs).
10. `pgrep tmux`: second tier (two lenses: this one read-from-source, scripting inferred).
11. `-V` shape: tail.
12. TPM install/update flows: second tier (runtime proofs).
Checked-and-fine: accepted.

### harness blind spots

1. Desktop discards the status line, pages contradict: kept as F1 (merged with ecosystem 1; the client.rs:3841 evidence is this lens's).
2. Synchronous `#()`: kept as F9 (measured).
3. Resize storms: second tier (measured on the pin only; zz by source).
4. TUI sidebar at 80 columns: kept as F14.
5. Status rows never compared at cell level: became instrument 3 in the pre-cycle pass.
6. GPUI never run, overlay states without a consumer: folded into cycle 16 lane A (the gpui matrix).
7. Timing knobs as readbacks: second tier.
8. Scale untested, history-limit cap: cap kept in the second tier; scale in the tail.
9. Keypress latency: tail (inferred).
10. Abrupt daemon death: tail (inferred).
Checked-and-fine: accepted.

### proofs

1. terminal-runtime and resurrect: merged into F6.
2. Corpus proves installers not runtimes: second tier (runtime proofs).
3. Stderr never compared outside smoke mode: second tier (harness holes).
4. Popup underlay focus asymmetric and unregistered: second tier (harness holes; the vim autoread victim is the only named one).
5. 18 resource-only closes, session-activity family on unit tests: second tier (harness holes).
6. `history.hyperlink-reset` vacuous: tail, with the status change recommended.
7. Fixtures steering the pin (`client_theme`): second tier (harness holes).
8. Four stdout-shape fixtures: second tier (harness holes).
9. Per-side observables in attached-client.sh: second tier (harness holes).
10. Evidence drift: second tier (harness holes), `check.sh` rule scheduled.
Checked-and-fine: accepted; the 1235/1235 pointer check is cited under "what held up".

### entrypoint

1. Pane processes get no `tmux` on PATH: merged into F3 (primary evidence: lib.rs callers, pane spawn env, packaging).
2. The harness never drives the real entrypoint: merged into F3, and became instrument 2 in the pre-cycle pass.
3. Config discovery: merged into F2.
4. `-V` and `set -u`: tail; the `set -u` case does not occur in tmux-yank (verified).
5. CLI surface unregistered: second tier.
6. `zz` inside a real tmux pane refuses: second tier.
7. Job wrapper execs the bundled `zz`: tail.
Checked-and-fine: accepted except "TERM_PROGRAM=tmux for panes", which is wrong (session.rs:4822 sets `zz`).

### scripting and control mode

1. Control notifications never diffed: merged into F12 (its recorded pin bytes are the fixture's expected values).
2. `-CC` never run under iTerm2: merged into F12.
3. `load-buffer -` / `save-buffer -`: merged into F7 (inferred here, measured by accepted-groups).
4. Process identity, `pgrep tmux`: second tier (merged with ecosystem 10).
5. tmuxp, tmuxinator, libtmux, byobu never composed: second tier.
6. `%layout-change` from a stale snapshot: merged into F12.
7. Control divergences not registry items: merged into F12.
8. `%window-close` never emitted: tail.
Checked-and-fine: accepted.

### prose with no slug

1. Shift on special keys never named: kept as F5.
2. Custom key table never reset: kept as F10.
3. Detached pane pty 80x24: second tier.
4. Control-mode `$NAME`: tail.
5. CLI output bytes: kept as F15.
6. Attached fixture red, stale PASS, handoff stale: kept as F11 (extended with my own verification of run.sh, summary.md, HANDOFF, CAMPAIGN-LOG and progress.py).
7. Initial `pane_title`: second tier.
8. Control client geometry clamps the pty: second tier (folded into the prose-only slugs item).
9. Copy-mode search string scope: tail.
10. `display-panes` template output: tail.
11. GPUI overlay residues: tail.
12. Retained pane history in the GUI: tail.
13. Long tail of prose-only edges: second tier (the slugs item in cycle 16 lane B).
Its note that resurrect "degrades gracefully" is corrected above (the capture still runs `-S -0`).

### dropped

Nothing was dropped outright. The findings rated "inferred" by a single lens (nested popup attach, keypress latency, daemon-death termios, overlay-style reasons) are in the tail rather than the ranking; the one inferred claim that two lenses shared (`pgrep tmux`) stays in the second tier with the read-from-source evidence.
