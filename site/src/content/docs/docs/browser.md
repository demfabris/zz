---
title: Browser panes
description: A full Chromium, running as an ordinary pane.
---

A browser pane is a real Chromium, rendered off-screen and composited on the
same GPU surface as your terminals: Metal and IOSurface on macOS, wgpu on
Linux, D3D11 on Windows. It splits, zooms, focuses, and targets like any other pane:

```sh
zz new-browser https://crates.io
zz split-browser -h                  # split the current pane
zz set-browser-url -t %3 https://docs.rs
```

URLs typed in the address bar are `http(s)` only; `localhost` and loopback
addresses default to `http`. Clicking a URL in a terminal pane opens it in
the nearest browser pane of the same window. The OS browser is only used
when the window has none.

## Profiles

Every pane runs a named, persistent, zz-owned profile. Cookies, cache, and
storage are shared between panes with the same profile name and isolated
across names. zz never touches your Chrome profile.

## Chrome import

**Import Chrome data** (macOS and Linux) decrypts a read-only snapshot of a
Chrome profile's cookie and history databases, via the system keychain, and
injects cookies into the pane's zz profile. Chrome's files are never
written. History fills the "Recently visited" list on blank pages.
Passwords and autofill are never imported.

You can also import a Cookie-Editor JSON or Netscape `cookies.txt` file.

## Element picker

Click the inspector icon, then click any element on the page. You get an
inspector-style source context, including the React source file and line
when the page exposes it, plus a screenshot of that element, together in
one clipboard entry, ready to paste wherever you need it.

Chromium's own DevTools are one keystroke away (`cmd-alt-i` /
`ctrl-shift-i`, or right-click → Inspect element).

## Limits

No find-in-page yet. Downloads, file dialogs, and permission prompts are
denied; popups open a new tab in the same pane instead of a window.
