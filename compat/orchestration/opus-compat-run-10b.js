export const meta = {
  name: 'opus-compat-run-10b',
  description: 'Cycle 10b: Opus 5 gate for the cycle-10 client lane (popup Kitty viewport, popup pointer trio, desktop grid measurement), relaunched 2026-09-03 after the machine-move pause',
  phases: [
    { title: 'Review', detail: 'one Opus 5 reviewer at xhigh, adversarial, foreground only (skipped in stage gate)' },
    { title: 'Integrate', detail: 'Opus 5 MAIN gate at xhigh for the single client branch: workspace tests, clippy, delta corpus, records, board ledger' },
  ],
}

const A = args || {}
const M = {
  root: A.root || '/home/demfabris/dev/zz',
  dev: A.dev || '/home/demfabris/dev',
  holder: A.holder || 'ubuntu/orchestrator',
  machine: A.machine || '8-core, 30 GB Ubuntu 26.04 box (ubuntu)',
  cores: A.cores || 8,
  workerJobs: A.workerJobs || 4,
  workerThreads: A.workerThreads || 2,
  gateJobs: A.gateJobs || 8,
  gateThreads: A.gateThreads || 4,
  shards: A.shards || 4,
  protected: A.protected || "the user's zz daemon on the default socket under ~/.local/share/zz and any tmux server on the default socket /tmp/tmux-1000/default (none were running at launch; never assume that stays true)",
  bash: A.bash || '/bin/bash',
  boxNote: A.boxNote || "This box: /bin/bash is 5.3 (mapfile and associative arrays work); the file system is btrfs and accepts non-UTF-8 file names. Never write 'rm -rf $HOME' or 'rm -rf ~' even after re-exporting HOME to a scratch path: a local hook denies it; put the scratch directory in a plain variable (D=/tmp/<name>; rm -rf \"$D\"; mkdir -p \"$D\"; export HOME=\"$D\").",
  gitNote: A.gitNote || 'NETWORK GIT: origin is SSH (git@github.com:demfabris/zz.git) and works non-interactively on this box; never change the remote URL. Set GIT_TERMINAL_PROMPT=0 on network commands so a credential miss fails instead of hanging.',
}

const CLIENT_REPORT = {
  "branch": "campaign/batch-client-choosers-popups-opus",
  "fronts_done": [
    {
      "front": "display-popup.behavior-fidelity",
      "items_closed": [
        "semantic:display-popup-kitty-images",
        "semantic:display-popup-border-drag",
        "semantic:display-popup-context-menu",
        "semantic:display-popup-to-pane"
      ],
      "proofs": [
        "ZZ_COMPAT_TMUX=/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=/home/demfabris/dev/zz/compat/.cache/plugins compat/run.sh --strict-geometry smoke/display-popup-kitty-noresize smoke/display-popup-kitty smoke/display-popup-drag smoke/display-popup-menu smoke/display-popup-to-pane",
        "cargo test -p zz-terminal --jobs 4 -- --test-threads=2",
        "cargo test -p zz-protocol --jobs 4 -- --test-threads=2",
        "cargo test -p zz-mux --jobs 4 -- --test-threads=2",
        "cargo test -p zz-tui --jobs 4 -- --test-threads=2",
        "cargo test -p zz-client --jobs 4 -- --test-threads=2",
        "cargo test -p zz --lib --jobs 4 -- --test-threads=2",
        "cargo test -p zz-daemon --lib --jobs 4 -- --test-threads=2",
        "cargo build -p zz-client-ffi -p zz-tui -p zz --jobs 4",
        "cargo clippy -p zz-protocol --all-targets --all-features --jobs 4 -- -D warnings",
        "cargo clippy -p zz-terminal --all-targets --all-features --jobs 4 -- -D warnings",
        "cargo clippy -p zz-daemon --all-targets --all-features --jobs 4 -- -D warnings",
        "cargo clippy -p zz --all-targets --all-features --jobs 4 -- -D warnings",
        "cargo clippy -p zz-mux --all-targets --all-features --jobs 4 -- -D warnings",
        "python3 compat/tmux-tracker.py check",
        "ZZ_COMPAT_TMUX=/home/demfabris/dev/zz/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=/home/demfabris/dev/zz/compat/.cache/plugins compat/run.sh --check-summary"
      ]
    }
  ],
  "fronts_skipped": [
    {
      "front": "choosers.command-flags",
      "why": "Over budget with a named blocker, and the sizing above it was wrong in four places, all now written into the group reason. Both remaining keys that matter (x, X) and ':' raise a prompt through mode_tree_set_prompt, and that prompt belongs to the MODE (mtd->prompt, drawn inside the mode's own screen, answered by mode_tree_key before anything else), not to the client. zz has no prompt that belongs to an overlay: command_prompts is a per-client slot and popup_other_overlay_present already counts choose_trees, so no prompt can be raised while a chooser is open. flag:choose-tree:-y is PROMPT_ACCEPT on those two prompts and cannot be accepted before they exist. The shape that unblocks it is the one display-popup's context menu took this run (an overlay owning its own nested overlay, inserted by the overlay rather than by the command), which is why the popup work was worth doing first. Corrections recorded in the reason: the kill prompts carry their target ('Kill session %s? ', 'Kill window %u? ', 'Kill pane %u? ', 'Kill %u tagged? ', and X is inert when nothing is tagged); ':' raises '(%u tagged) ' or '(current) '; -y only fires where PROMPT_SINGLE is also set, so it answers x and X and never ':'; O steps a per-mode sequence (window_tree_order_seq is index/name/activity/z, window_buffer_order_seq is creation/name/size) and sort_next_order wraps from an order outside the sequence too; and window_buffer_key adds e, d, D and P on top of mode_tree_key, so tagging is load-bearing in the buffer chooser as well. No code landed for this group."
    },
    {
      "front": "rendering.geometry-residue",
      "why": "semantic:attached-gui-pane-width stays open, but two claims in its reason were refuted and the reason now records the measurement plus what actually closes it. Code did land: the desktop client's grid measurement is now the pure function terminal_grid_size in crates/zz/src/terminal/element.rs (pixel extent plus cell metrics in, columns/rows/physical cell size out) with unit tests for an exact fit, a box that is not a whole number of cells (which floors, and is where a cell of drift comes from), one device pixel of slack, the scale factor, and a degenerate box. Refutation 1: a harness for the desktop producer already existed - crates/zz/src/workspace/view.rs drives TerminalView::update_geometry inside a #[gpui::test] window and captures the writeback through MuxClient::record_input_for_test. Refutation 2: the prescribed writeback reorder is not a reorder. terminal_geometry_for_mode already carves from the engine (engine.pane_geometry) under window-size manual, and client_terminal_geometry already carves pane_geometry_at_window_extent for a Control client; only an Interactive client under latest/largest/smallest has its own per-pane report used verbatim, and that report is what reaches the PTY while the format reports the engine's allocation. Replacing it with a carve of inner.client_sizes breaks, because zz's interactive clients draw chrome inside the window they report (the raw TUI's ~29-column sidebar and two rows, the desktop client's own), while window_width/window_height are contractually the client's whole size - the attached-client fixture pins 141x31, 101x21 and 73x19 straight against the outer pane sizes. Carving the PTY from that extent oversizes and clips the pane; carving the window extent from the drawn grid changes what window_width reports and breaks those pinned rows. Closing this needs a decision about which extent the window formats report for a client that draws chrome, not a writeback change, so the writeback was left alone."
    }
  ],
  "touched_commands": [
    "display-popup",
    "display-menu",
    "split-window"
  ],
  "touched_packages": [
    "zz-terminal",
    "zz-protocol",
    "zz-daemon",
    "zz-tui",
    "zz"
  ],
  "notes": "PROTOCOL BUMP: YES, 95 -> 96, all sites done in commit eb8a5d2 (message.rs constant + same-file assert, hunt_claims.rs renamed to protocol_version_on_this_commit_is_ninety_six with the pinned hello-frame hex 0x5F -> 0x60 in both positions, wire-protocol.md title/constant/byte row/changelog, knowledge/protocol/index.md, knowledge/index.md, knowledge/crates/zz-protocol.md twice). The change is APPENDED: PopupAction::Pointer { pointer: PopupPointer, view: Option<TerminalViewAction> } after Close, plus the two new types it carries, PopupPointer (column, row, button, drag, release, meta) and PopupPointerButton (None, Left, Middle, Right). Nothing else on the wire changed. cargo test -p zz-protocol, cargo test -p zz-client and cargo build -p zz-client-ffi -p zz-tui -p zz all green; the C ABI needed no header change because PopupAction is not re-exported through include/zz-client.h.\n\nZONE EXCURSIONS (all listed, all minimal):\n- crates/zz-terminal/src/session.rs: ONE line beyond the allowed per-view-viewport region - terminal.resize(geometry...) right after Terminal::new in run_pty_worker, plus one regression test. The prescribed recipe (latest_viewport's fallback / publish's by_view clearing) named the wrong producer and I could not implement it as written; the measurement is in the group reason. The popup terminal is attach_view'd at spawn so by_view is populated from the first publish, and both the per-view snapshot and the fallback build placements from the same KittyGraphicsState call. What actually failed is that Terminal::new takes no cell pixel geometry and libghostty's Terminal.resize is the only thing that sets width_px/height_px, while a Kitty placement's render info is measured in cell pixels - so a session that never saw a Command::Resize dropped every placement on the info.pixel_width == 0 guard. Ordinary panes hide it (the first layout pass resizes them); a popup is spawned at its content size and never resized.\n- crates/zz-protocol/src/message.rs: PopupAction only, plus the bump. Hunks are the PopupAction enum and the three new type definitions right after it, nowhere near byte-argv/output territory.\n- crates/zz-protocol/src/lib.rs: two names added to the re-export list.\n- crates/zz-daemon/src/daemon.rs: the popup regions only - PopupSession gains a pointer field, input_popup gains the Pointer arm (popup_pointer), plus raise_popup_menu, popup_menu_choice, popup_make_pane, the popup_menu_rows/popup_drag_origin/PopupBorder helpers next to popup_content_size, MenuSession gains popup_owner, and input_menu gains one early branch for it. No connection/queue/worker/Control/display-panes code touched.\n- crates/zz/src/terminal/element.rs: measurement extraction plus tests (this is the geometry-residue front, GUI zone).\n\nWHAT THE PIN ACTUALLY DOES, where it contradicted the batch:\n- The batch said to relax display-menu's any_overlay_present so a menu can be raised over a popup. Do NOT do that: cmd_display_menu_exec returns CMD_RETURN_NORMAL the moment tc->overlay_draw != NULL, and server_client_set_overlay clears the existing overlay before installing a new one. Measured on the pin with a popup up on an attached client: display-menu -c drew nothing, its item never ran, and the next key went into the popup's job. zz's gate already matches and stays. The popup's context menu is the popup's own pd->md, so zz now inserts a MenuSession marked popup_owner from the popup's pointer arm and routes the chosen row through its key the way popup_menu_done switches on it. This is also the shape the chooser prompt needs.\n- The drag does not arm on the press. popup_key_cb reads the border from the report it is handling and the button from the previous one, so MOVE is armed by the first DRAG report that is still on the border, and the box only moves on the one after that. Press, one drag away from the border, release moves nothing at all on the pin - I measured that dead end before finding the working shape.\n- A mouse menu's button RELEASE is itself a menu key and closes the menu on both binaries, so the menu scenario raises it with a press and no release.\n\nFALSIFICATION (both measured, both recorded in the registry):\n- Without the session.rs resize call, 7 of the 9 checks in smoke/display-popup-kitty-noresize fail on the zz side; only popup-content-reaches-the-client and (vacuously) closing-the-popup-places-nothing-new pass.\n- With the daemon's Pointer arm stubbed out, smoke/display-popup-drag fails border-drag-moves-the-popup on the zz side.\nThe no-resize scenario also compares the image id inside the a=p escape across the replacement and checks that the delete in the replaced snapshot names the id the opened snapshot placed, because every popup paint suspends and re-places so a bare delete-plus-place proves nothing - that was the gate's objection and it is now asserted.\n\nSCENARIO NAMING: the batch named smoke/chooser-keys.txt and smoke/chooser-kill-keys.txt; those were not written because the chooser front was skipped. The four that landed are smoke/display-popup-kitty-noresize.txt, smoke/display-popup-drag.txt, smoke/display-popup-menu.txt and smoke/display-popup-to-pane.txt, each with a fixture, each clean on BOTH binaries, all four summary rows added; compat/run.sh --check-summary is green at 203 scenarios / 2539 steps.\n\nREGISTRY: display-popup.behavior-fidelity emptied and moved from gaps[] to closed[] with closed_on 2026-09-02 (I had to re-sort closed[] by id afterwards - the tracker enforces it and the append broke the order). choosers.command-flags and rendering.geometry-residue keep their items and gained the measurements above. No relocations, no re-scoped acceptance clauses, no product decisions recorded. catalog.rs and compat_manifest_tests.rs were NOT touched because no flag was promoted; cargo test -p zz-mux ran green anyway as the mandatory registry gate.\n\nFLAKES SEEN: daemon::tests::client_focus_closes_display_panes_and_preserves_chooser_modes failed once under the full --lib run at its take_reliable_messages assertion, then passed three times out of three exact-solo. That is the documented flake, not a regression; 818 of 819 passed on the loaded run and the whole suite passed on two earlier full runs this session.\n\nPRE-EXISTING CLIPPY NOISE: cargo clippy -p zz-tui --all-targets [--all-features] fails with 11 dead-code errors, every one of them in crates/zz-daemon/src/{lifecycle,paths,transport}.rs, which my diff never touches. Building zz-daemon as a library dependency of zz-tui leaves its daemon-binary-only items unreferenced. cargo clippy -p zz-daemon --all-targets --all-features is green, and so is CI's workspace-wide invocation, which builds the daemon binary. Nothing to fix on this branch.\n\nRESIDUE LEFT OPEN AND RECORDED: the GPUI client still sends PopupAction::TerminalView for popup pointer events, so the border/drag/menu policy is the raw TUI's today. The acceptance names attached clients and the raw TUI proves it on both binaries, but the desktop client's own pointer path is unfinished and is written into the closed group's resolution so it is not lost.\n\nBRANCH: four commits. 52e608c (kitty per-view viewport), eb8a5d2 (popup pointer policy, carries the v96 bump), bb1ba5b (desktop pane grid measurement), ff66ddc (drops three prose lines from the popup pointer test - a separate commit rather than an amend because eb8a5d2 and bb1ba5b were already pushed and force-pushing is off the table)."
}

const CLIENT_REVIEW = {
  "lane": "client",
  "verdict": "approve-with-fixes",
  "confirmed_defects": [
    {
      "front": "display-popup.behavior-fidelity (semantic:display-popup-context-menu)",
      "severity": "must-fix",
      "description": "The popup menu draws one blank row more than the pin whenever no paste buffer exists, which is the default state of a fresh server and the state the worker's own scenario runs in. popup_menu_rows(None) in crates/zz-daemon/src/daemon.rs emits [Close, None, None, Fill Space, Centre, None, h, v] and raise_popup_menu pushes every None as a separator, so zz's menu is Close / sep / sep / Fill Space / Centre / sep / h / v (8 rows, height 10). The pin's menu_add_item (menu.c) drops an item whose format expands empty (`if (*s == '\\0') { menu->count--; return; }`) and refuses a separator right after another one (`if (line && menu->items[menu->count - 1].name == NULL) return;`), so the pin draws Close / sep / Fill Space / Centre / sep / h / v (7 rows). Measured on both binaries with a real pty client (zzreview-popup-menu-rows, counting the '\u251c' separator glyphs in the menu snapshot): zz seps=3, pin seps=2. The doc comment on popup_menu_rows ('a blank line when no buffer exists, because menu_add_item turns an empty name into a separator') states the opposite of the pinned source. smoke/display-popup-menu only asserts row text presence, so it cannot see the row count; the mouse hit-box rows below the first separator are shifted by one on zz.",
      "suggested_fix": "In raise_popup_menu, build the rows the way menu_add_item does: skip the Paste entry outright when there is no buffer (do not push None for it), skip a separator when the previous pushed item is already a separator or when nothing has been pushed yet; fix the popup_menu_rows doc comment. Re-run smoke/display-popup-menu and the separator-count probe on both binaries."
    },
    {
      "front": "display-popup.behavior-fidelity (semantic:display-popup-context-menu)",
      "severity": "must-fix",
      "description": "A popup-owned MenuSession outlives the popup. take_popup / retire_popup / close_popup in daemon.rs never touch inner.menus, so when the popup closes while its menu is up (job exit with -E, display-popup -C, kill) the MenuSession marked popup_owner stays in inner.menus and the raw TUI keeps drawing it over the pane; the TUI's menu_input_route then swallows keys until the user presses Escape. On the pin the menu is pd->md, drawn only by popup_draw_cb, so it vanishes with the popup (and popup_resize_cb frees it on a client resize). Measured with the dead reviewer's zzreview-popup-menu-orphan on both binaries: pin clean:7; zz fails keys-after-the-popup-reach-the-pane want=yes got=no (the line typed after the -E popup's job exited never reached the pane's shell; it reached it only after an Escape).",
      "suggested_fix": "When a popup is taken (take_popup, or the same place retire_popup publishes Popup{state: None}), also remove a MenuSession with popup_owner == true for that client and publish EventPayload::Menu { state: None }; consider doing the same on the popup's client-resize path to match popup_resize_cb's menu_free_cb. Re-run the orphan probe: popup -E with a 4s job, raise the menu with button 3 outside, wait for exit, type a line, assert it reaches the pane without an Escape."
    },
    {
      "front": "display-popup.behavior-fidelity (semantic:display-popup-border-drag)",
      "severity": "must-fix",
      "description": "The 'previous report' popup_pointer consults is not the pin's. The pin reads m->lb/lx/ly, which tty_keys_mouse fills from tty->mouse_last_* and updates on EVERY mouse report (tty-keys.c: 'Update last mouse state' runs unconditionally), while zz's PopupPointerState.last_* is only written when the outcome is not Job and is skipped entirely on the outside-non-button-3 early return. So a drag that started on the border, left it into the content (Job outcome on zz, last_* untouched, last_drag still false, last_button still Left) and came back onto the border arms MOVE on zz but not on the pin, where `!MOUSE_DRAG(m->lb)` is false because the previous report was a drag. Measured on both binaries with SGR reports on a real pty client (zzreview-drag-reentry: press at top border col+4, drag inside at (col+6,row+3), drag back onto the top border at (col+8,row), drag to (col+14,row+5), release): pin unmoved, zz moved:5,10.",
      "suggested_fix": "In popup_pointer, update cursor.last_column/last_row/last_button/last_drag on every report before returning, including the Job outcome and the outside early return (the pin's tty->mouse_last_* has no exception); dx/dy for the arm then come from the previous report the way m->lx/m->ly do. Keep the smoke/display-popup-drag sequence passing and add the re-entry sequence to it or to a sibling scenario."
    },
    {
      "front": "display-popup.behavior-fidelity (semantic:display-popup-context-menu)",
      "severity": "must-fix",
      "description": "Fill Space resizes the popup's job on zz and not on the pin. popup_menu_done case 'F' in the pinned popup.c sets pd->sx/sy/px/py and calls server_redraw_client only; neither pd->s nor the job is resized (only popup_resize_cb and the SIZE drag call job_resize). popup_menu_choice \"F\" in daemon.rs calls terminal.resize(popup_content_size(..)). Measured with a popup job trapping WINCH and writing `stty size` (zzreview-fill-space): pin winch=no after F; zz winch=yes:27 69. The resolution text claims Fill Space and Centre are 'the two popup_menu_done box moves', i.e. fidelity, and records no divergence.",
      "suggested_fix": "Either drop the terminal.resize in the \"F\" arm so the job keeps its size the way the pin leaves it, or keep it and write the divergence into the group resolution as a deliberate choice with this measurement (the pin's F leaves the content at its old size in the top-left corner of the enlarged box). Either way the resolution must stop claiming the two are identical."
    },
    {
      "front": "display-popup.behavior-fidelity (semantic:display-popup-context-menu)",
      "severity": "must-fix",
      "description": "Centre and Fill Space rewrite the preferred placement on zz; the pin does not. popup_menu_done 'C' and 'F' change pd->px/py (and sx/sy for F) but never ppx/ppy/psx/psy, so the next owning-client resize (popup_resize_cb) puts the box back at the placement display-popup asked for; only popup_handle_drag's MOVE/SIZE update ppx/ppy/psx/psy. popup_menu_choice sets popup.preferred for both keys. Measured (zzreview-centre-resize: raise menu, press C, snap, resize the client 100x30 -> 90x26, snap): pin after-resize=origin (back at -x 5 -y 5), zz after-resize=centred. This contradicts the ppx/ppy contract the same group proved in display-popup-resize-lifecycle.",
      "suggested_fix": "Remove the popup.preferred assignment from the \"F\" | \"C\" arm of popup_menu_choice (leave it in the drag MOVE/SIZE path, where the pin updates ppx/ppy/psx/psy). Re-run smoke/display-popup-resize-lifecycle and the centre-then-resize probe."
    },
    {
      "front": "display-popup.behavior-fidelity (wire mirror)",
      "severity": "must-fix",
      "description": "knowledge/protocol/wire-protocol.md line 160, the ProtocolMessage schema row for `Popup`, still reads `action: PopupAction::{Text(String), Key { input, text_follows }, TerminalView(TerminalViewAction), Close}`; the v96 `Pointer { pointer, view }` variant appears only in the changelog entry at line 619. The page now contradicts crates/zz-protocol/src/message.rs, which it cites as its source.",
      "suggested_fix": "Add `Pointer { pointer: PopupPointer, view: Option<TerminalViewAction> }` to the schema row at line 160 (one-line edit); everything else in the bump (constant, same-file assert, hunt_claims name and 0x60 bytes, title, byte row, changelog, protocol/index.md, knowledge/index.md, crates/zz-protocol.md twice) is present and consistent."
    }
  ],
  "checks_run": [
    "GIT_TERMINAL_PROMPT=0 git -C /home/demfabris/dev/zz fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*' (origin/main = 997c4e3 = merge-base at fetch time; tip ff66ddc)",
    "reused /home/demfabris/dev/zz-review-client (already at ff66ddc; dead reviewer's dirty run.sh + zzreview-* probes saved to scratchpad, then git checkout -- . && git clean -fd)",
    "cargo build -p zz-tui -p zz -p zz-client-ffi --jobs 8 (ok)",
    "cargo test -p zz-protocol -p zz-terminal -p zz-mux --jobs 8 -- --test-threads=4 (all ok; session::tests::a_session_that_is_never_resized_still_publishes_kitty_placements ok)",
    "cargo test -p zz-tui -p zz-client --jobs 8 -- --test-threads=4 (ok)",
    "cargo test -p zz --lib --jobs 8 -- --test-threads=4 (671 ok incl. the two terminal_grid_size tests)",
    "cargo test -p zz-daemon --lib --jobs 8 -- --test-threads=4 (818 passed, 1 failed: daemon::tests::history_request_is_guarded_clamped_and_returns_self_contained_rows at take_reliable_messages(&unattached_mailbox).is_empty())",
    "cargo test -p zz-daemon --lib --jobs 8 -- --test-threads=1 --exact daemon::tests::history_request_is_guarded_clamped_and_returns_self_contained_rows x3 (3/3 ok: load flake)",
    "cargo clippy -p zz-daemon -p zz-protocol -p zz-terminal --all-targets --all-features --jobs 8 -- -D warnings (ok)",
    "ZZ_COMPAT_TMUX=... ZZ_COMPAT_CORPUS=... compat/run.sh --strict-geometry smoke/display-popup-kitty-noresize smoke/display-popup-kitty smoke/display-popup-drag smoke/display-popup-menu smoke/display-popup-to-pane smoke/display-popup-resize-lifecycle smoke/display-popup-style-refresh (all clean both binaries)",
    "compat/run.sh --strict-geometry smoke/display-menu-resize-lifecycle smoke/display-menu-mouse smoke/display-menu-action-queue smoke/display-menu-paste smoke/display-menu-cell-layout smoke/display-menu-shortcut-grammar smoke/display-menu-style-refresh smoke/args-parse-display-menu (all clean)",
    "compat/run.sh --strict-geometry smoke/zzreview-drag-reentry smoke/zzreview-fill-space smoke/zzreview-centre-resize smoke/zzreview-to-pane-direction smoke/zzreview-popup-leak smoke/zzreview-popup-menu-orphan smoke/zzreview-popup-menu-rows (review probes; leak and to-pane-direction clean, the other five diverge as reported)",
    "compat/run.sh --strict-geometry smoke/zzreview-menu-over-popup (display-menu -c over an open popup: drawn=no, item never ran on both binaries)",
    "compat/run.sh --check-summary with probes removed (summary current: 203 scenarios, 2539 steps; attached-client PASS)",
    "python3 compat/tmux-tracker.py check (valid, gaps.md current); python3 -m json.tool compat/tmux-gaps.json (ok); closed[] order verified sorted; registry structural diff main vs tip (only display-popup.behavior-fidelity moved; choosers.command-flags and rendering.geometry-residue reasons only)",
    "falsification: deleted the six-line terminal.resize after Terminal::new in crates/zz-terminal/src/session.rs, cargo test -p zz-terminal -- --exact session::tests::a_session_that_is_never_resized_still_publishes_kitty_placements FAILED (unresized session never published a Kitty placement), file restored",
    "git diff origin/main...HEAD scanned for added non-doc // comments (none), git log for attribution trailers (none), include/zz-client.h for PopupAction (absent, no header change needed)",
    "pinned source read: popup.c popup_key_cb/popup_handle_drag/popup_menu_done/popup_make_pane/popup_resize_cb/popup_draw_cb/popup_free, menu.c menu_add_item/menu_key_cb/menu_free_cb, tty-keys.c SGR decode and mouse_last_* update, cmd-display-menu.c:312 overlay_draw refusal, server-client.c server_client_set_overlay"
  ],
  "notes": "CONTRACT AUDIT. Acceptance clauses unchanged (no re-scope): (1) pinned pointer probes cover context-menu and border-drag policy without input leakage or duplicate action execution; (2) attached clients cover popup-to-pane conversion and popup Kitty rendering, replacement, close cleanup. All four closes are proved through a real pty client on both binaries: display-popup-drag, display-popup-menu, display-popup-to-pane drive SGR mouse bytes through chooser-drive.py; display-popup-kitty-noresize drives no resize, compares the a=p image id across the replacement and asserts the delete names the opened id, and the pin side asserts no graphics escape (want_place=no etc. per side). Input leakage on the drag path is clean (dead reviewer's popup-leak probe: the job saw exactly the inside press/release and nothing from the border drag on both). Duplicate execution: input_menu removes the MenuSession before popup_menu_choice runs and the TUI holds input while menu_action_pending. The two pin facts the batch asked for hold: cmd_display_menu_exec returns at tc->overlay_draw != NULL (cmd-display-menu.c:312) and my paired probe shows display-menu -c over a popup draws nothing and runs nothing on both; the popup's menu is pd->md (popup_key_cb `menu:` label). 'h' is LAYOUT_LEFTRIGHT on the pin and Axis::Horizontal is split-window -h in zz-mux; probe shows side-by-side on both with the attached client drawing the converted pane on both (zz geom 0:0,0,35x29 1:36,0,35x29 within the raw TUI's 71-column grid; pin 50/49 within 100). The `!MOUSE_DRAG(m->lb)` arming condition the worker used is in the pinned popup_key_cb, but its previous-report memory is tty-level (defect 3). The five must-fix items above are all in the closed items' own policy and all contradict the resolution text, which claims fidelity for F/C and the menu rows; none is a fixture-blindness case, and each is a few lines in daemon.rs plus a one-line doc edit, so the close stands once they land and smoke/display-popup-{drag,menu,to-pane,resize-lifecycle} plus the probes re-run clean. My probe scenarios and fixtures are saved at /tmp/claude-1000/-home-demfabris-dev-zz/3301196e-23b0-41fc-a9c1-fead41e0cfd2/scratchpad/myprobes/ (zzreview-drag-reentry, fill-space, centre-resize, to-pane-direction, menu-over-popup, plus the dead reviewer's popup-leak, popup-menu-orphan, popup-menu-rows); each is the worker's display-popup-menu.sh header plus a short body, so the gate can drop them into compat/scenarios/smoke/ to re-verify. SUSPICIONS, not confirmed: (a) the pin drops the popup's menu on a client resize (popup_resize_cb -> menu_free_cb); zz's popup resize path probably keeps the popup-owned MenuSession, untested; (b) pointer.meta is any ALT, the pin requires modifiers == META exactly (shift/ctrl+alt drags differ); (c) the pin's Paste row carries #[underscore] on the buffer name, zz draws it plain; (d) raise_popup_menu's horizontal origin uses width/2 where the pin uses (menu->width+4)/2, identical only if MENU_ROW_MARGIN == 4, not verified; (e) on a split failure zz's popup_make_pane retires the popup with terminate=true, while the pin returns from popup_make_pane leaving the popup up (lc == NULL early return in the pinned source), a divergence on a too-small window. RESIDUE DISCLOSURE: all three carried into the registry (GPUI PopupAction::TerminalView residue in the closed resolution; the chooser prompt as mtd->prompt in choosers.command-flags with the kill-prompt targets, PROMPT_SINGLE and order sequences; the window-format extent decision, terminal_grid_size and record_input_for_test in rendering.geometry-residue). INVARIANTS: zone excursions match the notes exactly (session.rs one hunk + one test; message.rs PopupAction + bump; lib.rs re-exports; daemon.rs popup/menu regions and the zz_protocol import block; element.rs extraction); no code comments; doc comments attached; no trailers; JSON round-trips; closed[] sorted; tracker green; check-summary green at 203/2539; open items 457 -> 453. FLAKE: the one daemon red is the documented take_reliable_messages timing flake (3/3 solo). REBASE NOTES for the gate: at my fetch origin/main was still 997c4e3, the branch's base, so it applies as-is; if the queue/copy lanes land first with their own 95->96 bumps this bump must be renumbered to the next free version at every site: message.rs constant and the detached_reason_holds_its_appended_wire_field assert, hunt_claims.rs test name and both hello-frame bytes (0x60 at positions 6 and 9), wire-protocol.md title/constant/byte row/changelog ordering plus the schema row fix above, knowledge/protocol/index.md, knowledge/index.md, knowledge/crates/zz-protocol.md (two rows). daemon.rs conflict surface: the zz_protocol import list at the top, PopupSession and MenuSession struct definitions, input_popup's match, input_menu's head, helpers next to popup_content_size. catalog.rs and compat_manifest_tests.rs untouched here (no flag promoted). summary.md adds four rows in alphabetical position. The review worktree /home/demfabris/dev/zz-review-client pre-existed (not created by me) and is left clean at ff66ddc; stray harness processes on /tmp/zzc-2856857.sock belong to the zz-opus-dint lane, not mine, and were left alone."
}

const GATED_BRANCH = A.gatedBranch || 'campaign/batch-client-choosers-popups-opus-gated'

const REVIEW_SCHEMA = {
  type: 'object',
  required: ['lane', 'verdict', 'confirmed_defects', 'checks_run', 'notes'],
  properties: {
    lane: { type: 'string' },
    verdict: { type: 'string', enum: ['approve', 'approve-with-fixes', 'reject'] },
    confirmed_defects: { type: 'array', items: { type: 'object', required: ['front', 'severity', 'description', 'suggested_fix'], properties: {
      front: { type: 'string' },
      severity: { type: 'string', enum: ['blocker', 'must-fix', 'nit'] },
      description: { type: 'string' },
      suggested_fix: { type: 'string' },
    } } },
    checks_run: { type: 'array', items: { type: 'string' } },
    notes: { type: 'string' },
  },
}

const GATE_SCHEMA = {
  type: 'object',
  required: ['merges', 'progress_after', 'problems'],
  properties: {
    merges: { type: 'array', items: { type: 'object', required: ['branch', 'merged', 'merge_commit'], properties: {
      branch: { type: 'string' }, merged: { type: 'boolean' }, merge_commit: { type: 'string' },
      gate_summary: { type: 'string' }, review_actions: { type: 'string' }, flakes: { type: 'string' },
    } } },
    progress_after: { type: 'string' },
    board_updates: { type: 'string' },
    problems: { type: 'string' },
  },
}

const RUN_ENV = `ZZ_COMPAT_TMUX=${M.root}/compat/.cache/tmux-src/tmux ZZ_COMPAT_CORPUS=${M.root}/compat/.cache/plugins`

const FOREGROUND = `FOREGROUND ONLY: run every command in the foreground (a plain Bash call with a timeout; split long scenario batches into several calls). Never use Monitor, run_in_background, or any background task, and never end your turn to wait for one: a subagent that ends its turn without its structured report is a failed agent and its work is lost. You are done only when you have emitted the structured report.`

const REVIEW_COMMON = `You are an adversarial code reviewer for the zz tmux-compat campaign (repo demfabris/zz). A worker pushed a campaign branch in cycle 10; its first reviewer died before reporting, so your verdict decides what the integration gate trusts. Read-only toward history: NEVER push, NEVER commit, NEVER touch the board or GitHub issues, never edit ${M.root} itself. ${FOREGROUND}

SETUP: GIT_TERMINAL_PROMPT=0 git -C ${M.root} fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*' (${M.gitNote}). Scratch worktree at the branch tip: git -C ${M.root} worktree add ${M.dev}/REVIEWDIR <branch-tip-sha> (if REVIEWDIR exists from a previous cycle and is clean, checkout --detach the tip there instead to reuse its build; a dirty leftover from the dead reviewer may be reset with git -C ${M.dev}/REVIEWDIR checkout -- . && git clean -fd, it is your lane's scratch); remove a worktree you created when done (worktree remove --force). The branch was written against origin/main 997c4e3; origin/main has since taken the cycle-10 queue and copy lanes (both also bumped the protocol to 96), so review the branch AT ITS OWN TIP and note anything the gate's rebase will have to reconcile (the protocol changelog, catalog counters, compat_manifest_tests counts, daemon.rs regions). ${M.boxNote}

MACHINE ETIQUETTE: the box is otherwise idle, but stay at cargo test -p <pkg> --jobs ${M.gateJobs} -- --test-threads=${M.gateThreads}, no workspace-scale anything, cargo output to a log file + check exit code. Timeout-guard cli_binary tests (wait_exit can hang under load). Throwaway pin servers -L zzprobe-$$ only, kill after; never kill servers you did not start (${M.protected}), never pkill tmux or zz.

METHOD, in order of value:
1. CONTRACT AUDIT: for every closed OR relocated slug (worker report + git diff origin/main...HEAD -- compat/tmux-gaps.json, using the merge-base since main moved), find the proof in the diff that asserts the group acceptance clause. A test asserting zz behavior with no pin derivation is a defect; quote the clause. The four popup closes name attached clients: each must be proved by a real pty client on both binaries (the pin emits no Kitty graphics and the scenario asserts that on its side; the pointer trio is proved with SGR mouse bytes). A RE-SCOPED acceptance clause is legitimate only when the reason records the old clause, the refuting measurement, and the probe; re-run that probe on the pin yourself. The worker recorded that the pin refuses a menu over a popup (cmd_display_menu_exec returns when tc->overlay_draw is set) and that the popup's context menu is the popup's own pd->md: verify both on the pin.
2. PROOFS AT TIP: run the worker's claimed proof suites yourself at the branch tip (cargo test -p for every touched crate; the named scenarios). ALSO run cargo test -p zz-daemon --lib, cargo test -p zz-client and cargo build -p zz-tui -p zz -p zz-client-ffi (a wire type changed). Any red at tip is a blocker regardless of the report.
3. ORACLE SPOT-CHECKS: 3-5 riskiest claims (the drag arming on the second border report, the button-3 menu on the left/top border and outside, popup_make_pane's split direction and what happens to the popup, the Kitty placement without a resize, the pixel-geometry claim about libghostty's resize) verified against the pinned binary yourself (${M.root}/compat/.cache/tmux-src/tmux; the pinned C source sits beside the binary, read it when subtle). Past best catches were pin-side gating the worker's fixture had configured away, a probe that passes for the wrong reason, state left armed after a failure, and a popup Kitty close whose fixture resized the client before every snapshot: look for the configuration that would make a fixture blind.
4. TEST HONESTY: run the branch's new/changed tests and its scenarios, AND the pre-existing scenarios that exercise the same surface (display-popup-resize-lifecycle, display-popup-style-refresh, smoke/display-popup-kitty, the display-menu scenarios, smoke/display-menu-resize-lifecycle) (${RUN_ENV} compat/run.sh --strict-geometry <names>). Failing/ignored/tautological = blocker. Check the durable registry resolution carries every divergence the worker disclosed in its notes (the GPUI client still sends PopupAction::TerminalView for popup pointer events; the chooser prompt belongs to the mode; the window-format extent decision the geometry residue needs).
5. INVARIANTS: zone discipline (out-of-zone files listed in notes); wire rule (wire-reachable change => complete 95->96 bump incl. hex 0x5F->0x60 fixture and knowledge mirrors, changelog names every variant, field and type and says appended honestly); no code comments; doc comments still attached to the fn they describe; no attribution trailers; registry round-trips (python3 -m json.tool); tracker check green on the branch; --check-summary green.
6. CALIBRATION: default to refuting each close, but confirmed_defects only with PROOF (probe, failing rerun, quoted contradiction). Suspicion goes in notes. blocker = wrong close or would break main; must-fix = gate applies before merge; nit = mention. When a blocker is a wrong close, say whether reverting that commit applies cleanly at tip and what it takes with it.
VERDICT: approve / approve-with-fixes / reject (a blocker the gate cannot fix in minutes). checks_run lists exact commands. Thorough but bounded: well under an hour.`

const review = (A.stage === 'gate') ? CLIENT_REVIEW : await agent(REVIEW_COMMON + `

LANE: client. BRANCH: ${CLIENT_REPORT.branch}. REVIEWDIR: zz-review-client.
WORKER REPORT (verify, do not trust):
${JSON.stringify(CLIENT_REPORT, null, 2)}`, { label: 'review:client', phase: 'Review', model: 'opus', effort: 'xhigh', schema: REVIEW_SCHEMA })

if (A.stage === 'gate') log('Gate-only stage: using the embedded cycle-10 client review (approve-with-fixes, six must-fixes) from 2026-09-03')
if ((A.stage || 'all') === 'review') {
  log('Review-only stage: the gate runs later by resuming this run with stage all')
  return { review }
}

phase('Integrate')
const summaries = [{ key: 'client', lock_front: 'F-CLIENT-CHOOSERS-POPUPS-V2', review: review || null, ...CLIENT_REPORT }]
const gatePrompt = `You are the integration gate for the zz tmux-compat campaign (repo demfabris/zz, board = GitHub issue 7). You integrate ONE branch, the cycle-10 client lane, whose reviewer died in the main cycle-10 run; the queue and copy lanes of cycle 10 are already on origin/main together with the cycle-10 ledger recompute (2af51ff). SINCE THAT PAUSE origin/main took 22 commits of NON-CAMPAIGN work (the iOS/iPad client, C ABI verbs for paste, command replies and command-output cancel, agent-pane work, and 'Import the host tmux config from the daemon' which added one command spec and moved the catalog counters). None of it moved the meter. A merge probe on 2026-09-03 (git merge-tree --write-tree origin/main <gated tip>) reported exactly ONE conflict, in the generated knowledge/tmux/gaps.md; compat/tmux-gaps.json, crates/zz-daemon/src/daemon.rs and crates/zz-protocol/src/{lib,message}.rs all auto-merged. Re-measure rather than trust that, but expect the rebase to be cheap and the catalog/manifest recount to be the real work. If this gate runs on a different machine than the ubuntu box, the machine facts come in through args (root, dev, holder, cores, jobs, shards, protected, bash, boxNote, gitNote), the same way opus-compat-run-10.js takes them. You run ALONE on this ${M.machine}, full speed. ${FOREGROUND}

Lane summary, worker report + Fable review verdict:
${JSON.stringify(summaries, null, 2)}

REVIEW VERDICT BINDS YOU: approve-with-fixes => apply every must-fix on the branch (own follow-up commit) before its gate, re-running the reviewer's failing probe to prove each fix. reject => do NOT merge; post the blockers as a board note on the lock front, push the rebased tip as campaign/batch-client-choosers-popups-opus-gated, and stop. A blocker you can genuinely fix in minutes may be fixed and merged with the probe re-run as proof; a wrong close whose revert applies cleanly is such a fix (precedent 0fec342 + 9cab1fa: revert, then a records commit that puts the reviewer's measurement into the group reason and acceptance). review_actions must account for every confirmed defect. Missing review (null) => do a compressed contract audit yourself before gating.

BOARD IDENTITY: ZZ_BOARD_HOLDER=${M.holder} python3 compat/board.py <cmd> from inside a repo checkout. Verbs: release/withdraw REQUIRE --reason; note takes --note; candidate takes --commit --branch --base + repeatable --proof; integrated takes --merge + optional --gate; renew <FRONT> --lease 2h (always give a unit); withdraw needs TRIAGE held. The orchestrator holds F-CLIENT-CHOOSERS-POPUPS-V2 (expired => claim it back as ${M.holder} before the ledger step). BOARD FALLBACK: if a board command fails on GitHub authentication, append the exact command line to ${M.root}/compat/orchestration/board-replay-10.sh (do not commit that file), say so in board_updates, and continue.

${M.gitNote} Fetch git fetch origin +refs/heads/main:refs/remotes/origin/main '+refs/heads/campaign/*:refs/remotes/origin/campaign/*'; push git push origin HEAD:main. The shared checkout's local main branch may be stale; never use it, never touch it.
knowledge/tmux/gaps.md is generated: regenerate with tmux-tracker.py write-report on every conflict, never hand-merge it.
PROTOCOL RECONCILE: origin/main is already at PROTOCOL_VERSION 96 (the queue and copy lanes bumped it) and this branch bumped 95 -> 96 too; the rebase keeps ONE constant at 96, every v96 changelog bullet preserved (this lane's PopupAction::Pointer, PopupPointer and PopupPointerButton join the others as separate bullets), the hunt_claims fixture at 0x60 once; verify cargo test -p zz-protocol after reconciling. catalog.rs counters, compat_manifest_tests.rs counts and the daemon.rs popup/menu regions may conflict with what landed: resolve by recounting and keeping both hunks, then cargo test -p zz-mux -p zz-protocol, and grep the tree for conflict markers before building. When the rebase conflicts on compat/tmux-gaps.json, ${M.root}/compat/orchestration/gaps-merge.py BASE OURS THEIRS OUT merges the two sides by record id and exits 2 on a record both sides changed differently (feed it git show :1:compat/tmux-gaps.json, :2:, :3:), then regenerate gaps.md and recount.

THIS BOX: ${M.boxNote} Use the shared build directory CARGO_TARGET_DIR=${M.dev}/zz-gate-target for the gate worktree (leave it in place afterwards). Never touch ${M.protected}.

STEPS:
1. Fresh worktree: git -C ${M.root} worktree add ${M.root}-gate-client origin/main (remove leftovers with --force first). The lane's tip was already rebased onto 2af51ff and pushed as ${GATED_BRANCH} when the campaign paused on 2026-09-03 (the original campaign/batch-client-choosers-popups-opus stays at ff66ddc): fetch it, check it out in the gate worktree, and rebase it onto the current origin/main (which HAS moved: 22 non-campaign commits, e91f5c2 at launch); no fixes were applied yet, so every must-fix in the review is still owed.
2. Claim MAIN --lease 2h; hold it until the ledger recompute is pushed.
3. Gate stages, in order:
   a. cargo test --workspace --all-features --no-fail-fast --jobs ${M.gateJobs} -- --test-threads=${M.gateThreads} > log 2>&1 (check exit code; never pipe through tail). wait_exit_holds_the_control_process_until_a_second_blank_line can HANG under load; if the run wedges >20min with no output, sample the process; a lost-wakeup hang there counts as the known flake (verify solo).
   b. cargo clippy --workspace --all-targets --all-features -- -D warnings
   c. ${RUN_ENV} compat/run.sh --strict-geometry --delta origin/main..HEAD --commands display-popup,display-menu,split-window. Run --list TWICE and reconcile against git diff --name-only for compat/scenarios; shard up to ${M.shards} concurrent run.sh invocations over DISJOINT scenario subsets with separate result logs (compat/run.sh hard-codes RESULTS_DIR, so shards stay apart only by disjoint scenario names); run smoke/source-replay-diagnostics SOLO after the shards; any divergence re-runs alone before being called real.
   d. python3 compat/tmux-tracker.py check && python3 compat/board_test.py && compat/run.sh --check-summary
4. Flake rules: lone timing test failing loaded + passing exact-solo = flake, proceed (known list: copy-mode reconcile, client_focus_closes… (also fails about one solo run in three; two solo passes of three count), event_hooks_fire_after_mutation_with_captured_formats, history_request_is_guarded…, daemon_native_split_resize_commits_exactly…, nested_alias_queue_bubbles_shutdown…, control_sourced_run_shell_closes_before_raw_output…, request_full_enqueues_only_the_requested_visible_pane, display_menu_resize_lifecycle::a_resize_moves_the_menu…, zz-terminal pty_output_drains_while_the_input_writer_is_backpressured, wait_exit… hang, concurrent_default_interactive… "not a terminal" incl. misattribution, behavior-options one TOPO row under shard load). Anything else red = real: fix if minutes, else do not merge (push the gated tip as campaign/batch-client-choosers-popups-opus-gated), record, stop.
5. Push main. Non-fast-forward: fetch, rebase, bounded rerun (zz-tui, zz, zz-daemon --lib tests + the lane's scenarios), push. Never force. The campaign branch stays at its old tip on origin (never force it); say so in the report.
6. Ledger against F-CLIENT-CHOOSERS-POPUPS-V2: candidate (--commit tip --branch campaign/batch-client-choosers-popups-opus --base <pre-push main> --proof per stage), note (--note: groups covered, slugs closed, reviewer verdict + what you did about defects, the two skipped groups' measurements), integrated (--merge <sha> --gate "workspace+clippy+delta green"), release (--reason naming what stays open: choosers.command-flags and rendering.geometry-residue).
7. Holding MAIN: update TMUX_COMPAT_TRACKER.md from the merged registry: the four headline lines (Campaign delivery, Live work "<open> OPEN + <blocked> BLOCKED = <sum>", Ledger settlement "100 x (<closed> CLOSED + <accepted> ACCEPTED) / (<closed> CLOSED + <len(gaps)> LIVE)" one decimal, Exit evidence scenarios/steps from --check-summary), the Orchestration line, the Current checkpoint rows, the Campaign dashboard table rows (Live unresolved, Latest differential, Differential SHA-256 = sha256 of compat/results/summary.md, Ledger settlement), the Ledger settlement calculation block, and ADD the client merge to the existing "cycle-10 integration checkpoint" table (its Merges, Review stage, Workspace gates, Differential, Records gate and Summary SHA-256 rows), noting that the lane landed in a follow-up gate after its reviewer died. Records commit ("Recompute the live ledger after the cycle-10 client merge"), push, release MAIN.
8. Claim TRIAGE; if the worker's skip reasons prove a group contract is unprovable as written (the chooser prompt belongs to the mode; the geometry residue needs a decision about which extent the window formats report for a chrome-drawing client), post each as a residual on F-CLIENT-CHOOSERS-POPUPS-V2; release TRIAGE.
9. python3 compat/progress.py, full output.
10. Remove your zz-gate-client worktree. Leave zz-opus-* and zz-review-*.

Never stash/reset anything in ${M.root}. Never kill tmux or zz servers you did not start (${M.protected}). Report via structured output: merged/sha/gate_summary/review_actions/flakes, full progress output, board records, problems.`

const gate = await agent(gatePrompt, { label: 'gate:client', phase: 'Integrate', model: 'opus', effort: 'xhigh', schema: GATE_SCHEMA })

return { review, gate }
