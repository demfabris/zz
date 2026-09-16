# Gate review pending

The customize lane has not marked TUI-014 verified. The gate must rebuild and
repeat both newly asserted cases and their self-check sabotages. Inspect the
foreground-job supervisor as part of the fixture change: it is identical on both
sides and supplies the parent process needed for terminal job control.

Inspect the per-pane tree ownership, its reuse of the chooser grid, and the
ClientSuspendState tail append in the existing v104 entry. The default tree is
compared without a zz-row mask; the sibling extension is an explicit reveal path.

The opening and teardown cases do not prove every customize interaction. The
unimplemented and unmeasured interactions are listed in notes.md and the design.
Retained failures from broader tests must remain visible even after an isolated
rerun passes. This file records review scope, not an independent review verdict.
