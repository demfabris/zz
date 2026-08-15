---
type: Design Plan
title: Scene-streaming remote attach ("mosh for GUIs")
description: Partially retired 2026-08-01. QUIC/zstd/predictor deleted; surviving pieces (history ring, RequestFull, local-scroll, reconnect) plus zz attach are current. Historical record of the scene-streaming campaign.
status: Partially Retired
tags:
- remote
- streaming
- quic
- mosh
- prediction
- roaming
- design-plan
timestamp: 2026-08-15T00:00:00Z
---

# Overview

> **Partially retired 2026-08-01.** The ssh-only consolidation deleted the QUIC transport, so the
> milestones that were *made of* QUIC went with it: **M1** (`quic.rs`, pinned-key mutual TLS,
> `--listen`, `zz quic-identity`, `quic://` hosts), the per-frame unidirectional streams and
> negotiated zstd half of **M2** (the packed terminal lane now always rides one ordered stream,
> uncompressed), **M4**'s predictor (`terminal/predict.rs` and the `predict` mux option), and
> **M5** ([remote browser egress](/designs/remote-browser-egress.md)). What survives is
> transport-independent and still current: **M0**'s `Transport` seam and `Endpoint` (now
> `unix://` + `ssh://`), **M2.5**'s client history ring with `HistoryRequest`/`HistoryChunk`
> backfill and `history-trickle`, **M2**'s per-pane `RequestFull` repair, **M3**'s local-scroll
> overlay and frozen-frame auto-reconnect (`HostState::Reconnecting`). **M6**'s TUI client landed
> 2026-08-09 as `zz attach` ([tui-client](/designs/tui-client.md)); remaining TUI work is that
> design's open rungs, not this one. The premise . stream the renderer-neutral scene, rasterize
> locally . is unchanged; only the pipe is. QUIC returns only if a mobile client ever needs it,
> behind `Endpoint::parse`. Everything below is the design record as of 2026-07-31.
>
> **Status (2026-08-13): partially retired.** QUIC/zstd/predictor/egress-splice are gone. Surviving
> transport-independent pieces and `zz attach` are current. The live wire is protocol v55 over a
> unix socket or `ssh -L`, documented in [wire-protocol](/protocol/wire-protocol.md). Historical
> record of the 2026-07-31 campaign follows. M0 landed 2026-07-16 (commits
> `6e6509c`, `7e5410c`): `ZZ_SOCKET`/global `--socket` overrides, the monomorphized `Transport`
> trait seam in `crates/zz-daemon/src/transport.rs`, the `measure_attach` example, and
> `scripts/remote-attach.sh` . byte-identical attach through a forwarded socket, ~40 µs local
> overhead. **M1 implemented 2026-07-29; QUIC is compiled unconditionally as of 2026-07-31:**
> `QuicTransport` (`crates/zz-daemon/src/quic.rs`, quinn on smol . no tokio) carrying the
> unchanged framed protocol over one reliable bidi stream; **mandatory pinned-key mutual TLS**
> (Ed25519 SPKI fingerprints, client TOFU `known_hosts` keyed by unresolved host:port,
> daemon-side `authorized_clients` allowlist, no unauthenticated QUIC even on loopback);
> opt-in `zz daemon --listen`, `zz quic-identity`, and `quic://` fleet hosts
> ([fleet attach](/designs/fleet-attach.md)); since 2026-07-30 the listener also comes up
> from a `set -g listen <addr>` line in `zz/mux.conf` (auto-spawned daemons included) and
> `zz fleet add` pairs a host over ssh in one command. Version-skew policy as defined at M1: the
> exact-match `PROTOCOL_VERSION` gate stays, surfaced gracefully as the fleet section's
> incompatible row instead of a failure dialog. Idle timeout 10 s + 3 s keepalives bound
> dead-host detection.
> **M2, M2.5, M3, and M4 all shipped 2026-07-31** (protocol v37→v40): per-frame uni streams
> with RESET supersession and per-pane `RequestFull` (command-output frames deliberately ride
> the reliable control stream . they have no RequestFull recovery), negotiated zstd via the
> envelope flags byte (hello capabilities both ways, QUIC-writer-applied ≥1 KiB, unix
> byte-identical), the client history ring + `HistoryRequest`/`HistoryChunk` backfill with the
> `history-trickle` option (invalidation is client-observed total-shrink/column-change . **no
> wire history_epoch was needed**; a per-pane ring-mutation counter guards chunk staleness on
> at-cap panes), the local-scroll latency overlay with debounced `ScrollToOffset` sync, the
> conservative predictor with `predict off|auto|always` (the primary-screen guard is the
> confidence machinery . apps that never echo never render), and auto-reconnect with backoff +
> same-session re-attach + frozen-frame UX (`HostState::Reconnecting`; an attached remote that
> drops no longer falls back to local). The sections below are the design record; the shipped
> behavior is folded into the subsystem concept docs (wire-protocol.md tracks v55). M5's QUIC
> splice was deleted, then reintroduced as ssh `-D` in
> [remote browser egress](/designs/remote-browser-egress.md). M6's multi-client half was
> superseded by [multi-device attach](/designs/multi-device-attach.md); the TUI renderer shipped
> as `zz attach`.

Scene-streaming remote attach lets the [zz daemon](/crates/zz-daemon.md) live on a **remote host**
while GPUI clients attach over the network. Instead of streaming pixels (VNC/RDP: heavy, blurry,
DPI-locked) or raw bytes (ssh/tmux: no local intelligence, replay-on-reconnect), zz streams the
**renderer-neutral scene it already produces**: packed cells, interned styles, split layouts, and
damage. The client rasterizes locally with its own fonts, theme, DPI, and GPU.

```
today:     GPUI client ──unix socket / named pipe / ssh -L── daemon (PTYs, mux, frames)
historical proposal (deleted 2026-08-01):
           GPUI client ──QUIC over the internet──── same daemon, any host
           renders locally                          owns processes durably
```

The product outcome is tmux's persistence + mosh's latency-hiding + local-GPU rendering fidelity
in one tool: typing that feels local at 300 ms RTT (predictive echo), zero-RTT scrollback
navigation (state is synced, scrolling is a local read), wifi→5G roaming (session keyed to a
crypto identity, not a TCP 4-tuple), and instant reconnect after `cat 10GB.log` (latest-state
sync, not byte replay).

# Why zz is pre-adapted

Most of the hard part (a strict split between durable state and disposable renderers) already
exists. This table names each existing piece and its role in the remote design.

| Existing piece | Today | Role in remote attach |
|----------------|-------|-----------------------|
| [`mux` crate](/crates/zz-mux.md) | Renderer-free state machine | Runs unchanged on the remote host; it never knew where it was running |
| [Wire protocol](/protocol/wire-protocol.md) | Versioned, framed, two-lane protocol over a local socket | The wire vocabulary is already location-transparent; only the transport binding changes |
| [Packed terminal lanes](/protocol/terminal-lanes.md) | Hand-packed viewports/patches with dedup dictionaries | Already a compact scene encoding; needs epoching for lossy delivery |
| [Terminal frame](/concepts/terminal-frame.md) | Immutable renderer-neutral snapshots | The unit of state sync . "latest frame wins" is the mosh SSP idea zz already models |
| One-slot frame mailboxes ([OSR rendering](/browser/osr-rendering.md), server fanout) | Keep only the newest frame | Drop-stale semantics generalize directly to a lossy network |
| [Session persistence & detach](/concepts/session-persistence.md) | Closing every window detaches without killing PTYs | A network drop is just an involuntary detach; reattach already reseeds a full snapshot |
| `Event { sequence }` + `Resync` ([wire protocol](/protocol/wire-protocol.md)) | Clients detect gaps and request a fresh snapshot | The recovery primitive for packet loss and migration already exists |
| [Browser pane reattach](/browser/lifecycle.md) | Renderer state is transient; daemon persists last URL/[profile](/browser/profile.md); local CEF respawns on attach | Exactly the remote-browser model: durable facts sync from the daemon, the renderer is always local |
| Envelope `flags` byte ([framing](/protocol/wire-protocol.md)) | Reserved, must be 0 | Earmarked for the compression/checksum bits WAN transport needs |

# Target architecture

## Transport

Factor the daemon/client endpoint (`LocalListener`/`LocalStream`) behind a transport trait with two
implementations: the existing local socket/named pipe, and QUIC (e.g. `quinn`). Lane-to-QUIC
mapping:

| Lane | QUIC mapping | Why |
|------|--------------|-----|
| Control (lane 0) | One reliable bidirectional stream | Commands, snapshots, and events need ordering; head-of-line blocking is acceptable here |
| Terminal (lane 1) | One unidirectional stream **per frame**, per (pane, view): open, write, FIN; RESET a stale in-flight stream when a newer full viewport supersedes it | One slow pane must not block others; the wire-level twin of the one-slot mailbox . and the existing `NeedsFull` coalescing already degrades patch-chains to full-viewport-latest under backpressure, exactly right for a lossy link |
| Browser durable facts | Control lane (they are mux state) | URL/title/profile are small and ordered |

Compression (zstd, negotiated via the reserved `flags` byte / `ServerHello.capabilities`) applies
per-frame; full 200×60 viewports are ~30 KB raw and compress well, damage patches are typically
under 1 KB . frames under ~1 KB skip compression.

**Ordering without ordering (agreed M2 shape).** Patches already carry `base_generation` and
the client already rejects mismatches (`PatchError` → resync). Two additions make loss cheap: a
**dictionary epoch** (u32) . full viewports open a new epoch, patches name theirs, a mismatch
means a dropped ancestor . and a per-pane **`RequestFull`** message so recovery is one pane's
next frame instead of a global `Resync` avalanche. The unix path keeps today's single ordered
stream, byte-identical . the local-regression guard stays a hard rule.

## Identity, auth, roaming

The session is keyed to a crypto identity, not a TCP 4-tuple. QUIC-TLS with pinned keys (SSH-style
trust-on-first-use, or reusing `~/.ssh` keys) authenticates both ends; QUIC connection migration
provides roaming (close laptop, hop networks, reopen: still attached). The daemon's current
trust model is owner-only `0600` sockets; exposing a network listener is a deliberate,
opt-in expansion of that surface and must default to off.

## Predictive local echo (agreed 2026-07-30: conservative overlay, not a VT shadow)

Entirely client-side . zero daemon or wire changes. On a keystroke in a predictable context the
client paints the expected cell(s) and advances a predicted cursor as a render-time overlay on
the retained viewport (distinct understated style) while the input goes out unchanged. Incoming
frames reconcile: a predicted cell matching authoritative content retires invisibly; any
mismatch drops *all* pending predictions for the pane . retraction is flicker-free by
construction. Unconfirmed predictions time out (~1 s) into cautious mode.

**Guards (all must hold):** printable chars plus backspace-of-a-prediction only; primary screen
(alt-screen apps manage their own echo); pane at live tail; not near the right margin (no wrap
prediction); bracketed paste never predicted.

**Confidence, mosh-style:** predictions render only when transport RTT is worth it (~>40 ms;
quinn exposes RTT directly) and only after an initial round-trip confirms the context actually
echoes. On unix/local QUIC the predictor never draws. Config: `set -g predict off|auto|always`.

The original sketch here was a full client-side [libghostty-vt](/terminal/libghostty-vt.md)
shadow seeded from frames. Rejected for v1: seeding an emulator from a cell grid is new
machinery, every libghostty object is deliberately daemon-side, and the overlay's upgrade path
stays open if the guards prove too conservative.

## Scrollback locality (agreed 2026-07-30, new milestone M2.5)

The one dimension where a raw-byte-tee multiplexer structurally beats zz . scroll round-trips .
closed in scene form:

- **The free half.** A patch's `scroll` shift tells the client which rows just left the live
  grid; the client pushes them into a local per-pane ring as they scroll away. Recent history
  accumulates client-side for free, from frames it already receives, on every transport
  including unix.
- **The backfill half.** Pre-attach history pages in via `HistoryRequest{pane, before, count}` →
  `HistoryChunk` (packed rows + dictionary, newest→oldest) on the Control lane. Budgeted eager
  trickle after attach (default ~2 k lines, a config knob), on-demand beyond, prefetch when
  scrolling nears the cache edge.
- **Scrolling.** Cache-warm scroll is a local read . zero RTT. Cache-cold shows a loading
  shimmer and falls back to today's round-trip. Invalidation is one `history_epoch` per pane:
  clear-scrollback/reset bumps it, the client drops the ring.
- **Deliberate v1 cut:** copy-mode and search stay server-backed (they are per-view server
  state and feed copy-pipe); entering copy-mode sends the client's local offset so the server
  view snaps to where the user actually is. Passive reading is local; precision work
  round-trips. Revisit only if it grates.
- History content is per-pane, not per-view, so [multi-device](/designs/multi-device-attach.md)
  clients fill identical rings independently.

## Remote browser panes

The renderer never crosses the network, which is today's reattach model stretched over a WAN:

- **Durable facts** (URL, title, zz profile) live daemon-side and sync over the Control lane.
- **The CEF renderer spawns locally** on the attaching client, restoring those facts.
- **Optional egress via the daemon host**: a SOCKS/CONNECT tunnel over a dedicated QUIC stream
  lets the local renderer browse *from* the remote host's network (internal dashboards, VPN'd
  services) with zero extra setup. Cookie/profile locality is an open question below.

## Degraded clients

Because the scene is renderer-neutral, fidelity tiers fall out: full GPUI client, a TUI client
(`zz attach --tui` rendering mux state into a plain terminal over ssh), and a read-only viewer.
The synced client-side state also permits offline reading of scrollback while detached.

# Milestones

Ordered so each ships something usable and de-risks the next.

| # | Milestone | Scope | Exit criterion |
|---|-----------|-------|----------------|
| M0 | Location transparency over ssh | No new code paths beyond a TCP/forwarded-socket binding; run the existing protocol through `ssh -L` | `zz attach` to a daemon on another machine works; latency/bandwidth measured |
| M1 | QUIC transport + auth | Transport trait, quinn binding, pinned-key TLS, opt-in network listener | Attach over WAN without ssh; version-skew handshake policy defined |
| M2 | Lossy-tolerant terminal lane | Per-frame uni streams, RESET supersession, dictionary epochs, per-pane `RequestFull`, zstd via `flags` | Typing under 5% synthetic loss unaffected; slow pane never stalls others; unix path byte-identical |
| M2.5 | Scrollback locality | Client per-pane ring: scroll-away accumulation, chunked newest-first backfill, prefetch, `history_epoch` invalidation | Recent-history scroll is zero-RTT; cold attach scrolls 10 k lines shimmer-then-fill |
| M3 | Roaming + resilience | QUIC migration, session resumption, involuntary-detach == network-drop semantics | Wifi→hotspot mid-session without losing attach; reconnect is a diff, not a replay |
| M4 | Predictive echo | Conservative overlay predictor, RTT-gated confidence, kill-switch (no VT shadow . see above) | Blind test: typing at 200 ms simulated RTT indistinguishable from local for plain-echo shells; vim untouched |
| M5 | Remote browser panes | Settled in [remote browser egress](/designs/remote-browser-egress.md): per-connection QUIC streams, loopback CONNECT proxy, (profile, egress-host) cookie jars | Browser pane on a remote session reaches a host-internal URL through the tunnel |
| M6 | TUI renderer (stretch) | Multi-client half superseded by [multi-device attach](/designs/multi-device-attach.md); TUI renderer over the same scene remains | `--tui` attach from a stock terminal |

M0 is deliberately boring: it proves the "the protocol is already location-transparent" claim with
near-zero new code and produces the baseline measurements that justify (or kill) everything after.

# Testing (agreed 2026-07-30)

- **Impairment harness**: a UDP proxy in the test harness injecting delay/loss/reorder between
  client and daemon QUIC endpoints . deterministic, CI-friendly, no root/netem; `measure_attach`
  grows `--rtt`/`--loss` knobs so exit criteria are measured, not vibed.
- **Predictor**: pure client logic . table-driven guard matrix plus reconcile
  match/mismatch/timeout cases, no daemon involved.
- **Ring correctness**: property test . random live scrolling plus chunked backfill must
  reconstruct `capture-pane` ground truth exactly.
- **Loss paths**: forced epoch mismatch → `RequestFull` recovery; RESET supersession under a
  stalled stream.

# Hard parts & risks

| Risk | Detail | Mitigation |
|------|--------|------------|
| Version skew | The envelope rejects any `PROTOCOL_VERSION` mismatch; client and daemon on different machines make mismatches routine instead of rare | Define a handshake-time compat policy at M1 (minimum-supported window, or explicit "upgrade the daemon" UX) |
| Security surface | Daemon goes from owner-only local socket to a network listener | Off by default; pinned-key auth only; no password/anonymous mode; consider ssh-only (M0) as the long-term conservative tier |
| Prediction UX | Mispredictions must retract gracefully, not glitch | Conservative prediction scope (printable echo in known-echo contexts first); style predictions distinctly; kill-switch option |
| Cookie/profile locality | Local renderer + remote egress splits the browser's identity across two hosts | Decide at M5: daemon-side profile sync vs local-only profiles; default to local-only |
| Bandwidth spikes | Attach/resync sends full viewports for all visible panes (frames up to `MAX_FRAME_BYTES` = 64 MiB) | Compression from M2; lazy scrollback paging; per-pane prioritization by focus |
| Daemon durability | [Persistence is in-memory](/concepts/session-persistence.md); a remote host reboot still destroys sessions | Out of scope here; an on-disk restoration design would compose but is its own feature |
| Local-path regression | The local attach path must not pay for remote machinery | Transport trait keeps the unix-socket path allocation- and copy-identical; benchmark in CI |

# Non-goals

- **Pixel streaming** of any pane type; the browser renderer is always local.
- **Replacing ssh** for shells on hosts without a zz daemon.
- **On-disk session persistence**: orthogonal (see risk above).
- **Anonymous or password auth**: key-based identity only.

# Open questions

- ~~Does the client link libghostty-vt directly for the VT shadow, or does prediction use a
  smaller purpose-built model?~~ Answered 2026-07-30: neither . a conservative overlay with
  strict guards, no client-side VT for v1; the shadow remains the upgrade path.
- ~~Per-client window sizing for M6: per-client layouts, or tmux-style smallest-client-wins?~~
  Answered 2026-07-31: the latest terminal-input-active viewer owns the whole geometry; ties use
  the lowest `ClientId` . see
  [multi-device attach](/designs/multi-device-attach.md).
- ~~Scrollback sync depth: how much history syncs eagerly vs pages in on demand?~~ Answered
  2026-07-30: ~2 k-line eager trickle (config knob) plus on-demand paging with prefetch (M2.5).
- ~~Where does the QUIC listener config live?~~ Answered at M1 (CLI flag only), then
  absorbed into config on 2026-07-30: `set -g listen <addr>` in `zz/mux.conf` is read once
  at daemon startup (auto-spawned daemons included). Explicit `--listen` still wins and
  keeps its fail-fast preflight; the config path logs and continues serving the unix
  socket on any error, and a reload that changes the value warns instead of rebinding.

# Related

- [Fleet attach](/designs/fleet-attach.md) . the multi-host client composition (one GUI, N
  daemons) that rides this design's M0 seam and composes with M1+
- [Multi-device attach](/designs/multi-device-attach.md) . supersedes M6's multi-client half
- [Remote browser egress](/designs/remote-browser-egress.md) . the settled M5 design
- [Session persistence & daemon lifecycle](/concepts/session-persistence.md) . the detach/attach
  semantics this design stretches over a network
- [zz wire protocol](/protocol/wire-protocol.md) and
  [packed terminal lanes](/protocol/terminal-lanes.md) . the wire substrate being re-bound to QUIC
- [Terminal frame](/concepts/terminal-frame.md) . the immutable scene unit that makes
  latest-state sync possible
- [Browser lifecycle](/browser/lifecycle.md) and [profile](/browser/profile.md) . the
  local-renderer/durable-facts split reused for remote browser panes
