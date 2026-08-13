---
type: Reference
title: CEF artifact lock
description: Where the CEF/Chromium pin and its per-platform archive SHA-1 mirror live, and how the archive is actually verified before extraction.
resource: third_party/cef/ARTIFACTS.md
tags: [cef, chromium, pin, sha1, reference]
timestamp: 2026-07-27T00:00:00Z
---

# Overview

zz resolves the `cef` and `cef-dll-sys` Rust crates in `Cargo.lock`; that version maps to a CEF
build, which maps to a Chromium build. Before extracting a downloaded minimal distribution, the
`download-cef` build helper (pulled in by the upstream `cef` crate, *not* an
[`xtask`](/crates/zz-xtask.md) subcommand) fetches CEF's own `index.json` and verifies the archive
against the published SHA-1. `xtask` only runs after that, assembling and validating the bundle.

# The pin

**`third_party/cef/ARTIFACTS.md` is the in-repo record.** It carries the resolved crate version, the
CEF and Chromium versions it maps to, and a per-target table of minimal-distribution archive names
and SHA-1 hashes read from `https://cef-builds.spotifycdn.com/index.json`. Read it there; this
document deliberately does not restate the numbers, because a second copy is a second thing to go
stale, and it would go stale silently.

That table is a **reviewable mirror, not an input**: the downloader never reads it, so a mismatch
between it and the official index means the file is out of date, not that the build is unsafe. Its
job is to make a CEF bump legible in review . you can see which eight archives changed and confirm
the hashes came from the index on a stated date.

When updating CEF, refresh the Cargo lock, `third_party/cef/ARTIFACTS.md`, the CI cache key, and all
three platform bundle smoke tests together (see [updating CEF](/playbooks/updating-cef.md)).

# Citations

- `third_party/cef/ARTIFACTS.md` . the in-repo mirror and the date its values were read
- CEF builds index: `https://cef-builds.spotifycdn.com/index.json` . what the downloader verifies against

# Related

- [xtask crate](/crates/zz-xtask.md) . assembles and validates the bundle after the verified extraction
- [Build/verify a CEF bundle playbook](/playbooks/build-cef-bundle.md) . operational use
- [Updating CEF playbook](/playbooks/updating-cef.md) . coordinated bump procedure
- [CEF runtime concept](/browser/cef-runtime.md) . what the verified distribution provides at runtime
