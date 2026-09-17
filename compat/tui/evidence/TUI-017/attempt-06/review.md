# Lane audit; gate pending

This is the lane's evidence audit, not an independent gate approval. TUI-015 and
TUI-017 stay at review, with three and four ordinary owned records respectively.
The only carried recorded-to-asserted flip is lock-client. This continuation
adds no flips and weakens no assertion.

The child-start experiment supports keeping the synchronized split observation:
40 measured pairs show essentially equal startup medians, 20 candidate-slower
pairs, and no consistent slowdown. It verifies the live child's window and the
parked caller on every sample. The complete min/median/tail distributions and
wide uncertainty interval remain visible. The six direct differential runs pass;
both early-return and wrong-window sabotages fail as intended.

The source audit keeps session-only compound dispatch separate from internal
pane/window fallbacks, and preserves main's own session-ID pins. Capture changes
are outside the split spawn path. The clean-main comparison uses the same
headless package/profile and all 524 archived crate files match origin/main.

Read notes.md and the raw outputs for checks, retries and limits. In particular,
failed full-suite and attached runs must not be converted into unconditional
pass claims merely because a focused rerun passes.
