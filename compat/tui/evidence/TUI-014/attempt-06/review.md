# Modes lane self-verification, 2026-09-16

This is the implementing lane's audit of the rejected review, not independent
reviewer approval. The independent gate still needs to review the pushed tip.

1. Clock input and cancellation: own probe asserts injected `x`, BTab, KPEnter,
   underlying shell contents, `copy-mode -q`, and the final `0/` mode state.
2. Stack restoration: own probe asserts `2/switch-mode`, Escape to
   `1/clock-mode`, restored clock cells, then terminal contents after a key.
3. Switch arguments: own probe asserts custom row format and Enter's command
   template. `-k` and `-Z` are refused and recorded as unbuilt lifecycle variants,
   as the review permits; no claim covers their parity.
4. Identity expansion: the conditional nobody/root identity returns empty stdout
   and stderr with exit zero on both servers.
5. Tied windows: the plain format asserts alpha, cli, zulu order. Default styled
   window rows retain their separately attributed tail-style record.
6. Footprint: `footprint.txt` is the complete three-dot list against the fetched
   main, including production constructors, test helpers and generated reports.
7. Comments: `wire-and-source.txt` records zero added Rust comment/doc lines.
8. noattr: the exact unit test passes normally and fails when `base_cell` is
   removed. `noattr-mutation.txt` retains the expected failure and restoration.

`defect-probe-final.txt` contains 25 asserted comparisons, zero failures and two
explicit records. The self-check now catches independent format, template,
ordering and identity sabotages, alongside the key-leak, cancellation and stack
sabotages. Both seconds faces have assertions and colour sabotages.

TUI-014 remains active. `customize-mode-open` and `suspend-client` stay owned by
TUI-014 for the sibling customize lane. The interactive-refresh and socket-ACL
records remain open. Read `notes.md` and the command outputs for exact check
results and residuals; passing mode probes do not certify the entire corpus.
