# Capture fix pass, cycle 12

This pass starts from 82746ea3 and rebases its fourteen commits onto origin/main
be5709df (zz 0.10.0). The rebase combines the capture flag count with main's twenty
usage overrides and regenerates both conflicted reports. The branch is
campaign/tui-capture-12. The gate owns verification; this is worker evidence.

## Changes and limits

Styled live capture now tracks text extent separately from retained background
cells. `-J` and `-T` use text extent; ordinary styled capture still reads retained
backgrounds. A SpacerHead contributes default-coloured padding when the capture
keeps allocated cells, and joined capture skips it. SpacerTail remains skipped.
Tests exercise both 80 and 100 columns. The attached probe uses two clients at
80x24 and 100x30, plus a resize, EL, ED, clear, and scroll-region workloads.

The fixture adds four erased-background assertions and three wide-wrap assertions.
Each new scene runs an equivalence and a one-sided text sabotage in `--self-check`.
The existing `lock-client` record changes to a five-channel assertion with a
one-sided status-row sabotage. Other OS-lock decisions remain registered.

Tabs remain an ordinary TUI-017 residual, now named `capture-tab-trailing`,
`capture-tab-internal`, and `capture-tab-wide`. The pin retains a TAB cell and
prints a literal tab; Ghostty stores cursor motion and subsequent cell contents.
This is the same engine-storage root as the campaign's TAB-cell residual. The
DEC charset decision does not waive it. Indexed colour 1 stays recorded for the
sibling lane. This pass does not claim styled frozen-mode capture parity.

## Measurement qualifications

The initial build overlapped the first local edits. Its target resolver already
included the fix, but its capture code still exhibited the rejected behavior.
Do not treat 02-rebased-baseline.txt as a pristine build of f0b70b7c. The initial
binary hash and this qualification are in environment-initial.txt.

The detached probes 04, 06 and 08 did not establish actual 100-column terminal
storage: the model reported a resized window while the terminal could still
retain an 80-column grid. They remain as failed setup attempts. Probe 11 uses
two attached clients and reproduces the wide-wrap defect at 100 columns; later
runs of probe.py use that same attached setup. The probe prints observations and
DIFF lines; exit 0 means the probe completed, not that all comparisons matched.
Its extra default `-e`/`-N` erase measurements at 100 columns remain observations,
not a claim that retained allocation/erase history matches the pin.

Main's command-parse exit policy returns 2 where the pin returns 1 in eight
pre-existing assertions: refresh-missing-argument, lock-server-arity,
lock-server-unknown-flag, lock-session-arity, lock-client-arity,
lock-client-missing-argument, client-tree-unknown-flag, client-tree-usage.
The policy and its tests are unchanged from origin/main (d66501cc,
crates/zz-protocol/src/message.rs ServerError::exit_code and
crates/zz/src/lib.rs cli_exit_contract_preserves_explicit_status_and_classifies_errors).
These assertions remain assertions. No record or decision hides these failures.

The first scrubbed-HOME Cargo attempt (07) started downloading a separate toolchain
and dependency cache and was interrupted (130). Later commands preserve
CARGO_HOME and RUSTUP_HOME while scrubbing application HOME/XDG_CONFIG_HOME.
The wrapper appends --jobs after all arguments, so a temporary cargo adapter moves
that pair before `--`, or removes it for cargo fmt. All Cargo invocations, including
those within compat/check.sh, enter /tmp/zz-cargo.sh first and keep its shared lock
and memory cap. runs.jsonl records command exits; logs retain failed runs.

The first full daemon run passed 935 tests and failed two under parallel load.
Both passed alone using the same freshly built test executable: positive_delay_shell_job_retains_destroyed_target_and_keeps_missing_target_sessionless
and switch_client_key_table_and_formats_are_client_local. Direct integration-test
runs complete the targets that Cargo did not reach after the library failure.

The inherited catalog.rs change adds CLI flags and changes test inventory counts;
it does not change any serialized payload. PROTOCOL_VERSION stays 103.
The checked-out wire guard passes. Contrary to the batch's description, this base's
guard still inspects message.rs alone; the complete source diff was also inspected.

## Roster text moved out of the shell header

The following twenty-six lines came from the rejected branch's added shell header.
They describe that historical roster; the measurements above supersede its claims.

```text
                                                                                            TUI-015
lock-server/-session/     too many arguments, unknown       same                           PROVED
  -client arity and -t     flag, -t without an argument
lock-session -t with a    validates session, window and    same at the default pane       PROVED, with configured
  window or pane suffix     pane components                   base index                     pane-base-index recorded
                                                                                             under TUI-015
after-lock-session        neither is a hook name: the pin   same                           PROVED
after-lock-client           answers `invalid option`
lock-after-time           arms a per-client server timer    store-only at both scopes,     DECLARED, TUI-015
                                                              and an invalid value is
                                                              refused the pin's way
lock-command              spawned on the client tty         store-only at both scopes;     DECLARED, the pin's
                                                              the pin's own default is a     default is whatever
                                                              build-time choice              configure found
capture-pane -C           backslashes doubled, and with     same for retained cell facts   PROVED; DEC charset and
                            -e escaped style controls                                       low indexed colour
                                                                                             provenance recorded
capture-pane -L           each line numbered from the       same                           PROVED
                            history size, negative in
                            history, and with -J the
                            number of every joined row
                            inside the joined line
capture-pane -F -H -P -R  line flags, the line OSC 8        loudly unsupported             DECLARED, TUI-017
                            URIs, the pending input                                         capture.rich-transports,
                            buffer and the whole internal                                   each with the workload
                            grid                                                            its refusal names
```
