---
type: Design Plan
title: Fleet pairing and discovery - automatic sidebar pairing
description: Retired design record (2026-08-01) for app-lifetime discovery, contextual sidebar pairing, target-side one-time-code dialogs with a native daemon-only fallback, automatic two-way QUIC trust, and the mDNS-signaled firewall knock.
status: Retired
tags:
- remote
- multi-host
- pairing
- discovery
- quic
- notifications
- sidebar
- design-plan
timestamp: 2026-07-31T00:00:00Z
---

# Overview

> **Retired 2026-08-01.** The whole pairing and discovery surface was deleted in the ssh-only
> consolidation: `quic.rs` (with `quic/pairing.rs`), `discovery.rs`, `knock.rs`, `devices.rs`,
> `crates/zz/src/workspace/pairing.rs`, the `zz-notifications` crate, the Devices settings page,
> `zz quic-identity`, `daemon --listen`, the `listen` mux option, the `pair` mux command, and
> protocol v42's `IncomingPairingRequest`. zz no longer discovers, pairs, advertises, or knocks;
> a remote host is an `ssh://` entry a user adds by hand or through the sidebar's **+ add host**
> dialog, and ssh owns identity and trust. If a QUIC transport ever returns for mobile clients,
> it slots back in behind `Endpoint::parse`, and pairing would be redesigned from scratch rather
> than restored. Everything below is the historical record of the shipped 2026-07-31 design.
>
> **Historical status (2026-07-31): implemented.** The body below is an archive of that
> design, not current product behavior. Current remote attach is
> [fleet attach](/designs/fleet-attach.md) over `ssh://`.

# Archive

zz had one contextual GUI pairing operation: select an unpaired machine that automatic discovery
has placed in the sidebar or Settings -> Devices. There is no Add host, Pair this machine, Pair a
device picker, or discoverability-status workflow.

The user flow is:

1. Run zz. One app-lifetime browser discovers LAN/tailnet peers and projects unpaired machines as
   muted zz-logo + hostname rows such as `arch-desktop`; Settings shows the same rows.
2. Click a row. That click is the pairing intent, so the initiator immediately sends a narrow
   pre-auth pairing request. Its dialog shows the code field in a waiting state while the request
   is in flight.
3. The target daemon generates a two-minute `N-even-word-odd-word` code. If a GUI from that same
   machine is connected over the owner-only local socket, it opens an in-app dialog with the code.
   If only `zz daemon` is running, the operating system's native notification service is the
   fallback.
4. Read that code on the target and enter it on the initiator. The code field receives focus
   automatically and shows the remaining lifetime.
5. Both peers authenticate the exchange, save two-way trust, report persistence to each other,
   and exchange a final committed acknowledgement. Only then does the modal show Connected and
   close.

Every ordinary daemon binds a QUIC listener (starting at UDP 9922 and choosing the next free port)
and persists the actual port. Network discovery defaults on, which advertises that listener and
exposes the narrow request ALPN. The Devices safety switch persists an opt-out shared by the GUI
and daemon: off stops browsing, unregisters mDNS, and removes pairing/request ALPNs while retaining
the authenticated session ALPN used by already-paired devices. Manual `set -g listen`, `--listen`,
SSH fleet bootstrap, and headless trust files remain supported.

# Request and pairing protocols

While discovery is enabled, the `zz-pair-request/1` ALPN accepts only a versioned device name and
returns Ready, Busy, or Rejected. It does not grant session access or expose the PAKE channel. Request
handling has one active code, coalesces repeats from the same requester, rate-limits new requests
per source address, caps concurrent workers, and closes every request connection after a bounded
lifetime. A request is rejected when neither a local GUI nor the native notification fallback can
present the code.

After a request succeeds, the listener temporarily offers `zz-pair/2`. Pairing uses SPAKE2 with
the displayed code, binds key confirmation to quinn's TLS exporter and each role, and encrypts the
identity payload under the derived key. Three failed confirmations disarm the code; a success or
expiry withdraws its notification and removes the pairing ALPN again.

The encrypted identity payload carries the friendly name, the other long-lived Ed25519 role, the
bound daemon port, and the main zz protocol version. The peers exchange persistence results after
writing their registries, and the initiator waits for a target-side `committed` frame. The normal
`zz/1` session ALPN remains trust-gated after TLS negotiation, so making the request endpoint
discoverable does not make terminal sessions public.

# Canonical paired-device registry

GUI pairings live in the versioned, bounded `devices.json` registry under the private QUIC data
directory. One `PairedDevice` stores:

- the peer daemon fingerprint as its stable ID;
- its friendly name and both daemon/client fingerprints;
- one or more observed or selected QUIC routes;
- main-protocol compatibility and paired/updated timestamps.

Writes take a persistent file lock, validate every record, replace atomically, and keep the
directory/file at `0700`/`0600` on Unix. Re-pairing the same client identity updates a rotated
daemon identity instead of creating a second device.

The registry drives all three GUI-pairing consumers:

- incoming session authorization is the union of paired client fingerprints and the legacy
  `authorized_clients` file;
- outgoing sessions pin a matching route to the paired daemon fingerprint instead of TOFU;
- the app merges paired routes into its fleet host list and sidebar.

The manual path stays separate. `known_hosts`, `authorized_clients`, `host-*`, `zz fleet add`, and
the CLI `zz pair` flow are retained for SSH-managed or otherwise headless machines. A manual host
entry does not hide a discovered machine. Removing a device in Settings currently
revokes the local registry record and therefore local incoming and outgoing trust; remote
revocation remains a separate operation.

# Discovery

A bound daemon advertises `_zz._udp.local.` with its friendly hostname, bound port, stable daemon
fingerprint, operating system, and protocol metadata while network discovery is enabled. The `os`
property is optional, so a daemon from before it still decodes. The app owns one browser
for its lifetime and, when the `tailscale` CLI exists, performs a bounded `tailscale status --json`
scan followed by request-ALPN probes of online peers. A tailnet peer advertises nothing, so its
probe tries the listener port already recorded for that machine in the paired-device registry before
falling back to the default 9922; unpaired peers only ever see the default. The browser suppresses an advertisement whose fingerprint
matches the local daemon before it reaches the UI. For a compatible daemon still running from
before fingerprint-bearing advertisements, the local instance plus persisted listener port is a
narrow fallback. A different fingerprint always wins, so two genuinely distinct same-named
machines remain visible. mDNS wins when both sources describe the same machine.

Discovery results are sorted by name and filtered against canonical paired devices, including all
known routes. Unpaired results become muted, non-mux hostname rows with the same zz marker as paired
machines; paired results disappear from that projection because the fleet connector owns them and
connects automatically. Turning Network
discovery off drops the browser and clears those transient rows. No rendezvous server, relay,
internet-wide scan, or daemon-to-daemon federation is involved; the machines must be directly
reachable over their LAN or tailnet.

# Firewall knock

A host firewall that defaults to dropping inbound packets (the `ufw` case) still passes multicast
mDNS, so a machine appears in the sidebar while every QUIC dial to it times out. Dials therefore
carry their own hole punch. A handshake that has not completed after 750 ms publishes a transient
`_zz-knock._udp.local.` record whose instance name is a random nonce and whose TXT carries `target`
(the peer's advertised daemon fingerprint) and `port` (the dialer's own UDP source port). Nothing
about the in-flight connection changes; quinn's ordinary Initial retransmissions are what eventually
get through.

The target daemon browses that service for as long as it advertises itself, so the discovery
opt-out silences both. A record naming its own fingerprint makes it send three constant sixteen-byte
datagrams, a tenth of a second apart, from the QUIC listening socket itself. Sending from that
socket is the whole point: the outbound packets create the conntrack entry that makes the dialer's
retransmissions arrive as established reply traffic. Only private, unique-local, link-local, and
loopback destinations are answered, each destination at most once a second and the daemon at most
thirty bursts a minute, so the payload can never be amplified at anyone. The datagrams are never
parsed or acknowledged in either direction.

A dial that only succeeded after knocking sets `needs_knock` on that device's `devices.json` record,
and later dials to it publish the record immediately instead of waiting out the stall timer. A
handshake fast enough that a knock cannot explain it clears the flag again, so a fixed firewall
stops costing anything.

Dials that still fail are classified rather than surfaced raw: an unanswered handshake gets one
two-second request-ALPN probe, which separates a blocked port from a reachable daemon that refused
the dial, and a reset means nothing owns the port. The app turns that classification, the peer's
advertised `os`, and whether discovery can currently see the machine into one actionable line plus a
concrete command for the pairing dialog; the sidebar itself stays compact and shows only a failure X.

# Native notifications

`zz-notifications` is a reusable producer-neutral crate, not pairing-specific UI. Producers send a
`NotificationSpec` with a stable ID, category (`Pairing`, `Agent`, or `Terminal`), title, body,
urgency, and optional expiry. `NotificationCenter` owns native delivery, replacement, withdrawal,
capability reporting, and a recording backend for integration tests.

- macOS uses `UNUserNotificationCenter`, can request authorization from Settings, presents banners
  while the app is foregrounded, and offers System Settings when permission was denied;
- Linux uses the XDG desktop notification service over D-Bus; there is no standard permission
  prompt, so Settings reports a missing service and offers to check again;
- Windows uses native toast delivery (replacement and programmatic withdrawal remain system-owned
  with the current backend).

Native delivery is the daemon-only fallback. When the target app is open, the daemon sends an
`IncomingPairingRequest` event only to interactive clients on its owner-only local transport whose
device name matches the daemon's machine. Remote QUIC clients, SSH-forwarded clients from another
machine, and command clients never receive the one-time code. If no local GUI accepts the event,
pairing refuses to arm when native delivery also fails, so the initiator receives an actionable
error instead of waiting for a code the target user cannot see.

# UI and lifecycle

Clicking a discovered row immediately opens the dialog in Requesting, then it moves through Awaiting
code -> Pairing -> Connected. The code field is visible but disabled during Requesting; Ready enables
and focuses it, and the footer exposes Connect and Cancel. Request failures and expiry keep the
selected device in place with a visible Retry action. The dialog refreshes the countdown once per
second and closes shortly after bilateral success. On the target, an incoming local pairing event
pushes a second dialog showing the large monospace code and its own countdown; it can surface even
while that app is viewing another host. The initiator republishes its merged fleet immediately; an open
target GUI detects the daemon-written registry revision through its existing 500ms settings poll,
so both sidebars update without an app restart.

Every machine uses the same sidebar identity treatment: the flat zz mark followed by its hostname.
The local machine alone expands into its session/window/pane tree; remote paired machines stay one
row and switch to their daemon-selected current/default session when clicked. Connected rows are
quiet, connecting/reconnecting/loading rows show a spinner, and failed rows show an X. Discovered
unpaired rows use the same treatment at muted opacity and open pairing when clicked.

Settings -> Devices contains the Network discovery safety switch, live nearby-device rows, local
device identity and actionable notification readiness, canonical paired-device rows with local
Remove confirmation, and collapsed technical details for routes and trust fingerprints. Legacy
manual fleet hosts are not presented as GUI pairings.

# Verification

- Pure tests cover PGP-code parsing, SPAKE2 confirmation and exporter mismatch, gate expiry and
  three-failure disarm, registry upsert/rotation/removal, discovery ordering/filtering, durable
  discovery opt-out, compact machine-row/sidebar projection, visible custom-dialog actions, local-only pairing-code
  fan-out, target-app delivery while viewing another host, and reusable notification
  replacement/withdrawal.
- Knock coverage is loopback-only: the record's TXT round trip, the responder refusing another
  daemon's fingerprint and any public address, both rate limits, and a burst that leaves the
  listening socket's own source port without disturbing a live dial. A real dropped-inbound firewall
  is still a manual acceptance step.
- Ignored loopback integration tests cover request -> delivered code -> pairing -> notification
  withdrawal, PAKE -> bilateral registry persistence -> authenticated QUIC session, and mDNS.
- The release handoff requires the full daemon/app/UI suites plus `just build mac`, which builds,
  bundles, signs, and verifies the macOS app.

# Future work

- Refresh and rank multiple saved routes as network conditions change.
- Add authenticated remote revocation so Remove can become two-sided when the peer is reachable.
- Carry the same notification service into Agent completion/approval and terminal activity events.
- Reuse the pairing channel for QR-based mobile pairing.

# Related

- [Fleet attach - one GUI, every daemon](/designs/fleet-attach.md) - host registry, sidebar attach,
  and the retained manual SSH path
- [Scene-streaming remote attach](/designs/scene-streaming-remote.md) - QUIC transport used after
  trust is committed
- [zz-daemon](/crates/zz-daemon.md) - request endpoint, trust registry, discovery, and pairing
- `zz-notifications` - the reusable native delivery crate this design introduced; deleted
  2026-08-01 with everything else here, so it has no concept doc any more
