Fix pass after the c0e9bd83 review rejection, 2026-09-17.

Rebased campaign/tui-customize-2 onto origin/main f9c52359, resolving generated
gap reports by regeneration. This pass targets campaign/tui-customize-3.

The first pre-fix focused run failed during setup because run_on_both does not
expand the PANE placeholder. The second reproduces array erasure and C-c
closure, then cascades because the old loop sends C-d after C-c already closed
the mode. The fixture now opens a fresh customize mode for each unbound key.
Those failed runs remain evidence of the probe development, not final proof.
The independent pre-fix self-check catches alias erasure, C-c closure and the
duplicate-window style control. Fresh attachments now isolate each style-tail
control from the earlier customize screens.

The initial filtered mux run predates the new tests and passes three tests.
The next run passes five tests, including all 76 array options, but fails the
local-scope test: its after-select-pane hook is session-scoped, so the window
case uses the wrong hook. The corrected test uses pane-focus-in.

Canceled only the lane's waiting build on slot 1 (exit 143, no cargo child) and
requeued through /tmp/zz-cargo.sh on slot 0. Other lanes' processes are untouched.

Credential-pattern inspection found values still present in two census-hooks
members of attempt-15-lifetime/corpus-logs.tar.gz. Removed the complete
show-environment dump blocks, updated member hashes and sizes, and amended the
rebased evidence commit before creating further commits. The new branch does
not carry the unredacted archive commit. census-hooks now queries only its
named test variable while firing after-show-environment. New fixture and corpus
runs use env -i with explicit required values; no environment dumps are saved.

The v1 attached array probe found hooks are absent from pinned customize-mode (window_customize_build_option rejects OPTIONS_TABLE_IS_HOOK). Removed hook rows from zz and narrowed edit coverage to the eight editable arrays, with a separate exclusion test. Stopped the four v1 corpus process groups at their first rows (143); retained partial outputs and restarted against v2. The v1 full package tests and clippy pass, but require reruns after this correction.

The minimal environment omitted a UTF-8 locale. The first chooser run fails because the pin draws ACS border letters instead of UTF-8 glyphs. Stopped its fixture pipeline, the in-progress client roster and verifier (143), retained their logs, and restarted all attached proofs with LANG=C.UTF-8 and LC_ALL=C.UTF-8. The binary is unchanged.

The shared origin/main ref advanced from the initial rebase base f9c52359 to 38c50df4 while proofs ran (the capture lane plus credential scrubbing). An isolated /home/demfabris/dev/zz-c11-modes-main-baseline worktree builds that tip for failure attribution. The candidate still has the measured initial rebase base f9c52359; main-advanced-paths.txt names the later runtime/harness changes.

The broad tail fill passes the two requested default cases but regresses short dim, colored and underlined rows. Short rows leave default cells in the pin’s allocation; an exact allocation boundary can retain the last style in capture. The corrected renderer clears an unselected row before writing only its contents, so the outer grid allocates and clears the same cells instead of receiving painted padding. Extra attached cases cover short dim, foreground, background, underline and a background run ending at column 20. The v3 package tests and clippy pass. The build initially reused stale current-main protocol artifacts from the shared target; touching this worktree’s crate roots forced recompilation without changing their contents.

Stopped the v2 candidate corpus batches and attached proof pipelines (143), keeping their partial and first-pass logs. The rejected-tip nine-row baseline completed with exit 1 and raw first/final logs retained. A fresh proof pass will follow integration of the main changes that arrived during this run.

The v4 renderer also flushes text at grid allocation boundaries. All ten extra style comparisons and both requested window controls/sabotages pass. Full mux and TUI package tests pass. The remaining development logs are retained; final proofs restart after the second rebase.

The 143 raw development and baseline corpus logs are preserved in corpus-logs.tar.gz. corpus-log-manifest.json lists every member, size and SHA-256; each archive member was verified before removing its unpacked duplicate.
