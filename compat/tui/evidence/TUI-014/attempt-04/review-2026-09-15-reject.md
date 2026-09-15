```json
{
  "lane": "modes",
  "branch": "campaign/tui-modes-11",
  "tip": "f4b74b7d",
  "verdict": "reject",
  "confirmed_defects": [
    {
      "obligation": "TUI-014",
      "severity": "blocker",
      "description": "Clock input bypass: after clock-mode, send-keys -t PANE x exits 0 on both servers, but the pin consumes x and leaves mode 0/ while zz stays in 1/clock-mode. After raw Escape, the pin shell is \"$ \" and zz is \"$ x\". Separately, copy-mode -q cancels the pin clock but leaves zz in clock-mode. The new hook only handles raw input_key/KeyDecision::Pass (crates/zz-daemon/src/daemon.rs:18440); command.rs:8093 only cancels the client copy session. Proof: /tmp/zz-review-modes/send-final.txt and spot.txt (spot-copy-mode-q).",
      "suggested_fix": "Route command-injected keys and generic mode cancellation through the pane mode lifecycle before PTY delivery. Add asserted send-keys and copy-mode -q cases, including the underlying shell contents."
    },
    {
      "obligation": "TUI-014",
      "severity": "blocker",
      "description": "Mode stacking is lost. clock-mode followed by switch-mode reports 2/switch-mode on the pin and 1/switch-mode on zz. Escape restores 1/clock-mode and its screen on the pin, but zz reports 0/ and exposes the terminal. daemon.rs:9317 replaces the map value; status.rs:1688 returns a boolean count. This is absent from the registered residue. Proof: /tmp/zz-review-modes/stack-final.txt, with equal baseline and equal screen after both modes are closed.",
      "suggested_fix": "Preserve the mode stack and restore the previous mode on teardown; report the actual mode count. Assert clock -> switch -> Escape -> clock -> key -> terminal against the pin."
    },
    {
      "obligation": "TUI-014",
      "severity": "blocker",
      "description": "switch-mode accepts and silently ignores supported arguments. In command.rs:8393-8409 only -w is retained. -F \"REVIEW-#{session_name}\" produces REVIEW-alpha/REVIEW-cli/REVIEW-zulu on the pin but default rows on zz. A positional command \"set-option -g @review-command yes\" executes on Enter on the pin (show-options outputs \"yes\\n\", exit 0); zz instead switches sessions and show-options exits 1 with \"invalid option: @review-command\\n\". With two panes, switch-mode -k then Escape kills the target on the pin and leaves it on zz. The vocabulary residue does not name these flags/template failures. Proof: /tmp/zz-review-modes/spot-final.txt and extra3.txt (the first -k probe).",
      "suggested_fix": "Implement format, command-template, and kill-on-exit semantics with pin assertions, or explicitly measure and register these unbuilt variants before closing command:switch-mode. Do not silently accept discarded arguments."
    },
    {
      "obligation": "TUI-014",
      "severity": "blocker",
      "description": "server-access does not expand its identity argument. server-access \"#{?#{==:1,1},nobody,root}\" has empty stdout/stderr and exits 0 on the pin, but zz exits 1 with \"unknown user: #{?#{==:1,1},nobody,root}\\n\". command.rs:8521 converts the raw argument directly to a string; the pin uses format_single before lookup in cmd-server-access.c:81. This no-action case needs no second-identity ACL support and is not covered by semantic:multi-user-socket-acl. Proof: /tmp/zz-review-modes/timer-final.txt, formatted-nobody.",
      "suggested_fix": "Expand the argument in the correct command context before user/group lookup and assert exact stdout, stderr, and exit status for formatted identities."
    },
    {
      "obligation": "TUI-014",
      "severity": "must-fix",
      "description": "The switch-mode-windows record is too narrow. With three sessions and equal window names, the pin orders the tied rows alpha, cli, zulu; zz orders cli, alpha, zulu. Thus cells and row order differ, not only the final style reset as compat/tui-client-commands.sh:833 and attempt-04/notes.md claim. The zz sort uses window name then WindowId (chooser_presentation.rs:573 and daemon.rs:37089). Proof: /tmp/zz-review-modes/spot-final.txt, spot-switch-w.",
      "suggested_fix": "Match the pin tie ordering and add a multi-session duplicate-name fixture, or expand the recorded gap with this measured semantic difference; remove the blanket every-cell-matches claim."
    },
    {
      "obligation": "TUI-014",
      "severity": "blocker",
      "description": "The actual three-dot footprint exceeds the literal assigned zones. attempt-04/notes.md:241-243 discloses the fifteen mode: None test-helper companions, so those are declared mechanical changes, but there is also a production constructor in zz-mux/src/model.rs. Additional outside-zone files include zz-protocol/src/lib.rs; zz-mux/src/lib.rs and compat_manifest_tests.rs; zz-mux/tests/hunt_claims.rs; three smoke fixture files (command-flag-errors.sh, command-flag-errors.tsv, config-discovery-import.sh); knowledge/designs/tmux-superset-roadmap.md; knowledge/tmux/divergences.md; and generated knowledge/tmux/gaps.md and tui-parity.md. The wire document is separately required by the wire rule. Mechanical helper files are zz-client/src/completion.rs and status_bar.rs, zz-client-ffi/src/ffi.rs, zz-tui/src/sidebar.rs, zz-protocol/tests/hunt_claims.rs, and zz GUI control_mode.rs, mux/client.rs, workspace/sidebar.rs and workspace/view.rs, besides already allowed core.rs. These support the implementation, but the literal gate zones do not cover the full inventory. See /tmp/zz-review-modes/changed-files.txt.",
      "suggested_fix": "Reconcile the gate ownership with the complete actual file list, explicitly including mechanical consumers and corpus updates, or narrow the patch to the authorized zones. Revalidate the resulting merge."
    },
    {
      "obligation": "TUI-014",
      "severity": "must-fix",
      "description": "The diff adds Rust comments/doc comments despite AGENTS.md saying no added code comments; examples include command.rs:8373, snapshot.rs:465 and render/pane_mode.rs:11. The added Rust comment/doc lines total 160.",
      "suggested_fix": "Remove the newly added code comments, retaining necessary explanation in the evidence notes or knowledge documentation."
    },
    {
      "obligation": "TUI-014",
      "severity": "nit",
      "description": "a_switch_row_keeps_its_dim_runs_over_the_selection_style does not include noattr in selection_style (crates/zz-tui/src/render/pane_mode.rs:357), although noattr suppression is the regression it names. The live fixture and independent explicit-noattr probe exercise the fix, so this is a unit-test weakness, not a fabricated runtime proof.",
      "suggested_fix": "Include noattr in the test input and verify that removing the base_cell fix makes this specific test fail."
    }
  ],
  "checks_run": [
    "Fetched the required main/campaign refs, detached at f4b74b7da5e253c8f528b3a31056d209fcef0607, and reviewed git diff 2e5094d8...HEAD. Built zz locally with the required shared two-slot lock, 8G/4G cap and jobs=3 before any runtime probe. Used that worktree binary and pinned tmux d77c9dc6 throughout.",
    "Exact requested delta --list selected 187 rows. Sequential strict-geometry chunks completed 76 unique rows / 1,384 steps, including the worker sample, all direct capture-pane scenario rows and status-background-jobs. Every completed row passed the harness; known/known-terminal-runtime had its exact six documented FMT divergences. 111 rows were not run within the bound: /tmp/zz-review-modes/corpus-unrun.txt. Full corpus coverage is NOT certified. The prompt chunk was rerun cleanly after a reviewer-wrapper error following its first successful fixture run.",
    "compat/tui-client-commands.sh x3: PASS, each 95 asserted / 27 recorded, TUI-014=2 and unattributed=0. --self-check: PASS, 13 ok lines (the notes say nine). All new clock, switch and access-entry channel sabotages passed. Independent one-sided clock-color sabotage detected screen only and became equal after restoration.",
    "compat/tui-choosers.sh and --self-check: PASS, 78 asserted / 0 recorded. compat/tui-copy-mode.sh and --self-check: PASS, 147 / 0. compat/tui-screen-diff.sh: PASS, 147 asserted / 6 recorded. compat/attached-client.sh: PASS. No load-flake retry was needed for these runs.",
    "cargo test -p zz-protocol -p zz-mux -p zz-tui -p zz-client -p zz-client-ffi: PASS. cargo test -p zz-daemon --lib with scrubbed HOME/XDG_CONFIG_HOME: PASS, 918 tests. cargo test -p zz: PASS, including 125 CLI integration tests; one pre-existing real-home discovery test is ignored. All cargo calls used the required lock/caps, jobs=3 and test-threads=3.",
    "cargo clippy on all seven requested crates --all-targets --all-features -- -D warnings: PASS. cargo fmt --all -- --check: PASS. The desktop GUI build passed.",
    "Both trackers, wire-version.py, compat/check.sh, and verify-claims.py --run TUI-014 --zz <own build> with RUN_ENV and TMUX_BIN: PASS. The live verifier reproduced 95/27 and 78/0 and correctly accepts active with two TUI-014 records. Read origin/main owner-token resolution; every branch token resolves.",
    "Own pinned outer-tmux probes: whole-screen clock opening/closing at 80x24; style 12 with red, colour196 and #ff00aa; second-client attach; resize to 100x30 and 40x10 then back to 80x24: PASS. Both seconds faces and teardown matched in one additional reviewer run; they remain absent from campaign assertions.",
    "Own three-session switch probe with explicit noattr mode-style: default screen/cursor/state and Escape matched. Typing zu reproduced the registered filtering difference. -w exposed an additional row-order difference. -F, custom Enter command, -k, mode stacking, command-injected keys and copy-mode -q reproduced the listed defects. Fresh isolated stack.txt and send.txt are the decisive stack/input proofs.",
    "Own server-access probes: bare, -l, -w unknown user, -g unknown group, -a/-d conflict, -r/-w conflict, -d nobody and -t %0 matched exact CLI channels. Formatted-nobody reproduced the unregistered mismatch. customize-mode and suspend-client measurements matched the honest TUI-014 records; all clients and servers were throwaways.",
    "Clock timer sampled on the actual daemon PID: no clock thread before entry, one zz-clock-mode thread waking while open, none after closing and waiting 1.2 seconds. No recurring empty-mode wakeup was observed.",
    "Evidence audit: 38 files in attempt-03 and 30 in attempt-04; no .log files and no git check-ignore matches in either attempt directory. Evidence includes retained failed runs, consistent commands/results and captures; original historical execution cannot be independently attested. Current reruns reproduce the principal green totals. No attribution trailers or added Unicode ASCII-escape churn; generated reports pass their checks. The noattr and EL/space capture issues are real on the pin; chooser captures remain byte-identical after the shared renderer change.",
    "git diff --check excluding raw evidence: PASS. The full whitespace check flags trailing spaces/end blank lines in captured evidence, which must be distinguished from source formatting. Final git merge-tree remains read-only; the seven conflict paths are listed below.",
    "Final repro repeats on the relinked binary: stack-final.txt, send-final.txt, timer-final.txt and spot-final.txt reproduce the confirmed behavioral mismatches; measurement scripts exit successfully after recording DIFFs, which are reported as defects, not passing parity tests. Final worktree status is clean."
  ],
  "touched_commands": [
    "clock-mode",
    "switch-mode",
    "customize-mode",
    "suspend-client",
    "server-access",
    "choose-client",
    "choose-tree",
    "choose-buffer",
    "capture-pane",
    "set-option"
  ],
  "touched_packages": [
    "zz-protocol",
    "zz-mux",
    "zz-daemon",
    "zz-tui",
    "zz-client",
    "zz-client-ffi",
    "zz"
  ],
  "every_clause_asserted": "Clause 1: yes for the inherited choose-client and info-preview contract; the chooser fixture and its self-check pass. Clause 2: no, correctly still active: customize-mode-open and suspend-client remain measured records owned by TUI-014. clock-mode 24/12 opening and teardown, default switch-mode opening and Escape teardown, and the listed single-user server-access cases genuinely assert against the pin. They do not cover the reproduced failures above. switch-mode navigation/search/mouse are registered in the interactive gap prose; -w output has a recorded fixture case; multi-user ACL mutation is recorded under the socket-ACL gap. Seconds clock faces are not asserted by the committed campaign fixture; both 24-with-seconds and 12-with-seconds matched in one fresh reviewer run, including teardown. Clause 3: no as a complete command contract; the asserted cases compare stdout/stderr/status, screen and state with working channel sabotages, but unrecorded argument, input and stack mismatches remain. TUI-011 is untouched; TUI-014 acceptance is unchanged, status active/proof null remains honest, and edits are limited to evidence_note, next_action and sources. All five removed command/option gap items have dated 2026-09-15 measurements. All owner tokens resolve under origin/main verify-claims.py.",
  "wire": "PASS for the unreleased-103 wire rule. snapshot.rs appends #[serde(default)] mode: Option<PaneMode> after border_status_text, with no existing field reorder. The new enum carries Clock { time, colour } and Switch { rows, selected, offset, selection_style, prompt, prompt_style }; lib.rs exports it. Producer stamping and the raw TUI consumer are in the same diff. message.rs and PROTOCOL_VERSION=103 are unchanged. hunt_claims.rs retains the decimal 103 and both 0x67 byte pins. The documentation adds one paragraph/bullet block inside the single existing v103 entry (18 physical lines, not the claimed one physical line). wire-version.py and compat/check.sh accept 103 as unreleased after shipped 102. There are 15 test-helper mode: None insertions plus the production mux model constructor; these extend beyond snapshot.rs.",
  "gui_reach": "The GUI builds and its tests/clippy pass. base_cell and Grid::emit_into are local to zz-tui and do not change GUI presentation. The eight inserted GUI lines are test-helper mode: None fields. The GUI ignores PaneSnapshot.mode and continues showing the underlying terminal when the daemon owns a clock mode; raw key events can still reach the new daemon mode handler, while ordinary text input bypasses it. No GUI mode surface was implemented.",
  "expected_gate_conflicts": "git merge-tree --write-tree origin/main HEAD at current main 1077951108aabe956104ac8de62da319aa06db4b (the shared remote-tracking ref advanced from the initially fetched 7e7cb1ee during review) exits 1 with seven conflicts: compat/tui-choosers.sh, compat/tui-client-commands.sh, crates/zz-daemon/src/daemon.rs, crates/zz-mux/src/lib.rs, crates/zz-protocol/src/catalog.rs, knowledge/protocol/wire-protocol.md, knowledge/tmux/gaps.md. No merge was applied.",
  "notes": "REJECT: green standard fixtures do not cover the reproduced mode lifecycle and argument failures. The complete 187-row corpus was not finished within the requested time bound; 76 rows completed and 111 remain explicitly listed. One reviewer harness error occurred after the first prompt chunk had finished successfully: editing the still-running wrapper changed its read offset. The corrected wrapper passed bash -n and the entire chunk was rerun with exit 0. No fixture failure was disguised as a flake. The initial purpose-built binary SHA256 was d29e1c4e0da3df3497a590121a8a97a819ff069881b718fa0d2e35df229603b8. cargo test -p zz relinked target/debug/zz at 19:11:34 without source changes, yielding ca8802a35c22ef7460b5a08b3a7ce4ae76894d6d8e501266c8d7b9b1ad5b1679; later proofs used that same-tip worktree build. Thus the review did not preserve one immutable binary hash throughout. The decisive stack, injected-key and formatted-identity failures were confirmed again on the relinked build. The pin hash remained fe82462af83f515034c25bb8576eef8a11a0ad3a5c1029af70b0973b028aed5d. Seconds faces matched once in reviewer probes, but should gain a stable synchronized/deterministic campaign assertion before claiming complete option coverage; the lane standalone probe stores only the default face. A split-pane switch-mode -Z probe lost the pin session and was inconclusive; no -Z defect is claimed. Notes and captures are under /tmp/zz-review-modes. No commit, push, board/issue operation, shared-checkout edit, or worktree removal was performed. The final switch/clock run reconfirmed the format, custom-command, copy-mode cancellation and window-order findings on the relinked build. Cleanup ran the requested pgrep; its only match was the audit shell itself. No review-owned live zz/tmux process was found. A daemon started at 16:03 using this worktree was left untouched because it predates the review. No binary copy was made under /tmp. Review finished within 110 minutes of the 18:27:25 build completion; the merged result was not applied or tested."
}
```