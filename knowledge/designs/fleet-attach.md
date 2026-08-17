---
type: Design Plan
title: Fleet attach . one GUI, every daemon
description: Design and v1 record of one GPUI client aggregating sessions from every machine running a zz daemon - host-keyed connections, a merged multi-host sidebar tree, ssh-forwarded sockets behind a transport-agnostic endpoint seam; daemons never talk to each other.
status: Complete
tags:
- remote
- multi-host
- client
- ssh
- choose-tree
- design-plan
timestamp: 2026-07-29T18:28:47Z
---

# Overview

> **Status: v1 implemented (2026-07-29), unix + ssh arms.** Landed as designed with zero
> daemon/wire changes: `Endpoint`/`connect_endpoint` in `zz-daemon`'s client half
> (`crates/zz-daemon/src/endpoint.rs`), `host-<name> = <uri>` config keys, the
> `HostConnection` map + remote connect machinery in `crates/zz/src/mux/`, and the
> client-composed fleet section in choose-tree. One deviation from the plan below: the
> "manual `reconnect` command" first became automatic fallback-to-local on attached-host
> disconnect, and scene-streaming M3 (shipped 2026-07-31) then replaced that too . an
> involuntary remote disconnect now freezes the last frame and auto-reconnects with backoff
> to the same session (`HostState::Reconnecting`); fallback-to-local remains only for
> voluntary host removal (see Failure handling). Rides the M0 transport groundwork from
> [scene-streaming remote attach](/designs/scene-streaming-remote.md); the QUIC arm (M1)
> shipped 2026-07-29 and is compiled unconditionally as of 2026-07-31. **Setup became one command on
> 2026-07-30**: `zz fleet add <name> <ssh-host>` bootstraps the QUIC trust root over ssh
> (fingerprint exchange both ways, `known_hosts` pin, remote `authorized_clients`, remote
> mux.conf `set -g listen`, local `host-*` line), and daemons consume `set -g listen <addr>`
> from `zz/mux.conf` at startup . auto-spawned daemons included . so no manual
> `zz daemon --listen` ordering exists anymore. **The chooser-based fleet surface and the
> ssh-first setup are being superseded**: [fleet pairing & discovery](/designs/fleet-pairing.md)
> moves the fleet section into the persistent sidebar, replaces the ssh bootstrap with a
> short-code pairing flow, and adds mDNS/tailnet discovery.
>
> **2026-08-01 . ssh-only consolidation; the paragraph above is reversed.** Pairing, discovery,
> knock, and the QUIC transport were deleted; [fleet pairing](/designs/fleet-pairing.md) is
> retired. This design is now the whole remote story, and it got simpler than v1: the sidebar
> renders one merged tree in which *every* host . local and remote alike . expands into its own
> sessions/windows/panes, and clicking a session attaches that session rather than the daemon's
> default. Every host row carries an ellipsis menu . **Add host** on the local row, **Close host**
> on a remote one . which is the GUI half of `zz fleet add <name> <ssh-destination>` and
> `zz fleet remove <name>`: both parse with `Endpoint::parse`, both validate with
> `config::validate_fleet_host`, and both write one `host-<name> = ssh://[user@]host[:port]`
> line (the dialog takes only the destination and names the entry after its host). There is
> no bootstrap, no `--port`, no `host-setup`, no `known_hosts` pin, and no `set -g listen`:
> `zz fleet add` writes one config line, `zz fleet list` prints name + endpoint, and
> `zz fleet remove` deletes the line. Close host is never gated on connectivity . an unreachable
> machine is the one you most need to remove . and it hands the attachment back to local first,
> because `reconcile_hosts` retains an attached host's connection until detach. `Endpoint` is `unix://` + `ssh://` only, and `quic://` is
> once again reserved rather than implemented.

Fleet attach turns the GPUI client into a **multi-daemon client**: the window on whatever machine
you sit at aggregates sessions from every machine you own that runs a zz daemon. "Two-way" is
free by symmetry . every machine runs the same daemon + GUI, so machine B aggregating machine A
is just A appearing in B's host list. Daemons never learn about each other; there is no
federation, no mesh, no daemon↔daemon protocol.

```
laptop GUI ──┬── unix socket ──────── laptop daemon   (implicit "local" host)
             ├── managed ssh -L ───── desktop daemon
             └── managed ssh -L ───── server daemon
```

The persistent sidebar is the cross-machine surface. Every daemon is one zz-logo + hostname row and
every machine can expand into session/window/pane navigation; only the local tree auto-expands when
it first joins the fleet. Clicking a machine row attaches its daemon-selected current/default
session, lazily creating numeric `0` if that daemon is empty, while clicking a descendant attaches
that exact owning session before selecting it. One host's session is on screen at a time.

# Scope decisions

Settled up front; not to be re-litigated without new information.

| Decision | Choice | Why |
|----------|--------|-----|
| Aggregation actor | GUI only (v1) | CLI/agents keep talking to their local daemon; cross-machine programmatic targeting is future work with two candidate shapes (below) |
| Mixing granularity | Per-session | Closest to today's one-session attach; the attach flow grows a host dimension instead of the pane/tab model growing one |
| Transport | Transport-agnostic endpoint seam; unix + ssh arms implemented, `quic://` reserved | ssh already owns auth/encryption/identity; M0 proved the protocol is location-transparent over a forwarded socket; QUIC slots in behind the same seam at M1 |
| Remote pane kinds | Terminal + browser | PTY-on-remote is the point; browser panes are already client-local by design. Agent/editor panes are behind the experimental flags for the zz v1 launch, and the GUI never pushes those two config keys to remote hosts, so remote daemons keep them off . the daemon-side hard gate enforces it with no new code |

# Architecture

## Host model

- **`HostId`**: a small client-local integer keyed by the config `host` name. `MuxClient` grows
  from one connection to a `HashMap<HostId, HostConnection>`.
- **`HostConnection`** is today's connection state unchanged . stream, reader thread, sequence
  tracking, resync logic. Per-host isolation means one slow or dead host cannot corrupt another
  host's event stream.
- **The local daemon is just `host local`** with a `unix://` endpoint, implicit and always
  present. The only local-host special case: auto-spawn-on-dial-failure applies to the local host
  *only*. A remote outage must never spawn a useless local daemon (today's
  `connect_or_spawn_daemon` would).
- Everything above the connection layer keys by `(HostId, SessionId)` and friends. The wire
  format, message vocabulary, and `PROTOCOL_VERSION` are untouched . which is why this ships
  before M1.

## Endpoints and config

```
# zz/config
host-desktop = ssh://desktop
host-server  = ssh://fabrico@server.tail1234.ts.net
# reserved for M1:
# host-gpu   = quic://gpu:7777
```

Host entries keep the file's strict `key = value` grammar . `host-<name>` is a client-reserved
prefix matched before the `ConfigKey` enum, never forwarded to any daemon. The endpoint is an
opaque URI parsed into `Endpoint::Local(PathBuf) | Endpoint::Ssh(SshEndpoint)`, with `Quic`
reserved. The enum lives in **`zz-daemon`'s client half** (the crate already ships
`InteractiveClient`; new public API is `Endpoint` + `InteractiveClient::connect_endpoint`) so
the `Transport` traits stay crate-private. The **ssh arm**: on first use, spawn a managed
`ssh -N -L <scratch>/zz-<host>.sock:<remote_sock>` child (the `scripts/remote-attach.sh` recipe,
`StreamLocalBindMask=0177`, `umask 077`), then dial the forwarded path with the existing
`LocalTransport`. Auth, encryption, and host identity are entirely ssh's problem . existing keys,
agent, and `~/.ssh/config` aliases just work. The ssh child's lifetime is owned by its
`HostConnection`; child exit transitions the host to disconnected.

## Attach flow and routing

- Configured hosts connect in the background at GUI boot. Each connection keeps its latest snapshot
  even while another host is attached, which lets the merged tree, bell flags, and connection state
  remain live without switching machines. Registration and resync do not create a session on those
  background daemons.
- The sidebar projects every daemon's session/window/pane hierarchy under its machine row. Clicking
  a machine row asks that daemon to select its current/default session, or lazily create the next
  numeric session if it is empty; clicking any descendant attaches its owning session before
  selecting it. Clicking the local hostname while viewing remote
  switches back symmetrically. A spinner represents connecting/reconnecting/loading, an X
  represents failure, and connected hosts carry no status label. Only the local tree auto-expands;
  remote trees remain user-controlled.
- Attached-session routing (input, prefix commands, resync requests, status line) is a one-field
  lookup on the attached host's connection . routing already flows through `MuxClient`.
- The one-client-per-session rule was **lifted 2026-07-31** by the shipped
  [multi-device attach](/designs/multi-device-attach.md): `SessionAlreadyAttached` no longer
  exists; any number of the user's devices co-attach with per-client views, focus, and
  presence, and `attach-session -d` steals.
- Config overrides: the client pushes its `zz/config` daemon-key overrides
  ([layered configuration](/designs/layered-config-and-settings-view.md)) to whichever host it
  attaches, minus the two experimental pane-kind keys. Terminal font names are resolved against the
  GUI machine that renders them: a daemon platform default becomes the GUI platform default, while
  an explicit stack keeps only locally installed families and falls back locally when none remain.

## Version skew

v1 keeps the exact-match `PROTOCOL_VERSION` gate. An incompatible daemon remains isolated to its
machine row and shows the same failure X as other connection failures; it cannot block other hosts.
No compat window yet: the fleet builds from one repo, and a negotiation layer now would be
speculative. The seam for later already exists (`ServerHello.capabilities`). The one discipline
adopted now: version bumps stay honest . any wire change bumps . so "same version = safe" stays true.

## Failure handling

- Host states: `Disconnected → Connecting → Connected | Reconnecting(attempt) |
  Unreachable(reason) | Incompatible(local, remote)`. The sidebar reduces these to three visual
  states: quiet when ready, a spinner while in flight, and an X after failure.
- ssh child exit or stream error while attached → the host enters
  `HostState::Reconnecting` (scene-streaming M3, shipped 2026-07-31): the client freezes
  the last frame, retries with 1/2/4/8/16/30 s backoff, and re-attaches to the last
  attached session; session state survives daemon-side. This superseded both the
  originally planned manual `reconnect` command and the v1 automatic
  fallback-to-local . the attached host retries indefinitely (unattached hosts stop after
  3 attempts); automatic fallback happens only on voluntary host removal. The escape hatch
  for a host that never returns is the sidebar: the local connection is redialed
  non-attached on host switch, so the local hostname row stays clickable throughout. Abrupt LOCAL
  daemon death (kill -9) still surfaces the disconnect banner (the `ServerStopping`
  presentation path).
- Remote dial gets its own timeout budget (seconds, not the local 3 s spawn-poll). **Reversed for
  the ssh arm on 2026-08-01**: `SshForward::start` now probes the resolved remote socket and, if it
  is absent, starts `zz daemon --socket <path>` detached over the same ssh exec (login-shell PATH),
  mirroring local auto-spawn; ssh failures classify into actionable `HostState::Unreachable`
  reasons (login rejected / zz not installed / socket never appeared). The QUIC arm still never
  auto-spawns.

# Testing

- The multi-host logic is transport-independent: core tests run **two local daemons on two
  scratch sockets** posing as two hosts . choose-tree union, attach/detach, per-host failure
  isolation, no ssh required, CI-friendly.
- ssh arm: unit-test the forward-command construction; smoke via `ssh localhost` reusing the
  `remote-attach.sh` recipe (manual/opt-in, like the M0 proof).
- `measure_attach` grows a `--host` flag so attach-latency baselines exist per endpoint type.

# Future work (explicitly not v1)

Recorded so later phases inherit intent, ordered roughly by expected sequence.

- **QUIC arm (M1)**: shipped 2026-07-29, then **deleted 2026-08-01** with the ssh-only
  consolidation. `Endpoint::parse` rejects `quic://` and points at `ssh://`. See the
  [scene-streaming status block](/designs/scene-streaming-remote.md). `zz fleet add`
  still adds an `ssh://` host.
- **Pairing without ssh**: designed in [fleet pairing & discovery](/designs/fleet-pairing.md)
  . SPAKE2 short codes over the QUIC listener, two-way trust per pairing; the QR/mobile
  variant rides the same channel.
- **Steal-attach**: tmux-style "detach the other client and take the session" from the
  attached-elsewhere row. Falls out of [multi-device attach](/designs/multi-device-attach.md)
  as attach + detach-others.
- **Auto-reconnect**: subsumed by M3 (QUIC migration + session resumption); do not build a
  bespoke backoff loop before then.
- **Cross-machine CLI/agents** . the deferred half of "machines operate each other": a script or
  agent on the desktop addressing a server pane (`zz -t server:dev.1 send-keys`) with no GUI
  involved. Two candidate shapes, deliberately undecided: (a) the CLI learns the `host` config
  and dials remote endpoints directly . no daemon changes, mirrors the GUI; (b) daemons relay for
  their local clients . heavier, but keeps agents' environment untouched. Known gotcha either
  way: the GUI injects the *client-local* `ZZ_SOCKET` into agent children, so an agent's
  `zz send-keys` today always targets the client's own daemon.
- **Mobile thin clients**: scene streaming makes a phone client a rasterizer, not a port . the
  protocol crate is renderer-neutral and postcard is embedded-friendly, so a client is QUIC dial
  + terminal-codec decoder + Metal/Skia rasterizer + a keyboard. Daemon holds all state; a client
  killed by the OS just re-attaches (`Resync` is the recovery primitive). Natural first tier is
  the read-only viewer from the [degraded clients](/designs/scene-streaming-remote.md) ladder .
  glance at agents from a phone . then interactive. Browser panes fit as-is: "renderer is always
  local" means WKWebView restoring the same durable facts. Hard prerequisite: M1 (mobile cannot
  sanely manage `ssh -L`).
- **Remote browser egress (M5)**: today a remote session's browser pane renders in the client's
  CEF, so its connections originate from the client machine and host-internal URLs
  (`localhost:3000`, VPN'd dashboards) fail. M5 adds an optional SOCKS/CONNECT tunnel over a
  dedicated QUIC stream inside the authenticated zz connection . `ssh -D` with zero setup,
  scoped to the pane. Cookie/profile locality settled 2026-07-30: client-local jars keyed
  (profile, egress-host) . full design in
  [remote browser egress](/designs/remote-browser-egress.md).
- **Granularity evolution**: per-tab or per-pane host mixing composes on top of the same
  `HostConnection` map if per-session ever feels limiting; nothing in v1 forecloses it.

# Non-goals

- **Daemon↔daemon protocol** of any kind . no federation, discovery, or relay.
- **Inventing auth**: ssh owns identity until M1's pinned keys; no password/anonymous mode ever.
- **Version-compat negotiation**: exact match, presented gracefully.
- **Fixing cross-machine path quirks**: `browser screenshot <path>` writes to the GUI's disk,
  clipboard/`copy-pipe` split across machines, `#()` status commands expand daemon-side
  ([status line](/tmux/status-line.md)) . documented semantics, not v1 bugs.

# Open questions

- Which `zz/config` daemon keys should push to *remote* hosts on attach? Local prefs as
  overrides is the layered-config "remote-attach-clean shape", but some keys are arguably
  per-host, and two clients pushing different overrides is last-writer-wins today
  ([layered configuration](/designs/layered-config-and-settings-view.md)).
- Host identity across config edits: the `host` name keys `HostId`; what happens to a live
  connection when its endpoint line changes or disappears on config reload?
- ssh child management: reuse `ControlMaster` when the user's ssh config has it? How is a
  silently-stalled forward (ssh alive, tunnel dead) detected . TCP keepalive equivalent,
  `ServerAliveInterval`, or protocol-level ping?

# Related

- [Scene-streaming remote attach](/designs/scene-streaming-remote.md) . the transport groundwork
  (M0) this rides and the milestone ladder (M1–M6) it composes with
- [Layered configuration & native settings view](/designs/layered-config-and-settings-view.md) .
  the override push channel that gains a per-host dimension
- [zz wire protocol](/protocol/wire-protocol.md) . unchanged by this design; the point
- [Session persistence & detach](/concepts/session-persistence.md) . the attach semantics that
  grow a host dimension
- [Browser lifecycle](/browser/lifecycle.md) and [profile](/browser/profile.md) . why browser
  panes are remote-ready as-is
