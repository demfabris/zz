TUI-014 remains at review. This file requests gate review; it is not an independent approval of this tip.

Review bd0adfb79fe19110b43e787401071ce0f9a26110 after the rebase checkpoint bd0f76d7. The branch contains the accepted mode stack and customization work, ported onto origin/main eef2df941183af8ab7b16bf648dfadcc4486d22e and the headless zz-cli split. Read the complete three-dot footprint, including inherited protocol, client, TUI, desktop and evidence changes.

The new behavior accepts switch-mode -k and -Z. Killing follows the switch entry through a clock above it, and targets the pane displaying the mode. Zoom restoration follows the entry's original zoom state, including one-pane entry and a split introduced later. Reopening the top entry does nothing; promoting a suspended entry updates its kill flag while retaining its original zoom lifetime. A mode-triggered death does not synthesize an after-kill-pane command hook.

The shared fixture flips switch-mode-kill, switch-mode-kill-exit and switch-mode-zoom, adds 20 assertions, and catches omitted flags with one-sided sabotages. The pre-fix binary fails the new self-check controls. The roster is 170 asserted and 25 recorded, with TUI-014 owning zero records, clients.interactive-refresh dropping from 16 to 13, and unattributed=0.

Zero ownership does not imply complete TUI-014 parity. Five mode records remain owned by clients.interactive-refresh: clock-over-copy, clock-over-copy-key, copy-over-clock, switch-mode-windows and switch-mode-duplicate-windows. Movement, filtering and mouse behavior in the switcher also remain outside this implementation. Broader customization editing and transport limits remain as documented by the inherited lane.

The pin crashes on fresh -Z entry in an unzoomed split window. The retained differential enters in one pane, then splits and zooms while the mode is open; a zz unit test separately covers fresh split-pane entry. No successful pin comparison is claimed for the crashing path.

Review the retained failures as well as the passing runs. The first three candidate attached-client runs fail the alert freeze checkpoint, while the saved checkpoint binary passes; the final unchanged-fixture candidate run passes. Timestamped probes reach the candidate's final capture beyond the five-second alert budget. A cleanup-lock experiment did not resolve it and was reverted. The full package run also contains CLI and daemon test failures, with isolated reruns recorded separately. The control return-code matrix and agent slow-client soak remain failing, and a later serial CLI run requires termination of a stalled wait-exit child. The first verifier fails a seconds-clock capture; the final verifier passes. No clean all-regression result or verified status is claimed.

All wire appends remain in the single unreleased v104 entry after shipped v103. The new lifetime flags add no wire fields. The final evidence-only commit does not change the runtime tested at bd0adfb7.
