---
type: Design Plan
title: Remote browser egress . ssh -D with zero setup
description: Routing a remote session's browser pane traffic through the attached ssh host - the composite CEF profile points its proxy preference straight at `ssh -D` (socks5-direct), sshd resolves and dials remotely, client-local cookie jars stay keyed (profile, egress-host), and there is zero zz proxy code on either end. Shipped on QUIC (v41), deleted with that transport, reintroduced on ssh and simplified to socks5-direct 2026-08-07.
status: Complete
tags:
- remote
- browser
- quic
- proxy
- fleet
- design-plan
timestamp: 2026-07-30T23:00:00Z
---

# Overview

> **Reintroduced 2026-08-07 on ssh, simplified to socks5-direct the same day.** The shape
> today: CEF's composite profile carries a `socks5://127.0.0.1:<port>` proxy preference
> pointing directly at the `ssh -D` listener carried by the existing `-N -L` forward child
> (`ssh_forward_command` in `crates/zz-daemon/src/endpoint.rs`). Chromium hands hostnames to
> the proxy, sshd resolves and dials them on the host — there is no zz proxy code at all, on
> either end. The port is pre-reserved (`ssh -D 0` never reports its choice); a lost bind race
> is retried once on a fresh port; `Drop` hands both the `-L` and `-D` specs back to the
> shared ControlMaster so reconnects don't stack SOCKS listeners. A reconnect that lands on a
> new port re-applies the CEF preference (`BrowserController::refresh_egress`, best-effort,
> retried by every snapshot refresh). Kill-switch: app-side `browser-egress` config key
> (default on, never crosses the wire). Windows is excluded: its attach path is a bridged pipe
> precisely because unowned loopback listeners are the thing it avoids.
>
> **Why the CONNECT proxy died.** The v41 layer shipped again on 2026-08-07 and immediately
> broke the flagship use case: through an HTTP proxy Chromium sends plain-http requests in
> absolute-form, the proxy forwarded them unmodified leaning on RFC 7230 §5.3.2, and vite
> answers absolute-form with an empty response — `localhost:3000` rendered blank while https
> (opaque CONNECT) worked. Fixing that meant per-request HTTP parsing, body framing, Upgrade
> handling, and cross-origin connection juggling — all to avoid Chromium's SOCKS5 client,
> which the settled table below rejected on the belief that it resolves DNS locally. That
> belief is false for Chromium (curl/Firefox lore; Chromium's socks5:// sends hostnames to
> the proxy — verified against Chromium's proxy docs and by a headless-Chrome render of the
> real vite app through `socks5://` + `<-loopback>`). With the premise gone, the layer's only
> job was gone; it was deleted (−1035 lines). The composite profile, cookie locality, and
> `<-loopback>` override all survive; the settled-decisions table and gotchas below are the
> historical v41 record, superseded where they concern CONNECT-vs-SOCKS.
>
> **Retired 2026-08-01** (later reversed, see above). The ssh-only consolidation deleted the
> QUIC transport this feature was built on, and egress went with it: `crates/zz-daemon/src/egress.rs`,
> `crates/zz/src/browser/egress_proxy.rs`, `BrowserProfilePaths::egress_profile_name` and the
> composite-context/proxy-preference plumbing in `zz-browser`, the `egress-v1` capability, and
> the `browser-egress` mux option are all gone at protocol v43. The return path is
> `ssh -D`: the ssh arm already owns the connection, so a SOCKS/dynamic forward covers the same
> need without a bespoke stream kind. Everything below is the historical record of the shipped
> v41 design, not current behavior.
>
> **Status: implemented 2026-07-31 . both slices shipped** (daemon splice + caps,
> client CONNECT proxy + CEF wiring; `browser-egress` option at protocol
> v41). One design refinement discovered during implementation: CEF proxy preferences are
> per request context (= per profile), so egress runs on a client-local **composite
> profile** `P@egress-<hash8-of-host>` . the composite's context carries the proxy pref and
> its cache path IS the (profile, egress-host) cookie jar; the plain local context is
> untouched and the composite never crosses the wire. Chromium's default loopback bypass is
> overridden (`bypass_list: "<-loopback>"`) so `localhost:3000` tunnels. This was the "M5" slot in the
> [scene-streaming milestone ladder](/designs/scene-streaming-remote.md) and the
> "remote browser egress" bullet in [fleet attach](/designs/fleet-attach.md)'s future work.

Browser panes always render in the client's CEF .
[renderer-is-always-local](/browser/lifecycle.md) is load-bearing. So when the GUI attaches
to `server`'s session, a browser pane pointed at `localhost:3000` resolves on the *client*
machine: the dev server on `server` is unreachable, as is anything behind its VPN or internal
DNS. Egress fixes this by routing the pane's network traffic through the daemon host . `ssh
-D` semantics with zero setup, riding the authenticated QUIC connection that
[pairing](/designs/fleet-pairing.md) already established.

```
client machine                                  daemon host
┌─────────────────────────────┐                ┌──────────────────────┐
│ CEF pane ──► local CONNECT  │   QUIC bidi    │ dial host:port from  │
│ (renders,    proxy listener ├── stream per ──┤ daemon's network,    │
│  cookies,    127.0.0.1:eph  │   connection   │ splice bytes         │
│  GPU local)                 │                │                      │
└─────────────────────────────┘                └──────────────────────┘
```

Only network egress changes. Rendering, compositing, input, and devtools stay exactly as they
are.

# Settled decisions

Settled 2026-07-30; not to be re-litigated without new information.

| Decision | Choice | Why |
|----------|--------|-----|
| Proxy protocol | HTTP CONNECT (+ absolute-form for plain http), not SOCKS5 | Chromium's SOCKS5 resolves DNS *locally* . the classic gotcha that would kill internal-hostname resolution. CONNECT passes the hostname through, so resolution lands daemon-side by construction |
| Wire | One client-opened QUIC bidi stream per browser TCP connection | The connection is already multi-stream (M2); no new listener, no new auth surface . the stream rides the mutual pins from pairing |
| Stream identification | One-byte stream-kind preface (`egress = 1`), then postcard `EgressOpen{pane, host, port}` → `EgressAccept \| EgressRefuse{reason}` → raw splice | Cheap, future-proofs other client-opened stream kinds |
| Prioritization | quinn stream priorities: control > terminal frames > egress | A fat download never lags typing . something `ssh -D` cannot do |
| Client proxy impl | Hand-rolled on smol, ~250 lines | Every proxy crate worth using drags tokio in; zz's QUIC stack is deliberately tokio-free |
| Cookie locality | Client-local, keyed **(profile, egress-host)**, stored under the profile dir with a host suffix | An egress pane is "a browser on that machine": cookies and egress IP travel together (IP-pinned corp SSO behaves), local jars untouched, and CEF's shared-cache-path restriction is sidestepped |
| Target policy | No allowlist | The client already runs arbitrary shell commands on this host; restricting sockets is theater. Bounds instead: 128 concurrent egress streams per client |
| Default | **On** for browser panes in remote sessions; `set -g browser-egress off` kill-switch | `localhost:3000` in `server`'s session meaning `server`'s 3000 *is* the feature. Local sessions never touch any of this |
| Transport | QUIC-only | The ssh-forwarded arm has no stream multiplexing to ride; not worth a bodge now that pairing/QUIC is the mainline path |

# Architecture

## Client side

Per egress pane, a loopback listener on `127.0.0.1:<ephemeral>` speaking HTTP CONNECT for
https/wss and absolute-form for plain http (parse the request line, open the tunnel, replay
absolute-form requests in origin-form down it). The pane's `CefRequestContext` gets its
`proxy` preference pointed at the listener. The listener's lifetime is the pane's.

## Daemon side

Validate the opening client is attached to the pane's session and egress is enabled, dial
`host:port` from the daemon's own network namespace . the point: `localhost`, tailnet-only
names, and corp DNS all resolve *there* . then splice with reused buffers. QUIC flow control
is the backpressure story end to end. Pane close, detach, or host disconnect resets that
pane's streams; the browser shows a normal network error and reload reconnects.

# Gotchas

| Gotcha | Detail |
|--------|--------|
| Chromium HTTP-proxy absolute-form (the CONNECT design's killer) | Plain-http requests through an HTTP proxy are absolute-form; vite and friends answer them with nothing. An opaque-tunnel-only proxy cannot fix this without a per-request rewriting HTTP parser. This is why socks5-direct replaced the CONNECT layer |
| cef-rs `CefString::default()` out-params | `Borrowed(None)` inside, marshals to a NULL `cef_string_t*`; libcef 151+'s C shims null-check out-params and fail the call with a bare `0` — silently, because the error string is the thing they could not write. Broke `set_preference` (and with it every egress pane) on the CEF 150→151 bump, 2026-08-07. Use `with_error_string` in `cef_runtime.rs`; repro/canary: `zz_browser_fixture --probe-egress-pref` |
| Chromium SOCKS5 local DNS | **Disproven 2026-08-07** — this was the reason CONNECT was chosen, but it is curl/Firefox lore: Chromium's socks5:// sends hostnames to the proxy (remote DNS). The one real limit: hostnames >255 bytes fail SOCKS5's DOMAIN field |
| tokio-free constraint | The QUIC stack is quinn-on-smol; any proxy dependency pulling tokio is disqualified regardless of quality |
| CEF shared cache paths | Two request contexts cannot share a cache path, which is why per-(profile, egress-host) jars are the *only* clean cookie shape, not merely a preference |
| HTTP/3 through proxies | Chromium silently falls back to h2/h1 . expected, not a bug |
| WebRTC | May bypass the proxy or fail; documented semantics, same spirit as the fleet clipboard/screenshot quirks |
| Egress ≠ security boundary | The daemon becomes a proxy *for an already fully-trusted client*; the kill-switch exists for policy comfort, not as a security control |

# Testing

The two-daemon loopback QUIC harness with an HTTP server bound to the "remote" daemon's
`127.0.0.1`: a fetch through the pane succeeds while a direct client-side fetch fails.
CONNECT/absolute-form parser unit tests; refusal, cap, and teardown paths; the M2 impairment
proxy reused for behavior under loss.

# Slices

| # | Slice | Exit criterion |
|---|-------|----------------|
| 1 | Wire + daemon splice + caps | Raw test client tunnels TCP through a scratch daemon |
| 2 | Loopback proxy + CEF context wiring + default-on policy | Remote session's browser pane reaches the daemon host's `localhost` service |

# Future work (explicitly not now)

- **UDP relay** (CONNECT-UDP-shaped datagrams) if WebRTC or h3-to-origin ever matters.
- **Per-pane egress toggle in the UI** beyond the config kill-switch.
- **Profile sync across machines** . stays out; local-only jars are the model.

# Non-goals

- **MASQUE conformance** . zz owns both ends; a one-byte preface beats an HTTP/3 framing
  layer.
- **A general-purpose proxy service** . egress exists for browser panes in remote sessions,
  nothing else.

# Related

- [Scene-streaming remote attach](/designs/scene-streaming-remote.md) . the milestone ladder
  (M5) and QUIC transport this rides
- [Fleet attach](/designs/fleet-attach.md) / [fleet pairing](/designs/fleet-pairing.md) . the
  authenticated connection and trust model underneath
- [Browser lifecycle](/browser/lifecycle.md) and [profile](/browser/profile.md) . the
  local-renderer/durable-facts split this preserves
