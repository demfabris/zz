#!/usr/bin/env python3
"""Cycle 14 of the tmux-compat campaign on Codex.

Two gpt-6-astra implementor lanes (keys contract; buffers and VT facts), one Codex reviewer
behind each, one serial Codex gate to main. Same shape as opus-compat-run-13.js, driven through
`codex exec` instead of the Claude Workflow tool.

    python3 compat/orchestration/codex-compat-run-14.py --run-dir ~/dev/zz-run-14 [--stage all|work|gate]
                                                        [--lane keys|buffers] [--no-renew]

Every agent writes <run-dir>/<label>.{prompt.md,schema.json,log,json}. A rerun with the same
run dir reuses any <label>.json already there, so a dead lane or gate resumes from what finished.
"""

import argparse
import datetime
import json
import os
import pathlib
import subprocess
import sys
import threading
import time

M = dict(
    root="/home/demfabris/dev/zz",
    dev="/home/demfabris/dev",
    holder="ubuntu/orchestrator",
    machine="8-core, 30 GB Ubuntu 26.04 box (ubuntu)",
    cores=8,
    workerJobs=4,
    workerThreads=2,
    gateJobs=8,
    gateThreads=4,
    shards=4,
    protected="the user's zz daemon on the default socket under ~/.local/share/zz and any tmux server on the default socket /tmp/tmux-1000/default (none were running at launch; never assume that stays true)",
    boxNote="This box: /bin/bash is 5.3 (mapfile and associative arrays work); the file system is btrfs and accepts non-UTF-8 file names. Never write 'rm -rf $HOME' or 'rm -rf ~' even after re-exporting HOME to a scratch path; put the scratch directory in a plain variable (D=/tmp/<name>; rm -rf \"$D\"; mkdir -p \"$D\"; export HOME=\"$D\"). Three corpus rows are red on this box before any lane and are tolerated by the stored summary: smoke/remain-on-exit-format, smoke/format-modifier-interrogate, smoke/pane-engine-knobs-input; they are environment, not yours.",
    gitNote="NETWORK GIT: origin is SSH (git@github.com:demfabris/zz.git) and works non-interactively on this box; never change the remote URL. Set GIT_TERMINAL_PROMPT=0 on network commands so a credential miss fails instead of hanging.",
)

CODEX = [
    "codex", "exec",
    "-m", "gpt-6-astra",
    "-c", "model_reasoning_effort=medium",
    "-c", "service_tier=default",
    "-s", "danger-full-access",
    "--skip-git-repo-check",
    "--color", "never",
]

RUN_ENV = f"ZZ_COMPAT_TMUX={M['root']}/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS={M['root']}/compat/.cache/plugins"

WORKER_SCHEMA = {
    "type": "object", "additionalProperties": False,
    "required": ["branch", "fronts_done", "fronts_skipped", "touched_commands", "touched_packages", "notes"],
    "properties": {
        "branch": {"type": "string", "description": "campaign/* branch pushed to origin, or empty string if nothing pushed"},
        "fronts_done": {"type": "array", "items": {"type": "object", "additionalProperties": False, "required": ["front", "items_closed", "proofs"], "properties": {
            "front": {"type": "string", "description": "registry group id this entry covers"},
            "items_closed": {"type": "array", "items": {"type": "string"}},
            "proofs": {"type": "array", "items": {"type": "string"}, "description": "exact commands that ran green AT THE FINAL TIP"},
        }}},
        "fronts_skipped": {"type": "array", "items": {"type": "object", "additionalProperties": False, "required": ["front", "why"], "properties": {"front": {"type": "string"}, "why": {"type": "string"}}}},
        "touched_commands": {"type": "array", "items": {"type": "string"}, "description": "tmux verb names the diff touches, for the delta corpus"},
        "touched_packages": {"type": "array", "items": {"type": "string"}},
        "notes": {"type": "string", "description": "everything reviewer and integrator must know: zone excursions, protocol bump done or not, flaky tests seen, registry subtleties, slugs left open and why"},
    },
}

REVIEW_SCHEMA = {
    "type": "object", "additionalProperties": False,
    "required": ["lane", "verdict", "confirmed_defects", "checks_run", "notes"],
    "properties": {
        "lane": {"type": "string"},
        "verdict": {"type": "string", "enum": ["approve", "approve-with-fixes", "reject"]},
        "confirmed_defects": {"type": "array", "items": {"type": "object", "additionalProperties": False, "required": ["front", "severity", "description", "suggested_fix"], "properties": {
            "front": {"type": "string"},
            "severity": {"type": "string", "enum": ["blocker", "must-fix", "nit"]},
            "description": {"type": "string"},
            "suggested_fix": {"type": "string"},
        }}},
        "checks_run": {"type": "array", "items": {"type": "string"}},
        "notes": {"type": "string"},
    },
}

GATE_SCHEMA = {
    "type": "object", "additionalProperties": False,
    "required": ["merges", "progress_after", "board_updates", "attached_client", "problems"],
    "properties": {
        "merges": {"type": "array", "items": {"type": "object", "additionalProperties": False, "required": ["branch", "merged", "merge_commit", "gate_summary", "review_actions", "flakes"], "properties": {
            "branch": {"type": "string"}, "merged": {"type": "boolean"}, "merge_commit": {"type": "string"},
            "gate_summary": {"type": "string"}, "review_actions": {"type": "string"}, "flakes": {"type": "string"},
        }}},
        "progress_after": {"type": "string"},
        "board_updates": {"type": "string"},
        "attached_client": {"type": "string", "description": "the full run's outcome at the final tip: the Recorded at stamp and whether --check-summary is green on the pushed main"},
        "problems": {"type": "string"},
    },
}

COMMON = f"""You are an autonomous worker on the zz tmux-compat campaign (repo demfabris/zz). You are Codex (gpt-6-astra) running non-interactively under `codex exec`; ONE other worker runs in parallel on this {M['machine']}, and the user codes here too. Rules that are not negotiable:

HOW YOU REPORT
- Your FINAL message must be ONLY the JSON object the output schema describes (branch, fronts_done, fronts_skipped, touched_commands, touched_packages, notes). No prose around it. A worker whose final message is not that report is a failed agent and its lane is dropped.
- Run every command synchronously and wait for it. Never leave a process running in the background when you finish; never end your turn to "wait" for anything. You are done only when the branch is pushed and the report is your final message.
- Check `date` when you start each group. Every group carries a HARD BUDGET in minutes; it is a ceiling, not a target.

SETUP
- Your working directory is already your worktree, checked out detached at origin/main by the orchestrator. Work ONLY there. The shared checkout is {M['root']}: read it, never edit, stash, reset, or clean it (other sessions' uncommitted work lives there; its local main branch may be stale, always use origin/main). On any conflict touching knowledge/tmux/gaps.md, regenerate it with python3 compat/tmux-tracker.py write-report; never hand-merge that generated file.
- {M['gitNote']} Fetch with git fetch origin +refs/heads/main:refs/remotes/origin/main and push with git push origin HEAD:refs/heads/campaign/BRANCHNAME.
- Your worktree was last built at a cycle-13 lane tip, so the first build recompiles the workspace crates but not the dependencies; budget 10-15 minutes for it and do not mistake it for a hang.
- {M['boxNote']}

GROUND TRUTH
- The oracle is pinned tmux d77c9dc6 (next-3.8). Prebuilt binary: {M['root']}/compat/.cache/tmux-src/tmux (source tree beside it, read the C freely). Probe with THROWAWAY servers only: -L zzprobe-$$ sockets; kill your servers when done. Never kill a tmux or zz server you did not start: the other lane has live ones and so may the user ({M['protected']}); never use pkill/killall on tmux or zz.
- Differential scenarios: compat/scenarios/ (smoke under compat/scenarios/smoke/ with fixtures). Run: {RUN_ENV} compat/run.sh --strict-geometry <scenario-name>. Read 2-3 existing scenarios first to copy the format. A second window in a scenario must be created with new-window -n <name> (bare new-window flakes on automatic-rename). Real pty clients on both binaries: compat/scenarios/smoke/fixtures/pty-drive.py, chooser-drive.py, graphics-drive.py and client-exit-actions.py show the pattern; anything that needs an attached client (copy mode, prompts, choosers, menus, popups, focus, mouse, KEY BINDINGS FIRING) is proved that way, never through a detached differential row. Mouse and paste reach an attached client as bytes on its pty. A `launcher:` scenario header (see smoke/launcher-installed-layout) runs the zz side through the zz_cli launcher on the default socket with no harness wrapper; use it when the claim is about what an installed zz does. compat/status-row.sh is the row-level status differential.
- Fixture lessons: line-buffer Python stdout (sys.stdout.reconfigure(line_buffering=True)); bounded WNOHANG reaps that report stalled instead of blocking; never hold a control client's stdin open with sleep | client -C under run-shell; set -g status-keys emacs before measuring prompts on the pin unless the probe is about vi keys (EDITOR in the environment flips it); use -f /dev/null servers so the box's own ~/.tmux.conf cannot hide a derivation; one Control connection per command when a differential needs two commands down one pipe. A prompt can be raised from the CLI with command-prompt -t <client>. The zz raw TUI reserves a sidebar (about 29 columns) and 2 rows around a pane, so its screen is not cell-comparable to the pin's 80x23 pane; daemon-side facts (formats, list-panes, capture-pane, hooks writing to files, list-keys, the copy-mode format family, #{{client_key_table}}) are the comparable channel.

REGISTRY
- Contracts live in compat/tmux-gaps.json; a gap group's acceptance list IS the contract. The frozen agreed scope (compat/progress-baseline.json, 304 items) is CLOSED; everything this cycle touches is scope added since the freeze, which python3 compat/progress.py reports as "scope added since freeze (tracked, not diluting the %)". The retrospective that found your items is compat/orchestration/CAMPAIGN-REVIEW.md: read your findings' rows there first, they carry the measurements and file:line evidence.
- NEW OPEN GROUP: append to gaps[] with exactly the fields id, title, decision ("adopt"), status ("open"), priority ("now"), ease, impact (subset of admin/daily/gui/remote/scripts), owner (one of client/daemon/gui/mux/protocol/terminal), items (sorted, each matching an item pattern in compat/tmux-tracker.py ITEM_PATTERNS, e.g. semantic:<slug>, key:<table>:<key>, binding:<table>:<key>, format:<name>), evidence (resource:/scenario:/file: entries that exist), acceptance (the measured pin behaviour as clauses), depends_on ([]), reason. Every item belongs to exactly one group, so an item RELOCATED out of an accepted group is removed from that group's items array and the accepted group's reason and acceptance are rewritten to say what left and why (quote the old clause, the measurement that refuted it, and the probe). When you PROVE an item, remove its slug from the group's items array and update reason/evidence; a group whose items array empties moves from gaps[] to closed[] with closed_on and resolution. Patterns: git show 9e85bc00 -- compat/tmux-gaps.json (plain close), 1f24a1f1 (relocation), c6ce82c4 (flag promotion: catalog.rs unsupported_flag -> real option, plus the hard-coded (supported, unsupported) counters and usage_overrides length in catalog.rs, plus PINNED_TMUX_USAGE_OVERRIDES adjustments), 0fec342 and 9cab1fa (a reverted close and the reason that records why). Script edits with json.dump(..., indent=2) + trailing newline.
- A PRODUCT DECISION handed to you by this prompt is recorded with the old clause, the measured pin behaviour, the new stance, and the sentence "decided 2026-09-05 by the orchestrator under fabrico's instruction to continue the campaign past the frozen scope; reversible" so the owner can undo it.
- crates/zz-mux/src/compat_manifest_tests.rs hard-codes partition counts (tracked == divergent for keys, constant/delegated format partitions, catalog supported/unsupported, option consumers); cargo test -p zz-mux is a mandatory gate for ANY registry edit. Read python3 compat/tmux-tracker.py check rules (STATUSES open/blocked/accepted; accepted requires decision native|never; park requires blocked) before relocating anything.
- python3 compat/tmux-tracker.py check before each commit; if it flags report freshness, regenerate (write-report) and commit the regenerated file too.
- SUMMARY RULE: compat/run.sh --check-summary is RED on main right now BY DESIGN: the stored attached-client PASS carries no `Recorded at:` stamp and the check refuses it; only a full `compat/run.sh --attached-client` run at the merged tip stamps it, and the GATE does that after your lane merges. Do not try to stamp it on your branch. If you add scenarios, add their summary rows to compat/results/summary.md so the scenario INVENTORY matches: run compat/run.sh --check-summary and confirm the only error it prints is the "carries no commit stamp" one, with no inventory diff before it.

WIRE PROTOCOL RULE: current PROTOCOL_VERSION is 98. If your diff adds or changes ANYTHING wire-reachable (new ProtocolMessage variants, new or changed fields anywhere in a message or snapshot, appended enum variants on any type that rides a message, a changed type on CommandInvocation or CommandResponse) bump 98 -> 99 in the same commit. All sites mandatory: crates/zz-protocol/src/message.rs constant + same-file assert test; crates/zz-protocol/tests/hunt_claims.rs version test (currently protocol_version_on_this_commit_is_ninety_eight; rename to ..._ninety_nine, assert 99, pinned hex hello-frame bytes 0x62 -> 0x63 in every position; grep the hex, decimal greps miss it); knowledge/protocol/wire-protocol.md title/constant/changelog/byte rows (say inserted vs appended vs changed honestly, and name every variant, field and type the bump carries); knowledge/protocol/index.md + knowledge/index.md (v98) mirrors; knowledge/crates/zz-protocol.md twice. Then cargo test -p zz-protocol --jobs {M['workerJobs']}. NEITHER lane is expected to need a bump this cycle. If you find an unavoidable wire change, bump to 99, keep the changelog entry SELF-CONTAINED, and say so prominently in notes; the gate reconciles if both lanes did it. No wire change = no bump; say which in notes either way. MuxEffect, CommandSpec and TerminalWorkerOptions are NOT wire; anything on ProtocolMessage, MuxSnapshot, ClientHello, CommandInvocation, CommandResponse, or an enum they carry IS. The C ABI in crates/zz-client-ffi and clients/ios consume the client core: if a wire type you change is re-exported through include/zz-client.h, keep the header and the ffi crate compiling (cargo build -p zz-client-ffi) and say so in notes.

MACHINE ETIQUETTE
- Cap parallelism ({M['cores']} cores shared two ways): cargo build/test --jobs {M['workerJobs']}, test runs -- --test-threads={M['workerThreads']}. NEVER workspace-scale builds/tests; focused cargo test -p <pkg> and cargo clippy -p <pkg> --all-targets --all-features --jobs {M['workerJobs']} -- -D warnings per touched crate only.
- DOWNSTREAM RULE: if your diff touches crates/zz-mux/src/command.rs or model.rs target resolution, effect shapes, formats.rs producers, or anything zz-daemon calls, ALSO run cargo test -p zz-daemon --lib --jobs {M['workerJobs']} -- --test-threads={M['workerThreads']} before your last commit. If it touches a wire type, ALSO cargo test -p zz-client and cargo build -p zz-tui -p zz (the GUI crate compiles headless).
- Never pipe cargo test through tail/grep (masks the exit code): > log 2>&1, check exit status, read the log.
- Load-flake rule: fails loaded + passes exact-solo = flake. Known: client_focus_closes_display_panes_and_preserves_chooser_modes (also fails about one run in three exact-solo; two solo passes out of three count as green), event_hooks_fire_after_mutation_with_captured_formats, history_request_is_guarded_clamped_and_returns_self_contained_rows, copy-mode reconcile tests, daemon_native_split_resize_commits_exactly_and_rejects_stale_contexts, nested_alias_queue_bubbles_shutdown_and_yield_to_its_parent, control_sourced_run_shell_closes_before_raw_output_and_same_line_continues, request_full_enqueues_only_the_requested_visible_pane, display_menu_resize_lifecycle::a_resize_moves_the_menu_and_keeps_everything_else, zz-terminal pty_output_drains_while_the_input_writer_is_backpressured, wait_exit_holds_the_control_process_until_a_second_blank_line (can HANG under load, passes solo; use a timeout when running cli_binary tests), concurrent_default_interactive_attaches_share_session_zero (headless "not a terminal", may be misattributed to a neighbouring daemon test), smoke/source-replay-diagnostics (pin-side crash under concurrent scenario load; clean solo), behavior-options (one TOPO row under shard load, clean solo).

DELIVERY
- One commit per registry group (or tight family), subject matching repo style (git log --oneline -10). NO attribution trailers. NO comments in code. Never insert a new fn between an existing doc block and the fn it documents.
- PROOFS AT TIP: after your LAST commit, re-run every proof command you list and cargo test -p for every touched crate (plus zz-daemon under the downstream rule). Stale proofs are a reviewer blocker.
- Work groups in the order given. When a budget is spent, STOP that group, write what you measured and what remains into the group reason, and move to the next one. An unprovable acceptance clause is a finding, not a failure: record it precisely in the group reason. When an item resists a decision, MEASURE FIRST on the pin and write the measurement down; a measurement beats a narrowing.
- Your branch gets an adversarial Codex review before integration: every closed slug needs a proof in your diff that asserts the PINNED behavior. A test asserting zz's current behavior without pin derivation will be flagged and the close reverted; so will a close proved by a headless unit test where the acceptance names an attached client. Every relocation needs the accepted group's reason to state the measured pin behavior and the product stance. Every divergence you discover goes into the durable registry reason, not only into your report. Probes you write must live in the repo (a fixture under compat/scenarios/smoke/fixtures or a scenario), never only under /tmp.
- When done (or stopping): push campaign/BRANCHNAME (never force, never main), report via the JSON final message. Stopping early: push what compiles as campaign/BRANCHNAME-wip and say what never ran.
- NO GitHub comments, NO issue 7, NO compat/board.py mutations. The orchestrator speaks for you.
"""

BATCH_KEYS = COMMON + f"""
YOUR BATCH: the keys contract, retrospective findings 4, 5 and 10, three groups. BRANCHNAME: batch-keys-contract. Your worktree: {M['dev']}/zz-opus-panes (already at origin/main).
Your zones: crates/zz-protocol/src/key.rs; crates/zz-tui/src/; crates/zz-client/src/; crates/zz/src/ for the GPUI key input path ONLY (not lib.rs's CLI read/print paths, not control_mode.rs; the other worker owns those). Declared excursion, minimal and listed in notes: crates/zz-daemon/src/daemon.rs ONLY at the switch-client -T storage (around line 12659) and the per-client key-table state it keeps. The other worker owns the rest of daemon.rs, crates/zz-terminal, crates/zz-mux/src/formats.rs and command.rs, and crates/zz-protocol/src/catalog.rs. compat/tmux-gaps.json and compat/results/summary.md are shared: keep your edits to your own groups and rows.
PROTOCOL: no bump expected. Key names travel as strings today; check before you assume, and if a new wire field is genuinely unavoidable, bump to 99 and say so loudly.

1. keys.default-prefix, SPLIT. HARD BUDGET 90 MINUTES. The group is accepted native with 61 binding:prefix:<key> items under the one-sentence reason "Picker and sidebar bindings are part of the zz GUI experience." The review measured that it omits 33 stock prefix keys and changes four (knowledge/tmux/key-tables.md:190-206 lists them; crates/zz-protocol/src/key.rs:53-90 is the table): `x` and `&` kill without confirm-before, `]` pastes without -p, `?` drops -N, `d` is unbound (the raw TUI detaches on the chrome chord `ui C-d`, crates/zz-client/src/chrome.rs:692). confirm-before and paste-buffer -p are implemented. PRODUCT DECISION (record it as the rule says): adopt the pin's EXACT default command for `x & ] ? d PPage f . ( ) L m M i ~ # - ' M-n M-p` (read key-bindings.c in the pinned source for each string; do not type them from memory), and keep native only where a native zz verb genuinely replaces the pin's (`% " s w r e t D C C-z Tab BTab * @ g < >` and the picker/sidebar keys). Mechanics: create an open group keys.prefix-stock-commands (decision adopt) holding the adopted binding:prefix:<key> slugs relocated out of keys.default-prefix; widen keys.default-prefix's reason to enumerate what stays native and why, per key; then close the new group with proof. Proof, two parts: (a) a detached scenario diffing `list-keys -T prefix` for each adopted key on both binaries (the strings must be identical); (b) an attached pty proof on both binaries that `prefix x` raises the confirm-before prompt (the pin's prompt text is `kill-pane #P? (y/n)`) and that `n` leaves the pane alive while `y` kills it, and that `prefix d` detaches the client. `prefix ]` into a pane with bracketed paste enabled must arrive as a bracketed paste on both.
2. keys.shift-modifier, NEW OPEN GROUP. HARD BUDGET 120 MINUTES. `bind -n S-Left previous-window` (and every S-Up/S-Down/S-Right/BTab binding) loads clean, lists, and can never fire: key.rs input_key_name folds shift only for character keys (lines 1218-1246, no S- branch), the mux stores the spelling; the pin names them in key-string.c:351 and binds S-Up/S-Down by default at key-bindings.c:457-460. The closed group choosers.command-flags recorded "S-Up and S-Down cannot be delivered by zz at all ... recorded rather than built". Items: semantic:shift-special-key-names plus one key:root:<name> per key you deliver (S-Up, S-Down, S-Left, S-Right, BTab at least; add S-Home/S-End/S-PPage/S-NPage/S-Insert/S-Delete/S-F<n> if the fold is generic, and it should be). Fix: emit S- for special keys in the shared key contract, deliver from the raw TUI (which decodes escape sequences itself: xterm modifier parameter 2 and the legacy CSI 1;2 forms both mean shift) AND from the GPUI client (which sees the platform modifier), and map shifted Tab to BTab. Proof: an attached pty scenario on both binaries where `bind -n S-Left previous-window` and `bind -n S-Right next-window` switch the window (read #{{window_index}} after each key), the TUI receiving the same bytes the pin receives (ESC [ 1 ; 2 D). The GPUI half cannot be driven by a pty: prove it with a #[gpui::test] or a unit test on the key fold in crates/zz/src that feeds a shift+Left platform event and asserts the name S-Left reaches the daemon, and say in notes that the desktop half is unit-proved.
3. keys.table-lifecycle, NEW OPEN GROUP. HARD BUDGET 90 MINUTES. After switch-client -T <table> a bound key fires and the table is never left, and unbound keys are swallowed while parked (key.rs:957-984: unbound key with a table set returns Ignore and keeps the table; only prefix resets). Measured on the pin with a control client (the review's prose lens). The pin's rule is server-client.c server_client_key_callback (lines 1490-1497 and 1536-1556 in the pinned source): read it in full and reproduce it, in particular that after a dispatch the client returns to the root table unless the binding is repeat (-r) and repeat-time is running; that an unbound key in a non-root table resets the client to root and RETRIES the key there; and that when the root table has no binding either and the first table tried was not root, the key is dropped rather than forwarded to the pane. Items: semantic:key-table-reset-after-dispatch, semantic:unbound-key-retry-in-root. Proof: an attached pty scenario on both binaries: `bind -T resize Left resize-pane -L 5; bind -T resize Right resize-pane -R 5; bind -n M-r switch-client -T resize`, then M-r Left, read #{{client_key_table}} (root on both) and #{{pane_width}}; then M-r followed by an unbound key `a`, read #{{client_key_table}} and confirm through capture-pane whether the `a` reached the shell on each binary (the pin drops it); then a -r binding with repeat-time to show the table survives within the repeat window. The excursion into daemon.rs is for whatever per-client table state lives there; keep it minimal and name the hunks in notes.
"""

BATCH_BUFFERS = COMMON + f"""
YOUR BATCH: buffers and VT facts, retrospective findings 6, 7, 13 and 15 plus the first plugin runtime fixture, five pieces. BRANCHNAME: batch-buffers-vt-facts. Your worktree: {M['dev']}/zz-opus-dint (already at origin/main, warm zz-daemon build).
Your zones: crates/zz-daemon/src/ (daemon.rs, status.rs, lib.rs, everything but the switch-client -T key-table storage around daemon.rs:12659, which the other worker may touch); crates/zz-terminal/src/; crates/zz-mux/src/formats.rs and command.rs; crates/zz-protocol/src/catalog.rs; crates/zz/src/lib.rs ONLY in the CLI read (read_stdin_payload) and print/output paths. crates/zz-protocol/src/message.rs only if a bump is unavoidable. The other worker owns key.rs, zz-tui, zz-client, and crates/zz/src's key input path. compat/tmux-gaps.json and compat/results/summary.md are shared: keep your edits to your own groups and rows.
PROTOCOL: no bump expected. v98 already carries RawText arguments and byte-clean output sinks; the terminal facts travel worker -> daemon in-process and are expanded daemon-side.

1. Buffer standard streams. HARD BUDGET 120 MINUTES. `save-buffer -` and `load-buffer -` are refused (crates/zz-daemon/src/daemon.rs:14359 and :14412 UnsupportedCommand). They live as semantic:buffer-standard-streams inside the accepted group protocol.binary-streams, whose reason says "Reopen when a named workload needs" them; the workload is in the campaign's own corpus: oh-my-tmux's prefix y (compat/.cache/plugins/.tmux.conf lines 131-139, `save-buffer - | xclip`) and the wiki idiom `xclip -o | tmux load-buffer - ; tmux paste-buffer`. The pin's cmd-save-buffer.c:115 writes to the command client's stdout, the path show-buffer already uses; load-buffer - reads the command client's stdin. Mechanics: relocate semantic:buffer-standard-streams into a new open group buffers.standard-streams (decision adopt), rewrite protocol.binary-streams's reason and third acceptance clause so the refusal list is `display-message -I`, `split-window -I`, `source-file -` and show-buffer's binary policy (quote the old clause and the workload that refuted it), then close the new group: `save-buffer -` as show-buffer's byte-clean stdout path (with -a semantics preserved where the pin has them) and `load-buffer -` through the CLI's existing bounded read_stdin_payload as a RawText argument (say the cap in the reason). Proof: a smoke scenario through run-shell on both binaries (`printf 'a\\0b\\xff' | tmux load-buffer - ; tmux save-buffer - | od -c`) plus a differential row for `load-buffer -b name -` and `save-buffer -a -`.
2. The four terminal facts. HARD BUDGET 150 MINUTES. #{{history_size}}, #{{cursor_x}}, #{{cursor_y}} and #{{alternate_on}} answer 0 (crates/zz-mux/src/formats.rs:496,540,541,545 Zero). They sit in the accepted group formats.terminal-runtime (28 names) whose reason says to reopen "once a workload asks": tmux-resurrect's save.sh:126-131 reads history_size and cursor_y to size its capture and :143 runs `capture-pane -epJ -S "-$history_size"`, which is `-S -0` on zz, so restored panes silently lose their scrollback. Reopen THOSE FOUR NAMES ONLY (not the other 24) by relocating format:history_size, format:cursor_x, format:cursor_y and format:alternate_on into a new open group formats.terminal-facts (decision adopt); rewrite formats.terminal-runtime's reason and first acceptance clause to 24 names and say why these four left. Back them through the bounded terminal-fact channel the reason already names: pane_pb_state already flows from the worker's byte filter, and libghostty exposes cursor and scrollback getters; keep PTY drain unblocked (the group's second clause). Proof: a differential scenario that runs `seq 1 100` in an 80x24 pane, settles, and reads `display -p '#{{history_size}} #{{cursor_x}} #{{cursor_y}} #{{alternate_on}}'` on both (the pin answered `79 23` for the first two of history_size/cursor_y after seq 1 100 in the review's probe; measure the exact tuple yourself); then `printf '\\033[?1049h'` and read alternate_on again; then `\\033[?1049l`. If a name cannot be made exact within the budget, close what is exact and leave the rest open with the measurement in the reason.
3. CLI output bytes. HARD BUDGET 45 MINUTES. crates/zz/src/lib.rs:1853-1863 returns early on empty output and appends a newline: `display -p ''` prints nothing where the pin prints one `\\n`, and `show-buffer` gains a trailing newline where the pin writes the raw bytes (5 raw bytes for a 5-byte buffer, measured). Two scenarios were written around it instead of a slug; find them (grep compat/scenarios for show-buffer and `display -p ''`) and make them assert the pin's bytes. New open group clients.cli-output-bytes with semantic:cli-output-byte-fidelity; close it: write bytes as the pin's command client does. Proof: a smoke scenario piping both through `od -c` on both binaries.
4. refresh-client -S. HARD BUDGET 45 MINUTES. `refresh-client -S` and bare `refresh-client` error with "interactive behavior" (daemon.rs:13094-13099); the pin's cmd-refresh-client.c:282-287 only forces a status job refresh (server_status_client / status_force) and a repaint. The slug semantic:refresh-client-interactive lives in the accepted group clients.interactive-refresh, whose second acceptance clause says the "interactive redraw and pan family stays loudly unsupported". RE-SCOPE that clause: -S forces an immediate #() re-run and returns 0, bare does the same plus a no-op redraw, the pan family (-c -D -L -R -U -l -r) stays refused; record the old clause and the pin measurement. Proof: a scenario where status-right carries `#(cat /tmp/<file>)` with status-interval 0 (or a long one), the file changes, `refresh-client -S` is run, and `display -p '#{{T:status-right}}'` (or the status row through the daemon's status sampler) shows the new value on both binaries, exit 0 on both; and a row proving `refresh-client -U` still errors on zz.
5. tmux-resurrect save fixture. HARD BUDGET 60 MINUTES. The plugin corpus is proved only by parse and list-keys; no plugin runtime path runs. Write smoke/resurrect-save: on both binaries source compat/.cache/plugins/tmux-resurrect/resurrect.tmux with `set -g @resurrect-capture-pane-contents 'on'` and a scratch `@resurrect-dir`, put `seq 1 100` in a pane, run scripts/save.sh (through run-shell the way the plugin's binding does), and diff the saved pane-content file and the session line (normalise the timestamp, the dir and the pid; keep the line count). This is the runtime proof for piece 2 and the first per-plugin runtime fixture; add it to the summary rows. If the plugin needs a `tmux` on PATH inside the pane, note that the harness wrapper provides one and that the product does not (retrospective finding 3, cycle 15's decision), and say so in the scenario's header comment and in notes.
"""

BATCH_INSTRUMENT = COMMON + f"""
YOUR BATCH: make `compat/run.sh --attached-client` PASS on this box, so the gate can stamp the summary footer; this is the instrument pass the retrospective put before cycle 14 and HANDOFF.md's "The instrument pass, as left" describes. BRANCHNAME: instrument-attached-fixture. Your worktree: {M['dev']}/zz-opus-termopts (already at origin/main; last built at a cycle-10 tip, so the first cargo build -p zz recompiles the workspace crates, 10-15 minutes). Two code lanes are running on this box at the same time, so builds and pty timings are under load; the gate will face the same load, so make the fixture robust to it rather than waiting for a quiet box.
Your zones: compat/ ONLY: compat/attached-client.sh, compat/scenarios/** (fixtures and the three named scenarios), compat/diff-scenario.sh, compat/run.sh, compat/results/summary.md, and a registry note if a fixture failure turns out to be a real zz defect. NO crates/ change: if a probe fails because zz genuinely diverges, record the measurement (a known differential row, or a registry item with the probe) and make the fixture name that divergence explicitly; never patch the product and never weaken a probe to pass.
PROTOCOL: none; no Rust.

What today's full run at 6b6171f measured on this box (log: /tmp/claude-1000/-home-demfabris-dev-zz/5b711658-e6a1-4b30-a11a-77510f6f0c70/scratchpad/full-run.log; per-scenario logs under {M['root']}/compat/results/):
1. THE FIXTURE, HARD BUDGET 120 MINUTES. compat/attached-client.sh exited 1 with `error: zz command-output mode changed during settle; expected attached|copy-mode-vi, got attached|root` (the zz screen dump that followed showed the raw TUI's own detach/re-attach lines from probe_side, and the tmux side was fine), so cause (c) below is the one that bit today. The last CAMPAIGN-LOG.md entry and HANDOFF.md list the measured causes from eight earlier runs and the order to fix them: (a) probe_command_prompt on the TMUX side raced its own BSpace keys against the prompt opening (answered `mainprompted`); a settle after `C-b ,` before the keys fixes it: wait for the prompt to be open (a daemon-side fact or the rendered prompt row), never a bare sleep; (b) probe_side once never saw ATTACHED_ROOT_OK because the first keys beat the zz client's readiness: wait for the client to be attached and drawing before the first key; (c) the command-output view under vi: #{{client_key_table}} is the observable (do NOT swap it for pane_in_mode, which measures nothing there), and one run saw it drop to root after Escape where a direct probe keeps the view open; resolve it by PROBE on both binaries and make the fixture assert what both actually do, recording any real divergence; (d) the search prompt string was already fixed. Run the fixture solo first with a zz built at your tip (`compat/attached-client.sh $PWD/target/debug/zz {M['root']}/compat/.cache/tmux-src/tmux`, about ten minutes) to see which probe fails NOW, then fix in the order above. A dying run leaves zz daemons on /tmp/zza-*.sock that the trap does not reach: reap them by pid from the socket name, never by pkill -f; fix the trap so it reaps them too. Proof: the fixture passing TWICE in a row solo at your tip, under whatever load the box has.
2. THREE ENVIRONMENT ROWS, HARD BUDGET 60 MINUTES together. They fail the full run on this box (run.sh marks a nonzero scenario as failed, so no summary can be written here until they pass): (a) smoke/format-modifier-interrogate fails on BOTH sides identically (`absent-smxx want=[0] got=[1]` and `no-feature-strikethrough want=[0] got=[1]`): the harness's outer TERM carries smxx, so the probe measures the box's terminfo rather than the binaries; pin or scrub TERM for that fixture (or in diff-scenario.sh for both sides if that is the honest general fix) and say which. (b) smoke/remain-on-exit-format fails on the PIN side: `signal-notice-is-drawn want=[DEADFMT[][term]] got=[DEADFMT[][15]]`; the pin prints the signal number on Linux where the fixture's literal expects the macOS name: read how the pinned source formats the dead-pane notice (window.c / screen-write, the remain-on-exit status line) and derive the expectation the way the pin does so both platforms pass. (c) smoke/pane-engine-knobs-input fails on the PIN side (`'sh ~/pane-engine-knobs-input.sh' returned 1`), recorded as a pin-side flake under load: run it solo three times; if it is load-only, make the fixture wait for the condition it races instead of tolerating a failure; if it fails solo, measure why and fix or record.
3. IF TIME REMAINS (do not start it with less than 100 minutes left): one full `ZZ_COMPAT_ZZ=$PWD/target/debug/zz {RUN_ENV} compat/run.sh --attached-client` at your tip, which on a pass rewrites compat/results/summary.md with `Recorded at: <your tip>`; commit that summary as its own commit ("Record the attached-client run at <sha>") and push. If you skip it, say so; the gate re-records at the merged tip either way.
Every wait you add is a wait-for-condition with a bounded timeout that fails loudly, never a sleep; every probe keeps its observables and expected strings; the reviewer will diff your fixture against the old one looking for a blinded probe. Report the fixture's before/after outcome per probe in notes.
"""

REVIEW_INSTRUMENT = """LANE-SPECIFIC SPOT-CHECKS (instrument, compat-only): this lane changes only the harness, so the whole review is about whether the fixture got BLINDED. Diff compat/attached-client.sh and the three scenario fixtures against origin/main probe by probe: every wait must be a bounded wait-for-condition on the same observable as before, every expected string must be unchanged or justified by a pin measurement quoted in the diff, and a probe that now passes on zz must be shown to still FAIL if you break the behaviour it guards (pick two probes and sabotage them on a throwaway copy). Do NOT build zz in your review worktree (it has no warm target): the worker built it at the same tip in /home/demfabris/dev/zz-opus-termopts/target/debug/zz; verify `git -C /home/demfabris/dev/zz-opus-termopts rev-parse HEAD` equals the tip and the tree is clean, then use that binary (ZZ_COMPAT_ZZ=... for run.sh, the path directly for attached-client.sh). Run compat/attached-client.sh yourself twice at the tip with that zz and the pin; run the three environment rows solo; confirm no crates/ file changed (git diff --stat origin/main...HEAD) and that any recorded divergence has a durable home. If the lane committed a stamped summary, confirm `compat/run.sh --check-summary` is green at that commit."""

REVIEW_COMMON = f"""You are an adversarial code reviewer for the zz tmux-compat campaign (repo demfabris/zz). You are Codex (gpt-6-astra) running non-interactively under `codex exec`. A worker just pushed a campaign branch; your verdict decides what the integration gate trusts. Read-only toward history: NEVER push, NEVER commit, NEVER touch the board or GitHub issues, never edit {M['root']} itself.

HOW YOU REPORT: your FINAL message must be ONLY the JSON object the output schema describes (lane, verdict, confirmed_defects, checks_run, notes). Run every command synchronously; never leave anything running; never end your turn to wait. A reviewer whose final message is not the report is a failed agent and the gate audits the lane itself.

SETUP: your working directory is the review worktree the orchestrator checked out detached at the branch tip. {M['gitNote']} The shared checkout is {M['root']}; read it, never edit it. {M['boxNote']}

MACHINE ETIQUETTE: the other lane may still be working on {M['cores']} cores. cargo test -p <pkg> --jobs {M['workerJobs']} -- --test-threads={M['workerThreads']} only, no workspace-scale anything, cargo output to a log file + check exit code. Timeout-guard cli_binary tests (wait_exit can hang under load). Throwaway pin servers -L zzprobe-$$ only, kill after; never kill servers you did not start ({M['protected']}), never pkill tmux or zz.

METHOD, in order of value:
1. CONTRACT AUDIT: for every closed OR relocated slug (worker report + git diff origin/main...HEAD -- compat/tmux-gaps.json), find the proof in the diff that asserts the group acceptance clause. A test asserting zz behavior with no pin derivation is a defect; quote the clause. A relocation OUT of an accepted group is legitimate only when that group's reason and acceptance now record what left, the old clause, and the measurement that refuted it; a relocation INTO an accepted group is legitimate only when its reason states the measured pin behavior and the product stance. A RE-SCOPED acceptance clause is legitimate only when the reason records the old clause, the refuting measurement, and the probe; re-run that probe on the pin yourself. A PRODUCT DECISION the prompt handed the worker is legitimate only when the reason quotes the old clause, the measured pin behaviour, the new stance and the reversibility sentence, AND every binding closed under it is proved twice: identical list-keys strings on both binaries and the binding doing its job on an attached client. Anything proved for key bindings firing, copy mode, prompts, choosers, menus, popups, focus, mouse, or hooks must drive a real attached client where the clause names one; a headless or detached proof of those is a defect (the GPUI half of a key fold may be unit-proved if the notes say so and the TUI half is pty-proved).
2. PROOFS AT TIP: run the worker's claimed proof suites yourself at the branch tip (cargo test -p for every touched crate; the named scenarios). ALSO run cargo test -p zz-daemon --lib whenever the diff touches crates/zz-mux/src/command.rs, formats.rs, model.rs or layout.rs, even if the worker did not claim it, and cargo test -p zz-client plus cargo build -p zz-tui -p zz -p zz-client-ffi whenever a wire type changed. Any red at tip is a blocker regardless of the report.
3. ORACLE SPOT-CHECKS: 3-5 riskiest claims verified against the pinned binary yourself ({M['root']}/compat/.cache/tmux-src/tmux; the pinned C source sits beside the binary, read it when subtle). Past best catches were pin-side gating the worker's fixture had configured away, a probe that passes for the wrong reason, a fixture that resizes or reconfigures the client so it cannot see the ordinary flow, and state left armed after a failure: look for the configuration that would make a fixture blind.
4. TEST HONESTY: run the branch's new/changed tests and its scenarios, AND the pre-existing scenarios that exercise the same surface ({RUN_ENV} compat/run.sh --strict-geometry <names>). Failing/ignored/tautological = blocker. Check the durable registry resolution carries every divergence the worker disclosed in its notes. Check compat/run.sh --check-summary on the branch prints no scenario-inventory diff (the "carries no commit stamp" error alone is expected: the gate stamps the summary after the merge).
5. INVARIANTS: zone discipline (out-of-zone files listed in notes); wire rule (NEITHER lane was expected to bump; a wire-reachable change is itself worth flagging; if genuinely unavoidable it needs the complete 98->99 bump incl. the hex 0x62->0x63 fixture and the knowledge mirrors); no code comments; doc comments still attached to the fn they describe; no attribution trailers; registry round-trips (python3 -m json.tool); python3 compat/tmux-tracker.py check green on the branch; cargo test -p zz-mux green (manifest counts).
6. CALIBRATION: default to refuting each close, but confirmed_defects only with PROOF (probe, failing rerun, quoted contradiction). Suspicion goes in notes. blocker = wrong close or would break main; must-fix = gate applies before merge; nit = mention. When a blocker is a wrong close, say whether reverting that commit applies cleanly at tip and what it takes with it.
VERDICT: approve / approve-with-fixes / reject (a blocker the gate cannot fix in minutes). checks_run lists exact commands. Thorough but bounded: well under an hour.
"""

REVIEW_KEYS = """LANE-SPECIFIC SPOT-CHECKS (keys): on the pin, `prefix x` must show `kill-pane #P? (y/n)` with #P expanded and `n` must leave the pane; check zz's prompt text is identical through the daemon's prompt state or the TUI row, not by trusting the binding string. For S-Left: write ESC [ 1 ; 2 D to the TUI's pty and confirm #{window_index} moved on BOTH binaries; then confirm a plain Left still reaches the pane (no over-folding). For the table lifecycle: with a control client on the pin, reproduce the worker's M-r / Left / `a` sequence and confirm the pin drops the unbound `a` (capture-pane shows no `a`), then the same on zz. Confirm the adopted prefix keys' list-keys strings are byte-identical to the pin's on both binaries, including the `-N` note text where the pin has one."""

REVIEW_BUFFERS = """LANE-SPECIFIC SPOT-CHECKS (buffers): `printf 'a\\0b\\xff' | tmux load-buffer - ; tmux save-buffer - | od -c` must be byte-identical on both binaries through run-shell, and `save-buffer -a -` must match the pin's -a-to-stdout behaviour (read cmd-save-buffer.c for what -a means with `-`). `show-buffer` on a buffer with no trailing newline must print exactly the buffer bytes on zz (od -c), and `display -p ''` exactly one newline. For the terminal facts, measure the pin yourself after `seq 1 100` in an 80x24 pane and after ESC [ ? 1 0 4 9 h, and confirm zz answers the same tuple through the scenario, not through a unit test alone; also confirm the other 24 names still answer their pinned defaults (run the pre-existing formats scenarios). For refresh-client -S: on the pin, prove -S actually re-runs #() (a counter file incremented) and that zz does the same; confirm -U still errors on zz. For the resurrect fixture: read save.sh and confirm the fixture runs the real script, not a reimplementation, and that the saved pane-content file on zz carries the scrollback lines the pin's does."""

LANES = [
    dict(key="instrument", prompt=BATCH_INSTRUMENT, lock="F-INSTRUMENT-ATTACHED-FIXTURE", workdir="zz-opus-termopts", reviewdir="zz-review-copy", review_extra=REVIEW_INSTRUMENT),
    dict(key="keys", prompt=BATCH_KEYS, lock="F-KEYS-CONTRACT", workdir="zz-opus-panes", reviewdir="zz-review-client", review_extra=REVIEW_KEYS),
    dict(key="buffers", prompt=BATCH_BUFFERS, lock="F-BUFFERS-VT-FACTS", workdir="zz-opus-dint", reviewdir="zz-review-dint", review_extra=REVIEW_BUFFERS),
]


def gate_prompt(summaries):
    return f"""You are the integration gate for the zz tmux-compat campaign (repo demfabris/zz, board = GitHub issue 7). You are Codex (gpt-6-astra) running non-interactively under `codex exec`; all workers and reviewers are done; you run ALONE on this {M['machine']}, full speed. Run every command synchronously and wait for it; never leave anything running; never end your turn to wait. Your FINAL message must be ONLY the JSON object the output schema describes (merges, progress_after, board_updates, attached_client, problems). Integrate IN THIS ORDER: instrument first (compat-only: the attached fixture and three environment rows; it must be on main before the stamped run in step 7), keys second (key.rs, the clients, one declared daemon.rs hunk, and commit 93f90bea's fix of the racy client_focus_closes_display_panes_and_preserves_chooser_modes assertion, which the buffers reviewer requires to be on main before the buffers daemon suite is judged), buffers third (its daemon.rs diff is the wide one and its tests the slowest; rebase it onto the keys merge so its daemon run carries the fixed assertion). One gate per branch, and push main as soon as each lane's gate is green (if you die mid-run the orchestrator resumes from what already landed). A branch whose diff touches nothing under crates/ (git diff --stat origin/main...HEAD) skips stages 3a and 3b and runs 3c and 3d only. Both code lanes were REJECTED once and fixed on the same branch, then re-reviewed: the summaries below carry the re-review verdicts; the first rejections were about the client_focus_closes_display_panes_and_preserves_chooser_modes flake (the keys lane fixed the racy assertion in 93f90bea, so after the keys rebase the buffers tree gets it too), the missing attached proofs and -N notes on the keys lane, and the 24-name terminal-runtime contract on the buffers lane.

Lane summaries, worker report + Codex review verdict each:
{json.dumps(summaries, indent=2)}

REVIEW VERDICTS BIND YOU: approve-with-fixes => apply every must-fix on the branch (own follow-up commit) before its gate, re-running the reviewer's failing probe to prove each fix. reject => do NOT merge; post the blockers as a board note on the lock front, leave the branch, continue. A blocker you can genuinely fix in minutes may be fixed and merged with the probe re-run as proof; a wrong close whose revert applies cleanly is such a fix (precedent 0fec342 + 9cab1fa: revert, then a records commit that puts the reviewer's measurement into the group reason and acceptance). review_actions must account for every confirmed defect. Missing review (null) => do a compressed contract audit yourself before gating. If you SKIP a lane, push its rebased-and-fixed tip to origin as campaign/<name>-gated before removing the worktree.

BOARD IDENTITY: ZZ_BOARD_HOLDER={M['holder']} python3 compat/board.py <cmd> from inside a repo checkout. Verbs: release/withdraw REQUIRE --reason; note takes --note; candidate takes --commit --branch --base + repeatable --proof; integrated takes --merge + optional --gate; renew <FRONT> --lease 2h (always give a unit); front needs --contract --zones [--priority <int> --kind --deps --notes]; withdraw and front need TRIAGE held; integrated needs MAIN held. The orchestrator holds the three lock fronts F-INSTRUMENT-ATTACHED-FIXTURE (instrument lane), F-KEYS-CONTRACT (keys lane) and F-BUFFERS-VT-FACTS (buffers lane) (8-10h leases from launch; expired => claim back as {M['holder']} before that lane's ledger step). BOARD FALLBACK: if a board command fails on GitHub authentication, append the exact command line you meant to run to {M['root']}/compat/orchestration/board-replay-14.sh (do not commit that file), say so in board_updates, and continue the gate; the orchestrator replays the file. Never let a board failure stop a merge.

{M['gitNote']} Fetch git fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*'; push git push origin HEAD:main. The shared checkout {M['root']} is where you start; its local main branch may be stale; never use it, never edit it, never stash or reset in it.
knowledge/tmux/gaps.md is generated: regenerate with tmux-tracker.py write-report on every conflict, never hand-merge it.
PROTOCOL RECONCILE: NEITHER lane was expected to bump, so a branch that carries a 98 -> 99 bump is a finding: check its reviewer justified it. If one or both did bump, reconcile to ONE constant at 99 with every v99 changelog bullet preserved side by side and one hunt_claims fixture at 0x63; verify cargo test -p zz-protocol after reconciling. catalog.rs counters, the option-consumer roster and compat_manifest_tests.rs counts conflict between lanes routinely: resolve by recounting, then cargo test -p zz-mux and -p zz-protocol, and grep the tree for conflict markers before building. Both lanes may touch crates/zz-daemon/src/daemon.rs in declared, disjoint regions (buffers: everything; keys: the switch-client -T key-table storage only); rebase conflicts there are resolved by keeping both hunks and letting the workspace run judge. When a rebase conflicts on compat/tmux-gaps.json, {M['root']}/compat/orchestration/gaps-merge.py BASE OURS THEIRS OUT merges the two sides by record id and exits 2 on a record both sides changed differently (feed it git show :1:compat/tmux-gaps.json, :2:, :3:), then regenerate gaps.md and recount. compat/results/summary.md conflicts are row unions: keep every row from both sides, sorted as the file is.

THIS BOX: {M['boxNote']} Use ONE shared build directory for every gate worktree, CARGO_TARGET_DIR={M['dev']}/zz-gate-target (gates are serial, so it is never contended; leave it in place for the next cycle). Never touch {M['protected']}. Before gating a branch, run git merge-tree --write-tree origin/main <tip> first; it predicts the conflicts and costs one command.

PER BRANCH, in order:
1. Fresh worktree: git -C {M['root']} worktree add {M['root']}-gate-<key> origin/main (remove leftovers with --force first). Rebase branch onto origin/main; compat/tmux-gaps.json conflicts between lanes are normal: merge both sides.
2. First branch only: claim MAIN --lease 3h; hold across all, renew before each subsequent gate, release after the ledger recompute.
3. Code-branch gate stages, in order:
   a. cargo test --workspace --all-features --no-fail-fast --jobs {M['gateJobs']} -- --test-threads={M['gateThreads']} > log 2>&1 (check exit code; never pipe through tail). Timeout-guard: wait_exit_holds_the_control_process_until_a_second_blank_line can HANG under load; if the run wedges >20min with no output, sample the process; a lost-wakeup hang there counts as the known flake (verify solo).
   b. cargo clippy --workspace --all-targets --all-features -- -D warnings
   c. {RUN_ENV} compat/run.sh --strict-geometry --delta origin/main..HEAD --commands <lane touched_commands>. Run --list TWICE and reconcile against git diff --name-only for compat/scenarios; for the keys lane ADD every smoke/*key* and the attached fixtures the lane names; for the buffers lane ADD every buffer, show-buffer, display-message and formats scenario. Shard up to {M['shards']} concurrent run.sh invocations over DISJOINT scenario subsets with separate result logs (compat/run.sh hard-codes RESULTS_DIR, so shards stay apart only by disjoint scenario names); run smoke/source-replay-diagnostics SOLO after the shards (pin-side crash under load); any divergence re-runs alone before being called real.
   d. python3 compat/tmux-tracker.py check && python3 compat/board_test.py. Then compat/run.sh --check-summary: it is EXPECTED to die with the "carries no commit stamp" error at this stage; what must NOT appear is a scenario-inventory diff before that error. The stamp lands in step 7.
4. Flake rules: lone timing test failing loaded + passing exact-solo = flake, proceed (known list: copy-mode reconcile, client_focus_closes… (also fails about one solo run in three; two solo passes of three count), event_hooks_fire_after_mutation_with_captured_formats, history_request_is_guarded…, daemon_native_split_resize_commits_exactly…, nested_alias_queue_bubbles_shutdown…, control_sourced_run_shell_closes_before_raw_output…, request_full_enqueues_only_the_requested_visible_pane, display_menu_resize_lifecycle::a_resize_moves_the_menu…, zz-terminal pty_output_drains_while_the_input_writer_is_backpressured, wait_exit… hang, concurrent_default_interactive… "not a terminal" incl. misattribution, behavior-options one TOPO row under shard load). Anything else red = real: fix if minutes, else SKIP branch (no push to main; push the gated tip as campaign/<name>-gated), record, continue.
5. Push main. Non-fast-forward: fetch; user-authored commits + conflict-free disjoint rebase => bounded rerun (lane package tests + its scenarios), push. Never force. Campaign branches rewritten by the rebase stay at their old tips on origin (never force them); say so in the report.
6. Ledger per successful push against that lane's lock_front: candidate (--commit tip --branch campaign/<name> --base <pre-push main> --proof per stage), note (--note: groups covered, slugs closed/relocated, decisions recorded, reviewer verdict + what you did about defects), integrated (--merge <sha> --gate "workspace+clippy+delta green"), release (--reason "batch integrated at <sha>"). A lane that left one of its groups open gets integrated + a release whose --reason names what is still open.
7. After ALL branches, holding MAIN, in the last gate worktree at the pushed main tip: THE STAMPED FULL RUN. cargo build -p zz (CARGO_TARGET_DIR as above, so zz and zz_cli sit at {M['dev']}/zz-gate-target/debug), then {RUN_ENV} ZZ_COMPAT_ZZ={M['dev']}/zz-gate-target/debug/zz compat/run.sh --attached-client > log 2>&1 (about 30-40 minutes; the fixture alone is ten). On PASS it rewrites compat/results/summary.md with `Recorded at: <sha>`. If the fixture fails: it has known races (a settle after `C-b ,` before the rename keys, a readiness wait before probe_side's first key; see the last CAMPAIGN-LOG.md entry), re-run it once solo (compat/attached-client.sh <zz> <pin tmux>); if it fails again, name the probe in attached_client, leave the summary as it was, and do NOT hand-edit a PASS. A dying run leaves zz daemons on /tmp/zza-*.sock: reap them by pid from the socket name, never by pkill -f. If a corpus row (not the fixture) fails at the merged tip, that is a regression one of the lanes introduced: fix it if minutes, else report it. Then recompute TMUX_COMPAT_TRACKER.md from the merged registry: the headline lines (Campaign delivery; Live work "<open> OPEN + <blocked> BLOCKED = <sum>" counting the post-freeze groups honestly; Ledger settlement; Exit evidence with the scenario and step counts and the stamped attached-client PASS or its failure), the Orchestration line (cycle 14 integrated on the ubuntu box on Codex gpt-6-astra lanes at high reasoning, buffers then keys, one gate alone), the Current checkpoint rows, the Campaign dashboard table rows (Live unresolved, Latest differential, Differential SHA-256 = sha256 of compat/results/summary.md, Ledger settlement), and a new "### 2026-09-05 cycle-14 integration checkpoint" table like the cycle-13 one above it. ALSO refresh the bundle pages that carry live registry and corpus numbers, listed under "Pages that carry live checkpoint numbers" in compat/orchestration/HANDOFF.md. Records commit ("Recompute the live ledger after the cycle-14 merges", including the stamped summary.md), then compat/run.sh --check-summary MUST print "summary current" on that commit (the stamp commit is an ancestor and the fixture script is unchanged since; commits that touched crates/ after the stamp only print a drift warning, never rerun the sweep for them), push, ledger it as integrated MAIN --merge <sha>, release MAIN.
8. Claim TRIAGE. Withdraw fronts fully mooted by the merges (none expected besides the lane locks; the F-SPLIT-MUX-*-V5 chain stays). If a worker's skip reasons prove a group contract is unprovable as written, post that as a residual on the relevant front. Release TRIAGE.
9. python3 compat/progress.py, full output, into progress_after.
10. Remove only your own {M['root']}-gate-* worktrees (after pushing any skipped lane's gated tip). Leave zz-opus-* and zz-review-*.

Never stash/reset anything in {M['root']}. Never kill tmux or zz servers you did not start ({M['protected']}). Report per branch merged/sha/gate_summary/review_actions/flakes, full progress output, board records, the attached-client outcome, problems."""


def log(msg):
    stamp = datetime.datetime.now().strftime("%H:%M:%S")
    print(f"[{stamp}] {msg}", flush=True)


def sh(cmd, cwd=None, check=True, env=None):
    e = dict(os.environ, GIT_TERMINAL_PROMPT="0")
    if env:
        e.update(env)
    r = subprocess.run(cmd, cwd=cwd, env=e, text=True, capture_output=True)
    if check and r.returncode != 0:
        raise RuntimeError(f"{' '.join(cmd)} failed ({r.returncode}): {r.stderr.strip()}")
    return r.stdout.strip()


def run_codex(run_dir, label, cwd, prompt, schema):
    out = run_dir / f"{label}.json"
    if out.exists():
        log(f"{label}: reusing {out}")
        return json.loads(out.read_text())
    (run_dir / f"{label}.prompt.md").write_text(prompt)
    schema_path = run_dir / f"{label}.schema.json"
    schema_path.write_text(json.dumps(schema, indent=2))
    cmd = CODEX + ["-C", str(cwd), "--output-schema", str(schema_path), "-o", str(out), "-"]
    log(f"{label}: codex exec in {cwd}")
    started = time.time()
    with open(run_dir / f"{label}.log", "w") as logf:
        r = subprocess.run(cmd, input=prompt, stdout=logf, stderr=subprocess.STDOUT, text=True)
    mins = (time.time() - started) / 60
    if r.returncode != 0 or not out.exists():
        log(f"{label}: codex exited {r.returncode} after {mins:.0f} min with no report")
        return None
    try:
        report = json.loads(out.read_text())
    except json.JSONDecodeError as exc:
        log(f"{label}: report is not JSON ({exc})")
        return None
    log(f"{label}: done in {mins:.0f} min")
    return report


def prepare_worktree(path, ref):
    root = M["root"]
    if path.exists():
        if sh(["git", "-C", str(path), "status", "--short"]):
            raise RuntimeError(f"{path} is dirty; refusing to reuse it")
        sh(["git", "-C", str(path), "checkout", "--detach", ref])
    else:
        sh(["git", "-C", root, "worktree", "add", str(path), ref])


def renew_loop(stop, fronts):
    while not stop.wait(3600):
        for front in fronts:
            try:
                sh(["python3", "compat/board.py", "renew", front, "--lease", "4h"], cwd=M["root"], env={"ZZ_BOARD_HOLDER": M["holder"]})
                log(f"renewed {front}")
            except RuntimeError as exc:
                log(f"renew {front} failed: {exc}")


def run_lane(run_dir, lane, results):
    key = lane["key"]
    workdir = pathlib.Path(M["dev"]) / lane["workdir"]
    if not (run_dir / f"worker-{key}.json").exists():
        prepare_worktree(workdir, "origin/main")
    worker = run_codex(run_dir, f"worker-{key}", workdir, lane["prompt"], WORKER_SCHEMA)
    results[key] = dict(lane=lane, worker=worker, review=None)
    if not worker or not worker.get("branch"):
        log(f"{key}: no branch pushed; skipping review")
        return
    branch = worker["branch"].removeprefix("refs/heads/")
    sh(["git", "-C", M["root"], "fetch", "origin", "+refs/heads/main:refs/remotes/origin/main", "+refs/heads/campaign/*:refs/remotes/origin/campaign/*"])
    tip = sh(["git", "-C", M["root"], "rev-parse", f"origin/{branch}"])
    reviewdir = pathlib.Path(M["dev"]) / lane["reviewdir"]
    if not (run_dir / f"review-{key}.json").exists():
        prepare_worktree(reviewdir, tip)
    prompt = REVIEW_COMMON + "\n" + lane["review_extra"] + f"""

LANE: {key}. BRANCH: {branch} (tip {tip}). Your worktree is {reviewdir}, checked out at that tip.
WORKER REPORT (verify, do not trust):
{json.dumps(worker, indent=2)}"""
    results[key]["review"] = run_codex(run_dir, f"review-{key}", reviewdir, prompt, REVIEW_SCHEMA)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--run-dir", required=True)
    ap.add_argument("--stage", choices=["all", "work", "gate"], default="all")
    ap.add_argument("--lane", choices=[l["key"] for l in LANES], action="append")
    ap.add_argument("--no-renew", action="store_true")
    a = ap.parse_args()
    run_dir = pathlib.Path(a.run_dir).expanduser()
    run_dir.mkdir(parents=True, exist_ok=True)
    lanes = [l for l in LANES if not a.lane or l["key"] in a.lane]
    log(f"cycle 14 on Codex gpt-6-astra: lanes {[l['key'] for l in lanes]}, stage {a.stage}, run dir {run_dir}")

    sh(["git", "-C", M["root"], "fetch", "origin", "+refs/heads/main:refs/remotes/origin/main", "+refs/heads/campaign/*:refs/remotes/origin/campaign/*"])
    log(f"origin/main is {sh(['git', '-C', M['root'], 'rev-parse', '--short', 'origin/main'])}")

    stop = threading.Event()
    renewer = None
    if not a.no_renew:
        renewer = threading.Thread(target=renew_loop, args=(stop, [l["lock"] for l in lanes] + ["MAIN"]), daemon=True)
        renewer.start()

    results = {}
    if a.stage in ("all", "work"):
        threads = [threading.Thread(target=run_lane, args=(run_dir, lane, results)) for lane in lanes]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
    else:
        for lane in lanes:
            key = lane["key"]
            wp, rp = run_dir / f"worker-{key}.json", run_dir / f"review-{key}.json"
            results[key] = dict(lane=lane, worker=json.loads(wp.read_text()) if wp.exists() else None, review=json.loads(rp.read_text()) if rp.exists() else None)

    summaries = [dict(key=k, lock_front=r["lane"]["lock"], review=r["review"], **r["worker"]) for k, r in results.items() if r["worker"] and r["worker"].get("branch")]
    (run_dir / "summaries.json").write_text(json.dumps(summaries, indent=2))
    log(f"lanes complete: {len(summaries)}/{len(lanes)} branches pushed")
    if a.stage == "work":
        stop.set()
        return

    gate = None
    if summaries:
        gate = run_codex(run_dir, "gate", pathlib.Path(M["root"]), gate_prompt(summaries), GATE_SCHEMA)
    else:
        log("no branches to integrate: all workers came back empty")
    stop.set()
    (run_dir / "result.json").write_text(json.dumps(dict(lanes=results, gate=gate), indent=2, default=str))
    log("cycle 14 finished" if gate else "cycle 14 ended without a gate report")


if __name__ == "__main__":
    main()
