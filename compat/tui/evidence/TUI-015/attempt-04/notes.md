# Configured pane indices in session targets

The three pane-base-index records become assertions: lock-session, has-session
and list-windows accept `=cli:win.1` with pane-base-index 1. TUI-015 remains at
review with zero ordinary owned records. Its four OS-locking decision records
remain `decided:TUI-015`.

`MuxEngine::resolve_session` carries the existing configured pane-index lookup
into the session-only resolver. The compound parser remains gated by
`TargetSlot::Session`; internal window and pane fallbacks retain name-only
lookup. Absolute pane IDs remain independent of pane-base-index. The new mux
test checks overrides, unset inheritance, absolute IDs and the independent
window base index. The 21-step `pane-index-scopes.txt` probe matches the pin in
all five channels, including the pin's handling of explicit option scopes.

The shared fixture asserts all three cases and sabotages each by changing only
zz's pane-base-index to 2 after establishing equivalence. All three focused
self-checks pass on the fixed binary and fail with exit 1 on a separate build
whose engine resolver again uses the unconfigured state lookup.

Shared fixture, reverted-fix, attached, copy-mode, screen, delta, build and test
artifacts are in `../../TUI-017/attempt-07/`; that attempt's `notes.md` describes
the complete implementation, failures, source identity and limits. The complete
footprint includes the native terminal provenance work as well as the resolver.
This is a gate handoff, not verification.
