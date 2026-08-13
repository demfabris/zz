---
type: Concept
title: Named zz profiles & persistent request contexts
description: Named zz-owned CEF profiles isolate browser state on the client's disk, and explicit Chrome cookie and history import uses bounded read-only snapshots.
resource: crates/zz-browser/src/profile.rs
tags: [browser, profile, cef, request-context, cookies, security]
timestamp: 2026-08-01T00:00:00Z
---

# Overview

Every zz browser session runs against one **named, private, persistent CEF
profile** owned by zz, never a Chrome, Chromium, or Edge live profile.
`profile.rs` resolves and locks the on-disk paths (`BrowserProfilePaths`), while
`cef_runtime.rs` keeps one `RequestContext` per profile name. Sessions with the
same name share cookies, cache, local storage, and other site data; sessions with
different names are isolated. A user-invoked import may read ephemeral,
read-only snapshots of one selected Chrome profile's cookie and history databases,
but CEF is never pointed at those databases and zz never modifies them. Browser
storage/credential material is never logged.

# Path layout

`resolve_profile_paths()` builds a two-level layout under the platform
application-data directory:

```text
<platform data dir>/zz/browser/root/          <- root  (CEF root_cache_path)
<platform data dir>/zz/browser/root/zz-default <- default profile
<platform data dir>/zz/browser/root/zz-profile-<hex> <- named profile
<platform data dir>/zz/browser/recent-pages   <- app-owned browser history list (not read by CEF)
<platform data dir>/zz/browser/chrome-profile-metadata.json <- sanitized chooser labels
```

Every profile is an **immediate child** of `root` because CEF's Chrome browser
context only accepts persistent profiles that are direct children of
`root_cache_path`. The default keeps its historical `zz-default` directory;
other UTF-8 names are hex-encoded after `zz-profile-`, so separators and `..`
cannot escape the root. Names are trimmed, limited to 64 bytes, and reject
control characters; legacy `zz-default` descriptors canonicalize to `default`.
Per-platform `platform_data_dir()`:

| Platform | Data directory | Resulting profile |
| --- | --- | --- |
| Linux | `$XDG_DATA_HOME` or `~/.local/share` | `.../zz/browser/root/zz-default` |
| macOS | `~/Library/Application Support` | `.../zz/browser/root/zz-default` |
| Windows | `%LOCALAPPDATA%` | `...\zz\browser\root\zz-default` |

# Schema . `BrowserProfilePaths`

| Field | Type | Role |
| --- | --- | --- |
| `root` | `PathBuf` | CEF `root_cache_path`; parent of every persistent profile. |
| `profile` | `PathBuf` | The backward-compatible `zz-default` cache path used by the `default` profile. |

`ensure()` prepares `default`; `ensure_profile(name)` canonicalizes a name,
creates its immediate-child directory (and thus `root`), then applies
`restrict_to_current_user` to both: `0o700` on Unix
(`fs::Permissions::from_mode`), a no-op on non-Unix. Name and I/O failures
surface through `BrowserProfileError` / `BrowserError::Profile`.

# Persistent request context

State isolation happens at two layers, both with `persist_session_cookies = 1`:

1. **Global** (`Settings` in `bootstrap_args`): `root_cache_path = root`.
2. **Per-profile**: a `RequestContext` created with
   `RequestContextSettings { cache_path = profile_path, persist_session_cookies: 1 }`
   and a `ProfileRequestContextHandler` that fires profile-tagged
   `RuntimeSignal::RequestContextInitialized` when CEF reports the context ready.

The default context is created **after** CEF global init (on
`ContextInitialized`) and gates `RuntimePhase::Running`. Additional contexts are
created lazily by `ensure_profile_context` when a pane first requests that name;
the controller retains the pending pane and pumps CEF until its tagged callback
arrives. `BrowserRuntime` holds every context in `profile_contexts`, and each
`BrowserSession` clones and retains its selected context (`_request_context`).

# One jar per profile, always local

Every browser pane renders in the client's CEF, so a pane in a remote session
resolves `localhost:3000` on the **GUI machine**, not the daemon host. That is a
documented consequence of renderer-is-always-local, not a bug: reach a remote
host's internal services with an ordinary `ssh -D` SOCKS forward.

zz used to paper over it with a **composite egress profile**
(`<profile>@egress-<hash8>`) whose CEF request context carried a proxy preference
and whose cache path was a per-(profile, egress-host) cookie jar. That, the
`egress_profile_name`/`ensure_egress_profile` pair, and the tunnel behind them
were deleted on 2026-08-01 with the QUIC transport (see
[remote browser egress](/designs/remote-browser-egress.md)). A profile name now
means exactly one directory and one jar, wherever the pane's session lives.

# Selecting a profile

- `new-browser -p Work [URL]` and `split-browser -p Work [URL]` persist `Work` in
  the pane's `BrowserDescriptor`.
- The browser menu displays the current profile. Its **Switch profile** submenu
  offers the default zz profile plus stable Google Chrome profiles detected from
  Chrome's `Local State` metadata; selecting an entry executes
  `set-browser-profile -t <pane> <name>`. The daemon updates the descriptor and
  only that pane is recreated against the selected context.
- Each detected identity maps deterministically to a zz-owned name of the form
  `chrome:<Chrome storage key>`. Selecting it creates that separate zz profile on
  first use; returning to it restores only the state accumulated in zz.
- **Import Chrome data** reuses the detected identities as read-only source
  profiles without switching the pane. The pane's current zz profile remains the
  cookie destination, and imported history remains app-owned rather than entering CEF.
- Discovery begins in the background when the browser view mounts, not while a
  popup menu is open. Failures retain any sanitized cached labels and expose
  **Retry Chrome profile discovery**; zz does not replace the OS permission flow
  with a file chooser.

## Installed Chrome discovery

`crates/zz-chrome-import/src/profiles.rs` refreshes the chooser once in the background
when the browser view mounts. It reads at most 16 MiB from the stable Google
Chrome `Local State` file for the current OS:

| Platform | Metadata path |
| --- | --- |
| Linux | `$XDG_CONFIG_HOME/google-chrome/Local State` or `~/.config/google-chrome/Local State` |
| macOS | `~/Library/Application Support/Google/Chrome/Local State` |
| Windows | `%LOCALAPPDATA%\Google\Chrome\User Data\Local State` |

Only `profile.info_cache` display metadata and `profile.profiles_order` are
used. Stable signed-in and signed-out entries are included; entries with
`is_ephemeral = true` are omitted. Storage keys, labels, and generated profile
names are bounded and reject separators, traversal, and control characters.
Discovery itself never opens a Chrome profile directory, cookie/history/login
database, or platform credential store, and it never logs detected names or
email addresses.

On macOS, the path is derived with `NSHomeDirectory()` rather than the shell's
`HOME`, so app-bundle launches resolve the actual user Library consistently.
macOS protects Chrome's app-data container through TCC and may require the user
to approve access. A permission denial tells the user to enable zz under
**System Settings → Privacy & Security → Full Disk Access**, then quit and reopen
the app. Local bundles prefer a stable Apple Development signature so that grant
survives rebuilds; an ad-hoc signature cannot be recognized as the same responsible
code after its hash changes. That consent prompt has no bypass.
zz writes only the sanitized profile name, generated zz profile name, and optional account label to
`chrome-profile-metadata.json`. The cache is versioned, bounded to 128 KiB,
revalidated and deduplicated on load, capped at 64 entries, and set to mode
`0600` on Unix. Opaque GAIA IDs and the source JSON are never retained. If
direct discovery is denied later, cached labels remain useful while Retry reruns
discovery after the user changes permission.

## What is isolated / persisted

- Persisted in the selected profile: ordinary cookies (including session
  cookies), HTTP cache, and local storage, proven by the local fixture's persistent
  cookie/local-storage counters.
- Persisted beside the root (never read by CEF): `recent-pages`, the app-owned
  plain-text browser history list (`crates/zz/src/browser/recent_pages.rs`, one
  `unix-seconds\turl\ttitle` line per entry, newest first, capped at 5,000) that
  backs the browser pane's blank-page empty state as up to eight inline, washed URL
  rows without an enclosing card. Only `http(s)` URLs are recorded. Chrome history
  imports merge by URL and keep the newer visit; Chrome's database is read through
  a snapshot and never written.
- Persisted beside the root (never read by CEF): the bounded, user-only Chrome
  profile-label cache described above. It contains no Chrome browsing state or
  credential material.
- Explicit **Import Chrome data** chooses a detected Chrome source profile, then
  snapshots its cookie SQLite database plus available WAL/SHM sidecars into a
  user-private temporary directory bounded to 512 MiB. Every usable unpartitioned
  cookie is normalized in bounded chunks without a 10,000-row profile truncation,
  deduplicated by name/domain/path (later expiry wins), and written to the pane's
  current zz profile. Encrypted values are unlocked through macOS Keychain or Linux
  Secret Service; those services may show their own consent prompt.
- The same explicit action snapshots Chrome's `History` database read-only and
  merges the newest 5,000 HTTP(S) URLs into `recent-pages`, deduplicated by URL.
  Cookie and history snapshots, decrypted values, and derived keys are discarded
  after the operation; no raw rows or values enter diagnostics.
- Explicit cookie-file import accepts Cookie-Editor JSON and Netscape
  `cookies.txt`, normalizes supported records in `cookies.rs`, writes them only
  through CEF's `CookieManager`, and flushes the profile store. Invalid,
  expired, partitioned, or otherwise unrepresentable records are skipped and
  counted; automatic Chrome imports pass through the same normalization and CEF
  write path. Values and raw rows never enter diagnostics.
- **Not** imported: cache, local storage, saved passwords, autofill data, or
  partitioned cookies. Chrome profile switching still names isolated zz storage;
  source selection for import never reuses Chrome's request context or writes to
  Chrome's files.

## Clearing current-site data

The browser menu's **Clear site data** action derives an HTTP(S) origin from the
live main-frame URL and calls Chromium's `Storage.clearDataForOrigin` DevTools
method with `storageTypes = "all"`. It clears cookies and persistent storage
owned by that origin, then reloads the page. The operation intentionally does
not widen into a profile-wide HTTP-cache clear; parent-domain cookies visible
to a subdomain and session DOM storage are outside this command's contract.

# Restore on reattach

The daemon retains each browser pane's canonical profile name alongside its last
URL. Closing every GPUI window detaches the client without keeping transient
Chromium renderer state alive; when a GUI reattaches, a browser pane
[re-creates its session](/browser/lifecycle.md) with that name and URL, and the
selected profile's persisted cookies/cache/local storage are already present. See
[session persistence](/concepts/session-persistence.md) for how panes and their URLs
survive detach in the daemon.

The daemon stores the profile name and nothing else, so a pane lands in the same
jar on the client's disk however it was reattached and from wherever its session
is hosted.

# Key files

| File | Role |
| --- | --- |
| `src/profile.rs` | `BrowserProfilePaths`, `resolve_profile_paths`, per-platform data dir, user-only permissions. |
| `src/cookies.rs` | Bounded Cookie-Editor/Netscape parsing and CEF-neutral cookie normalization. |
| `src/cef_runtime.rs` | Global `root_cache_path` setting and per-profile `RequestContext` creation. |
| `crates/zz-chrome-import/src/profiles.rs` | Background Chrome profile discovery, safe platform paths, and the bounded sanitized label cache. |
| `crates/zz-chrome-import/src/cookie.rs` | Site-scoped and profile-wide queries, cookie-identity deduplication, bounded SQLite snapshot, platform key lookup, Chrome decryption, and chunked CEF-neutral normalization. |
| `crates/zz-chrome-import/src/history.rs` | Bounded read-only Chrome history snapshot, Chromium timestamp conversion, and newest-5,000 HTTP(S) extraction. |
| `crates/zz/src/browser/recent_pages.rs` | Bounded app-owned history persistence and newest-visit URL merge used by the blank browser state. |

# Related

- The request context lifecycle is part of the
  [runtime & session lifecycle](/browser/lifecycle.md) and the
  [CEF runtime](/browser/cef-runtime.md) startup sequence.
- Address input that seeds a restored session is normalized by
  [input translation](/browser/input-translation.md).
- The retired tunnel that once gave remote panes a second jar:
  [remote browser egress](/designs/remote-browser-egress.md).
- Part of the [zz-browser crate](/crates/zz-browser.md).
