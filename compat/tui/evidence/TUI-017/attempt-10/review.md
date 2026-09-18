# Pending gate review

The lane leaves TUI-015 and TUI-017 at review. TUI-015 is untouched by this
revision: its resolver and its three configured `pane-base-index` cases are the
ones the previous review accepted. TUI-017 carries fabrico's 2026-09-18 tab
ruling, the pane-size fix behind the eight 100x30 erased-background cases and
the patch-hash keyed Ghostty build.

The gate should inspect the benchmark table and its instruction attribution
(the retained ICH hunk is the only workload that still costs anything), the
seventeen tab cases now filed under `decided:TUI-017`, the nine edited-tab
cases that stay asserted, the eight new 100x30 assertions and their reverted
probe, and the corpus rows that fail on the main control as well. No
verification decision is made here.
