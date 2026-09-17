# Empty exact windows with configured pane indices

The lane rebases onto main `38c50df4`, including review fix `cf8e0b8b` and
its six asserted empty-exact-window cases. The resolver keeps main's removal
of the leading `=` from an empty exact window component, then recurses through
`resolve_named_session_with_pane_index` with the configured lookup callback.
Compound parsing remains gated by `TargetSlot::Session`.

The extended `session_targets_use_configured_pane_indices` unit test covers
empty exact windows at global and window indices, explicit session names,
exact session names, current-session targets, absolute pane IDs, empty pane
components and missing indices. It passes alone and in the full mux package.

The 25-step `empty-exact-pane-index.txt` probe passes all five differential
channels. With `pane-base-index 1`, `lock-session`, `has-session` and
`list-windows` accept `cli:=.1` on both binaries. The `.0` and `.9` errors
match. The probe also measures session and window overrides, unsetting the
window override, server scope and the separate `base-index` option.

The first probe renamed the harness's `w` session. Commands matched, but all
26 topology and geometry queries failed because the harness still queried
`w`. The initial script, output and transcript remain here. The corrected
probe creates a separate `cli` session and leaves `w` available for queries.

The combined evidence and review request live in TUI-017/attempt-08. Both
obligations remain at review; the gate owns verification.
