---
type: Subsystem
title: PTY drain topology (the IO fast path)
description: How macOS keeps its tuned inline PTY actor while Linux reads idle panes on their shard and lends busy ones to a bounded gather stage that overlaps reads with VT parsing; includes the probe and benchmark results behind each platform choice.
resource: crates/zz-terminal/src/session.rs
tags: [pty, throughput, drain, spin-bridge, poll, benchmark, session]
timestamp: 2026-10-03T22:15:00Z
---

# Overview

`run_terminal` in `session.rs` is the terminal actor. It owns the libghostty-vt state (see
[libghostty-vt](/terminal/libghostty-vt.md)), command handling, and snapshot publishing.
macOS and other Unix targets drain the PTY inline with the actor. Linux reads an idle pane on
its shard too, and lends a busy one to a bounded gather thread per shard, which reads while
the actor parses the previous batch. Windows keeps the
portable blocking-reader path. Each platform keeps libghostty state on the actor thread.

Benchmarked 2026-07-28 (`bench/`, Mac16,5, 180×50, medians of 5 hyperfine runs):

| terminal | cat 150 MiB ascii | cat 150 MiB unicode | DOOM-fire fps |
| --- | ---: | ---: | ---: |
| **zz (this design)** | **342.4 MB/s** | **99.0** | **671.5** |
| ghostty tip (nightly termio) | 293.6 | 94.1 | 650.5 |
| cmux (embeds ghostty tip) | 284.9 | 85.9 | 627.4 |
| ghostty 1.3.2 stable | 94.3 | 68.9 | 630.5 |
| zz before this design | 161.1 | 108.0 | 631.7 |

Benchmarked 2026-08-05 on Linux 7.1.5, Ryzen 7 7800X3D, exact 180×50 grid:

| terminal/path | cat 150 MiB ASCII | cat 150 MiB mixed UTF-8 | DOOM-fire fps |
| --- | ---: | ---: | ---: |
| zz inline actor | 444.39 MB/s | 74.43 MB/s | 1043.94 |
| **zz Linux gather** | **644.09 MB/s** | **79.85 MB/s** | **1283.80** |
| strongest submitted rival | Ghostty-tip 636.94 | Rio 123.58 | Alacritty 1255.05 |

The Linux ASCII result uses 15 runs with a 232.888 ms median. Its individual runs span
232.170–234.539 ms. The DOOM result uses the benchmark's full 30-second duration.

# The physics: a 1024-byte bucket brigade

A macOS pty hands the consumer **exactly 1024 bytes per read** during a flood, no matter the
buffer size offered, and the producer blocks after filling that queue. Baud, termios flags,
and buffer sizes do not change this (probed exhaustively . see the constants' doc comments).
So 150 MiB = 153,600 mandatory exchanges, and throughput is purely:

```
throughput = 1 KiB / (cost of one producer↔consumer exchange)
```

```mermaid
sequenceDiagram
    participant cat as cat (producer)
    participant K as kernel pty queue (~1 KiB)
    participant zz as zz actor (consumer)
    cat->>K: write(1024) . fills queue, blocks
    K-->>zz: readable
    Note over zz: any scheduler nap here<br/>costs more than the read itself
    zz->>K: read() → 1024 bytes
    K-->>cat: queue drained . unblocked
    Note over cat: refill lands within microseconds
    cat->>K: write(1024) ...153,600 times
```

Every microsecond shaved off one exchange is ~150 ms off the benchmark. The design question
is therefore: **what does the consumer pay per exchange?**

# Three topologies, one lesson each

## v1 . reader thread + channel (161 MB/s)

```mermaid
flowchart LR
    cat["cat"] -->|"1 KiB writes"| pty["kernel pty queue"]
    pty -->|"blocking read"| reader["zz-pty-reader thread"]
    reader -->|"crossbeam channel<br/>+ buffer pool recycle"| actor["actor thread<br/>select_biased! over 5 arms"]
    actor --> parse["vt_write parse"]
    actor --> book["per-burst bookkeeping"]
    actor --> pub["publish snapshot"]
```

Per exchange this paid: the reader's wake out of `read()`, a channel send, the actor's pass
through crossbeam `select_biased!` (which registers/unregisters every arm on every call),
buffer-pool recycling, and **full bookkeeping** . effects flush, `active_screen()` FFI,
view reconcile . per ~1 KiB message. Measured ceiling ~162 MB/s headless, and a C probe of
the same two-thread shape confirmed the topology itself was the limit.

*Lesson: the hop is expensive, but so is everything you casually do "per burst" when a burst
is one kilobyte.*

## v2 . naive inline actor (125 MB/s . worse!)

First rewrite: actor owns the fd, sleeps in `poll(2)`, and per wake does
`poll → read → FIONREAD → parse → full bookkeeping`. It **regressed**, and `sample(1)`
showed 66% of actor time asleep inside `poll` waiting for cat's next kilobyte. The raw
probe explained it:

| consumer strategy (raw pump, no parse) | MB/s |
| --- | ---: |
| poll(2) then read, per KiB | 136 |
| blocking read loop | 218 |
| nonblock read + spin 64 on EAGAIN | 281 |
| nonblock read + spin 256 | 332 |
| **nonblock read + spin 512** | **348** |
| FIONREAD-gated spin (any budget) | 19–66 |

*Lessons: (a) `poll` per quantum costs ~40% of everything; (b) sleeping at all mid-flood is
the real tax . the refill you're waiting on lands in microseconds; (c) FIONREAD never
observes the refill in time and spinning on it is catastrophic. The 2026-07-27 conclusion
"a bounded spin measured 7x worse, never spin" was true only of the FIONREAD shape.*

## v3 . inline actor with spin bridge (342 MB/s, shipped)

```mermaid
flowchart TD
    sleep["poll(2) on pty fd + wake pipe<br/>timeout = next deadline"] --> wake{"what woke us?"}
    wake -->|"wake pipe byte"| cmds["drain pipe, try_recv commands<br/>and search results"]
    wake -->|"pty readable"| drain["drain turn"]
    wake -->|"timeout"| gate
    cmds --> gate
    drain --> r["read nonblocking, 64 KiB buffer"]
    r -->|"n bytes"| parse["vt_write inline<br/>burst += n, spins = 0"]
    parse -->|"burst < 256 KiB turn budget"| r
    parse -->|"budget hit"| gate["16 ms publish gate:<br/>effects flush, view reconcile,<br/>snapshot publish"]
    r -->|"EAGAIN, burst ≥ 1 KiB,<br/>spins < bridge_spins (8..512)"| spin["spin: retry read<br/>(refill lands in ~µs)"]
    spin --> r
    r -->|"EAGAIN, interactive burst<br/>or spins exhausted"| gate
    gate --> sleep
```

Per exchange during a flood: one nonblocking `read`, one `vt_write`, and a handful of spent
spins. No wake, no hop, no bookkeeping. Everything else happens once per 16 ms frame or
once per 256 KiB turn.

# The code, piece by piece

All symbols live in `crates/zz-terminal/src/session.rs`. The compile-time split keeps the
macOS inline path under `cfg(all(unix, not(target_os = "linux")))`, selects
`gather_pty_linux` on Linux, and retains `read_pty` for non-Unix targets.

## The Linux gather pipeline (`gather_pty_linux`)

Linux PTY reads average about 2.9 KiB and vary across 1, 2, 3, 5, and 7 KiB, with occasional
64 KiB reads. A pure PTY pump drained the 150 MiB fixture in 250–254 ms. The pinned
libghostty parser consumed the same ASCII fixture in 83.75 ms. The former inline actor paid
both costs in series:

```
254 ms PTY drain + 84 ms parse = 338 ms predicted
337.55 ms observed
```

The Linux path rotates four preallocated 64 KiB buffers per pane through bounded Crossbeam
channels. A pane starts at home: its shard polls and reads the PTY master directly
(`DirectReader`), so an idle or interactive pane costs no gather wake. Once 64 KiB arrive
with no 10 ms pause, `lend_if_busy` hands the pane to its shard's `zz-pty-gather-N` thread,
which the shard starts with its first PTY pane. The gather owns `read` and `poll` for every
pane it holds, plus its own wake pipe, which carries launches, exits and buffer returns. It
sends a pane back 100 ms after the pane's last delivered batch, provided the pane holds a
free buffer, so a starved pane stays with the gather. `go_home` parks the pane in its
`GatherSlot` under the slot lock and then sends a payload-free `ReaderMessage::Home` after
the pane's last data, so bytes keep their order and only one side reads the fd at a time.
The message carries no pane because a Crossbeam sender stored inside its own channel is
never freed. On the shard, a direct read turn stops after a short read that hit `EAGAIN`, so
an echo costs two PTY reads. On alienware this removed every gather wake and read from an
echo, cut chatty instructions per second by about 11% and chatty CPU by 11 to 28%; the echo
median moved only 30 to 130 us, less than the gather hop was expected to cost.

The actor consumes at most four batches per turn and
returns each buffer to the pool. A pane whose four buffers are all with its actor leaves the
poll set until one comes back, so a stalled pane does not hold up the others on the shard,
and a full ring still lets kernel flow control backpressure the child. The thread reads every
ready pane in turn, one read per pane per pass, so several busy panes keep their PTY queues
refilling in parallel. A pane that fills a buffer goes on into its next free one. A pane whose
read comes back empty while it holds at least 1 KiB waits in the next zero-timeout poll instead
of being read again, and sends its partial batch after 16 polls that miss it, so a busy pane
neither waits for its neighbours nor holds back their partial batches. Every eight passes that
poll also admits newly ready panes and picks up launches, exits and buffer returns; the thread
blocks in `poll` only when no pane is reading or waiting. `ZZ_PTY_GATHER=0` keeps every pane on its shard.

One thread cannot keep several saturated PTYs as full as one reader per pane. A Linux PTY read
copies out of the 4 KiB line discipline buffer, and a `read` or `poll` that finds it empty
waits in `flush_work` for the kernel worker refilling it, even on a nonblocking fd, so a lone
reader stalls on one pane while the others have data. Asking `FIONREAD` first never waits, but
it costs more than the wait: in a C model on alienware with four `cat` panes, FIONREAD-gated
reads reached 124–152 MB/s, the plain gather 169–205, and one blocking reader per pane
235–284 (two threads 194–215, four 230–244). So when a pane fills a buffer and another pane on
the shard filled one in the last 10 ms, the gather lends every busy pane but one to a thread of
its own, at most three per shard, named like the gather. A lent pane takes its buffers, its
partial batch and its bridge state along, and its lease wakes whichever thread holds it, so
buffer returns and exits still land. The helper hands the pane back after 100 ms without a full
buffer and stays for one more second, so a pane that bursts beside a busy one goes back to the
same thread instead of a new one each burst; only then does it exit, so a quiet shard runs one
gather thread. A helper never lends panes itself, even when its home gather is gone, and a shard
whose home gather stopped starts a new one for its next pane. Single-shard probe, four busy
ASCII panes: 242.5 MB/s, against 247.4 for the per-pane readers before the gather and 177.3 for
the gather alone.

```mermaid
flowchart LR
    child["PTY child"] --> kernel["Linux PTY queue"]
    kernel --> shard["shard poll + direct read<br/>idle and interactive panes<br/>spin 8..16"]
    shard -->|"64 KiB with no 10 ms pause"| gather["zz-pty-gather-N, one per shard<br/>plus one per extra busy pane<br/>poll + nonblocking read<br/>spin 16"]
    gather -->|"Home after 100 ms quiet"| shard
    shard --> actor["zz-terminal actor<br/>vt_write + commands"]
    gather -->|"4 × 64 KiB bounded pool"| actor
    actor -->|"recycle buffer"| gather
    actor -->|"snapshot at 16 ms gate"| render["GUI"]
```

A partial batch below 1 KiB goes to the actor at its first `EAGAIN`. Saturated output gets
16 direct read retries, enough to bridge Linux queue refills without the 512-spin macOS
budget. The shard's direct reads use the same cap: on Linux `PTY_BRIDGE_SPIN_MAX` is
`PTY_GATHER_BRIDGE_SPIN_MAX`, so `bridge_spins` moves between 8 and 16 there; with 512, bursts
written as many small writes cost about 45% more read syscalls. The shard sleeps in `poll` on its wake pipe; the gather marks the pane ready and
writes that pipe at most once until the shard drains it.

## The macOS inline spin bridge (`Wake::PtyReadable` arm)

The constants carry their own justification:

```rust
/// A burst at least this large is a saturated stream, not interactive echo;
/// only then is an empty queue worth bridging with spin retries.
const PTY_BRIDGE_THRESHOLD_BYTES: usize = 1024;
/// Nonblocking read retries that bridge a saturated producer's ~µs kernel
/// queue refill gap before giving up and sleeping in poll(2). Probed on
/// Mac16,5/macOS 27: blocking reads 218 MB/s, poll-per-KiB 136, spin 64/256/
/// 512 → 281/332/348. FIONREAD-gated spins (the 2026-07-27 "7x worse" scar)
/// never observe the refill and are NOT the same thing.
const PTY_BRIDGE_SPIN_MAX: u32 = 512;
const PTY_BRIDGE_SPIN_MIN: u32 = 8;
```

The budget adapts per pane: `bridge_spins` starts at 512, halves on every spin-out (floor 8)
and doubles on every read that a spin bridged. The drain turn itself:

```rust
let mut burst = 0_usize;
let mut spins = 0_u32;
let turn_started = Instant::now();
loop {
    match rustix::io::read(&drain_fd, &mut read_buffer[..]) {
        Ok(0) => { reader_eof = true; break; }
        Ok(length) => {
            terminal.vt_write(&read_buffer[..length]);
            output_pending = true;
            if spins > 0 { bridge_spins = (bridge_spins * 2).min(PTY_BRIDGE_SPIN_MAX); }
            burst += length;
            spins = 0;
            if burst >= PTY_DRAIN_TURN_BYTES             // service commands
                || turn_started.elapsed() >= PTY_DRAIN_TURN_TIME { break; }
        }
        Err(rustix::io::Errno::INTR) => {}
        Err(rustix::io::Errno::AGAIN) => {
            if burst >= PTY_BRIDGE_THRESHOLD_BYTES {
                if spins < bridge_spins {
                    spins += 1;
                    continue;                             // bridge the refill gap
                }
                bridge_spins = (bridge_spins / 2).max(PTY_BRIDGE_SPIN_MIN);
            }
            break;                                        // interactive, or burst over
        }
        Err(_) => { reader_eof = true; break; }           // EIO: slave closed
    }
}
```

The two guards are the whole latency story: an interactive echo (burst < 1 KiB) hits the
first `EAGAIN` and falls straight back to the poll sleep . an idle pane burns nothing .
while a saturated stream stays hot. The turn budget is bounded twice: in bytes
(`PTY_DRAIN_TURN_BYTES`, 256 KiB ≈ 0.8 ms at the release bench's ~320 MB/s cat-ascii
rate) and, since 2026-08-19, in wall time (`PTY_DRAIN_TURN_TIME`, 1 ms). The byte bound
is the fast-path break; the time bound is what the actor's 2 s command budgets actually
assume, and it holds even when the parser runs far below the byte bound's assumed rate —
under loaded parallel test runs with a Zig `Debug` VT build, a single 256 KiB turn was
observed stretching to ~2.8 s (that figure is the dev-profile daemon test environment
under CPU contention, not the release bench), blowing every capture timeout behind it
before the time bound existed.

### Why the budget adapts: the loaded-host collapse (2026-09-28)

The spin only pays while the producer runs on another core. Each 1 KiB drain wakes the
writer (macOS blocks `cat` after every KiB: 153.7k voluntary switches per 150 MiB), and the
refill normally lands within about 5 failed reads. When the host is CPU-saturated (parallel
cargo builds, load 25-90 on 16 cores), the writer waits in the run queue instead. A fixed
512 budget then keeps the actor spinning at 100% of a core on failed reads (about 3.1k
instructions each), which takes a core from the writer it is waiting on, so it waits longer.
The detached 150 MiB ASCII probe fell from ~250 MB/s to 12-15 MB/s and 3-6x the
instructions, below tmux under the same load. The perf gate saw 2 of 36 runs at 12-13 MB/s
and ~24 Ginstr.

Two bounded runs under 20 extra spinning processes, variants alternating within each run:

| drain | MB/s | Ginstr | daemon CPU |
| --- | --- | --- | --- |
| spin 512 fixed | 13.9-15.7 | 52-55 | 7.1-7.6 s |
| spin 64 fixed | 18-19 | 34-37 | 5.3-5.5 s |
| adaptive 8..512 | 35.6-44 | 15.5-16.6 | 2.4-2.5 s |
| no spin | 43-51 | 12 | 1.9-2.0 s |
| tmux 3.7c | 15.6-18.2 | 62 | 5.8-6.0 s |

On a quiet host the adaptive budget costs nothing: 254 vs 256 MB/s median, 8.07 vs 8.06
Ginstr, 40 alternating runs each. No spin at all costs about 9% there. The instruction count
fits `7.4 G + 0.23 G per second of flood (16 ms snapshots) + 3.1k per failed read` over 150
instrumented runs (max residual 0.27 G), so a slow run with 3x instructions is a spinning
run, not a parse problem. Diagnose with `proc_pid_rusage` `ri_runnable_time` minus CPU time:
a starved run shows seconds of run-queue wait for the writer and the daemon.

## The macOS sleep and wake pipe (`wait_for_wake`, `ActorWake`)

The actor sleeps in `rustix::event::poll` on two fds: the pty and a self-pipe. `CommandSender` and the
search worker write one byte to the pipe **after** each successful channel send. `InputSender`
rejects a full command before this step. The actor re-checks its channels before every sleep:

```mermaid
sequenceDiagram
    participant G as GPUI thread (CommandSender)
    participant C as admitted channel
    participant P as wake pipe
    participant A as actor
    A->>C: try_recv . empty
    G->>C: send(command) succeeds
    A->>P: poll begins (about to sleep)
    G->>P: write 1 wake byte (after send)
    P-->>A: poll returns immediately
    A->>P: drain pipe
    A->>C: try_recv → command
    Note over A: the actor sees the accepted send:<br/>either try_recv finds it before sleep,<br/>or the byte ends the poll
```

Control commands retain priority over PTY input and output because the actor checks the control lane
first on every wake. The actor excludes the PTY-input lane from the wait set whenever `PtyWriter` has
a backlog, while PTY reads, resize, capture, and pure view actions keep running. Continuing reads lets
an echoing terminal or a full-duplex child consume input without deadlocking behind its own output.
Output views and non-Unix targets use `ActorWake::none()`: the same call sites, zero cfg noise.

A daemon path can hold these bytes with `hold_actor_wakes()`. Inside the hold, a notify marks its
pipe and returns, and the hold writes one byte per pipe when it ends. A send that finds the
one-command control queue full writes the held bytes before it blocks, and a `request` writes them
after its own command is queued and before it waits for the reply, so neither sleeps behind a wake
its own hold kept back. A hold never
touches the shard's pending flag, so wakes from other threads still go out at once. Attach
(`attach_collect_event_hooks`) and detach (`detach_client_state`) hold, so a shard wakes once for the
resize, view, and stream commands of its pane instead of once per command. The drain stops at the
first short read: a pipe returns every queued byte, so the second read could only return `EAGAIN`.

## The 16 ms gate (bookkeeping moved out of the hot loop)

v2's other mistake was running per-burst bookkeeping per kilobyte. Now `output_pending`
is the only thing the drain sets; the top of the loop owns the rest:

```rust
if output_pending
    && (reader_eof || last_content_publish.elapsed() >= CONTENT_PUBLISH_STALENESS)
{
    drain_effects_if_writer_ready(&effects, &mut writer)?; // pty query replies
    let output_screen = terminal.active_screen()?; // FFI . once per frame now
    /* note_output for every view, reconcile_view_screen, search debounce */
    publisher.publish(snapshot(/* Content */)?);
    last_content_publish = Instant::now();
    output_pending = false;
}
```

Interactive output still publishes immediately (elapsed is ≥ 16 ms when a pane was quiet);
a flood pays for effects flushes, view reconciliation, and the snapshot **once per frame
instead of once per kilobyte**. The gate is also where parse-rate visibility lives: each
drain site accumulates bytes and elapsed µs into a `VtWriteDiagnostics` local (armed only
when `zz_terminal::diagnostics` is at trace, a relaxed atomic check per read) and the gate
emits one `vt_write parsed_bytes= calls= elapsed_us=` line per publish under the
`zz_terminal::diagnostics::vt` target — the only measurement of `vt_write` itself. Note
the arming gate is the parent target, which also arms the per-read raw-byte dump under
`zz_terminal::diagnostics::pty`; to time the parser without drowning in byte dumps, use
`zz_terminal::diagnostics=trace,zz_terminal::diagnostics::pty=warn`. The trade: terminal query replies (DA/DSR) batch up to
16 ms mid-flood when the writer is ready; the actor holds replies while prior bytes drain.
The search-refresh debounce runs from the same deadline sweep. Child status arrives as an event from the per-session
`zz-child-wait` thread parked in `wait()`, so an idle actor sleeps `IDLE_SLEEP` (1 hour)
rather than waking on a status tick. The `Wake::Deadline` arm stays empty; the top of the next actor
turn handles due work, including a 16 ms writer retry.

## The writer (`PtyWriter`)

`O_NONBLOCK` is a property of the open file description, and every dup of the master shares
it . so making the drain fd nonblocking makes the *writer* nonblocking too. `PtyWriter`
wraps a second dup and keeps a FIFO byte buffer. Each `Write` queues the complete slice, tries up to
1 MiB per flush attempt, and retains the remainder on `EAGAIN`; it never reports a silent partial write.
While bytes remain, the actor stops consuming further PTY input but continues PTY output and control work.
The input sender reserves each whole command before enqueueing it, so producers never block behind
the writer. The first 256 commands take channel slots charged at their payload plus a 4 KiB floor;
later ones wait in order in an overflow charged at their payload plus 256 bytes (kept when they reach a slot), and each freed slot
pulls the oldest one in. Only the 64 MiB budget rejects a command, and a rejected command contributes
no bytes. Each input wake writes up to 256 queued commands within the 1 ms drain turn, so a typed
burst costs one read and one published frame per turn rather than one per key. The actor also leaves libghostty-generated replies in the bounded
`PtyEffects` buffer until the writer drains. Symmetric gotcha to remember: you cannot make the reader
nonblocking "privately".

# Why the platforms differ

Ghostty nightly uses a gather pipeline on POSIX. Its implementation adds a 1 ms bridge poll,
a 3 ms batch budget, and an idle pipe that lets the parser interrupt a gather wait. zz uses a
smaller Linux variant: four Crossbeam-owned buffers, spin 16, and no bridge poll. Linux probing
showed that direct retries catch the next queue refill, while sending the current partial batch
keeps terminal-query latency bounded.

macOS keeps the inline actor because its 1 KiB PTY queue made the old per-batch channel hop cost
more than the parse overlap saved. Linux's deeper queue amortizes the channel hop across much
larger batches. The snapshot seam described in [rendering-parity](/terminal/rendering-parity.md)
also keeps both paths independent from GPUI paint locks.

An exact-grid `perf` capture assigns 0.37% of sampled user cycles to `zz-pty-gather`, 44.71%
to the terminal actor, and 54.46% to the GUI. The gather thread spends most of its time in
kernel reads and lets the actor use its core for parsing.

# Traps for whoever touches this next

* **EAGAIN-spin ≠ FIONREAD-spin.** Retrying `read` on a nonblocking fd sees the refill;
  polling FIONREAD does not (19–66 MB/s, catastrophic). Do not "simplify" the bridge into
  an ioctl loop.
* **The spin budgets are platform-specific.** `PTY_BRIDGE_SPIN_MAX = 512` belongs to the
  macOS inline path. Linux uses `PTY_GATHER_BRIDGE_SPIN_MAX = 16`; changing one must not
  move the other platform onto the same topology.
* **Do not add per-read logging or FFI work.** The macOS path pays it per 1 KiB exchange,
  and the Linux gather thread must remain cheaper than the parser it overlaps. The
  `VtWriteDiagnostics` accumulation is the allowed exception: off, it costs one relaxed
  atomic load per read; on, everything still buffers into a local and logs once per 16 ms
  publish.
* **The Zig optimize mode of the VT engine dominates every number here.**
  `libghostty-vt-sys`'s build script checks Cargo's `DEBUG` env before `OPT_LEVEL`, so
  until 2026-08-19 every dev/test build silently compiled the parser at Zig `Debug` —
  roughly 6x slower, which is what caused the daemon's load-flake set (pipe throughput
  1.36 → 7.75 MB/s when A/B'd against `ReleaseSafe`). Dev builds now default to
  `ReleaseSafe` (safety checks kept, optimizer on); release stays `ReleaseFast` via
  `OPT_LEVEL`; `LIBGHOSTTY_VT_SYS_OPTIMIZE` overrides everything for anyone actually
  debugging Zig code. Do not benchmark, profile, or chase timing flakes without first
  confirming which mode the archive was built at. One more scar: the `ReleaseSafe`
  archive built from Ghostty pin `7aa9591` interposed a `memset` whose C ABI was wrong
  (it took the fill as a byte, not an `int` truncated to its low byte), which corrupted
  Rust hash tables in any statically linked binary — deterministic infinite probe loops
  in plain `HashMap::insert`. The pin now sits at the upstream fix (`20c3eae`); if a
  future pin bump resurrects inexplicable hangs in pure-Rust map code, suspect the
  archive's exported libc symbols first. (The 6x parse factor is the research estimate;
  the daemon's own end-to-end pipe test measured 5.7x.)
* **Keep the Linux ring bounded and ordered.** The actor must parse every data batch before
  EOF, return buffers after `vt_write`, and stop draining when all four buffers are in flight.
* **The unicode fixture solicits thousands of DA responses**; their shell echo floods the
  viewport and garbles capture-based smoke tests. Time headless runs with a
  `; touch /tmp/marker` sentinel and wall-clock polling, never `capture-pane | rg`.
* **Windows still runs the v1 reader-thread path** (`cfg(not(unix))`); CI builds it. Keep
  `read_pty` and friends compiling until someone ports the inline drain to overlapped IO.
