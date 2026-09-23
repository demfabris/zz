---
type: Research
title: macOS CPU, GPU, and memory investigation
description: Measured macOS resource costs, proven improvements, and deferred protocol and renderer designs from the September 2026 performance investigation.
resource: scripts/profile-macos.sh
tags:
- performance
- macos
- memory
- cpu
- gpu
- protocol
timestamp: 2026-09-23T03:14:56Z
---

# Investigation record

Started 2026-09-23 UTC from `513922a9b8e26f511c9820db0634bef93d9b75a2` on
`main`, with a clean working tree. Host: Apple M4 Max (`Mac16,5`), 16 CPU
cores, 48 GiB memory, macOS 27.0 (`26A428`), Rust 1.97.0. Results below distinguish isolated mechanism experiments from measurements
of the final application bundles.

The requested acceptance rule is lower CPU, GPU work, or memory without an
unresolved behavior or latency drawback. Candidate changes with tradeoffs stay
in the review queue. A lower RSS value alone does not prove lower memory
pressure. Record physical footprint as well, identify GPU resources, and keep
startup and steady-state costs separate.

Raw local evidence lives under `/tmp/zz-perf-20260923/`; reproducible traces
use the repository's `just profile-*` recipes. These paths may contain process
arguments or terminal content and are not publication artifacts.
Selected evidence is also retained in the ignored
`target/performance/2026-09-23/` directory; start with its `README.md`.
It includes source fixtures, hashes, review outputs, and parked patches.

## Applied changes

| Change | Proven effect | Scope |
| --- | --- | --- |
| Select option names before constructing metadata | Removes work for nonmatching names | No cache or protocol state added. |
| Reuse one borrowed format universe per client update | Removes repeated full-model construction for labels and borders | Client-specific formats and fresh state preserved. |
| Read stored uid/user directly | Avoids a full status context for two strings | Same identity values. |
| Remove unused C ABI Zig signal-stack TLS | About 71 MiB less footprint at 64 dormant panes in isolated matched runs | One native fork line; C/Rust signal behavior checked. |
| Copy only retained text/diff prefixes | Structured diff capacity 48 → 12 MiB in the realistic transcript fixture | Raw JSON remains unchanged. |
| Borrow ordinary Markdown source for display | Removes a duplicate buffer; 31.25 → 15.625 MiB capacity in the fixture | Truncated previews still own separate text. |
| Check appearance Arc identity before deep equality | About 150 ns → 1 ns for identical shared settings | Small notification-path saving; no whole-GUI CPU claim. |
| Release engine lock before inspecting agent runtime | Removes a reproduced production lock cycle | Temporary request-local row allocation measured below. |
| Repair profiling launch and sparse Metal summaries | Correct app invocation and full-duration rate denominators | Improves measurement validity. |

The final daemon workload uses 82–97% less CPU per rename in subscribed layouts.
At 64 windows and one subscriber, idle RSS falls from 270.69 to 207.06 MiB and
footprint from 259.89 to 196.44 MiB. The separate TLS experiment isolates that
memory mechanism; the final matrix includes all implemented changes.

The final desktop comparison shows a smaller one-pane memory gain: GUI RSS
134.67 → 131.84 MiB, daemon RSS 31.34 → 26.59 MiB. Idle GUI CPU and GPU work
are unchanged within the measured variation. Sustained output shows no
repeatable regression after four alternating pairs.

The main GPU follow-up is safe atlas retirement: visible image-pane churn
retains about 76.6 MiB, but the first cleanup prototype exposes an existing
in-flight overwrite race. That patch stays parked. A standalone Metal control
also reproduces 224 MiB of driver backing on this Mac; application allocation
counts alone cannot justify calling it a zz leak.

## Measurement setup

- Build matching optimized binaries and symbols with `just profile-build mac`.
- Run each GUI against a fresh private daemon socket and the same isolated
  config: update checks disabled and `/bin/sh` as the default shell.
- Compare idle terminal, active terminal output, multiple panes/windows, and a
  local browser fixture. Record visibility, size, warmup, and process identity.
- Run clean timing separately from trace logging, `sample`, and `vmmap`.
- Final runs use AC power with no recorded thermal/performance warning. The host
  already had about 1.3 GiB of system swap in use; it was not reset for testing.
- Use repeated alternating baseline/candidate runs before claiming a gain.
- Check correctness and resource teardown as well as throughput.

The initial profiling launcher omitted the `app` command added to zz's direct
launch contract. `scripts/profile-macos.sh` now passes it for ordinary and
Instruments launch captures, and matches the same arguments during cleanup.
Shell syntax and the existing startup-summary tests pass. The Metal summary
now divides frame rates and GPU activity by the full capture duration; its
previous activity-span denominator overstated sparse idle work. Its reported
GPU percentage is the union of this process's GPU intervals divided by capture
time, not shader occupancy or total device utilization. A regression
checks that one millisecond of GPU work in 20 seconds reports 0.005%, not 100%.

### Excluded runs and observation controls

The final GUI harness checks a single active, unhidden native window at
3,008 × 1,662 points and scale 2 before and after each clean sample. It records
PID/start identities, pane readiness, delivered output counters, browser RGB
readiness, and child-process teardown. `sample` and `vmmap` start only after
the clean interval; samples containing the watchdog's diagnostic process are
rejected. Chromium profiles are unique per final run and removed after their
helpers exit.

The first idle attempt was rejected because the pointer opened a hover tooltip
and created a second window. An immediate retry was refused while a pointer
preflight still ran; no measurement started. Both are preserved under
`discarded-*`. The accepted idle matrix began after that preflight ended.

A later read-only computer-use app selection raced the end of the browser
capture and opened the saved app again, without the private profiling arguments.
It briefly created a new default daemon and an empty shell. There were no
attached clients when checked during cleanup. The newly tracked GUI, daemon,
and shell were stopped; the dead socket was removed after verifying its
recorded identity and refused connection. Both overlapping 16-window runs
were excluded and repeated after teardown. The original observations and
cleanup record are preserved under `discarded-cua-overlap/` and
`accidental-cua-launch/`. No further app selection was used during captures.

## Source findings

| Area | Evidence | Experiment / decision |
| --- | --- | --- |
| Terminal wire | Existing per-view row patches, latest-frame mailboxes, exact sizing, and bounded buffer recycling | Preserve current convergence and backpressure guarantees; a blanket rewrite would duplicate work already done. |
| Mux metadata | `publish_mux_snapshots` rebuilt the format universe for each window label while holding the daemon lock | Share one immutable, lazy universe within each client's update; preserve client-specific stamping. |
| Status timer | Status refresh constructs a full snapshot and format facts before selecting recipients | Measure zero-recipient and multi-session cases; preserve interval and per-client semantics. |
| Chromium bootstrap | macOS loads the CEF framework before first browser use | Standalone loader experiment shows a cost; the lazy-loading variant still needs GUI A/B and first-browser latency measurements. |
| Metal path target | Pinned GPUI allocates a full-window private path texture before knowing whether the scene uses paths | Controls show unused allocated texture storage is unresident at idle; the lazy-allocation proposal stays parked. |
| Display link | Visible macOS windows keep `CVDisplayLink` callbacks active while GPUI skips clean frames | Attribute idle CPU separately from GPU presentation; demand scheduling needs animation, input, and wakeup correctness. |

## Independent allocation experiments

These isolate one mechanism; they are not whole-app idle-memory results.

| Experiment | Before | Candidate | Evidence |
| --- | --- | --- | --- |
| 32 tool outputs, each 4 MiB, truncated to 512 KiB, actual mimalloc build | 128 MiB live String capacity; 130.11 MiB footprint | 16 MiB live capacity; 25.74 MiB footprint | Five repetitions in `memory/payload-mimalloc-capacity.txt`; unconditional shrinking rejected after replacement-throughput test; see below. |
| Cold process loading the bundled Chromium framework | About 1.3 MiB footprint | About 16.8 MiB footprint after load | Five `dlopen` repetitions in `memory/cef-load-repeated.txt`; loading costs 19–24 ms warm-cache CPU and adds about 57.6 MiB RSS. |

A repeated-replacement benchmark found 11.6% more cap-helper elapsed time for
unconditional shrinking (about 6 microseconds per oversized update). The
construction fixture above favors reduced page pressure and hides that cost.
The shrinking candidate stays parked. The implemented borrowed-input change
copies only the retained prefix for text and diff inputs, preserving the owned
JSON path. A generic Cow
version added short-input overhead, so the selected experiment uses a private
bounded-copy helper and preserves the existing owned-string API.

The scratch CPU sampler cross-checks `proc_pid_rusage` against
`time.process_time_ns()`. This host returns CPU counters in Mach absolute-time
ticks, with a 125/3 ns conversion from `mach_timebase_info`. Treating those
counters as nanoseconds would understate CPU by about 41.7 times. The converted
self-check agrees within 0.1%; evidence: `cpu-clock-check.json`.

The selected borrowed text/diff helper avoids copying discarded bytes in the
first place. A realistic fixture deserializes and applies eight Claude Write/Edit
updates with 4 MiB source strings through the actual transcript reducer and
mimalloc. Retained structured diff capacity falls from 48 to 12 MiB. Raw input
JSON remains 96 MiB, so total payload capacity falls from 144 to 108 MiB.
Visible serialized transcript hashes match. This is a reducer fixture, not an
idle GUI measurement; evidence: `memory/transcript-realistic-results.txt`.

Ordinary `AgentMarkdown` entries previously retained identical source and preview
strings. The implementation borrows the source for display until truncation requires
a separate preview. A 4,096-entry fixture with 4,000-byte entries reduces this
buffer's string capacity from 31.25 to 15.625 MiB, and process footprint from
34.27 to 18.22 MiB. Rendering state retains another copy, so this does not mean
the entire application owns only one copy. Adversarial review caught an append
boundary that compared against already-mutated source and missed a display
revision. The corrected implementation decides whether the incoming text needs
a preview before changing source; its counterexample now preserves the original
display and replacement revisions.

## GUI baseline and attribution

The preserved baseline bundle is `baseline/bundle/zz.app`. Its main executable
SHA-256 is `116058de5cc35c343ae12b34ec5dfa19f9a8331fd1662b37f690a505097c595c`;
the bundled CLI is
`89db0e9691d20500a9d9c9ba209758bf8e1826e0d9b1b4ba52f0842ec9a27098`.
Private config and daemon socket isolate these sessions from the running user
daemon. Browser fixtures use a named test profile and localhost content.

The monitor is an Odyssey G80SD. The app's initial window is 3,008 × 1,662
points at scale 2, giving a 6,016 × 3,324 drawable. Full `vmmap` identifies
three CAMetalLayer display drawables of 77.4 MiB each, about 232.1 MiB total.
This is real backing storage, not an accidental extra scale multiplication.

An initial terminal-only idle run measured approximately 135 MiB GUI RSS and
557 MiB physical footprint; the daemon was 30 MiB RSS / 18 MiB footprint.
The GUI used about 1% of one CPU core and 143 interrupt wakeups/s. A separate
20-second Metal capture recorded 40 presentations, 1.602 ms total encoder CPU,
and 209.582 ms of GPU interval union, about 1.048% of capture time. The two
presentations per second match cursor blink. The wakeup count is consistent
with a 120 Hz display link plus watchdog/config timers; that decomposition is
a source-backed hypothesis, not a per-timer trace attribution.

These initial timing runs overlap compilation and are provisional until quiet
matched comparisons. Physical footprint also varies when the system reclaims
purgeable graphics pages, so report allocation categories and min/max alongside
it. Do not identify every `IOAccelerator` region as GPU storage: mimalloc's
anonymous memory tag can receive that label. Explicit graphics regions and
named IOSurfaces provide stronger attribution.

An isolated headless GPUI Metal experiment reproduces much of the remaining
graphics category. At 6,016 × 3,324, its first empty render produces about
311 MiB of owned-unmapped graphics, including a 77.4 MiB output texture and
28 resident 8 MiB blocks. Before rendering, an unused path target reports
77.4 MiB through Metal's allocation counter without increasing physical
footprint. Consequently, the lazy-path experiment's reported allocation saving
is not proof of equal RSS or footprint savings.

A pure Metal control resolves the larger cost. Device plus queue uses 18.6 MiB
footprint; an empty command commit uses 18.9 MiB. One 64 × 64 clear pass,
without GPUI, application shaders, an atlas, or CEF, raises footprint to
249.8 MiB and owned-unmapped graphics to 224.8 MiB, including the same 28
8 MiB blocks. Metal reports only 983,040 bytes of application allocations.
Dropping textures and queue returns footprint to 20.0 MiB and removes that
graphics category. This is driver-managed render/queue backing on this host
and macOS version, not a zz object leak. Evidence: `render/minimal-metal-*`.

A second control written directly in Objective-C, with no Rust, GPUI, CEF, or
mimalloc, independently reproduces the result: 4.75 MiB before rendering,
236.13 MiB after a 64 × 64 clear, 339.86 MiB after a full-window clear, and
6.22 MiB after releasing the target and queue. The baseline GUI's owned,
unmapped graphics category is 243.906 MiB: 224 MiB in those driver blocks plus
19.906 MiB of render working storage. This is distinct from its 232.1 MiB of
display drawables and 12.6 MiB of mapped graphics resources. The countercheck
makes the attribution reproducible outside zz; it does not imply that every
Metal application or macOS version has the same overhead. See
`render/idle-metal-attribution.md` and `render/minimal-metal-footprint.m`.

A follow-up tests Apple's newer Metal 4 API and ordinary Metal 3 queue limits of
one and three command buffers. All four cases retain the same 28 × 8 MiB driver
blocks, about 236 MiB footprint after a 64 × 64 clear and 340 MiB at full size.
Every output pixel and separate API-validation runs pass. This allocation test
gives no reason to migrate GPUI or reduce queue capacity. Real-scene Metal 4 CPU
benefits remain unmeasured; migration would also require explicit residency,
barriers, resource ownership until completion, and compatibility with older
systems. Evidence: `render/metal4-comparison.md` and its standalone Objective-C
reproducer. [Apple's API overview](https://developer.apple.com/documentation/Metal/understanding-the-metal-4-core-api).

The visible browser fixture added approximately 304 MiB steady helper-process
footprint: GPU 190.2, network 28.6, storage 20.8, and two renderers 26.3 / 38.2
MiB. Chromium 152 keeps a spare renderer under its site-isolation policy;
source supports that explanation for the second renderer, but command lines
alone do not identify the spare. Disabling it trades navigation startup time
for memory. GUI-only CPU and Metal summaries exclude those helpers. The local
terminal output fixture requests about 1,000 lines/s and initially measured
12.6% GUI / 6.8% daemon CPU, again provisional under compilation load.

Later `vmmap` / `sample` probes can trigger zz's own stall diagnostic sampler.
The browser run's 619 MiB helper RSS peak includes a roughly 128 MiB diagnostic
`/usr/bin/sample` child, and must not be called Chromium memory. The clean
20-second CPU interval finishes before those probes. Retain process identities
when interpreting aggregate memory maxima.

## Final desktop comparison

The accepted matrix has fourteen runs: three alternating idle pairs, two
opposite-order output pairs, one browser pair with fresh profiles, and one
16-window pair. Each clean sample lasts 30 seconds after readiness and a
10-second warmup. All windows contain one terminal pane; in the many-window
case each terminal is 341 × 107 cells. This is a larger grid than the headless
daemon fixture, so those absolute memory figures should not be mixed.
The browser is a visible lower split displaying the retained localhost fixture.

Values below are medians across runs; memory first takes each run's median.
CPU percentages are fractions of one core. Browser and many-window results
have only one observation per variant. The output comparison receives the
longer follow-up described below.

| Scenario | Process | CPU, baseline → final | RSS, baseline → final | Footprint, baseline → final |
| --- | --- | ---: | ---: | ---: |
| Idle terminal | gui | 1.045 → 1.054% | 134.67 → 131.84 MiB | 556.08 → 552.66 MiB |
| Idle terminal | daemon | 0.197 → 0.073% | 31.34 → 26.59 MiB | 18.02 → 15.17 MiB |
| Continuous output | gui | 15.132 → 15.301% | 155.91 → 153.27 MiB | 578.04 → 569.60 MiB |
| Continuous output | daemon | 9.370 → 9.421% | 34.16 → 28.77 MiB | 19.45 → 15.52 MiB |
| Browser + terminal | gui | 2.058 → 1.977% | 328.00 → 324.95 MiB | 378.33 → 376.69 MiB |
| Browser + terminal | daemon | 0.156 → 0.071% | 34.09 → 28.69 MiB | 21.50 → 16.02 MiB |
| 16 terminal windows | gui | 1.340 → 1.418% | 139.78 → 137.22 MiB | 564.83 → 561.05 MiB |
| 16 terminal windows | daemon | 0.429 → 0.091% | 126.39 → 103.84 MiB | 113.02 → 91.36 MiB |

Idle GUI CPU spans 1.034–1.075% in baseline and 1.006–1.076% in final. There is
no demonstrated GUI idle CPU improvement. Wakeups remain about 140/s. Daemon
idle CPU spans 0.190–0.197% versus 0.064–0.075%; its wakeups remain about 11/s.
The change reduces work done on wakeup, rather than eliminating those timers.

Per-run idle GUI RSS medians span 134.38–135.38 MiB in baseline and
128.20–132.58 MiB in final. Corresponding footprint medians span
554.55–557.53 versus 552.39–553.58 MiB. Short within-run graphics peaks remain;
`final-gui-summary.json` retains both per-run and individual-sample ranges.
The one-pane desktop gain is modest beside the many-pane daemon saving.

Browser helper footprint is 302.55 → 304.13 MiB; their summed RSS is
489.03 → 489.83 MiB. Helper CPU was not measured in this matrix. Summed RSS
can count shared pages more than once, so it is not unique physical memory.
These figures cover the clean interval and exclude diagnostic subprocesses.
GUI-plus-helper footprint is about 681 MiB in both variants; there is no
established Chromium memory improvement.

The browser GUI also has 224 MiB less resident and virtual owned-unmapped
graphics storage than terminal-only idle, with 28 fewer regions. This repeats
in the earlier browser capture. The runs use different process lifetimes;
they do not prove that those allocations migrate into Chromium helpers.
The correct description is driver-managed backing observed in terminal-only
idle and minimal clear controls, not fixed application overhead. The standalone
Objective-C and Rust controls drain their per-command autorelease pools after
GPU completion and before measuring, so ordinary retained command autoreleases
do not explain those reproductions.

A follow-up holds exactly the same completed-clear device, queue, and texture
alive without submitting more GPU work. Footprint falls from 235.97 MiB at
zero seconds to 6.36 MiB at 20 seconds and stays there at 60 seconds. All 28
8 MiB regions disappear; residual owned graphics is 784 KiB, and the target
still reports 32,768 allocated bytes. Explicit queue/target release lowers
footprint to 5.55 MiB. This establishes temporary driver retention that can
expire without releasing those objects. It does not identify the retirement
policy or prove that browser helpers receive the same storage. A same-process
terminal → browser → terminal allocation trace remains the test for that
transition's cause. Evidence: `render/metal-aging.md` and
`render/metal-aging-results/summary.json`.

Every accepted run passed geometry, readiness, aligned sample coverage, and
normal child-process teardown checks. Both browser profiles were removed after
helpers exited. Evidence: `final-gui-results.json`, `final-gui-summary.json`,
`final-gui-*/`, and `render/output-comparison-audit.md`.

### Cursor visibility and temporary driver retention

A final diagnostic holds the same GUI process, foreground window, and terminal
geometry through visible → hidden → visible cursor phases. After about thirty
seconds hidden, owned-unmapped graphics falls from 243.9 to 19.9 MiB, with
28 fewer regions. Showing the cursor restores 243.9 MiB and all 28 regions.
Display-drawable residency stays at 232.1 MiB throughout. Pre-probe GUI
footprint is about 556 → 309 → 556 MiB; RSS barely changes in the hidden phase.
This establishes the memory reversal in one process and supports the connection
between cursor-driven rendering and retained driver backing.

The source already handles hidden cursors correctly: `cursor_should_blink`
requires cursor visibility, and the timer exits without another notification
when that predicate is false. Showing the cursor restarts it through the normal
snapshot/render path. No extra hidden-cursor redraw fix was needed.

**Submission counts from this diagnostic are unavailable.** Instruments reached
its 110-second recording limit, but the scratch driver's post-capture wait was
too short for finalization. Its fallback terminated the owned recorder group,
and the saved trace fails export with a missing-template error. The independent
memory counters and complete maps above remain valid; the earlier two clean
20-second final Metal traces are unaffected. The fixture restored the cursor,
and every recorded application, daemon, fixture, and recorder identity exited.

The existing `cursor-style-blink = false` setting selects a steady visible
cursor. It may allow similar driver retirement, but that setting was not
measured here and changes visible behavior. It remains an optional user-facing
tradeoff for review; no preference or production rendering policy was changed.
A future reproduction should allow longer trace finalization and retain valid
per-phase command counts before claiming zero GPU work while hidden. Evidence:
`render/cursor-retention.md` and `render/cursor-retention-results/summary.json`.

### Longer output follow-up

The first 30-second pair suggested about 3% more CPU per delivered line. The
reversed pair changed direction. Rather than dismissing that difference, two
additional opposite-order pairs used 20-second warmup and 60-second clean
samples. All four pairs are retained here. The fixture sleeps for a requested
millisecond per line; actual delivery is about 670–675 lines/s.

| Clean duration and run order | Delivered lines/s, baseline → final | GUI CPU / line, baseline → final | Daemon CPU / line, baseline → final |
| --- | ---: | ---: | ---: |
| 30 s, baseline/final | 674.03 → 671.56 | 221.96 → 229.20 µs | 136.17 → 141.12 µs |
| 30 s, final/baseline | 670.78 → 671.20 | 228.13 → 226.60 µs | 142.54 → 139.52 µs |
| 60 s, baseline/final | 669.98 → 672.75 | 228.30 → 219.74 µs | 141.91 → 136.91 µs |
| 60 s, final/baseline | 674.09 → 675.25 | 223.53 → 224.25 µs | 140.35 → 137.66 µs |

Across the four runs per variant, median GUI cost is 225.83 → 225.42 µs per
delivered line, with overlapping ranges of 221.96–228.30 and 219.74–229.20 µs.
Daemon medians are 141.13 → 138.59 µs, also with overlapping ranges. These
measurements establish no repeatable output regression and do not justify a
claim of faster sustained rendering or exact performance equivalence.
Geometry, Python executable, and delivered counter format match. No page-ins
or diagnostic samplers occur during the clean intervals. Frame/notification
counts were not recorded, so there is no proved rendering explanation for
the smaller differences. Evidence: `final-output-confirmation-summary.json`,
`final-output-confirmation-*/`, and `render/output-comparison-audit.md`.

### Final idle Metal capture

One quiet 20-second capture per saved bundle uses the same active window and
private daemon, after ten seconds of warmup. Both traces record 39 command
buffers and 39 presentations, or 1.95/s. The union of GPU intervals is
203.627 ms in baseline and 203.630 ms in final: 1.018% of capture time in both.
Encoder CPU totals are 3.038 and 2.938 ms respectively.

These results show no idle GPU improvement from the accepted changes. Cursor
blink still produces approximately two presentations per second. The capture
percentage describes elapsed GPU intervals attributed to the GUI process;
it is not total device utilization. Window geometry and process identities
remain valid, and both process groups exit normally. Evidence:
`final-metal-idle-1-control/summary.json` and the corresponding saved traces.

## Option lookup: direct and live baseline proof

`exact_tmux_option` previously constructed descriptors for nonmatching catalog
names before testing their names. The implementation selects the raw name first.
It adds no cache and changes no protocol state. Exhaustive descriptor/prefix
parity checks cover all 248 entries and the aliases.

One optimized executable linked both MuxEngine versions and alternated six
rounds, with scoped overrides and array append. Entire status-variable outputs
match. Median option-snapshot times:

| Sessions × windows per session | Baseline | Candidate |
| --- | ---: | ---: |
| 1 × 1 | 10.552 ms | 0.153 ms |
| 4 × 4 | 59.994 ms | 0.792 ms |
| 8 × 8 | 210.397 ms | 2.777 ms |

Allocation counts stay unchanged. This is a CPU result. A separate live
baseline, using actual PTYs and a native subscribed client, took 0.23 / 1.05 /
4.29 daemon CPU seconds for 20 renames at 1 / 16 / 64 windows. The long GUI
baseline build overlapped these early experiments; the final quiet comparison
below replaces them for application-level claims. Evidence: `protocol/mux_matched_bench.*` and
`protocol/live_daemon_baseline.*`.

A later quiet matrix uses 100 acknowledged renames, actual PTYs, fresh daemon
processes, and native subscribers held open through the final CPU sample. Build
and allocation experiments were paused. CPU seconds for the command interval:

| Windows | Subscribers | Baseline | Lookup candidate |
| ---: | ---: | ---: | ---: |
| 1 | 1 | 1.033 | 0.217 |
| 16 | 1 | 4.949 | 0.581 |
| 64 | 1 | 20.422 | 4.664 |
| 64 | 0 | 14.535 | 0.336 |
| 64 | 4 | 38.970 | 18.620 |

All ten owned daemons and clients exited, all sockets disappeared, and all 418
tracked PTY children exited. The first candidate CLI has matching crate features
and optimization settings, but Cargo changes dependency bitcode treatment when
building only that binary. The final full-bundle comparison below removes that build-setting uncertainty. Evidence: `protocol/live_matrix.json`
and `protocol/live_matrix_summary.json`.

Investigation of the remaining subscriber cost found that a sample of the optimized
64-window daemon attributes 1,662 of 2,146 active command-thread samples to
`stamp_snapshot_for_client` → `FormatContext::resolve` → `build_format_universe`.
Each window label rebuilds the whole format universe. The second candidate
creates a borrowed, lazy `FormatContextSnapshot` once for each client's update.
Window labels and border formats share its immutable universe while retaining
separate target contexts and expansion hooks. Its borrow prevents engine
mutation during the batch; no data is cached across updates.

Eight complete daemon snapshot fixtures compare serialized output against the
lookup-only candidate, covering empty through 64-window models, zero/one/four
clients, different attachments, nested session/window/pane loops, formatted
borders, splits, focus, bell, and clock mode. Outputs match. At 64 windows, one
client's full snapshot preparation falls from 40.43 to 1.51 ms, allocation
requests from 361,894 to 16,654, and cumulative requested bytes from 34.06 to 1.65 MB.
Those byte totals include transient allocations, not just retained memory.
Four clients fall from 171.67 to 13.66 ms. The one-window case remains about
88.4 microseconds with unchanged allocation counts. These timings overlapped compilation and describe an in-process
snapshot fixture; the final live-daemon matrix establishes the application gain. Evidence:
`protocol/daemon_context_*`. Two permanent mux regressions exercise context
and nested-format equality, shared allocation identity, and freshness after
mutation between batches.

The same fixture identifies another unnecessary universe construction:
`client_format_facts` builds a full status context just to take its stored
`uid` and `user` strings. Exposing the two existing borrowed engine getters
removes that work. Serialized snapshots still match with explicit nonempty
identity assertions and client-loop labels. On the shared-context candidate,
64-window preparation falls from 1.470 to 0.848 ms for one client and from
13.350 to 3.408 ms for four clients. The four-client allocation count falls
from 131,731 to 43,507. These helper timings overlap compilation; final live
measurements below determine the application gain.

The borrowed batch is explicitly dropped before pane modes are stamped.
Customize and switch modes construct their own format universe. Keeping the
batch alive through that work unnecessarily overlaps both allocations. A
64-window fixture reduces peak requested allocation from 691,538 to 420,768
bytes with identical output; the extra retained universe falls from 270,770
bytes to zero. This changes ownership duration, not caching or format behavior.
Evidence: `protocol/context_lifetime_bench.csv` and its source.

## Final daemon comparison

Both saved variants were built through `just profile-build mac`, with matching
Rust optimization settings and native `ReleaseFast`. The final GUI executable
SHA-256 is `567a1ab5e053f72bb3a11d58f622ae17b198018fd2fbc8678b6c0bfe34999250`;
its CLI is `df01634f9cbe59b54230f84bfead702bb4010cb70736cd263f62f639ee20b182`.
The source hashes in `final/manifest.json` identify the implementation measured.
The final native fetch used the published immutable commit through the normal
build script, without a source-directory or package-config override. The
actual GUI and CLI TLS sections each shrink by 262,144 bytes.

The quiet matrix took 446.9 seconds. Each case used one fresh baseline/final
pair, with real sleeping PTYs, native metadata subscribers, and 100 acknowledged
renames per leg. Variant order reversed between cases, with other builds and
experiments paused. This was not a repeated final trial of every case. CPU here is process CPU
per command, not wall-clock request latency or idle utilization.

| Windows | Subscribers | Baseline CPU / rename | Final CPU / rename | Reduction |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 1 | 10.627 ms | 1.938 ms | 81.8% |
| 16 | 1 | 49.142 ms | 3.210 ms | 93.5% |
| 64 | 1 | 201.932 ms | 7.142 ms | 96.5% |
| 64 | 0 | 143.644 ms | 3.494 ms | 97.6% |
| 64 | 4 | 384.189 ms | 20.921 ms | 94.6% |

All 1,000 renames were acknowledged. Snapshot event counts on the driving
connection matched within each pair: 102, 100, 100, 0, and 103. Those counts
include queued startup snapshots. Other message types and auxiliary
subscribers' traffic were not counted; the separate eight serialized fixtures
provide content-equality evidence.

Each leg also observed 30 seconds of idle time, spanning two default status
periods. These are single observations, with no refresh-event count, and serve
as supporting evidence. Memory values cover all final changes, including the
native TLS reduction, rather than isolating protocol work.

| Windows / subscribers | Idle CPU, baseline → final | RSS, baseline → final | Footprint, baseline → final |
| --- | ---: | ---: | ---: |
| 1 / 1 | 55.631 → 21.140 ms | 23.44 → 19.53 MiB | 12.95 → 9.08 MiB |
| 16 / 1 | 154.875 → 23.319 ms | 74.16 → 53.95 MiB | 63.69 → 43.45 MiB |
| 64 / 1 | 391.407 → 37.955 ms | 270.69 → 207.06 MiB | 259.89 → 196.44 MiB |
| 64 / 0 | 15.071 → 16.125 ms | 220.88 → 150.59 MiB | 210.92 → 140.78 MiB |
| 64 / 4 | 484.550 → 69.389 ms | 284.91 → 221.59 MiB | 273.78 → 211.28 MiB |

The zero-subscriber idle result does not demonstrate a CPU improvement. A
separate residual stack capture places 1,472 of 2,184 command-execution samples
under `refresh_status_filtered`, including 742 under `format_option_snapshot`
and 428 under `status_request`; 309 samples include `publish_mux_snapshots`.
These inclusive counts overlap. The remaining cost centers on status setup
and full option/context maps. Early recipient selection and more targeted
status inputs remain follow-up experiments, with format clocks and arbitrary
format dependencies preserved.

All eleven daemon/client groups, eleven sockets, and 482 tracked PTY children
were cleaned up, including the separate residual capture. Evidence:
`protocol/final-results.md`, `protocol/live_matrix_final.json`,
`protocol/residual-sample-summary.json`, and `protocol/final-audit.json`.

## Production deadlock found during validation

The first full workspace test run stopped progressing in three existing
agent-permission tests. Stack samples show a lock cycle: `Shared::inspect`
held the engine lock while reading agent runtime state, and the agent flusher
held its lane lock while publishing into the engine. Both functions match their
pre-investigation versions. The affected command is ordinary `zz inspect`, not
only a test helper.

The fix collects owned inspection fields while holding the engine lock, drops
that lock, then reads runtime permissions and serializes each row. The same
pattern already exists in `send_agent_resync`. Output order, target errors,
permission source, and agent replay ordering remain unchanged. Reading the
engine's cached agent state instead would be incorrect around agent restart,
shutdown, and the interval between runtime acceptance and publication.

All three formerly stuck tests passed ten independent runs each, and all five
existing inspect output/error tests passed. Opus independently reviewed lock
lifetimes and lifecycle semantics and recommended the change. The temporary
row buffer has a measured cost: approximately 99–123 KiB extra peak requested
allocation for 64 rows with short values, depending on the JSON map feature.
At 1,024 short rows the preserve-order fixture adds 1.85 MiB; longer values
make final output allocation dominate, with no extra measured peak. The buffer
is request-local, scales with the selected rows, and is freed on return. It
has no fixed global cap. This additional snapshot cost is recorded alongside the removal of a production deadlock.
Evidence: `memory/daemon-inspect-deadlock.md`, `memory/inspect-permission-repeat.json`,
`memory/inspect-row-buffer*.txt`, and `oracle/inspect-review.stdout.txt`.

## Daemon allocation owners

In the original baseline, 64 dormant terminal panes use 267 daemon threads.
Closing back to one
pane returns to 15 threads and lowers native malloc live bytes from about 82.5
to 4.0 MiB, while much of the mapped storage remains allocated or reusable.
This is not evidence of unbounded live-object growth.

Allocation-stack capture identifies about 70.7 MiB of the live native malloc
storage as 266 dyld thread-local-storage blocks of 278,528 usable bytes each.
The executable's `__thread_bss` contains a 256 KiB Zig
`Thread.maybeAttachSignalStack` buffer, instantiated for every Rust thread when
the TLS image is allocated. Ghostty row builders account for about 10.5 MiB.
The C ABI never registers that Zig stack: its IO implementation uses
`Threaded.init_single_threaded`, and Rust-owned threads do not enter Zig startup
or Zig thread wrappers. Zig's standard `std_options.signal_stack_size = null`
removes the unused buffer without changing locks, panic handling, or unwinding.
The candidate limits this option to C ABI builds.

A 256-thread Rust host linking matched `ReleaseFast` archives falls from
75.064 to 6.188 MiB median footprint, and its Mach-O TLS section shrinks by exactly 262,144 bytes.
Read-only signal queries before and after terminal operations prove the same
SIGSEGV/SIGBUS handlers, masks, flags, and Rust-owned 128 KiB alternate stack.
A plain C host has no alternate stack before or after operations in either
build. Eleven upstream Rust wrapper tests linked to the actual changed archive
pass, and all 780 exported symbols match. Ghostty's Debug `test-lib-vt` suite
also passes, but its default test runner owns `std_options` and therefore does
not exercise this override. Whole-daemon performance acceptance must compare the same
`ReleaseFast` native optimization setting; an initial `ReleaseSafe` candidate
was identified and excluded. See `memory/ghostty-tls-investigation.md`.

Two interleaved full-daemon runs per variant use the same Cargo command,
features, native `ReleaseFast` setting, and isolated sleep PTYs. Median results:

| State | Baseline footprint | Patched footprint |
| --- | ---: | ---: |
| No panes | 6.56 MiB | 4.20 MiB |
| 1 pane | 12.75 MiB | 8.79 MiB |
| 16 panes | 63.20 MiB | 43.46 MiB |
| 64 panes | 265.91 MiB | 194.63 MiB |
| Close back to 1 pane | 149.91 MiB | 79.55 MiB |

At 64 panes, RSS falls from 275.97 to 204.65 MiB and live native malloc from
82.476 to 13.119 MiB. The exact daemon TLS section shrinks by 262,144 bytes;
remaining TLS sizes match. This proves a daemon memory improvement, not a
71 MiB reduction for a one-pane GUI. Evidence:
`memory/ghostty-matched-daemon-summary.json` and
`memory/ghostty-matched-cli-tls-sections.json`.

Nine quiet alternating native-library timing pairs show no material regression:
two million empty write/render updates take 36.245 versus 36.611 ms process CPU,
short updates 97.266 versus 96.357 ms, and 5,000 terminal
create/write/resize/render/free cycles 164.184 versus 164.867 ms. Each difference
is about 1% or less, with overlapping run ranges. Concurrent preliminary runs
were discarded. Evidence: `memory/ghostty-final-cpu-summary.json`.

The published fork commit is
[`fa7986a9`](https://github.com/demfabris/ghostty/commit/fa7986a9dc3e582c46ebe248f66571ed740c7afe),
one added line on the existing upstream base. The normal build fetches that
immutable commit; no source rewriting, Cargo dependency bump, or binding change
is involved. Native fork maintenance is documented separately from the
Cargo-only `just forks` tool.

Four macOS workers account for each additional dormant pane: terminal actor,
child wait, eager search, and daemon publication. The actor already polls its
PTY directly; there is no separate macOS reader to merge. After the TLS fix,
64 panes reserve 546.2 MiB of virtual stacks but use only 8.34 MiB of resident
stack pages. Each added pane contributes about 128 KiB resident stacks and
20 KiB remaining native TLS. Pooling cannot claim the much larger virtual
reservation as RSS savings.

The same capture attributes 10.5 MiB of native live allocations to Ghostty
row builders and 161.2 MiB dirty pages to mimalloc. The latter is not a live
object count. Returning to one pane restores the original thread count and
small native live heap while allocators retain pages. A shared search pool or
child-exit service is a reasonable next experiment; a shared terminal reactor
also needs IO fairness, final-output ordering, cancellation, deadline, and
platform proofs. The designs remain parked. See
`memory/daemon-thread-followup.md` for source references and exact accounting.

## Correctness validation

- `cargo test --workspace --all-features --no-fail-fast` completed 82 test
  binaries/doc-test groups: 4,121 passed, one failed, five ignored. The sole
  failure was an unchanged directory-inheritance test comparing two successive
  OSC 7 URIs while the shell changed the hostname from `workstation` to
  `macbook`; both paths were correct. It passed in isolation. A preceding run
  encountered a local-port collision in a socket test, which also passed alone.
- The complete daemon library suite then passed with four test threads:
  1,005 passed, zero failed. Together with the workspace run, all 4,122 executed
  tests have passing results; this is not a claim that the initial maximally
  parallel workspace invocation was clean. Evidence: `final-workspace-retry.log`,
  `final-daemon-limited-parallel.log`, and `memory/cwd-inheritance-test-triage.md`.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes.
- Standard `zz-mux` suite: 567 tests; nearest daemon regressions: 8.
- Transcript reducer tests: 2 pass. Agent UI tests with the `agent` feature: 55 pass.
- Profiling summary tests: 4 pass. Profiling launcher shell syntax passes.
- Changed Rust files pass rustfmt, and the working diff has no whitespace errors.
- Workspace-wide formatting reports existing issues in unmodified iOS
  `momentum.rs` and `window.rs`; those files were not changed for this task.
- The additional daemon-only build without agent support fails because existing
  inspection/session code uses `serde` and `serde_json`, but the unchanged
  manifest enables those dependencies only through the `agent` feature. That
  configuration issue is recorded separately; no dependency changes were added.
- TUI campaign validation passes with its generated report restored to the
  unchanged ledger. The OKF validator reports zero errors and one existing
  warning about six older research documents.

## GPU image lifetime investigation

A real MetalAtlas experiment inserts a 1024-square RenderImage, drops its last
CPU Arc, and checks the atlas. The CPU object disappears but 4,227,072 GPU bytes
remain allocated under its image key. Inserting another image requires a
second texture. Explicit removal releases the texture. This establishes that
missing removal leaves an occupied tile, not merely reusable atlas capacity.

The desktop drains retired Kitty images during pane render. Closing an image
pane can release that entity without another render. The original window-scoped lifecycle test subsequently proved logical
ownership and close-path cleanup. Expanded retired-CPU and multiple-view
tests in the final parked proposal were not run; the independent Metal
ordering failure remains the acceptance blocker. Evidence: `render/atlas-lifetime-experiment.txt`.

The actual GUI workload opens and closes twenty image-bearing split panes,
keeping the original terminal visible. Computer-use inspection confirmed the
blue image was painted. Baseline graphics-tagged resident allocation grew from
12.6 to 89.2 MiB, with 19 additional regions; display drawables remained at
232.1 MiB. The lifecycle test fails on the original code after proving that the
view has been released, and passes with explicit window-atlas cleanup.
Evidence: `baseline-kitty-split-1/`, `render/kitty-lifecycle-baseline.log`, and
`render/kitty-lifecycle-fixed.log`. The earlier new-window workload did not
prove visibility and must not be used as GPU evidence.

**The cleanup patch is parked because a separate GPU ordering test fails.**
In the actual Metal atlas, a command buffer waits on a test event before reading
an existing small tile. The CPU removes that image and uploads a replacement
into the same texture coordinates, then releases the event. The earlier GPU
read sees replacement bytes `[153; 4]`, not its expected `[17; 4]`. This proves
unsafe reuse independently of timing luck. Whole-texture release has a different
lifetime: submitted command buffers retain their referenced textures, but that
does not protect bytes overwritten inside a still-live shared texture.

This ordering defect already exists in image replacement. Adding more cleanup
would exercise it more often, so the application change cannot claim to be a
no-drawback improvement. Proper retirement must remove logical image keys
immediately while delaying allocator reuse until every relevant submitted GPU
frame completes. It needs error-path and outstanding-frame tests, not an
arbitrary delay or a fixed number of frames. Evidence:
`render/atlas-reuse-experiment.txt` and `render/atlas-reuse-test.log`.

## Candidates kept for review

| Candidate | Evidence | Reason it remains parked |
| --- | --- | --- |
| Kitty image cleanup on pane/cache release | Twenty visible close cycles retain about 76.6 MiB graphics allocation | A deterministic test proves small atlas tiles can be overwritten before an earlier GPU reader completes; requires completion-based retirement in the Metal backend. |
| Defer macOS CEF framework loading | Standalone load adds about 57.6 MiB RSS / 15.5 MiB footprint | Moves roughly 19–24 ms warm-cache CPU to the first browser; needs GUI startup and first-browser comparison. |
| Allocate Metal path targets on first path | Isolated GPUI tests preserve pixels and reduce reported allocations by 14.625 MiB at 2,560 × 1,440 | The unused texture is unresident at idle, so no equal footprint saving is proved; first path also gains an allocation cost. |
| Bounded JSON serialization | Representative raw-input capacity 96 → 4 MiB; reducer about 22 → 2.6 ms | Repeated short-input regressions, especially escaped Unicode; generic early abort can hide later serializer errors. |
| Shrink oversized owned payloads | Large retention reduction | Repeated replacement helper elapsed time increased 11.6%. |
| Explicit mimalloc collection | In an isolated 32 MiB allocation/free fixture, collection makes about 31.9 MiB reusable | Future page faults and thread-local behavior need workload proof; no periodic collection was added. |
| Steady visible cursor preference | Hidden-cursor diagnostic releases 224 MiB graphics backing in the same GUI process | `cursor-style-blink = false` changes visible behavior and was not itself measured; no setting was changed. |
| Demand-driven display scheduling | Idle GUI still receives display-link callbacks while clean frames skip presentation | Requires lost-wakeup, animation, input, occlusion, and ProMotion correctness; a frame cap is not an equivalent fix. |
| Watchdog/config polling replacement | Two 100 ms watchdog loops and a 500 ms config poll | Disabling them removes diagnostics or discovery; a run-loop/event design needs separate correctness work. |
| Remaining metadata facts / early recipient selection | Repeated facts and no-recipient work exist under the daemon lock | Reassess after the implemented option lookup and per-client format batch; preserve client formats and notification ordering. |
| Inbound/buffer capacity decay | One inbound frame buffer can retain up to 64 MiB; mailbox pool up to 8 MiB/client | Returning capacity trades against later allocation; repeated-large-frame throughput remains unmeasured. |
| Journal serialization | Writing directly into a reserved output buffer removes one allocation in the fixture | Timings overlapped compilation; unreserved writing regressed small records 58–63%, and simple in-place prefix insertion regressed a large torn-tail case. |

A full metadata-delta wire format needs attach/resync checkpoints and typed
patches with base/target revisions. Global mux generation alone cannot describe
recipient focus, presence, client-sensitive formats, modes, and status changes.
Coalescing must not cross reliable-event barriers. Native, web, iOS, and TUI
reducers must converge through attach, detach, split, resize, close, focus,
reconnect, and slow-client recovery. The existing terminal row-patch protocol
already handles a different part of this problem.

The first delta prototype should compare complete personalized snapshots and
emit typed changes keyed by existing session/window/pane IDs. That retains the
current arbitrary-format semantics while measuring byte-count and reducer
effects. It still pays snapshot construction costs. Emitting patches directly
from commands would need a dependency model: renaming one window can change
other labels through nested window/session/client loops. That is a separate,
larger design decision.

Use an explicit result revision as the next patch's base, with a full-snapshot
fallback after mismatch or reconnect. The
[LSP semantic-token protocol](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocument_semanticTokens)
provides a primary reference for that exchange. Apply each patch coherently;
an invalid operation must reject the patch rather than leave partial state.
[RFC 6902](https://www.rfc-editor.org/rfc/rfc6902.html#section-5) documents the
failure rule, although zz should retain its typed binary objects rather than
adopt generic JSON-pointer operations. Reliable-event ordering and per-client
revision ownership need explicit tests before any wire change ships.

## Oracle

Launched Claude Opus 5.5 (`claude-opus-5-5`) with `xhigh` effort in read-only
plan mode, as requested. Its completed targeted review found no correctness defect in the three
patches, but challenged helper-level gains presented as whole-app results,
remaining owned JSON capacity, and tests that preserved undesirable allocation
behavior. The final record separates helper and application measurements and retains the
owned-JSON limitation. Output lives in the local `oracle/`
evidence directory. The initial broad request timed out at 20 minutes without
a response. The narrower retry returned a substantive review of the lookup, bounded-copy,
and appearance-pointer patches. Evidence: `oracle/patch-review.stdout.txt`.

A second review found the Markdown append-revision counterexample and questioned
GPU atlas reuse. Both concerns reproduced. The Markdown fix now preserves the
revision and passes its regression; Kitty cleanup remains parked behind proper
GPU completion tracking. Evidence: `oracle/lifecycle-review.stdout.txt`. A third
narrow review found no runtime defect in the shared format batch or Ghostty TLS
option. It identified the test-runner distinction above and the old written
no-patch policy. The current request explicitly permits measured fork changes,
so the policy documents now record that amendment. The redundant test guard was
removed, and the final C ABI archive passed the wrapper and signal checks again.
Evidence: `oracle/context-tls-review.stdout.txt`. No upstream pull request was
opened; the patch is published in the user's fork at its exact immutable pin.

A fourth review examined the production inspection deadlock, the temporary row
buffer, snapshot consistency, and agent restart/shutdown behavior. It endorsed
the two-phase fix and rejected replacing runtime state with the stale engine
cache. The report above records the measured allocation cost, rather than the
oracle's rough size estimate. Evidence: `oracle/inspect-review.stdout.txt`.

## Primary references

- [Apple: memory use of Metal apps](https://developer.apple.com/documentation/xcode/analyzing-the-memory-usage-of-your-metal-app)
  distinguishes allocation events, resident pages, and charged dirty memory.
- [Apple: profile and optimize memory](https://developer.apple.com/videos/play/wwdc2022/10106/)
  explains physical footprint and `proc_pid_rusage`.
- [Apple: CAMetalDisplayLink](https://developer.apple.com/documentation/quartzcore/cametaldisplaylink)
  documents display-synchronized callbacks and pausing them.
- [Apple: texture replacement](https://developer.apple.com/documentation/metal/mtltexture/replace(region:mipmaplevel:slice:withbytes:bytesperrow:bytesperimage:))
  explains immediate CPU writes and the need to synchronize GPU accesses.
- [Chromium: macOS display-link implementation](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/ui/display/mac/cv_display_link_mac.mm)
  provides a production reference for display-link lifetime.
- [Anthropic: Opus 5.5](https://platform.claude.com/docs/en/models/opus-5-5/overview)
  verifies the requested model ID.
