# Target grammar after the tab revision

The final source is `686506a1`, based on `origin/main` at `38c50df4`. The resolver
is unchanged from the previous reviewed integration. Empty exact window
normalization still carries the configured pane-index callback.

The 25-step probe from attempt-05 passes with zero topology, geometry, format,
output or warning divergences. In particular, `cli:=.1` under `pane-base-index 1`
succeeds for lock-session, has-session and list-windows. The same probe checks
session/window overrides, server scope, base-index independence, missing indices,
empty pane components and absolute pane IDs.

The shared fixture repetitions and focused fixed/reverted checks are collected
with the engine proof in TUI-017/attempt-09. Its final summary records the six
empty-exact-window assertions, owner tallies and claim verifier results. TUI-015
remains at review; the lane does not grant verification.
