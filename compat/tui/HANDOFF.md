# TUI parity handoff: 11/12 on main, three fix passes in flight, paused to move machines (2026-09-15)

The ubuntu box ran the campaign from 09:40 to about 20:30 local on 2026-09-15: cycle 9 was closed,
cycle 10's stream and context lanes landed, and every branch that failed adversarial review got a fix
pass. fabrico ordered the wrap-up at 20:30 ("lets wrap up the workflows as they come in, we need to
jump in another computer"). No new lane starts on this box after that; the three lanes that were
running are detached Codex CLI processes that push their own branches when they finish, so the next
box reads origin before it decides anything. The resume point is the branch table below, not a lane.

| Fact | Value |
| --- | --- |
| `origin/main` | `10779511` at the wrap-up (the menus-3 plus mouse-context landing); any later commit came from a lane named below |
| Ledger | **11/12 baseline verified** (only TUI-011 open, at review); added scope: TUI-016 verified, TUI-018 at review, TUI-014 and TUI-017 active, TUI-015 unmeasured on main (at review on its branch), TUI-013 unmeasured (needs macOS) |
| `PROTOCOL_VERSION` | **103 on main, unreleased** (v0.9.1 shipped 102); one v103 entry in `knowledge/protocol/wire-protocol.md`; `compat/wire-version.py` enforces it inside `compat/check.sh` |
| Board | `F-TUI-CYCLE-9-LANES` released at the wrap-up; MAIN and TRIAGE free; issue 7 carries every gate note of the day |
| Workers | Claude agents (Agent tool, `model: opus`) until 14:30, then Codex CLI lanes (`codex exec`, gpt-6-astra, reasoning high) on fabrico's instruction; the same prompts, zones, proofs and JSON reports for both |
| Main is green on this box | `cargo build -p zz`, workspace clippy `-D warnings`, `cargo fmt --check`, `compat/attached-client.sh` PASS, `compat/tui-overlays.sh --self-check`, the whole zz-daemon lib including `russh_socks`, every fixture the gates ran |

## What landed on main today, in order

| Commit | What |
| --- | --- |
| `287e3815` | Linux compile fix: `657e044a` (a macOS-only desktop commit) typed the CEF `on_key_event` os handle as `*mut u8`; a per-platform alias derived from cef's `EventHandle` |
| `627e717a` | Cycle 9's introspection lane (TUI-016 both clauses, TUI-017 clause 2), plus the three ubuntu-box reds fixed (the attached-client one was a real zz bug: the retained command-output view never applied the window's mode-keys; the overlays self-check control flushes its input line under dash; the russh_socks mock resolves `localhost` to both loopback families), plus clippy and rustfmt repairs for two pre-existing main reds |
| `55efc9c3`, `f24c5133`, `2e5094d8` | `compat/tui/verify-claims.py` attributes a shared fixture's recorded cases to one obligation (the fixture prints an `owners` tally; a case whose reason opens with `DECIDED ` is filed under `decided:<owner>` and not charged; an unattributed case is charged to every obligation on the fixture); TUI-016 verified on that basis |
| `60999cb6` | Three fixes from re-measuring TUI-006 on this box: the raw TUI's forced repaint no longer blanks a chooser for a frame while its presentation is pending (`crates/zz-tui/src/render.rs`), the chooser info mask collapses fill across an SGR reset, `messages-jobs-live` waits for the pending-job placeholder with `status-interval 1` |
| `7e7cb1ee` | Cycle 10's stream lane (the bounded caller-stream channel: `source-file -`, `display-message -I`, `split-window -I`, `compat/tui-command-streams.sh`); TUI-018 stays at review because the command-alias group's caller stream was measured and recorded (see below) |
| `10779511` | The menus second half (the pin's pane and window menus from a pointer, `status_range_start` on `MouseKey`, the display-menu `-x M/-y M/-x W/-y W` no-event positions) and the mouse-context lane (`mouse_word`, `mouse_line`, `mouse_hyperlink` read from the live grid or the requesting client's own copy-mode revision through a `FormatGrid` trait shared with copy mode); **TUI-008 verified** (`compat/tui-mouse.sh` 45 asserted, 0 recorded) and **TUI-012 verified** on three fresh `compat/tui-superset.sh` runs (190 asserted, 0 recorded) |

## The branches, and what each one needs next

Read `git log --oneline origin/main..origin/campaign/<branch>` and each branch's evidence directory
before acting; the Codex lanes that were still running at the wrap-up may have pushed after this
file was written, so the tips below are lower bounds.

| Branch | Tip at wrap-up | State | Next |
| --- | --- | --- | --- |
| `campaign/tui-stream-alias` | `ff58de8a` plus an evidence commit `57bf163f` in the `zz-box-reds` worktree | TUI-018's alias-group fix: `resolve_command_alias` carries `stdin` onto the expanded group invocation, the group loop spends it once through a serde-skipped `stdin_spent`, the CLI routes stdin to the first sink through `command_stdin_sink`; `compat/tui-command-streams.sh` 43 asserted, 0 recorded, 4 decided. Its review-and-gate lane REJECTED it on three measured blockers: (1) a spent source reader aborts the rest of the group on zz where the pin reports EBADF and resumes the queue, losing later members' stdout and state; (2) a command client sourcing a FILE whose line invokes an alias with `-` keeps its stdin on the pin (cfg.c preserves the invoking client through replay, `file_read` refuses only absent, attached or control clients) but zz answers `source-file from standard input is not supported`, and `knowledge/designs/command-stream-channel.md`'s sentence that a loaded config has no caller is false for that path; (3) a raw buffer writer inside an alias gains a trailing newline because the group's stdout claim falls back to the alias name and `default_stdout_claim` classifies it as Print. The verdict with probe transcripts is in `compat/tui/evidence/TUI-018/attempt-02/` on the branch (or the gate lane's report if it finished after this file) | A fix pass on the same branch, then a re-review and gate that verifies TUI-018. The fix-pass prompt was written and NOT launched at the wrap-up; its content is summarised in the three blockers above and in TUI-018's `next_action` once the gate lane's records land |
| `campaign/tui-capture-11` | `82746ea3` on `7e7cb1ee` | The capture fix pass after a REJECT of `campaign/tui-capture-10`: seven of the nine findings fixed, two recorded. Fixed: rows numbered from physical wrap boundaries (`-L` no longer sums ANSI byte lengths; the coloured 170-A wrap keeps its `NEXT` row), styled captures match named and bright colours, indexed 196, RGB, bold, underline, attribute resets and erased backgrounds, compound session targets (`session:window.pane`, `=` exact match, `%pane`, `cli:.%1`) resolved through their components in `crates/zz-mux/src/model.rs` (a granted excursion) for lock-session, has-session and list-windows, the negative lock hook assertions with a sabotage, the four refusal reasons opening with `DECIDED ` and carrying the sentence verbatim, `lock-session-current` attributed, the five lock screen records carrying the 2026-09-14 OS-locking decision, the shell comments removed, the counts corrected. Recorded, each owned: TUI-015 keeps three (`pane-base-index 1` changes the target faces of lock-session, has-session and list-windows; the option lives in `CommandEngine`, outside the resolver excursion), TUI-017 keeps one (explicit indexed colour 1 captures as named red because libghostty-vt stores both the same way) and decides the DEC charset provenance (SO/SI cannot be reproduced from a grid that keeps no charset fact). Fixture: `all 168 asserted comparisons identical, 30 recorded not asserted (owners TUI-014=6 TUI-015=3 TUI-017=1 decided:TUI-015=5 decided:TUI-016=1 decided:TUI-017=6 gap:clients.interactive-refresh=8 unattributed=0)`; both obligations ACTIVE, honestly | A re-review, then a gate that lands it (it is safe and improves parity even with both obligations active). Then one more lane for the four owned records: `pane-base-index` in the target grammar (CommandEngine) and the indexed-colour-1 class (an engine question: whether zz-terminal can keep the colour class the pin keeps) |
| `campaign/tui-modes-11` | `f4b74b7d` on `2e5094d8` | TUI-014's second lane: a server-owned per-pane mode (`ServerState.pane_modes`, published as a trailing `PaneSnapshot.mode: Option<PaneMode>` with `#[serde(default)]`, one v103 line), clock-mode and switch-mode asserted whole-screen against the pin, server-access matching the pin, customize-mode and suspend-client measured and RECORDED (owner TUI-014). REJECTED by review: keys injected with `send-keys` and a generic `copy-mode -q` do not end the clock on zz (the key leaks to the shell); no mode stack (clock then switch then Escape exposes the terminal where the pin restores the clock and reports 2 modes); switch-mode drops `-F` and its command template silently; server-access does not format-expand its identity argument; the switch-mode `-w` tie ordering differs (pin `alpha, cli, zulu`, zz `cli, alpha, zulu`); an undeclared zone footprint; 160 added comment lines | A Codex fix pass (`campaign/tui-modes-12`) was running at the wrap-up with a 150-minute budget; check origin for it. Then a re-review and a gate. TUI-014 stays active either way |
| `campaign/tui-mouse-menus-3`, `campaign/tui-mouse-context`, `campaign/tui-stream`, `campaign/tui-introspection`, `campaign/box-reds`, `campaign/zoom-tree-red`, `campaign/tui-capture-10`, `campaign/tui-modes-10` | landed or superseded | Nothing; history |

## What closes TUI-011, and what does not

TUI-011 is the last baseline item. It verifies when TUI-014, TUI-015, TUI-016, TUI-017 and TUI-018
are verified on main; the gate that lands the last of them re-runs `compat/tui-client-commands.sh`
three times plus `--self-check`, reads its owners tally, fills TUI-011's proof block and sets it
verified (its record says so in its own words). Today's position:

- TUI-016: verified.
- TUI-018: one fix pass away (the three alias-group blockers above).
- TUI-015 and TUI-017: read `campaign/tui-capture-11`'s report; whatever it kept active is one more
  lane, likely the `-C -e` charset bytes (the pin writes SO/SI around a DEC line-drawing run and
  zz writes UTF-8 box characters, and libghostty-vt keeps no charset fact per cell) and the
  remaining lock target faces.
- **TUI-014 cannot verify by building alone today or tomorrow**: two lanes (300 and 330 minutes)
  built clock-mode, switch-mode and server-access and left customize-mode (an options mode tree
  keyed per pane) and suspend-client (a client lifecycle state between attached and exited) measured
  and recorded. The routes to 12/12 are a third modes lane with those two as its whole batch, or a
  product decision by fabrico like the one made for TUI-015's lock surface on 2026-09-14 (zz does not
  imitate customize-mode and suspend-client under the superset principle; amend TUI-014's clauses to
  a measured refusal carrying the decision sentence). fabrico was asked twice on 2026-09-15 and had
  not decided at the wrap-up. Do not make that call in a lane.
- TUI-013 needs a macOS box (`run-2.js`).

## What each gate must know

1. **Trimmed gates.** Since 12:50 on 2026-09-15 an intermediate gate does not re-run the whole fixture
   tree, 3x fixture runs, workspace clippy or a second post-push sweep: one rebase onto current main;
   tests and clippy for touched crates only, `cargo test -p zz` once; the lane's own fixture once
   plus `--self-check` (`verify-claims.py --run` is the re-measure; the reviewer already ran 3x at the
   tip); `compat/attached-client.sh` once; the fixtures whose declared sources or files intersect the
   three-dot diff; the corpus delta once; records, trackers, `compat/check.sh`, push; after a push-time
   rebase only the lane's own fixtures. The FINAL close-out gate runs everything once: every fixture
   with `--self-check`, per-crate tests, workspace clippy, and a full `compat/run.sh` with
   `--attached-client` to restamp `compat/results/summary.md` (about 90 minutes), which is what CI's
   Linux leg has needed since August. Reviewers stay thorough; they caught overclaims again today.
2. **Attribution.** `compat/tui-client-commands.sh` is shared by TUI-011, TUI-014, TUI-015, TUI-016,
   TUI-017 and TUI-018. Its `case_owner` table names each recorded case's owner (an obligation id or
   `gap:<id>`); the summary line ends with `owners ... unattributed=0`; a reason opening with
   `DECIDED ` is a clause-mandated registration and is not charged. A gate verifying any of the six
   reads that tally, and a lane adding a recorded case must attribute it or it is charged to all.
3. **Wire.** 103 is unreleased; main's v103 entry carries `CommandPromptState.pane`,
   `ProtocolMessage::ClientTerminalType`, `view_action` and `press_action` on
   `InputMessage::MouseKey`, `status_range_start` on `MouseKey`, and `CommandInvocation.stdin`;
   `campaign/tui-modes-11` appends `PaneSnapshot.mode`. `crates/zz-protocol/tests/hunt_claims.rs`
   pins the number three times. `stdin_spent` on the alias branch is serde-skipped, not on the wire.
4. **Conflicts.** `knowledge/tmux/gaps.md` and `knowledge/tmux/tui-parity.md` are generated
   (`python3 compat/tmux-tracker.py write-report`, `python3 compat/tui/tracker.py write-report`);
   they conflict on nearly every replayed commit and are only ever regenerated. `compat/tui/campaign.json`
   and `compat/tmux-gaps.json` merge by record and by item, serialised with `ensure_ascii=False`.
   `crates/zz-mux/src/lib.rs` re-exports merge by union. `compat/tui-client-commands.sh` merges by
   union because each lane's cases live in their own functions.
5. **`compat/run.sh` builds zz outside the two cargo slots** (the stream gate measured an eight-minute
   unlocked build); set `ZZ_COMPAT_ZZ` to a built binary so it invokes cargo zero times.

## Residuals, none charged to a lane, all in the board notes

- The non-copy-mode client's `mouse_word`/`mouse_line` when another client holds the pane in copy
  mode: the pin has one mode per pane, zz one per view (TUI-008's `next_action`, measured with a
  two-client probe).
- `mouse_x`/`mouse_y` on a status-row click (pin `0,0`, zz `0,23`); `mouse_status_line` and
  `mouse_status_range` open in `formats.mouse-context`; a TAB cell answers differently because the
  engines store tabs differently; the pin's client re-emits OSC 8 around a linked cell where the raw
  TUI draws the underline alone.
- The backward emacs word selection (pin copies `beta`, zz `bet`); the history_size cap under
  `history-limit 5000` (zz 821 rows to the pin's 1980).
- `split-window -E` and every empty pane now carry the pin's empty-pane screen mode (`MODE_CRLF` on,
  cursor off) in the GUI too; declared in TUI-018's record, reversible.
- The board residual `F-ALIASES-MULTI-BODY` and TUI-018's alias-group work are the same root
  (`canonical_name` is `None` for a multi-member alias); close them together.
- `smoke/status-background-jobs` is environmental on this box; chooser and clock faces turn over
  under load above about 9 and must be re-run solo before they are charged.

## How this day ran, and what to copy

- **Warm the shared target once at main, reflink it into every worktree** (`cp -a --reflink=always
  ~/dev/zz/target <worktree>/target`, guarded by `flock /tmp/zz-target-warm.lock`), delete a lane's
  target the moment its consumer builds elsewhere. Disk peaked at 78% with seven lanes; 66% at the
  wrap-up. Never symlink `compat/.cache` into a worktree.
- **The harness kills any Bash call at 600 s even in the background**, so long orchestrator jobs
  run as `setsid nohup <script> &` and a Monitor watches for a completion marker.
- **Codex lanes**: prompt in a file, `cd <worktree> && codex exec --skip-git-repo-check -C <worktree>
  --output-last-message <report> - < <prompt>`, detached; the completion marker is
  `CODEX-DONE exit N` appended by the launcher (a lane's own shell echoes `exit $rc` lines, so match
  the marker, not `exit`). The provider can refuse mid-run ("Selected model is at capacity"): the
  worktree keeps the uncommitted work, and `codex exec resume --last - < <resume-prompt>` from the
  worktree continues the session. Codex has no message channel; corrections go through a resume.
  A Codex lane took 59 minutes for a fix the Claude lane before it could not finish in 90, and its
  reviews found real bugs on every branch they read.
- **Combine gates** when two reviewed branches wait (menus-3 plus context landed in one push).
- **A gate does not hold MAIN while it runs fixtures**; claim it for the push only.
- **The ubuntu box**: Ubuntu 26.04.1, 8 cores, 30 GB plus swap, btrfs, `/bin/sh` is dash. Cargo wrapper
  `S=$((RANDOM % 2)); systemd-run --user --scope -q -p MemoryMax=8G -p MemorySwapMax=4G flock -w 540
  /tmp/zz-cargo-slot-$S.lock cargo <args> --jobs 3` (10G and `--jobs 4` for a gate). Daemon tests
  need `HOME=/tmp/zz-emptyhome XDG_CONFIG_HOME=/tmp/zz-emptyhome/config`. Worktrees left as caches:
  `zz-box-reds` (the alias branch plus its evidence commit), `zz-tui-capture-10` (capture-11),
  `zz-tui-modes-10` (modes-11, the fix pass running there), `zz-gate-9` (menus-3, no target),
  `zz-tui-stream-review` (no target), `zz-tui-context-10` (no target).

## Resuming on another machine

1. `compat/fetch-tmux.sh` and `compat/fetch-corpus.sh`; `compat/check.sh` (wire 103 unreleased).
2. `python3 compat/tui/tracker.py check` and `ready`; `git fetch origin '+refs/heads/campaign/*:refs/remotes/origin/campaign/*'`
   and read the tips of `tui-stream-alias`, `tui-capture-11`, `tui-modes-11` and, if present,
   `tui-stream-alias-2`, `tui-modes-12`: a lane that finished after this file was written left its
   report as the last message of its log on the ubuntu box and its records on its branch.
3. `export ZZ_BOARD_HOLDER=<box>/orchestrator`; `python3 compat/board.py status`; claim
   `F-TUI-CYCLE-9-LANES` or mint a cycle 11 front under TRIAGE.
4. Get fabrico's decision on customize-mode and suspend-client, or plan a third modes lane.
5. In parallel: the TUI-018 fix pass (three blockers above), the capture re-review or its next lane,
   the modes re-review; gate each with the trimmed protocol; the last gate verifies TUI-011 if all
   five children are verified and then runs the close-out sweep and the corpus restamp for CI.
6. A macOS box unblocks TUI-013 (`run-2.js`, recipe in `compat/tui/README.md`).
