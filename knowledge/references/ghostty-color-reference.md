---
type: Reference
title: Ghostty X11 color reference
description: Provenance of crates/zz-terminal/src/x11-rgb.txt, copied verbatim from Ghostty's rgb.txt for X11 named-color lookups.
resource: third_party/ghostty-reference/UPSTREAM.md
tags: [ghostty, x11, color, reference, license]
timestamp: 2026-07-14T00:00:00Z
---

# Overview

`crates/zz-terminal/src/x11-rgb.txt` is copied verbatim from Ghostty's
`src/terminal/res/rgb.txt` at upstream commit
[`cf60af281bd7559a819aa25372cef01d623b8c5a`](https://github.com/ghostty-org/ghostty). Unlike the
[tmux behavioral reference](/references/tmux-upstream.md), this is a direct data-file copy rather
than a behavior-parity check: the file is a plain named-color table originally sourced
from the X.Org `rgb` project. It backs zz's support for Ghostty's embedded X11 named-color table in
terminal appearance configuration; see [terminal appearance](/terminal/appearance.md).

# Schema

| Field | Value |
| --- | --- |
| Local file | `crates/zz-terminal/src/x11-rgb.txt` |
| Upstream file | `src/terminal/res/rgb.txt` (Ghostty) |
| Pinned commit | `cf60af281bd7559a819aa25372cef01d623b8c5a` |
| Original data source | X.Org `rgb` project |
| License | MIT/X11 |

Ghostty records the data as sourced from the X.Org `rgb` project and licensed under the MIT/X11
license. The accompanying Ghostty MIT license is retained alongside this file in
`third_party/ghostty-reference/` for provenance.

# Citations

- Ghostty commit: `cf60af281bd7559a819aa25372cef01d623b8c5a`, file `src/terminal/res/rgb.txt`
- `third_party/ghostty-reference/UPSTREAM.md` (in-repo source of truth)

# Related

- [Terminal appearance concept](/terminal/appearance.md) . where this named-color table is consumed (Ghostty config palette/color parsing)
- [`zz-terminal` crate](/crates/zz-terminal.md) . owns the copied file
- [tmux upstream reference](/references/tmux-upstream.md) . sibling third-party pin, contrasting a data copy vs. a behavior-parity check
