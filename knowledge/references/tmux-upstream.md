---
type: Reference
title: tmux behavioral reference pin
description: The pinned upstream tmux commit zz's Rust multiplexer reimplementation is checked against, and where the per-behavior file map lives.
resource: third_party/tmux-reference/UPSTREAM.md
tags: [tmux, reference, pin, behavioral-compatibility]
timestamp: 2026-07-27T00:00:00Z
---

# Overview

zz's multiplexer is a Rust implementation: it does not compile, link, or run tmux, and no tmux C
source is copied into the codebase. Instead, command names, aliases, key-table behavior, and
`.tmux.conf` syntax are checked by hand against tmux at a single pinned commit,
[`d77c9dc6aa021e4bc61f0da128c591af695e6466`](https://github.com/tmux/tmux/tree/d77c9dc6aa021e4bc61f0da128c591af695e6466).

Only the deliberately supported subset is implemented; unsupported tmux configuration commands are
reported and skipped rather than approximated. The upstream tmux license is retained beside
`UPSTREAM.md` for provenance.

# Where the detail lives

**`third_party/tmux-reference/UPSTREAM.md` is the record.** It holds the pinned commit and a
per-behavior map of the upstream C files each area was checked against . `cmd-parse.y`/`arguments.c`
for tokenization, `key-bindings.c` and the `cmd-*.c` family for tables and command semantics,
`window-copy.c`/`grid-reader.c` for word classes and copy mode, `options-table.c` for option scope
and inheritance, and so on, down to the default bindings each one settles.

That map is not duplicated here. It is long, it changes whenever a new tmux behavior is implemented,
and a second copy would drift silently. Read it in the repo; this document records only that the pin
exists and what it means.

The [tmux compat concept](/tmux/tmux-compat.md) documents the resulting Rust behavior. See
[updating this reference](/playbooks/updating-tmux-reference.md) for the process to bump the pin.

# Citations

- `third_party/tmux-reference/UPSTREAM.md` . in-repo source of truth for the pin and the file map
- Pinned commit: [`d77c9dc6aa021e4bc61f0da128c591af695e6466`](https://github.com/tmux/tmux/tree/d77c9dc6aa021e4bc61f0da128c591af695e6466)

# Related

- [tmux compat concept](/tmux/tmux-compat.md) . the Rust reimplementation checked against this pin
- [Updating this reference](/playbooks/updating-tmux-reference.md) . how to bump the commit and re-verify
- [`mux` crate](/crates/zz-mux.md) . where the reimplementation lives
