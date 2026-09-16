---
type: Rust Crate
title: zz-cli crate
description: Headless CLI, daemon entrypoint, and terminal attach client shared with the desktop app.
resource: crates/zz-cli/src/lib.rs
tags: [crate, cli, daemon, tui, headless, packaging]
timestamp: 2026-09-16T00:00:00Z
---

# Overview

`zz-cli` provides the CLI verbs, `zz daemon`, `zz proxy`, `zz fleet`, and `zz attach`.
The `zz-cli` package builds the headless `zz_cli` binary; the `zz` package builds the
GPUI desktop binary `zz` and links the shared `zz-cli` library for CLI dispatch,
daemon spawning, and terminal attach through the `zz-tui` library.

Desktop bundles install `zz_cli` as `cli`: `lib/zz/cli` on Linux and
`Contents/MacOS/cli` on macOS. The `zz` symlink on PATH points to that `cli` file.
Headless archives instead ship the same binary as `zz`, alongside the MIT and Apache licenses.

The `daemon_executable` rule makes a GUI spawn the sibling `cli` and the headless
binary spawn itself. Sessions survive when the desktop client quits.
`zz app` opens the desktop app beside the headless binary, or explains that the
app is not installed on a headless host.

The headless dependency tree contains no GUI crates. CI's "Headless binary stays
GUI-free" step checks the normal dependencies with `cargo tree -p zz-cli`.
Install it on Linux or macOS with `install.sh --headless`, or run
`curl -fsSL https://zzmux.sh/install.sh | sh -s -- --headless`.
The installer puts `zz` in `~/.local/bin`; `--prefix <dir>` selects another prefix.
