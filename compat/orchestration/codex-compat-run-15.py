#!/usr/bin/env python3
"""Cycle 15 of the tmux-compat campaign on Codex: entrypoint and status.

Two gpt-6-astra implementor lanes (config discovery, pane PATH and the prefix remainder; status
jobs, control-mode notifications and the three measured defects), one Codex reviewer behind each
sharing the worker's warm target, one serial Codex gate to main. Same shape as
codex-compat-run-14.py.

    python3 compat/orchestration/codex-compat-run-15.py --run-dir ~/dev/zz-run-15 [--stage all|work|gate]
                                                        [--lane config|status] [--no-renew]

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
    boxNote="This box: /bin/bash is 5.3 (mapfile and associative arrays work); the file system is btrfs and accepts non-UTF-8 file names. Never write 'rm -rf $HOME' or 'rm -rf ~' even after re-exporting HOME to a scratch path; put the scratch directory in a plain variable (D=/tmp/<name>; rm -rf \"$D\"; mkdir -p \"$D\"; export HOME=\"$D\"). The whole corpus and the attached fixture pass on this box since cycle 14 (stamped at c282741787c9); a red row at your tip is yours. The box has a real ~/.tmux.conf (the user's dotfiles): never read or edit it, and never start a server that would load it (the harness scrubs HOME for you; your own probes use -f /dev/null or a scratch HOME).",
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
- Your working directory is already your worktree, created fresh by the orchestrator at origin/main. Work ONLY there. The shared checkout is {M['root']}: read it, never edit, stash, reset, or clean it (other sessions' uncommitted work lives there; its local main branch may be stale, always use origin/main). On any conflict touching knowledge/tmux/gaps.md, regenerate it with python3 compat/tmux-tracker.py write-report; never hand-merge that generated file.
- {M['gitNote']} Fetch with git fetch origin +refs/heads/main:refs/remotes/origin/main and push with git push origin HEAD:refs/heads/campaign/BRANCHNAME.
- Your worktree has a COLD target: the first cargo build compiles every dependency (gpui, CEF bindings, libghostty). Budget 25-35 minutes for it under the shared load and do not mistake it for a hang; start it FIRST, in the foreground, before you read code, so it overlaps your reading (cargo build -p zz-daemon -p zz --jobs {M['workerJobs']} > build.log 2>&1 is the one to warm; check its exit code).
- {M['boxNote']}

GROUND TRUTH
- The oracle is pinned tmux d77c9dc6 (next-3.8). Prebuilt binary: {M['root']}/compat/.cache/tmux-src/tmux (source tree beside it, read the C freely). Probe with THROWAWAY servers only: -L zzprobe-$$ sockets and -f /dev/null; kill your servers when done. Never kill a tmux or zz server you did not start: the other lane has live ones and so may the user ({M['protected']}); never use pkill/killall on tmux or zz.
- Differential scenarios: compat/scenarios/ (smoke under compat/scenarios/smoke/ with fixtures). Run: {RUN_ENV} compat/run.sh --strict-geometry <scenario-name>. Read 2-3 existing scenarios first to copy the format. A second window in a scenario must be created with new-window -n <name> (bare new-window flakes on automatic-rename). Real pty clients on both binaries: compat/scenarios/smoke/fixtures/pty-drive.py, chooser-drive.py, graphics-drive.py, client-exit-actions.py and keys-prefix-attached show the pattern; anything that needs an attached client (copy mode, prompts, choosers, menus, popups, focus, mouse, KEY BINDINGS FIRING, the status row as drawn) is proved that way, never through a detached differential row. Mouse and paste reach an attached client as bytes on its pty. A `launcher:` scenario header (see smoke/launcher-installed-layout) runs the zz side through the zz_cli launcher on the default socket with no harness wrapper on PATH; use it when the claim is about what an installed zz does. compat/status-row.sh is the row-level status differential; compat/attached-client.sh is the big attached fixture the summary stamp depends on.
- Fixture lessons: line-buffer Python stdout (sys.stdout.reconfigure(line_buffering=True)); bounded WNOHANG reaps that report stalled instead of blocking; never hold a control client's stdin open with sleep | client -C under run-shell; set -g status-keys emacs before measuring prompts on the pin unless the probe is about vi keys (EDITOR in the environment flips it); use -f /dev/null servers so the box's own ~/.tmux.conf cannot hide a derivation; one Control connection per command when a differential needs two commands down one pipe. A prompt can be raised from the CLI with command-prompt -t <client>. The zz raw TUI reserves a sidebar (about 29 columns) and 2 rows around a pane, so its screen is not cell-comparable to the pin's 80x23 pane; daemon-side facts (formats, list-panes, capture-pane, hooks writing to files, list-keys, the copy-mode format family, #{{client_key_table}}) are the comparable channel. Every wait is a bounded wait-for-condition on an observable, never a sleep.

REGISTRY
- Contracts live in compat/tmux-gaps.json; a gap group's acceptance list IS the contract. The frozen agreed scope (compat/progress-baseline.json, 304 items) is CLOSED; everything this cycle touches is scope added since the freeze, which python3 compat/progress.py reports as "scope added since freeze (tracked, not diluting the %)". The retrospective that found your items is compat/orchestration/CAMPAIGN-REVIEW.md: read your findings' rows there first, they carry the measurements and file:line evidence (line numbers there predate cycle 14; grep for the function names). The four groups cycle 14 left open carry their probes in their reasons: read them before touching them.
- NEW OPEN GROUP: append to gaps[] with exactly the fields id, title, decision ("adopt"), status ("open"), priority ("now"), ease, impact (subset of admin/daily/gui/remote/scripts), owner (one of client/daemon/gui/mux/protocol/terminal), items (sorted, each matching an item pattern in compat/tmux-tracker.py ITEM_PATTERNS, e.g. semantic:<slug>, key:<table>:<key>, binding:<table>:<key>, format:<name>), evidence (resource:/scenario:/file: entries that exist), acceptance (the measured pin behaviour as clauses), depends_on ([]), reason. Every item belongs to exactly one group, so an item RELOCATED out of an accepted group is removed from that group's items array and the accepted group's reason and acceptance are rewritten to say what left and why (quote the old clause, the measurement that refuted it, and the probe). When you PROVE an item, remove its slug from the group's items array and update reason/evidence; a group whose items array empties moves from gaps[] to closed[] with closed_on and resolution. Patterns: git show 9e85bc00 -- compat/tmux-gaps.json (plain close), 1f24a1f1 (relocation), c6ce82c4 (flag promotion: catalog.rs unsupported_flag -> real option, plus the hard-coded (supported, unsupported) counters and usage_overrides length in catalog.rs, plus PINNED_TMUX_USAGE_OVERRIDES adjustments), 0fec342 and 9cab1fa (a reverted close and the reason that records why), and cycle 14's 533d253c / c2827417 for post-freeze groups. Script edits with json.dump(..., indent=2) + trailing newline.
- A PRODUCT DECISION handed to you by this prompt was made by the owner: record it with the old clause, the measured pin behaviour, the new stance, and the sentence "decided 2026-09-05 by fabrico for cycle 15; reversible" so it can be undone knowingly.
- crates/zz-mux/src/compat_manifest_tests.rs hard-codes partition counts (tracked == divergent for keys, constant/delegated format partitions, catalog supported/unsupported, option consumers); cargo test -p zz-mux is a mandatory gate for ANY registry edit. Read python3 compat/tmux-tracker.py check rules (STATUSES open/blocked/accepted; accepted requires decision native|never; park requires blocked) before relocating anything.
- python3 compat/tmux-tracker.py check before each commit; if it flags report freshness, regenerate (write-report) and commit the regenerated file too.
- SUMMARY RULE: compat/run.sh --check-summary is GREEN on main (attached-client PASS recorded at c282741787c9). Commits that touch crates/ after that stamp only print a drift warning; the GATE re-records the stamp with a full run after your lane merges. Do not run the full sweep yourself and do not hand-edit the footer. If you add scenarios, add their summary rows to compat/results/summary.md so the scenario INVENTORY matches, then run compat/run.sh --check-summary and confirm it prints "summary current" (the drift warning is fine; an inventory diff is not).

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

BATCH_CONFIG = COMMON + f"""
YOUR BATCH: the entrypoint, retrospective findings 2 and 3 plus the prefix remainder cycle 14 left open, three groups. BRANCHNAME: batch-config-entrypoint. Your worktree: {M['dev']}/zz-lane-config (fresh at origin/main, cold target).
Your zones: crates/zz-daemon/src/paths.rs and lib.rs; crates/zz-daemon/src/daemon.rs ONLY in the startup config loading (startup_mux_config_files and what calls it) and the pane spawn environment (the two TMUX_PANE env sites); crates/zz/src/config/import*.rs and crates/zz/src/lib.rs ONLY for the import subcommand wiring (the other worker owns lib.rs's command output writer); crates/zz-protocol/src/key.rs for the three prefix bindings; crates/zz-mux/src/command.rs ONLY in step_window / step_window_in_session and find_window (the other worker owns the pane_dead_signal spelling in the same file); site/ and knowledge/ pages that describe config import or the pane environment. The other worker owns crates/zz-daemon/src/status.rs, crates/zz/src/control_mode.rs, crates/zz-client, crates/zz-tui and the rest of daemon.rs. compat/tmux-gaps.json and compat/results/summary.md are shared: keep your edits to your own groups and rows. Name every daemon.rs and lib.rs hunk in notes.
PROTOCOL: no bump expected. Config paths and PATH are daemon-local.

1. config.discovery, NEW OPEN GROUP. HARD BUDGET 150 MINUTES. zz boots from zz/mux.conf only (paths.rs default_mux_config; daemon.rs startup_mux_config_files) and never reads ~/.tmux.conf; the import command copies the first of three candidates (paths.rs tmux_config_candidates_for: no /etc/tmux.conf, first existing wins) verbatim into mux.conf, and that copy drifts from the file tpm and `bind r source-file ~/.tmux.conf` keep editing. The harness injects every config with -C source-file, so discovery was never measured. The slug semantic:config-files-native-discovery sits inside the accepted group presentation.native-status whose one-sentence reason never mentions config. The pin: Makefile.am's TMUX_CONF lists /etc/tmux.conf, ~/.tmux.conf, $XDG_CONFIG_HOME/tmux/tmux.conf and ~/.config/tmux/tmux.conf, and cfg.c start_cfg loads EVERY existing one in that order when no -f is given; -f replaces the whole list; read both files and quote them in the reason. PRODUCT DECISION (fabrico's, record it as the rule says): zz reads the tmux candidates IN PLACE, in the pin's order, and then zz/mux.conf LAST so zz-specific settings win; with -f the given file replaces the tmux candidates and mux.conf still layers on top; the import copy goes away (the import entry point stops copying and tells the user zz reads the file in place; delete the copy code rather than keeping a compatibility path; update site/src/content/docs/docs/tmux.md and any knowledge/ page that documents the copy). Mechanics: relocate the slug into config.discovery (decision adopt), rewrite presentation.native-status's reason to say what left and why, then close config.discovery with proof. HARNESS: diff-scenario.sh scrubs HOME and XDG_CONFIG_HOME to scratch dirs for both sides and starts the pin with -f /dev/null; make sure the zz side gets the same isolation by default (a scenario must opt in to discovery, e.g. a `home:` header or fixture files written into the scratch HOME before the servers start, so /etc/tmux.conf on some future box cannot leak into the corpus), and say how in the scenario's header comment. Proof: a scenario that starts both servers with no -f where the scratch HOME holds a ~/.tmux.conf AND an XDG tmux.conf, each setting a distinct user option and one shared option (so order is observable), then diffs show-options -g, the shared option's final value, list-keys -T prefix, and a marker set by mux.conf on the zz side; a second row proving -f <file> loads only that file on both; a third under the `launcher:` header so the installed layout is what gets measured. cargo test -p zz-daemon --lib for the startup tests you change.
2. pane.tmux-on-path, NEW OPEN GROUP. HARD BUDGET 90 MINUTES. Pane processes get no `tmux` on PATH: lib.rs configure_shell_job_environment is the only caller of configure_tmux_shim (jobs: run-shell, if-shell, status #()); the pane spawn env in daemon.rs sets TMUX, TMUX_PANE and ZZ_* and no PATH, and a daemon test asserts a pane has no ZZ_TMUX_EXECUTABLE. The harness prepends its own wrapper to PATH for the whole zz side (diff-scenario.sh ZZ_SHIM_DIR), so the plugin corpus was proved under a PATH real users never have: vim-tmux-navigator's vim half, fzf --tmux, tmux-thumbs, tmux-fingers, extrakto, tmux-fzf, tmux-sessionizer in a new-window, tmux-floax all run `tmux` from a pane. PRODUCT DECISION (fabrico's): the pane's PATH gets the daemon's private tmux wrapper directory prepended, the same one jobs already get, so inside a zz pane `tmux <command>` talks to the enclosing zz server the way $TMUX makes it on the pin. Measure the pin first: inside a pane, `tmux display -p '#{{pane_id}}'` answers the pane's own id, and bare `tmux` with TMUX set refuses with "sessions should be nested with care, unset $TMUX to force" (read tmux.c for the exact text); the review's tail notes zz's wrapper execs the bundled zz so a bare `tmux` from a job opens the desktop GUI: the wrapper in a pane must give the pin's nested refusal instead (measure zz's current answer and record it). Items: semantic:pane-tmux-on-path, semantic:pane-nested-tmux-refusal. Rewrite the daemon test that asserts the absence. Proof: a `launcher:` scenario (no harness wrapper on PATH) that send-keys `tmux display -p '#{{pane_id}}' > $D/out` into a pane on both binaries and diffs the file, then `tmux` bare and diffs the refusal text and exit status; plus a corpus row: vim-tmux-navigator's own script (compat/.cache/plugins) run from the pane shell, on both.
3. keys.prefix-stock-commands, the remainder. HARD BUDGET 90 MINUTES. Cycle 14 adopted 17 stock prefix keys and left binding:prefix:f, binding:prefix:M-n and binding:prefix:M-p open with acceptance clauses 3 and 4 written from the pin (read them in the group). f: the pin binds f to a command-prompt that runs find-window (key-bindings.c; copy the exact string and -N note); zz already has find-window as a mux verb (command.rs find_window) presented through the native chooser (choosers.native-presentation, accepted native, keeps presentation:find-window-native-chooser): the binding must prompt, and after a matching name is submitted open the filtered chooser and select the chosen window on an attached client. M-n/M-p: `next-window -a` / `previous-window -a` (step_window_in_session's alerted_only) must select the next/previous window with an alert where the predicate covers the pin's bell, activity AND silence classes; equal stored strings are not proof. Proof: extend smoke/keys-prefix-attached (or add a sibling) driving all three on both real pty clients: for M-n, monitor-activity on in a hidden window, write output into it, wait for the activity flag through #{{window_activity_flag}}, press prefix M-n and read #{{window_index}}; the same for a bell (printf '\\a') and for monitor-silence; for f, submit a window name and read #{{window_index}} after the chooser selection. Close the group when all three prove; otherwise leave the unproved binding open with the measurement.
"""

BATCH_STATUS = COMMON + f"""
YOUR BATCH: status and control mode, retrospective findings 9 and 12, plus the three defects cycle 14 measured and registered, five pieces. BRANCHNAME: batch-status-jobs-control-notify. Your worktree: {M['dev']}/zz-lane-status (fresh at origin/main, cold target).
Your zones: crates/zz-daemon/src/status.rs; crates/zz/src/control_mode.rs; crates/zz-daemon/src/daemon.rs ONLY at the status render call sites (where status.rs is invoked under the lock on the interval thread and on attach) and the control-mode notification emission; crates/zz-client/src/ and crates/zz-tui/src/ for the command-output prompt lifecycle; crates/zz/src/lib.rs ONLY the CLI command output writer (the other worker owns the import subcommand wiring in the same file); crates/zz-mux/src/command.rs ONLY the pane_dead_signal spelling (the other worker owns step_window and find_window in the same file); crates/zz-protocol/src/message.rs only if a bump is unavoidable. The other worker owns paths.rs, lib.rs's shim and job environment, daemon.rs's startup config loading and pane spawn env, key.rs, and the import code. compat/tmux-gaps.json and compat/results/summary.md are shared: keep your edits to your own groups and rows. Name every daemon.rs, lib.rs and command.rs hunk in notes.
PROTOCOL: no bump expected. Status output and control-mode lines are produced daemon-side or rendered by the control client from the snapshot it already receives; if the live-layout fix genuinely needs a new snapshot field, bump to 99 and say so loudly.

1. status.background-jobs, NEW OPEN GROUP. HARD BUDGET 150 MINUTES. Status #() runs synchronously under the status mutex with a 2 s cap (status.rs SHELL_TIMEOUT 2 s, SHELL_POLL_INTERVAL 10 ms, the blocking try_wait loop) and the daemon renders under the lock on the interval thread and on every attach (find the call sites in daemon.rs): attach stalls up to 2 s per slow segment, and a segment slower than 2 s renders blank forever where the pin shows the last value. The pin (format.c format_job_get, job.c): the job runs with JOB_NOWAIT, the format expands to the cached fj->out (empty on the first run), the job is re-run when status-interval has elapsed since fj->last, and refresh-client -S forces it; the review probed `[#(sleep 3; echo slow)]` answering `[]` in 4 ms. Read format_job_get in full and reproduce its rules (first expansion empty, cached output, re-run cadence, the per-client/per-format job key, what happens when the same #() appears in two formats). Items: semantic:status-shell-jobs-nonblocking, semantic:status-shell-jobs-cached-output, semantic:status-shell-jobs-rerun-cadence. Keep cycle 14's refresh-client -S clause green (clients.interactive-refresh, smoke/refresh-status). Proof: an attached fixture on both binaries with status-interval 1 and status-right '[#(sleep 3; echo slow)]': read the drawn row (compat/status-row.sh or #{{T:status-right}} through the daemon's sampler) immediately after attach (empty brackets on both), then after the job lands (slow on both), then change the script's output and show the re-run cadence; measure attach latency with the segment present (bounded, well under the 2 s cap) on both.
2. control-mode.notifications, NEW OPEN GROUP. HARD BUDGET 120 MINUTES. Control-mode notifications are never diffed against the pin: every transcript strips % lines (compat/scenarios/smoke/fixtures/source-file-control.sh's awk drops ^%), only about 6 of the pin's notification kinds appear anywhere, the deliberate divergences live in knowledge/tmux/divergences.md (the control-mode rows) and knowledge/designs/tmux-drop-in.md instead of the registry, and %layout-change after refresh-client -C is rendered from the client's last snapshot (control_mode.rs "window-layout-changed" arm). iTerm2 is the one mass consumer (tmux-drop-in.md around line 2100 describes TmuxGateway's startup sequence: -CC in a pty, the list-sessions/list-windows/list-panes format queries, refresh-client -C, the %pause/%continue flags). Build smoke/control-notify: a fixture that runs both binaries in -C, KEEPS the % lines, normalises ids, block numbers and timestamps, and replays a recorded command list covering every notification kind the pin can emit (read control-notify.c for the full list: %begin/%end/%error, %output, %extended-output, %layout-change, %window-add, %window-close, %window-renamed, %window-pane-changed, %unlinked-window-*, %session-changed, %session-renamed, %sessions-changed, %session-window-changed, %client-session-changed, %client-detached, %pause, %continue, %subscription-changed, %paste-buffer-changed, %paste-buffer-deleted, %message, %config-error, %exit); assert parity where zz matches and record every divergence as its own item with the measured bytes (the review's tail names two: zz never emits %window-close where the pin does when a window still linked in the client's session is unlinked elsewhere, and $NAME is not expanded in control mode). %layout-change after refresh-client -C must carry the live layout on zz, not the previous snapshot: fix it and prove it in the same fixture (resize through -C, read the %layout-change line on both). Items: semantic:control-notify-differential, semantic:layout-change-live-after-refresh, plus one semantic:control-notify-<kind> per divergence you keep open. The hour under real iTerm2 on a Mac stays a maintainer task; say so in the reason.
3. clients.cli-output-mixed-queue (open since cycle 14, read its reason and probe). HARD BUDGET 45 MINUTES. The cli-output-bytes fixture measures five bytes (hello) on the pin and six (hello plus LF) on zz for a mixed file_write / cmdq_print queue; the fixture's final assertion records the two behaviours instead of claiming parity. Fix the CLI output writer in crates/zz/src/lib.rs so the mixed queue writes the pin's bytes (read cmd-queue.c cmdq_print and the file_write path for the rule), flip the fixture's assertion to parity, close the group.
4. clients.command-output-pane-prompt (open since cycle 14, read its reason: it names the fixture probe and the exact divergence). HARD BUDGET 60 MINUTES. The pin keeps pane_in_mode=1 while opening and closing the stock / search prompt inside the command-output view; zz drops client_key_table from copy-mode-vi to root as soon as the prompt opens because MuxEffect::CommandPrompt dismisses overlays and takes the client-local command output with it. Fix the client so a command prompt raised over a command-output view keeps the view (and its key table) until the prompt is submitted or cancelled, on the raw TUI and the GPUI client (the GPUI half may be unit-proved if you say so), flip probe_command_output_prompt_lifecycle in compat/attached-client.sh to assert parity, run the whole attached fixture (compat/attached-client.sh <your zz> <pin>) twice, close the group.
5. formats.dead-signal-platform-name (open since cycle 14). HARD BUDGET 30 MINUTES. The pin's format_cb_pane_dead_signal calls sig2name, which prints sys_signame's lowercase name where libc has it (macOS, the BSDs) and the decimal number where it does not (glibc, musl: this pin prints 15); zz's command.rs normalises Terminated to term everywhere. PRODUCT DECISION (fabrico's): match the platform: the decimal number on Linux, the sys_signame spelling on macOS, exactly as tmux built on each OS prints. Fix the spelling in command.rs behind the target OS, make smoke/remain-on-exit-format assert parity instead of printing the known divergence (it currently derives the pin expectation from libc sys_signame availability), close the group.
"""

REVIEW_COMMON = f"""You are an adversarial code reviewer for the zz tmux-compat campaign (repo demfabris/zz). You are Codex (gpt-6-astra) running non-interactively under `codex exec`. A worker just pushed a campaign branch; your verdict decides what the integration gate trusts. Read-only toward history: NEVER push, NEVER commit, NEVER touch the board or GitHub issues, never edit {M['root']} itself.

HOW YOU REPORT: your FINAL message must be ONLY the JSON object the output schema describes (lane, verdict, confirmed_defects, checks_run, notes). Run every command synchronously; never leave anything running; never end your turn to wait. A reviewer whose final message is not the report is a failed agent and the gate audits the lane itself.

SETUP: your working directory is the review worktree the orchestrator checked out detached at the branch tip. CARGO_TARGET_DIR is set in your environment to the WORKER's warm target directory (the worker is finished, its tree is clean at the same tip, and nothing else uses that directory now): workspace crates rebuild once for your path, dependencies are reused. Never cargo clean it. {M['gitNote']} The shared checkout is {M['root']}; read it, never edit it. {M['boxNote']}

MACHINE ETIQUETTE: the other lane may still be working on {M['cores']} cores. cargo test -p <pkg> --jobs {M['workerJobs']} -- --test-threads={M['workerThreads']} only, no workspace-scale anything, cargo output to a log file + check exit code. Timeout-guard cli_binary tests (wait_exit can hang under load). Throwaway pin servers -L zzprobe-$$ -f /dev/null only, kill after; never kill servers you did not start ({M['protected']}), never pkill tmux or zz.

METHOD, in order of value:
1. CONTRACT AUDIT: for every closed OR relocated slug (worker report + git diff origin/main...HEAD -- compat/tmux-gaps.json), find the proof in the diff that asserts the group acceptance clause. A test asserting zz behavior with no pin derivation is a defect; quote the clause. A relocation OUT of an accepted group is legitimate only when that group's reason and acceptance now record what left, the old clause, and the measurement that refuted it; a relocation INTO an accepted group is legitimate only when its reason states the measured pin behavior and the product stance. A RE-SCOPED acceptance clause is legitimate only when the reason records the old clause, the refuting measurement, and the probe; re-run that probe on the pin yourself. A PRODUCT DECISION the prompt handed the worker (fabrico's three: tmux config read in place with mux.conf on top, the daemon's tmux wrapper dir on the pane PATH, the platform's dead-signal spelling) is legitimate only when the reason quotes the old clause, the measured pin behaviour, the new stance and the sentence "decided 2026-09-05 by fabrico for cycle 15; reversible". Anything proved for key bindings firing, copy mode, prompts, choosers, menus, popups, focus, mouse, hooks, or the drawn status row must drive a real attached client where the clause names one; a headless or detached proof of those is a defect (the GPUI half of a client fix may be unit-proved if the notes say so and the TUI half is pty-proved).
2. PROOFS AT TIP: run the worker's claimed proof suites yourself at the branch tip (cargo test -p for every touched crate; the named scenarios). ALSO run cargo test -p zz-daemon --lib whenever the diff touches crates/zz-mux/src/command.rs, formats.rs, model.rs or layout.rs, even if the worker did not claim it, and cargo test -p zz-client plus cargo build -p zz-tui -p zz -p zz-client-ffi whenever a wire type changed. Any red at tip is a blocker regardless of the report.
3. ORACLE SPOT-CHECKS: 3-5 riskiest claims verified against the pinned binary yourself ({M['root']}/compat/.cache/tmux-src/tmux; the pinned C source sits beside the binary, read it when subtle). Past best catches were pin-side gating the worker's fixture had configured away, a probe that passes for the wrong reason, a fixture that resizes or reconfigures the client so it cannot see the ordinary flow, and state left armed after a failure: look for the configuration that would make a fixture blind.
4. TEST HONESTY: run the branch's new/changed tests and its scenarios, AND the pre-existing scenarios that exercise the same surface ({RUN_ENV} compat/run.sh --strict-geometry <names>). Failing/ignored/tautological = blocker. Check the durable registry resolution carries every divergence the worker disclosed in its notes. Check compat/run.sh --check-summary on the branch prints "summary current" with no scenario-inventory diff (a crates/ drift warning is expected; the gate re-stamps after the merge).
5. INVARIANTS: zone discipline (out-of-zone files listed in notes); wire rule (NEITHER lane was expected to bump; a wire-reachable change is itself worth flagging; if genuinely unavoidable it needs the complete 98->99 bump incl. the hex 0x62->0x63 fixture and the knowledge mirrors); no code comments; doc comments still attached to the fn they describe; no attribution trailers; registry round-trips (python3 -m json.tool); python3 compat/tmux-tracker.py check green on the branch; cargo test -p zz-mux green (manifest counts).
6. CALIBRATION: default to refuting each close, but confirmed_defects only with PROOF (probe, failing rerun, quoted contradiction). Suspicion goes in notes. blocker = wrong close or would break main; must-fix = gate applies before merge; nit = mention. When a blocker is a wrong close, say whether reverting that commit applies cleanly at tip and what it takes with it. The load-flake rule binds you too: a test that fails loaded and passes exact-solo is a flake, not a defect; client_focus_closes_display_panes_and_preserves_chooser_modes fails about one solo run in three and two passes of three count as green.
VERDICT: approve / approve-with-fixes / reject (a blocker the gate cannot fix in minutes). checks_run lists exact commands. Thorough but bounded: well under an hour.
"""

REVIEW_CONFIG = """LANE-SPECIFIC SPOT-CHECKS (config): with a scratch HOME holding both ~/.tmux.conf and .config/tmux/tmux.conf, start the pin with no -f and confirm BOTH load in that order (a shared option's final value comes from the XDG file); then confirm zz does the same AND that mux.conf's value wins over both; then confirm -f <file> on zz loads that file and mux.conf only. Confirm the import command no longer writes mux.conf (run it against the scratch HOME and stat the file). Confirm the harness still isolates the zz side: read the diff-scenario.sh diff and show that a zz server started by a scenario without the opt-in cannot see any tmux.conf outside the scratch HOME (plant a marker file in the scratch HOME by hand on a throwaway copy of one scenario and show it is NOT loaded without the opt-in and IS loaded with it), then run three unrelated pre-existing scenarios at the tip and confirm they pass. For the pane PATH: under the `launcher:` header, from inside a pane on zz, `command -v tmux` must resolve to the daemon's wrapper (not the harness's, which must be absent from that PATH: print $PATH from the pane), `tmux display -p '#{pane_id}'` must answer the pane's own id, and bare `tmux` must print the pin's nested-session refusal with the pin's exit status; then confirm a job (run-shell) still gets the same wrapper. For the prefix remainder: on the pin, verify the f prompt string and the M-n/M-p notes from key-bindings.c against zz's list-keys -N output; drive M-n on both binaries yourself with a bell in a hidden window and read #{window_index}; confirm the fixture waits on #{window_activity_flag} (or the bell flag) rather than sleeping."""

REVIEW_STATUS = """LANE-SPECIFIC SPOT-CHECKS (status): on the pin, prove the three job rules yourself (first expansion empty, cached value after the job lands, re-run only after status-interval) with `#(date +%s%N)` and status-interval 2, reading #{T:status-right} at 0, 1, 3 and 5 seconds; then the same on zz through the fixture's channel, and time an attach with status-right '[#(sleep 3; echo slow)]' on zz (must not wait for the job). Confirm refresh-client -S still forces a re-run on zz (smoke/refresh-status). For control-notify: read the fixture's normaliser and make sure it does not erase the kind or ordering of % lines; sabotage one expected line on a throwaway copy and confirm the fixture fails; on the pin, confirm %window-close IS emitted for the unlink case the review describes and that zz's absence is recorded as an open item, not asserted away; resize through -C on both and confirm zz's %layout-change carries the new geometry. For the mixed queue: `od -c` both outputs yourself through the fixture's command. For the command-output prompt: run compat/attached-client.sh at the tip with the worker's zz twice, and check the probe now asserts pane_in_mode / client_key_table parity across prompt open, submit and cancel rather than a weaker observable. For the dead signal: on this Linux pin, `#{pane_dead_signal}` must read 15 on BOTH binaries after kill -TERM with remain-on-exit on, and the fixture must no longer branch on sys_signame availability for the zz side."""

LANES = [
    dict(key="config", prompt=BATCH_CONFIG, lock="F-CONFIG-ENTRYPOINT", workdir="zz-lane-config", reviewdir="zz-review-config", review_extra=REVIEW_CONFIG),
    dict(key="status", prompt=BATCH_STATUS, lock="F-STATUS-CONTROL-NOTIFY", workdir="zz-lane-status", reviewdir="zz-review-status", review_extra=REVIEW_STATUS),
]


def gate_prompt(summaries):
    return f"""You are the integration gate for the zz tmux-compat campaign (repo demfabris/zz, board = GitHub issue 7). You are Codex (gpt-6-astra) running non-interactively under `codex exec`; all workers and reviewers are done; you run ALONE on this {M['machine']}, full speed. Run every command synchronously and wait for it; never leave anything running; never end your turn to wait. Your FINAL message must be ONLY the JSON object the output schema describes (merges, progress_after, board_updates, attached_client, problems). Integrate IN THIS ORDER: config first (paths.rs, lib.rs, the startup and spawn hunks of daemon.rs, key.rs, command.rs's window stepping, the import code and docs), status second (status.rs, control_mode.rs, the clients, the status call sites of daemon.rs, lib.rs's output writer, command.rs's dead-signal spelling). One gate per branch, and push main as soon as each lane's gate is green (if you die mid-run the orchestrator resumes from what already landed). A branch whose diff touches nothing under crates/ (git diff --stat origin/main...HEAD) skips stages 3a and 3b and runs 3c and 3d only. The summaries below carry each lane's worker report and its Codex review verdict; a lane may have been rejected once and fixed on the same branch, in which case its review is the re-review.

Lane summaries, worker report + Codex review verdict each:
{json.dumps(summaries, indent=2)}

REVIEW VERDICTS BIND YOU: approve-with-fixes => apply every must-fix on the branch (own follow-up commit) before its gate, re-running the reviewer's failing probe to prove each fix. reject => do NOT merge; post the blockers as a board note on the lock front, leave the branch, continue. A blocker you can genuinely fix in minutes may be fixed and merged with the probe re-run as proof; a wrong close whose revert applies cleanly is such a fix (precedent 0fec342 + 9cab1fa: revert, then a records commit that puts the reviewer's measurement into the group reason and acceptance). review_actions must account for every confirmed defect. Missing review (null) => do a compressed contract audit yourself before gating. If you SKIP a lane, push its rebased-and-fixed tip to origin as campaign/<name>-gated before removing the worktree.

BOARD IDENTITY: ZZ_BOARD_HOLDER={M['holder']} python3 compat/board.py <cmd> from inside a repo checkout. Verbs: release/withdraw REQUIRE --reason; note takes --note; candidate takes --commit --branch --base + repeatable --proof; integrated takes --merge + optional --gate; renew <FRONT> --lease 2h (always give a unit); front needs --contract --zones [--priority <int> --kind --deps --notes]; withdraw and front need TRIAGE held; integrated needs MAIN held. The orchestrator holds the two lock fronts F-CONFIG-ENTRYPOINT (config lane) and F-STATUS-CONTROL-NOTIFY (status lane) (10h leases from launch; expired => claim back as {M['holder']} before that lane's ledger step). BOARD FALLBACK: if a board command fails on GitHub authentication, append the exact command line you meant to run to {M['root']}/compat/orchestration/board-replay-15.sh (do not commit that file), say so in board_updates, and continue the gate; the orchestrator replays the file. Never let a board failure stop a merge.

{M['gitNote']} Fetch git fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*'; push git push origin HEAD:main. The shared checkout {M['root']} is where you start; its local main branch may be stale; never use it, never edit it, never stash or reset in it.
knowledge/tmux/gaps.md is generated: regenerate with tmux-tracker.py write-report on every conflict, never hand-merge it.
PROTOCOL RECONCILE: NEITHER lane was expected to bump, so a branch that carries a 98 -> 99 bump is a finding: check its reviewer justified it. If one or both did bump, reconcile to ONE constant at 99 with every v99 changelog bullet preserved side by side and one hunt_claims fixture at 0x63; verify cargo test -p zz-protocol after reconciling. catalog.rs counters, the option-consumer roster and compat_manifest_tests.rs counts conflict between lanes routinely: resolve by recounting, then cargo test -p zz-mux and -p zz-protocol, and grep the tree for conflict markers before building. Both lanes touch crates/zz-daemon/src/daemon.rs, crates/zz/src/lib.rs and crates/zz-mux/src/command.rs in declared, disjoint regions (config: startup config loading, pane spawn env, import wiring, step_window/find_window; status: status render call sites, notification emission, the output writer, the dead-signal spelling); rebase conflicts there are resolved by keeping both hunks and letting the workspace run judge. When a rebase conflicts on compat/tmux-gaps.json, {M['root']}/compat/orchestration/gaps-merge.py BASE OURS THEIRS OUT merges the two sides by record id and exits 2 on a record both sides changed differently (feed it git show :1:compat/tmux-gaps.json, :2:, :3:), then regenerate gaps.md and recount. compat/results/summary.md conflicts are row unions: keep every row from both sides, sorted as the file is; the `Recorded at:` footer stays whatever main has until step 7.

THIS BOX: {M['boxNote']} Use ONE shared build directory for every gate worktree, CARGO_TARGET_DIR={M['dev']}/zz-gate-target (gates are serial, so it is never contended; its incremental cache was trimmed, so the first workspace build recompiles the workspace crates; leave the directory in place for the next cycle). Never touch {M['protected']}. Before gating a branch, run git merge-tree --write-tree origin/main <tip> first; it predicts the conflicts and costs one command. CONFIG DISCOVERY LANDED THIS CYCLE: every zz you start outside the harness now reads ~/.tmux.conf; the box has a real one (the user's), so your own probes and the full run's servers must keep HOME scrubbed or pass -f /dev/null exactly as the harness does; never let a gate server load the user's file.

PER BRANCH, in order:
1. Fresh worktree: git -C {M['root']} worktree add {M['root']}-gate-<key> origin/main (remove leftovers with --force first). Rebase branch onto origin/main; compat/tmux-gaps.json conflicts between lanes are normal: merge both sides.
2. First branch only: claim MAIN --lease 3h; hold across all, renew before each subsequent gate, release after the ledger recompute.
3. Code-branch gate stages, in order:
   a. cargo test --workspace --all-features --no-fail-fast --jobs {M['gateJobs']} -- --test-threads={M['gateThreads']} > log 2>&1 (check exit code; never pipe through tail). Timeout-guard: wait_exit_holds_the_control_process_until_a_second_blank_line can HANG under load; if the run wedges >20min with no output, sample the process; a lost-wakeup hang there counts as the known flake (verify solo).
   b. cargo clippy --workspace --all-targets --all-features -- -D warnings
   c. {RUN_ENV} compat/run.sh --strict-geometry --delta origin/main..HEAD --commands <lane touched_commands>. Run --list TWICE and reconcile against git diff --name-only for compat/scenarios; for the config lane ADD every source-file, config, launcher, keys-prefix and jobs scenario; for the status lane ADD every status, refresh, control, cli-output, remain-on-exit and command-output scenario plus the attached fixture. Shard up to {M['shards']} concurrent run.sh invocations over DISJOINT scenario subsets with separate result logs (compat/run.sh hard-codes RESULTS_DIR, so shards stay apart only by disjoint scenario names); run smoke/source-replay-diagnostics SOLO after the shards (pin-side crash under load); any divergence re-runs alone before being called real.
   d. python3 compat/tmux-tracker.py check && python3 compat/board_test.py. Then compat/run.sh --check-summary: it must print "summary current" with at most the crates/ drift warning; a scenario-inventory diff is a lane defect (fix the rows, they are a row union).
4. Flake rules: lone timing test failing loaded + passing exact-solo = flake, proceed (known list: copy-mode reconcile, client_focus_closes… (also fails about one solo run in three; two solo passes of three count), event_hooks_fire_after_mutation_with_captured_formats, history_request_is_guarded…, daemon_native_split_resize_commits_exactly…, nested_alias_queue_bubbles_shutdown…, control_sourced_run_shell_closes_before_raw_output…, request_full_enqueues_only_the_requested_visible_pane, display_menu_resize_lifecycle::a_resize_moves_the_menu…, zz-terminal pty_output_drains_while_the_input_writer_is_backpressured, wait_exit… hang, concurrent_default_interactive… "not a terminal" incl. misattribution, behavior-options one TOPO row under shard load). Anything else red = real: fix if minutes, else SKIP branch (no push to main; push the gated tip as campaign/<name>-gated), record, continue.
5. Push main. Non-fast-forward: fetch; user-authored commits + conflict-free disjoint rebase => bounded rerun (lane package tests + its scenarios), push. Never force. Campaign branches rewritten by the rebase stay at their old tips on origin (never force them); say so in the report.
6. Ledger per successful push against that lane's lock_front: candidate (--commit tip --branch campaign/<name> --base <pre-push main> --proof per stage), note (--note: groups covered, slugs closed/relocated, decisions recorded, reviewer verdict + what you did about defects), integrated (--merge <sha> --gate "workspace+clippy+delta green"), release (--reason "batch integrated at <sha>"). A lane that left one of its groups open gets integrated + a release whose --reason names what is still open.
7. After ALL branches, holding MAIN, in the last gate worktree at the pushed main tip: THE STAMPED FULL RUN. cargo build -p zz (CARGO_TARGET_DIR as above, so zz and zz_cli sit at {M['dev']}/zz-gate-target/debug), then {RUN_ENV} ZZ_COMPAT_ZZ={M['dev']}/zz-gate-target/debug/zz compat/run.sh --attached-client > log 2>&1 (about 30-40 minutes; the fixture alone is ten). On PASS it rewrites compat/results/summary.md with `Recorded at: <sha>`. If the fixture fails, re-run it once solo (compat/attached-client.sh <zz> <pin tmux>); if it fails again, name the probe in attached_client, leave the summary as it was, and do NOT hand-edit a PASS. A dying run leaves zz daemons on /tmp/zza-*.sock: reap them by pid from the socket name, never by pkill -f. If a corpus row (not the fixture) fails at the merged tip, that is a regression one of the lanes introduced: fix it if minutes, else report it. Then recompute TMUX_COMPAT_TRACKER.md from the merged registry: the headline lines (Campaign delivery; Live work "<open> OPEN + <blocked> BLOCKED = <sum>" counting the post-freeze groups honestly; Ledger settlement; Exit evidence with the scenario and step counts and the stamped attached-client PASS or its failure), the Orchestration line (cycle 15 integrated on the ubuntu box on Codex gpt-6-astra lanes at medium reasoning on the default tier, config then status, one gate alone), the Current checkpoint rows, the Campaign dashboard table rows (Live unresolved, Latest differential, Differential SHA-256 = sha256 of compat/results/summary.md, Ledger settlement), and a new "### 2026-09-05 cycle-15 integration checkpoint" table like the cycle-14 one above it. ALSO refresh the bundle pages that carry live registry and corpus numbers, listed under "Pages that carry live checkpoint numbers" in compat/orchestration/HANDOFF.md, and append a "### 2026-09-05 cycle-15 integration" entry to compat/orchestration/CAMPAIGN-LOG.md in the shape of the cycle-14 one (what merged, what stayed open, what the reviews caught, the stamp). Records commit ("Recompute the live ledger after the cycle-15 merges", including the stamped summary.md), then compat/run.sh --check-summary MUST print "summary current" on that commit, push, ledger it as integrated MAIN --merge <sha>, release MAIN.
8. Claim TRIAGE. Withdraw fronts fully mooted by the merges (none expected besides the lane locks; the F-SPLIT-MUX-*-V5 chain stays). If a worker's skip reasons prove a group contract is unprovable as written, post that as a residual on the relevant front. Release TRIAGE.
9. python3 compat/progress.py, full output, into progress_after.
10. Remove only your own {M['root']}-gate-* worktrees (after pushing any skipped lane's gated tip). Leave zz-lane-* and zz-review-*.

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


def run_codex(run_dir, label, cwd, prompt, schema, env=None):
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
        r = subprocess.run(cmd, input=prompt, stdout=logf, stderr=subprocess.STDOUT, text=True, env=dict(os.environ, **(env or {})))
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
    results[key]["review"] = run_codex(run_dir, f"review-{key}", reviewdir, prompt, REVIEW_SCHEMA, env={"CARGO_TARGET_DIR": str(workdir / "target")})


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
    log(f"cycle 15 on Codex gpt-6-astra: lanes {[l['key'] for l in lanes]}, stage {a.stage}, run dir {run_dir}")

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
    log("cycle 15 finished" if gate else "cycle 15 ended without a gate report")


if __name__ == "__main__":
    main()
