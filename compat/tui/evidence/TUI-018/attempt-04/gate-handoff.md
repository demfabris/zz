# TUI-018 protocol-correction handoff

This is the worker assessment, not an independent approval. Status remains review.
Source revision: 753c8497781e54dd6b4717e0e0e1243d2857737f, on origin/main
be5709dfcd9936d44519683e5da8dc6ab7748d96 (v0.10.0).

The orchestrator corrected the stale wire instruction. Protocol 104 and its decimal
and byte pins now pass the full protocol suite. The lane adds no serialized payload
field: stdin_spent remains serde-skipped. The v103 append history is byte-identical
to the release; the new v104 paragraph explains the version advance. The guard is
unchanged. wire-version.py and the full compat/check.sh both pass.

The 104 binary repeats 57 asserted stream comparisons, zero recorded and four cap
decisions. Its self-check catches 29 sabotages and passes both equivalences. The
shared client fixture repeats three CLI-status failures out of 84 assertions and
25 attributed records, with unattributed=0 and no TUI-018 records. Its self-check
passes. The full attached-client fixture passes. Clippy with all targets/features
and -D warnings, formatting and the focused caller-stream CLI tests pass.

The verifier exits 1 after confirming 57/0/4 and repeating the shared fixture's
three-of-84 failure summary. Its result is in verify-claims.txt and commands.txt.
The prior full package run and complete 222-row corpus remain in attempt-03. Neither
is represented as a protocol-104 rerun. See corpus-provenance.json for the exact 222
rows not repeated at 104. The version correction leaves the three measured alias
fixes intact. The shared CLI-status failures still prevent a clean obligation gate;
the previous corpus failures and two GUI test failures remain unresolved.

The remote main fetched in this attempt still contains the original message.rs-only
wire guard, despite the correction's wider-guard description. The payload-source
inventory confirms that message.rs is the only changed protocol payload source in
this lane, so the fetched guard covers its changes. No guard edit was made.
