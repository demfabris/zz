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

The first atlas cleanup prototype exposed an in-flight overwrite race. The
completion-aware follow-up below fixes that ordering and releases 36.28 MiB
of resident graphics memory after ten matched image-pane closes. Its reupload
latency remains a review tradeoff. A standalone Metal control also reproduces
224 MiB of driver backing on this Mac; allocation counts alone cannot justify
calling it a zz leak, and later aging evidence shows it is not permanent.

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
| Status timer | Status refresh constructs a full snapshot and format facts before selecting recipients | Follow-up proves empty-recipient allocation savings and matching-recipient behavior; CPU runs encountered external builds, so the shortcut remains unapplied. |
| Chromium bootstrap | macOS loads the CEF framework before first browser use | Follow-up GUI pairs prove 13.52–14.00 MiB terminal-only RSS savings from deferred loading; first-browser presentation latency remains unmeasured. |
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
ownership and close-path cleanup. At the phase-1 commit, expanded retired-CPU and multiple-view
tests in the parked proposal had not run; the independent Metal
ordering failure blocked acceptance. Evidence: `render/atlas-lifetime-experiment.txt`.

The actual GUI workload opens and closes twenty image-bearing split panes,
keeping the original terminal visible. Computer-use inspection confirmed the
blue image was painted. Baseline graphics-tagged resident allocation grew from
12.6 to 89.2 MiB, with 19 additional regions; display drawables remained at
232.1 MiB. The lifecycle test fails on the original code after proving that the
view has been released, and passes with explicit window-atlas cleanup.
Evidence: `baseline-kitty-split-1/`, `render/kitty-lifecycle-baseline.log`, and
`render/kitty-lifecycle-fixed.log`. The earlier new-window workload did not
prove visibility and must not be used as GPU evidence.

**At phase-1 commit `0f0d05aa`, cleanup remained parked because a separate GPU ordering test failed.**
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

## Follow-up: safe atlas cleanup

After committing phase 1 as `0f0d05aa`, the user requested validation of the
parked candidates one at a time. The first candidate combines desktop Kitty
image cleanup with completion-based Metal atlas retirement. The matched
application experiment now establishes a memory gain: ten image-pane closes
leave 36.28125 MiB of additional graphics allocation in the baseline and none
in the candidate. The candidate remains uncommitted and parked for review of
the costs below; the fork's main branch has not been promoted.

Fork candidate [`a64e53ec`](https://github.com/demfabris/zed/commit/a64e53ec8173bfa00c1cfa7d6a18d50be22f061b)
adds a frame guard to the existing Metal completion block. Removed image keys
leave lookup immediately. A rectangle inside a live shared texture remains
unavailable until all older frames finish; a whole unused texture leaves the
atlas while Metal retains the object for encoded work. Out-of-order completion,
uncommitted buffers, failed encoding, and renderer destruction all release the
same guard. The patch adds no GPU wait or fixed frame delay. Its main fork branch
has not yet been promoted.

Thirteen Metal tests pass with API Validation, including the previously failing
old-reader pixel experiment. Four desktop lifecycle regressions cover image
replacement, pane/cache release, overlapping focused views, and window closure.
The focused input handler can keep an old view alive after its replacement paints.
The first cleanup proposal therefore evicted an image still used by the new view;
explicit cache view counts prevent that. The complete desktop library suite passes
548 tests with one ignored test against the candidate fork.

The primary application pair is `baseline-allocation-d` and
`candidate-allocation-c`, executed candidate first on the same Mac. Each opens
and closes ten fresh panes containing one 1,024 × 1,024 RGBA image. Every close
has a preceding successful upload proof, and a final fresh upload succeeds
after measurement. Native window dimensions, scale, pane layout, configuration,
bundle versions, scenario settings, and capture-code hashes match. Both runs
exit normally with all owned processes gone, no cleanup escalation, and no
overlapping stall-watchdog sample.

| Measured image-sized graphics storage | Baseline | Candidate |
| --- | ---: | ---: |
| 4,128 KiB mappings before / after | 2 / 11 | 1 / 1 |
| Additional mappings after ten closes | 9 | 0 |
| Additional mapped and resident bytes | 38,043,648 | 0 |
| Additional mapped and resident MiB | 36.28125 | 0 |

An earlier normally completed baseline retained ten extra mappings, or
40.3125 MiB. The smaller primary result is retained. During its fifth close,
the old pane paints a zero-image frame before disappearing. Baseline rendering
already drains retired images when it gets another paint; that observed final
paint is consistent with reclaiming this one texture. The successful eviction
call is not logged, so this particular call attribution remains an inference.
The patch covers teardown that receives no such final paint.

The exact 4,227,072-byte mapping size matches the independently measured Metal
allocation for this image. Full non-coalesced maps, the controlled fresh-image
sequence, and lifecycle tests support the attribution; vmmap does not label
objects with RenderImage IDs. These are image-allocation savings, not a causal
estimate of total RSS/footprint, CPU, GPU utilization, or return-to-pane latency.
Raw process memory observations remain in `allocation-paired-results.json`.
The broader workspace checks, strict Clippy, and web build pass. Four unrelated
failures in the initial workspace test run pass their isolated retries; the
original full run itself was not clean.

Four alternating renderer benchmark pairs show no consistent normal-frame CPU
change. Median paired CPU changes are -0.66% for a fixed normal scene, -0.05% for
one small image retired per frame, +1.50% for sixteen, and +1.73% for a 4 MiB
upload per frame. Individual runs vary substantially and sometimes reverse
those directions. These figures do not prove zero overhead. The harness renders
monochrome sprites while allocating and retiring polychrome images; a separate
gated GPU test proves old image contents remain intact. Completed batches retain
stable resource bytes and release their atlas allocations after key removal.
Warmed frame tracking performs no allocations in 100,000 single-frame and
three-frame cycles on both the fork benchmark compiler (Rust 1.98.1) and the
shipping app compiler (Rust 1.97.0).

Ordinary window switching, hidden panes, zoom, and settings retain their terminal
views and uploaded images. The new reupload case is a pane or window moved out of
the attached session and returned after its last old view releases. That delay
has not been measured. Safe retirement
can also retain temporary rectangles while GPU readers remain outstanding.
Neither drawable count nor command-buffer count supplies a universal bound on
that temporary memory. These costs belong in the acceptance decision.

A fifth Claude Opus 5.5 xhigh review found no runtime blocker and required the
app change and fork pin to ship together. Source audits found no equivalent
immediate overwrite in normal WGPU or Windows rendering: their upload APIs order
replacement writes after earlier issued rendering. Linux and Windows runtime
checks have not run. The independent iOS `gpui_wgpu` pin now moves with the root
and web pins to avoid resolving two GPUI dependency trees.

The archived follow-up evidence is under
`target/performance/2026-09-23/followups/gpu-atlas/`, including both app bundles,
symbols, raw captures, the separate fork patch, test logs, and an SHA-256 file
manifest. Scratch originals live in
`/tmp/zz-perf-20260923-followups/gpu-atlas/` and
`/tmp/zz-perf-20260923/atlas-retirement/`. The former contains lifecycle tests,
review output, harness controls, lockfile validation, and archived app builds;
the latter contains the fork patch, Metal tests, renderer measurements, and
cross-platform source audit. Rejected live preflights remain excluded. Both
matched 0.13.0 bundles are built from release `136b9d84`, with the candidate
patch applied on one side. Compiler binaries, native Ghostty, profile settings,
CEF unsigned contents, and bundle versions match. The archived manifests and
`matched-build-comparison.json` record the comparison.

The follow-up also found a capture-isolation limit. A private socket and an
existing private `XDG_CONFIG_HOME/zz/config` isolate daemon sessions and config,
but production-identity macOS bundles still read and save
`~/Library/Application Support/zz/window-state.json`. Browser roots and agent
preferences also use the shared Application Support directory. There is no
runtime data-root override for these stores. Captures must check observed native
and pane geometry; matching it supports comparison without establishing full
preference isolation. Candidate diagnostics failed when the window resized,
lost active status, or left the onscreen inventory. They remain rejected. The
first also triggered the stall watchdog's sampler. Source inspection did not
establish the cause of the resize or focus change.

The allocation-only follow-up records focus and position changes while requiring
the same onscreen window, dimensions, scale, pane layout, and a fresh successful
image upload before every pane closes. It does not require every window pixel
to be unoccluded. Cached foreground-command labels are recorded but do not
define pane geometry; they can lag the already-verified fixture output.
It captures full non-coalesced memory maps to separate 4,128 KiB image storage
from conditional driver backing. Its final fresh-image upload check is automatic:
the native inspection tool selected the installed same-identifier app despite
receiving the test bundle's path, so its screenshot was rejected. These runs
cannot establish presentation, live visual correctness, CPU/GPU utilization,
latency, or causal savings in total RSS/footprint. Metal pixel tests provide
the separate correctness evidence. Earlier failed runs are not reclassified.

An early app-inspection attempt also launched an extra scratch-bundle GUI while
selecting the installed app's screenshot. That extra process was identified by
its exact executable and start time, then terminated; the installed GUI and
daemon stayed running. The final primary pair ran after this cleanup. Another
candidate capture completed measurement but failed while tracking a short-lived
process, so it remains diagnostic. The dedicated capture helper now treats an
`EPERM` result as disappearance only when a separate existence check confirms
that the process is gone. Live inaccessible targets still fail. Eight focused
helper tests pass; the original phase-1 helper remains unchanged.

The capture launcher now records its existing shutdown escalations and returns
failure if an otherwise successful capture needed one. The lifecycle drivers
reject those runs, require stable startup geometry, and keep final proof checks
outside their measured intervals. Shell syntax and the four existing profiling
summary tests pass; those summary tests do not exercise native process teardown.

## Deferred macOS CEF loading follow-up

The candidate moves framework loading from desktop bootstrap to the first
browser-runtime start. It uses CEF's scoped macOS loader, retains its owner until
the runtime drops, and reports a missing framework without panicking. Before
calling versioned CEF APIs, it compares the complete loaded framework version
with the compiled version. A mismatch reaches the browser error panel with a
restart instruction. This matters when the app bundle changes while a
terminal-only GUI remains open. The dedicated helper entry point is unchanged.

The standalone integration binary exposed a missing native link declaration:
the existing CEF sys adapter now links the macOS C++ standard library. The GUI
had previously obtained it through other dependencies. The CLI/daemon's linked
libraries remain unchanged. Browser tests pass, including a terminal-only
bootstrap/pump/shutdown/drop that never maps CEF and a missing-framework start
failure. The final targeted Claude Opus 5.5 xhigh review found no remaining
blocker. A separate source audit checked cookie import, site-data clearing,
profile/proxy contexts, popups, downloads, input, and DevTools for calls before
runtime initialization and found no missing start gate.

Both release bundles use the same temporary benchmark-only browser-data
override, fresh profiles, compiler, build recipe, and preexisting changes. That
override has been removed from production source. These bundles require their
private data-directory environment variable and must not be installed. Their
source patches differ only in the CEF runtime, its cold-runtime tests, and the
sys adapter's link declaration and documentation. Build manifests retain full
hashes; framework and CLI/daemon contents match after excluding signatures.

The native correctness passes `correctness-baseline-2` and
`correctness-candidate-1` both validate browser pixels in the owned native
window, literal input, navigation, and cookie/localStorage persistence across
closing and reopening a browser. Each exits through an exact-PID AppKit quit
request, with no forced cleanup and no surviving owned process. Screenshot
validation converts the embedded monitor ICC profile to sRGB in memory; the
original PNG remains unchanged. These diagnostic passes do not establish
resource or latency improvements.

Precreated browser descriptors also initialize and render correctly in both
builds. Their stricter shutdown controls revealed an existing problem: GPUI
waits only 200 ms for quit futures, while browser shutdown permits two seconds
of graceful closing followed by two seconds of forced closing. The unchanged
baseline logged a quit timeout after about 216 ms. Both restored-browser runs
therefore remain rejected as complete lifecycle proofs, despite passing their
individual rendering assertions. Normal process exit does not prove completion
of CEF shutdown. The candidate's never-initialized terminal-only shutdown does
complete cleanly. This timeout is recorded separately, without changing it as
part of the loading experiment.

Initial runs rejected for incorrect screenshot color interpretation or forced
SIGTERM teardown remain rejected. The revised comparison driver requests normal
AppKit termination after measurement, then lets the profiling recipe stop its
owned daemon. It checks PID start identities, exact executables, stable native
and pane geometry, complete samples, and compiler/profiler inventories. Native
proof and trace runs are excluded from resource comparisons. A separate
memory-only mode records background test activity and excludes CPU and timing
claims; ordinary comparisons retain the stricter quiet-process requirement.

Three ordinary matched pairs passed with 30-second terminal intervals and 120
memory samples per root process. The order was baseline/candidate,
candidate/baseline, baseline/candidate. All six runs used a 1,505 by 1,662-point
window at scale 2 and a 153 by 107-cell terminal, fresh browser profiles, five
seconds of warmup, and no detected compiler/profiler conflicts at the guards.
The memory-only fallback was not needed. The independent aggregation recomputes
the summaries from the raw interval and rejects partial samples or mismatched
settings, geometry, hashes, and cleanup.

| Pair | Baseline GUI RSS | Candidate GUI RSS | RSS reduction | GUI footprint reduction |
| --- | ---: | ---: | ---: | ---: |
| 1 | 132.41 MiB | 118.41 MiB | 14.00 MiB | 10.70 MiB |
| 2, reversed order | 132.06 MiB | 118.55 MiB | 13.52 MiB | 1.55 MiB |
| 3 | 131.45 MiB | 117.47 MiB | 13.98 MiB | 10.84 MiB |

The median paired RSS reduction is 13.98 MiB, about 10% of this terminal-only
GUI's RSS. Footprint falls in all three paired medians, with a 10.70 MiB median
reduction and a 1.55–10.84 MiB range. This range matters: candidate 2's saved
post-interval map contains 8.56 MiB more dirty graphics memory than candidate
1, close to their 8.59 MiB median footprint difference. The snapshot does not
explain the initial within-interval drop or establish its cause. No equally
large whole-app saving as the earlier standalone-loader experiment is claimed.
Daemon memory is unchanged within 0.13 MiB. The benefit lasts until the first
browser-runtime operation; Chromium remains loaded afterward.

Idle GUI CPU spans 1.12–1.20% of one core in the baseline and 1.17–1.21% in the
candidate. Paired differences are -0.009, +0.054, and +0.057 percentage points.
These runs establish no idle CPU improvement or GPU-utilization improvement.
The first-browser capture endpoint takes 263/189/205 ms in the baseline and
218/271/272 ms in the candidate. Its paired differences span -45 to +82 ms;
the median is +66 ms. Reopen takes 90–98 ms versus 98–100 ms. These are command
request to RGB capture-return observations, including readback, encoding, and
50 ms polling sleeps, not native presentation latency. They cannot rule out
a first-browser delay from moving framework loading onto that operation.

Decision on 2026-09-23: the terminal-only memory benefit is validated. Keep the
uncommitted candidate for review because the loading cost moves to the first
browser operation and a precise presentation-latency cost remains unmeasured.
No installed or Dev app was replaced or restarted. The existing initialized
CEF quit timeout remains a separate parked correctness issue.

Evidence is archived at
`target/performance/2026-09-23/followups/cef-lazy/`. Start with
`resource-pairs-final.json`, `comparison-provenance.json`, the accepted native
correctness directories, and `candidate.patch`. Raw rejected runs, oracle
responses, both isolated bundles, tool versions, and build/test logs remain
alongside them. The aggregation history also preserves a reporting-only
floating-point timestamp arithmetic correction; no raw samples changed.

Primary references: CEF's
[scoped macOS loader](https://raw.githubusercontent.com/chromiumembedded/cef/708dc14/libcef_dll/wrapper/cef_scoped_library_loader_mac.mm)
and [version interface](https://raw.githubusercontent.com/chromiumembedded/cef/708dc14/include/cef_version_info.h),
plus Apple's [normal application termination](https://developer.apple.com/documentation/appkit/nsrunningapplication/terminate()).

## Exceptional protocol-buffer capacity follow-up

The retention inventory includes the daemon's inbound frame, the native client's
`ProtocolReceiver::frame`, and its `ProtocolSender::frame`. All retain their
largest capacity between messages. The sender was missing from the earlier
inventory. The daemon's separate recycled outbound pool uses LIFO reuse, with
eight entries and 8 MiB combined capacity. A frame-size limit does not describe
all encoder allocation capacity or process RSS.

The first experiment tests a 2 MiB retained-capacity ceiling at completed
message boundaries. A native Unix-socket fixture uses the actual protocol
encoder and decoder, releasing the decoded message before its idle measurement.
Both endpoint buffers remain alive. Baseline retains capacity; the candidate
drops an exceptional buffer after each completed send or receive. No timer,
read timeout, allocator collection, or production code change is involved.
Separate binaries use production mimalloc directly or an allocation counter
around it, so instrumented allocation timings are not used for CPU comparison.

| Check | Baseline | Exceptional-capacity ceiling |
| --- | ---: | ---: |
| Retained sender + receiver capacity after a 6 MiB prompt | 18,874,407 bytes | 0 bytes |
| RSS after one prompt, unchanged through 3 seconds idle | 33.48 MiB | 33.48 MiB |
| Footprint after one prompt, unchanged through 3 seconds idle | 32.50 MiB | 32.50 MiB |
| Allocation + reallocation calls for four prompts, instrumented | 13 | 25 |
| Cumulative requested allocation bytes for those four prompts | 50,332,333 | 125,830,756 |
| Median process CPU for 32 prompts, five alternating-order pairs | 148.84 ms | 151.53 ms |
| Median elapsed time for 32 prompts | 134.92 ms | 134.83 ms |

CPU is higher in every large-message pair, with a 1.8% difference between the
medians. Elapsed time is effectively unchanged in these short trials. The
small-message control retains the same sender/receiver capacities and reuse
counts; its timing is too short and noisy to claim a speedup. Compiler checks
are clear at each boundary, but these are not controls for all background
system activity. Requested live bytes and cumulative allocation bytes are
distinct from allocator usable size, RSS, and physical footprint.

The repeated-message memory result is worse. After 32 prompts, the baseline
holds 33.47 MiB RSS and 32.48 MiB footprint; the candidate holds 52.06 MiB RSS
and 51.08 MiB footprint. A separate native follow-up keeps both endpoints alive
and observes the same 18.59 MiB excess at 0, 1, 3, and 10 seconds idle, despite
zero retained candidate frame capacity. This demonstrates a resident-memory
regression in this fixture; it does not identify the allocator's internal cause
or quantify a complete desktop/daemon workload.

Decision on 2026-09-23: reject the unconditional 2 MiB ceiling. It reduces
retained requested bytes but does not meet the RSS/footprint objective and adds
allocation work. No shipping buffer policy changed. Image, burst, and pooled
traffic experiments were not expanded after this rejection. Timed pool decay
and transport ownership changes remain separate unvalidated candidates. A
read timeout alone is insufficient for inbound decay because an interrupted
prefix/body read must preserve framing state.

The fixture, dependency/source fingerprints, compiler arguments, all 28 exited
process records, allocation counts, native memory observations, and the
ten-second follow-up are retained in
`target/performance/2026-09-23/followups/buffer-retention/`.

Primary references: Rust's [`Vec::clear`](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.clear)
preserves capacity, and [mimalloc's purge policy](https://microsoft.github.io/mimalloc/environment.html)
distinguishes freeing allocations from returning pages. Switching to
[`BytesMut`](https://github.com/tokio-rs/bytes/blob/master/src/bytes_mut.rs)
does not make `clear` release capacity. Hyper's
[adaptive read strategy](https://raw.githubusercontent.com/hyperium/hyper/master/src/proto/h1/io.rs)
adjusts future read size; its write-buffer reset still clears the vector.
Neither is evidence that a buffer-library substitution solves this retention.

## Native config observation follow-up

The existing config observer checks candidate file stamps every 500 ms. A
separate native helper compares that policy with the locked `notify`
`RecommendedWatcher` on macOS, using FSEvents. Both modes use the same binary,
allocator, config stamp detection, and candidate ordering. Candidate paths are
recreated beneath a private directory; real configuration is only read.

Six runs alternate polling/native order, with five seconds of warmup and
30 seconds of idle sampling each. All 72 subsequent edits were detected,
including in-place writes and atomic replacement. The table reports medians
across three runs per mode; delivery medians use 18 edits per method and mode.

| Measurement | Polling | Native observation |
| --- | ---: | ---: |
| Process CPU, percent of one core | 0.01822% | 0.000154% |
| Kernel interrupt wakeups per second | 2.000 | 0.033 |
| RSS | 6.55 MiB | 7.80 MiB |
| Physical footprint | 2.38 MiB | 3.14 MiB |
| In-place edit delivery after write completion | 215 ms | 11 ms |
| Atomic replacement delivery after write completion | 325 ms | 11 ms |

Native observation saves about 0.18 ms of process CPU per second, but costs
1.25 MiB RSS and 0.77 MiB footprint in this helper. Native in-place delivery
varies from 2.5 to 222 ms, so the median is not a latency bound. Kernel counters
do not count every scheduler activation; package-idle wakeups were zero in all
runs. No complete-app CPU or memory improvement is established: the helper
omits GPUI timers, executor submission, and foreground resumption. Linked
framework overhead is common to both modes.

Decision on 2026-09-23: park the replacement as a resource tradeoff. A complete
implementation would also need missing/recreated parent directories, candidate
priority changes, symlinks, overflow/rescan recovery, and backend failure
handling. This fixture covers existing parent directories only. The polling
observer and the separate two-loop stall watchdog remain unchanged; removing
the watchdog would lose automatic freeze diagnostics.

All six owned helper processes exited after the driver's expected SIGTERM
teardown, with no forced kill or survivors. Compiler checks were clear at run
boundaries. Source, compiler arguments, dependency fingerprints, raw process
counters, delivery observations, and summaries are retained in
`target/performance/2026-09-23/followups/config-observation/`.

Primary reference: [`notify`'s documented platform and editor caveats](https://docs.rs/notify/9.0.0-rc.5/notify/)
explain why file replacement and parent-directory observation require explicit
handling.

## Demand-driven display-link follow-up

The first native gate tests the cost of stopping the shared `CVDisplayLink`
when its final subscriber becomes idle. It compiles the actual pinned
`WindowFrameSource` implementation at `a64e53ec8173bfa00c1cfa7d6a18d50be22f061b`,
with one common atomic callback counter. All 44 external dependencies match
the fork lockfile. One optimized helper binary exercises continuous delivery,
stop/restart for each request, and restarting a subscriber while a second
subscriber keeps the same process's display link active.

On the external 120 Hz display, the accepted expanded run contains two rounds
in reversed mode order, with 24 requests per mode and cadence per round.
Fixed delays are 500 ms; varied delays span 471–537 ms. These delays begin after
the prior callback, so modes do not share absolute request schedules. Fixed
cadence can align with display phase; the varied-cadence results stay separate.

| Request to main-thread callback | Continuous | Sole-subscriber restart | Restart with active peer |
| --- | ---: | ---: | ---: |
| Fixed cadence median, 48 samples/mode | 1.45 ms | 9.94 ms | 1.61 ms |
| Varied cadence median, 48 samples/mode | 4.05 ms | 10.45 ms | 4.32 ms |
| Varied cadence observed p95 | 7.90 ms | 14.38 ms | 8.16 ms |

The restart penalty repeats in both rounds: 8.46–8.50 ms for fixed cadence and
6.93–7.62 ms for varied cadence, comparing each round's medians. The synchronous
`start()` call is much shorter: median 50–69 microseconds for sole-subscriber
restart. The extra wait occurs before callback delivery. This measures neither
GPUI drawing nor physical screen presentation; it is a rejection gate for the
underlying scheduling policy.

There is a real resource benefit in the helper. Continuous delivery makes
roughly 120 native/main-thread callbacks per second and uses 0.47–0.60% of one
CPU core. Restart mode delivers exactly the 24 requested callbacks per stage,
about 1.8/s, and uses 0.062–0.092% of one core. Kernel interrupt wakeups fall from
about 124/s to 5.3–5.6/s, including fixture timers. Keeping a peer subscribed
avoids the consistent restart penalty but retains the display's native ticks.
These are instrumented mechanism results, not a complete zz performance claim.

Decision on 2026-09-23: park stopping/restarting the sole display link because
it adds a repeatable first-callback delay. No app dependency pin changed for
this experiment. A three-file fork prototype and six proposed regression tests
are retained for review, but were not compiled or executed after the rejection
gate. Native presentation latency, live resize, tab activation, occlusion,
display changes, sleep/wake, and complete terminal/browser animation behavior
remain unvalidated. The result does not reject every demand-driven design;
other native pacing policies require their own resource and latency proof.

Claude Opus 5.5 at xhigh reviewed the architecture. Its concrete concerns were
closed-window source recreation, consuming demand before a callback is
available, retrying failed subscription despite pending demand, and preserving
high-rate input sustain. The scratch prototype guards closure, preserves
pending demand, and limits sustain rearming to the existing platform-waker
contract. This review is not runtime proof. No stop-delay heuristic was added.

Both helper runs exit normally with no forced cleanup, wrong-thread callbacks,
or callbacks after source destruction. Compiler/profiler checks are clear at
run boundaries; other system activity is not isolated. Source, lockfiles,
binary, raw samples, summaries, patches, and oracle output are retained under
`target/performance/2026-09-23/followups/display-demand/`.

Primary references: [Chromium's per-client frame demand](https://chromium.googlesource.com/chromium/src/+/f5df2996766bb4af356663ae173765e157ba089b/components/viz/service/frame_sinks/external_begin_frame_source_mac.cc)
and [Apple's display-aware pacing guidance](https://developer.apple.com/videos/play/wwdc2021/10147/).

## Early status recipient selection follow-up

`Shared::refresh_status_filtered` currently builds the mux snapshot, daemon
format facts, and option snapshot before filtering recipients. The candidate
keeps the format-clock update first, applies the identical recipient predicate
through a `Peekable` iterator, and returns before those builders when nobody
matches. Matching recipients retain the existing order and rendering path.
There is no new cache or wire format.

Fresh copies of the actual daemon module exercise the complete method.
Dependency provenance includes the earlier option-lookup and borrowed-context
improvements, linked protocol 106, and the pinned Ghostty archive. Separate
instrumented and native binaries use production mimalloc. Twenty paired
behavior cases pass: clocks update on empty calls, forced empty calls skip the
renderer, and matching calls emit the same status contents in the same order.
The comparison normalizes only event sequence numbers while checking their
monotonic order separately; 16 first-publication events per variant are checked.

| Complete refresh, 64-window fixture | Baseline allocations / reallocations | Candidate | Baseline cumulative requested bytes | Candidate |
| --- | ---: | ---: | ---: | ---: |
| Empty subscriber map | 12,445 / 166 | 0 / 0 | 913,302 | 0 |
| Session filter matches nobody | 12,709 / 182 | 0 / 0 | 924,973 | 0 |
| Client filter matches nobody | 12,544 / 166 | 0 / 0 | 918,639 | 0 |
| Matching recipient, with or without explicit filter | 23,969 / 1,221 | Unchanged | 2,198,060 | Unchanged |

These are allocation-counter results, not RSS or physical footprint. The CPU
acceptance gate remains unresolved: both native timing attempts encountered
transient external Cargo processes at a guard boundary. Their subcommands and
ownership could not be recovered before they exited. The raw timings remain
excluded rather than attributing them to a quiet machine. All 80 matrix
children exited normally; no external process was stopped or altered.
A separate bounded observation later caught real Cargo test compilation and
linking in another project, confirming that external build load is present.
It does not identify the earlier transient processes. That observation is
retained in `target/performance/2026-09-23/followups/build-activity/`.

Decision on 2026-09-23: retain the candidate for a controlled timing retry.
Allocation elimination and behavior are proved in the fixture, but no CPU,
elapsed-time, RSS, or end-to-end application gain is claimed. The production
patch and proposed permanent test remain unapplied; Cargo tests and private
CLI workload validation were not run for this candidate. Evidence and the
dependency/source audit are retained under
`target/performance/2026-09-23/followups/metadata-recipient/`.

## Watchdog activity-observer correctness gate

The existing stall detector observes progress on GPUI's foreground executor,
which runs through the main dispatch queue. A native run-loop observer watches
a different signal. A nested native loop can continue processing timers while
the serial main queue remains inside an outer task and cannot run the next
heartbeat. CEF's pump is called from a foreground task, so preserving this
distinction matters; this experiment does not claim an actual CEF freeze.

A standalone native helper reproduces the current chained 100 ms heartbeat,
independent 100 ms monitor, and strict age greater than 500 ms threshold. It
compares those semantics with a simple activity observer across three fresh
processes. Each includes a native 20 ms timer and a roughly 900 ms test phase.

| Case | Main-queue heartbeats during phase | Native timer firings | Heartbeat detector | Activity observer |
| --- | ---: | ---: | --- | --- |
| Healthy, 936 ms | 9 | 47 | No report | No report |
| Blocked main task, 905 ms | 0 | 0 | Reports and recovers | Reports and recovers |
| Nested native loop, 904 ms | 0 | 45 | Reports and recovers | Misses the stall |

The nested case also records 46 before-wait and 47 after-wait transitions,
plus nested entry and exit. The heartbeat detector reports at an observed age
of 506.6 ms. All causal assertions pass; all processes exit normally and are
reaped without signals. This is correctness evidence, not CPU or wakeup
measurement. The helper omits the production detector's synchronous two-second
stack-sampling subprocess, so its recovery timestamps are not production
recovery latency.

Decision on 2026-09-23: park the observer-only replacement because it loses
existing queue-starvation coverage. No production diagnostic code changed.
This does not reject every run-loop-aware design: preserving an outer work
scope or another queue-progress signal would require additional integration
and its own overhead validation. No such redesign was added here.

Source, compiler command/log, binary, raw timestamps, process inventories, and
assertion results are retained in
`target/performance/2026-09-23/followups/watchdog-observation/`.
Primary references: Apple's [serial main queue](https://developer.apple.com/documentation/dispatch/dispatchqueue/main)
and [run-loop modes and observers](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/Multithreading/RunLoopManagement/RunLoopManagement.html),
plus the matching [Chromium work-scope and nesting implementation](https://chromium.googlesource.com/chromium/src/+/refs/tags/152.0.7977.83/base/message_loop/message_pump_apple.mm).

## Reserved journal buffer follow-up

The fresh candidate writes JSON directly into the final newline-delimited
buffer, starting with the same 128-byte reserve used by pinned `serde_json`.
It keeps encoding in memory before the existing cap check and file write.
This removes the separate record-to-line copy without changing serialization,
delimiters, sequence numbering, or journal recovery rules.

The accepted native gate uses production mimalloc, six alternating paired
rounds, and 58 cases. A separate instrumented binary records allocations.
All output comparisons and JSON parse checks pass. Cases cover short/tool
records, escapes, Unicode, nested values, sizes through 1 MiB, both leading-LF
states, sequence boundaries, and complete line lengths around 128/256/4096.
Eighty process checks over the 10.41-second native capture find no active
compiler/profiler. Both helpers exit normally without cleanup signals.

Most cases improve: ordinary 64-byte chunks use about 5.6–5.9% less process
CPU, 4 KiB chunks 3.5–4.3% less, and the 1 MiB tool record 4.6–5.2% less. There
is a repeatable small drawback at growth boundaries:

| Complete line | Leading LF | CPU change, median paired rounds | Baseline to candidate median CPU |
| --- | --- | ---: | ---: |
| 129 bytes | No | +6.31% | 90.78 to 96.03 ns |
| 129 bytes | Yes | +5.87% | 89.09 to 94.13 ns |
| Escaped, 257 bytes | No | +2.05% | 160.06 to 163.71 ns |
| Boundary, 257 bytes | No | +2.38% | 141.22 to 146.32 ns |

Each listed case is slower in all six rounds. The 129-byte cases make two
allocation/reallocation calls in both variants, while returned capacity grows
from 129/130 to 256 bytes and cumulative requested bytes from 257/258 to 384.
These are allocator-API quantities; logical peak remains similar and no
physical-memory conclusion follows. The roughly five-nanosecond difference is
not evidence of a noticeable user delay or a complete append regression.

Decision on 2026-09-23: keep the candidate for review under the requested
tradeoff rule. It usually saves encoder work, but has a small repeatable
short-record cost. No production source changed. The complete
append/coalescing/flush gate was not run, so neither application CPU nor
append throughput improvement is established. No reserve tuning or additional
special case was added. The earlier unreserved and prefix-insertion variants
remain rejected.

Exact source, candidate patch, locked dependencies, native/allocation binaries,
raw per-case samples and process checks are retained in
`target/performance/2026-09-23/followups/journal-reserved/`.
Primary reference: pinned [`serde_json` serialization source](https://docs.rs/serde_json/1.0.151/src/serde_json/ser.rs.html).

## Selective Value JSON follow-up

The fresh candidate narrows only the private `json_payload` helper to
`serde_json::Value`, matching all four raw-input/output call paths. A top-level
string or direct object string member larger than the existing 512 KiB cap
selects bounded serialization. Ordinary values, nested shapes, arrays and
escape-expanded smaller strings retain the current formatter. Generic typed
content and Markdown serialization remain unchanged.

The bounded writer distinguishes its own limit error from unrelated errors,
repairs a partial UTF-8 tail, and uses the current truncation marker and cutoff.
Two focused tests pass, including unrelated-error rejection. The complete-call
fixture compares all 24 returned outputs and 205 cap-boundary cases exactly.
It measures selection, formatting, truncation and returned-buffer destruction,
with input construction outside the interval. Native timing and allocation
counting use separate optimized binaries with the production runtime dependency
versions, JSON features and mimalloc. Four build/procedural-macro dependencies
resolve newer versions in the scratch lockfile: `cc`, `find-msvc-tools`, `syn`
and `unicode-ident`. Both variants share them in the same binary; this is a
paired scratch result, not a build with complete application-lockfile parity.

Both captures pass the external-build/profiler guard: 74 checks during the
9.85-second native run and five during the 0.46-second allocation run. Six
alternating native rounds produce these results. Percentages below compare
the median process CPU per call; raw paired changes are retained separately.

| Payload | Baseline to candidate CPU per call | Change |
| --- | ---: | ---: |
| Null | 6.99 to 8.56 ns | +22.4% |
| Boolean | 6.91 to 8.35 ns | +20.9% |
| Large Unicode object string | 298 to 686 µs | +130.1% |
| Large escape-heavy object string | 541 to 896 µs | +65.6% |
| Direct Write content, 4 MiB | 1.198 to 1.076 ms | -10.1% |
| Direct Edit strings, 4 MiB each | 2.350 to 1.099 ms | -53.2% |

Each listed case changes in the same direction in all six paired rounds.
The selected top-level string just one byte above the cap is also slower in
five of six rounds, with a 4.1% ratio-of-medians increase. The Write/Edit cases
reduce returned capacity from about 8 MiB each to 512 KiB. Selected cases
request another 64 bytes for error handling. Ordinary cases retain their
current allocation behavior; their added CPU cost is small in absolute terms.

Decision on 2026-09-23: park this selective variant too. Preserving ordinary
formatting does not eliminate measured regressions, and the large Unicode and
escape-heavy cases show a substantial formatter cost despite reduced capacity.
No production source changed. No full reducer, GUI, uncollected RSS or footprint
gain is claimed; those later gates were unnecessary after the CPU drawback.
Both helpers exited normally without signals. Source, patch, exact extraction
checks, dependencies, binaries, raw samples and process inventories live in
`target/performance/2026-09-23/followups/json-value-fastpath/`.
Primary references: pinned [`serde_json` writer and error contract](https://docs.rs/serde_json/1.0.151/serde_json/fn.to_writer_pretty.html)
and Rust's [`Write` contract](https://doc.rust-lang.org/std/io/trait.Write.html).

## Cursor blink timeout follow-up

The earlier hidden-cursor diagnostic left the steady visible cursor unmeasured. A same-process run on
the profiling bundle switched one focused pane between DECSCUSR blinking and steady block phases:

| Phase | Presents/s | GPU ms/s | Footprint | Owned graphics resident | GUI CPU, 20 s window |
| --- | ---: | ---: | ---: | ---: | ---: |
| Blinking | 1.91 | 4.18 | 400.5 MiB | 228.8 MiB | 259.7 ms |
| Steady | 0 | 0 | 164.7 MiB | 4.8 MiB | 159.7 ms |
| Blinking again | 1.90 | 4.07 | 400.1 MiB | 228.8 MiB | 266.1 ms |

Instruments stayed attached, so CPU includes tracing overhead. After 45 seconds steady, the first
frames took 0.10 and 2.08 ms of GPU time, the same as ordinary blink frames; no re-backing cost was
visible. The shipped change stops blinking, cursor shown, 10 seconds after the last key, mouse, IME,
or focus change. Program output does not restart it, matching GTK, kitty and Alacritty. The matched
idle pair with no cursor escapes, baseline then candidate bundle:

| Bundle | Presents/s | GPU ms/s | Footprint | GUI CPU, 20 s window |
| --- | ---: | ---: | ---: | ---: |
| Baseline | 1.93 | 4.21 | 399.1 MiB | 245.6 ms |
| Blink timeout | 0.02 | 0.04 | 154.1 MiB | 166.4 ms |

Interrupt wakeups stay at about 120/s in every phase: the display link still ticks.

## Candidates kept for review

| Candidate | Evidence | Reason it remains parked |
| --- | --- | --- |
| Kitty image cleanup on pane/cache release | Matched app capture: ten closes retain 36.28125 MiB in baseline, zero in candidate | Memory gain and GPU ordering are validated. Moving a pane away and back can require reupload; its latency and added frame-bookkeeping cost remain review tradeoffs. |
| Defer macOS CEF framework loading | Three matched GUI pairs save 13.52–14.00 MiB RSS and 1.55–10.84 MiB footprint before the first browser | Memory gain validated; first-browser capture differences range from -45 to +82 ms and do not establish presentation latency. Loading cost moves to the first browser. Candidate remains uncommitted for review. |
| Allocate Metal path targets on first path | Isolated GPUI tests preserve pixels and reduce reported allocations by 14.625 MiB at 2,560 × 1,440 | The unused texture is unresident at idle, so no equal footprint saving is proved; first path also gains an allocation cost. |
| Bounded JSON serialization | Earlier broad prototype reduced raw-input capacity 96 → 4 MiB; selective Value follow-up reduces Write/Edit capacity about 8 MiB each → 512 KiB | Selective variant preserves output/error handling but raises large Unicode/escaped formatter CPU 130%/66% and adds small ordinary-value costs. Both variants parked; no fresh app RSS gain claimed. |
| Shrink oversized owned payloads | Large retention reduction | Repeated replacement helper elapsed time increased 11.6%. |
| Explicit mimalloc collection | In an isolated 32 MiB allocation/free fixture, collection makes about 31.9 MiB reusable | Future page faults and thread-local behavior need workload proof; no periodic collection was added. |
| Steady visible cursor preference | Superseded by the shipped 10 s blink timeout above | Users who want no blink at all can still set `cursor-style-blink = false`. |
| Demand-driven display scheduling | Native stop/restart helper cuts about 120 callbacks/s to 1.8/s and CPU from 0.47–0.60% to 0.062–0.092% of one core | Sole-link restart adds 6.9–8.5 ms to per-round median callback latency. Parked; scratch fork uncompiled, no pin change, full-app presentation/behavior unvalidated. |
| Native config observation | Helper saves 0.018 percentage points of one CPU core and 1.97 interrupt wakeups/s; all 72 edits detected | Adds 1.25 MiB RSS and 0.77 MiB footprint. Parked; complete-app gains and missing-directory/recovery behavior remain unvalidated. |
| Watchdog polling replacement | Native correctness gate shows a 904 ms nested loop advances 45 native timers but no main-queue heartbeat | Pure activity observer misses the stall that the current detector reports. Parked for lost diagnostic coverage; no performance gain claimed. |
| Early status recipient selection | Real-method fixture passes 20 paired behavior cases; empty/nonmatching calls eliminate roughly 12,500 allocations plus reallocations and 0.9 MB requested bytes | Both timing attempts hit external Cargo guards. CPU acceptance unresolved; candidate and permanent test remain unapplied. Broader facts caching and wire deltas are separate. |
| Inbound/buffer capacity decay | A 2 MiB immediate ceiling releases 18 MiB of frame capacity in the native fixture, but repeated large traffic leaves 18.59 MiB more RSS/footprint and uses 1.8% more median CPU | Immediate ceiling rejected. Timed pool decay and transport ownership changes remain unvalidated; mailbox recycling is already capped at 8 MiB/client. |
| Journal serialization | Fresh mimalloc gate passes 58 output cases; reserved direct writing usually lowers CPU and allocation calls | 129-byte lines use about 5 ns more CPU in all six rounds, with larger requested capacity and no call saving. Parked; complete append gain unmeasured. Earlier unreserved and prefix-insertion variants remain rejected. |

The final read-only metadata review finds no accepted post-phase-one measure
of encoded bytes, serialization CPU, or client-render cost. Current evidence
points primarily to status construction. Building a full snapshot and then
diffing it would retain that cost while adding comparison and per-client base
storage. Reconstructing the current full snapshot on the client would also
retain its tree scans and coarse notifications. Smaller messages alone do not
establish lower CPU or RSS. Broader facts caching and wire changes remain
parked until their intended cost is measured separately. Source ownership,
recipient differences, reliable admission side effects, all client consumers,
and the required proof are recorded in
`target/performance/2026-09-23/followups/metadata-wire-triage/`.

A full metadata-delta wire format needs attach/resync checkpoints and typed
patches with base/target revisions. Global mux generation alone cannot describe
recipient focus, presence, client-sensitive formats, modes, and status changes.
Coalescing must not cross reliable-event barriers. Native, web, iOS, and TUI
reducers must converge through attach, detach, split, resize, close, focus,
reconnect, and slow-client recovery. The existing terminal row-patch protocol
already handles a different part of this problem.

If measurements justify a delta prototype, it should compare complete personalized snapshots and
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
revision and passes its regression. At the phase-1 commit, Kitty cleanup remained
parked behind proper GPU completion tracking; the follow-up above tests that fix. Evidence: `oracle/lifecycle-review.stdout.txt`. A third
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
