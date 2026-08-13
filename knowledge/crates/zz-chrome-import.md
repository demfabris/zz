---
type: Rust Crate
title: zz-chrome-import crate
description: Store-agnostic Google Chrome data import - profile discovery, cookie snapshot/decryption, and read-only history extraction - isolating the app's only sqlite/crypto/keychain dependencies.
resource: crates/zz-chrome-import/src/lib.rs
tags: [browser, chrome, import, crate, sqlite]
timestamp: 2026-07-29T00:00:00Z
---

# Overview

`crates/zz-chrome-import` (package name `zz-chrome-import`) holds everything zz reads out of an
installed Google Chrome: profile discovery from `Local State`, cookie-database snapshots with
platform decryption, and read-only history extraction. It exists as a crate for dependency
hygiene . it is the sole owner of `rusqlite` (bundled `SQLite` C build), `aes`, `cbc`, `pbkdf2`,
`sha1`, `sha2`, `zeroize`, and the platform keychain surface (`security-framework` /
`objc2-foundation` on macOS, `oo7`/`smol` Secret Service on Linux) . none of which the `zz` app
crate names anymore.

The crate is gpui-free and store-agnostic. `history::import_history` takes an
[`ImportLimits`](/crates/zz-chrome-import/src/history.rs) (entry/URL/title byte caps supplied by the
caller from its own store's bounds) and returns `ImportedPage` rows; the app maps them onto its
`recent_pages::RecentPage`. Small filesystem helpers (`atomic_write`,
`restrict_to_current_user`, `truncate_utf8`) are deliberate private copies of app utilities .
duplicating ~40 lines beats a shared-utils crate.

# Modules

| Module | Owns |
| --- | --- |
| `profiles` | Bounded `Local State` parsing, Chrome-picker-ordered signed-in/out profile discovery, profile cache persistence, `chrome_user_data_dir`/storage-key resolution |
| `cookie` | Cookie DB + WAL snapshot into a private tempdir, v10/v11 value decryption via keychain/Secret Service, CEF-identity dedup, Cookie-Editor JSON and Netscape `cookies.txt` fallback parsers |
| `history` | Read-only `History` snapshot, newest-N HTTP(S) row extraction under `ImportLimits`, Chromium-epoch timestamp conversion |

Consumer: `crates/zz/src/browser/view.rs` (the browser three-dot menu's import actions). The
behavioral contract . what is and is not imported, permission-denied handling, macOS Full Disk
Access messaging . is documented from the user side in [browser profile](/browser/profile.md).
