# Session-target fix pass, cycle 12

This pass rebases 82746ea3 onto origin/main be5709df (zz 0.10.0).
The gate owns verification. TUI-015 remains at review with three pane-base-index
records for the sibling lane.

The session-only compound parser normalizes exact empty session/window components
before resolving them. `cli:=` and `=:` resolve the current window/session. An
omitted session in `:.%N` resolves the absolute pane globally; an explicit session
still requires containment. Named-session errors retain their precedence.
TargetSlot::Session still gates compound parsing. Internal window and pane
fallbacks keep name-only lookup. Both session_targets tests and
window_targets_accept_pane_forms_like_tmux pass.

The shared fixture adds the two exact-empty forms across lock-session, has-session
and list-windows, and a foreign-session absolute-pane case across those commands.
Self-check verifies equivalence for the empty forms, injects a one-sided rejected
result into the compared exit/stderr artifacts, and checks a global absolute pane
that exists on zz alone. The existing lock-client record closes with a status-row
sabotage. The pane-base-index records and their owner remain unchanged.

Shared build, fixture, probe and validation artifacts live in
../../TUI-017/attempt-03 (repository path compat/tui/evidence/TUI-017/attempt-03).
That directory's notes document the initial mixed build, retained failed probes,
and main's pre-existing exit-2 policy conflict. Those eight assertions remain red;
this worker does not claim that the shared fixture is wholly green.

## Repeated fixture result

All three runs (22, 23, 24) return exit 1 with the same eight inherited failures:

```text
8 of 186 asserted comparisons differ, 32 recorded (0 for a sibling lane, owners TUI-014=6 TUI-015=3 TUI-017=4 decided:TUI-015=4 decided:TUI-016=1 decided:TUI-017=6 gap:clients.interactive-refresh=8 unattributed=0)
```

All eighteen added regressions pass on all three runs. The one recorded-to-asserted
flip is lock-client: its attached channels now assert too. Self-check passes
(25, exit 0), and attached-client passes (29, exit 0). Both verify-claims runs
(32 and 33) return exit 1 because the shared fixture is nonzero.

The final delta corpus completes all 163 rows with 13 final divergent rows and
zero missing rows. Capture-pane and targets pass. Eight of the 21 first-run
failures pass on retry. The exact failure list and attribution limits are in
TUI-017/attempt-03/notes.md and corpus-summary.json; this is a partial delivery.
