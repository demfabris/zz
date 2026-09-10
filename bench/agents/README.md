# Agent benchmarks

Two harnesses that measure zz as a target for AI agents. Both run against real agents and a real
zz, append TSV rows, and print a summary; neither is part of `cargo test`.

## browser/ - capability parity for browser panes

Question: can an agent do in a zz browser pane, over the loopback CDP endpoint, what it can do in
the standalone browser its tool ships with?

- `site/server.mjs` serves a deterministic fixture site on `127.0.0.1:4173` (form with a
  confirmation code, delayed content, dialogs, popups, download, upload, iframe, shadow DOM, long
  page, hover menu, keyboard log, cookies and storage, console and network activity, redirect,
  pushState routes, a paginated table). No dependencies beyond Node.
- `matrix.sh zz|own` drives one primitive per row through agent-browser: `zz` attaches to the
  pane whose URL starts at the fixture site (created as window `bench` when missing) with
  `--cdp`, `own` uses agent-browser's managed Chromium. Rows cover observe, locate, act,
  navigate, wait, state and emulation, dialogs, uploads, downloads, tabs and popups, and
  screenshots; each row records pass or fail and wall time. `report.sh` renders the two columns
  side by side.
- `tasks.sh MODEL zz|own TASK` runs one agent-driven task through the Antigravity CLI (`agy`)
  with agent-browser on PATH and the same prompt on both backends; `tasks-all.sh` runs the
  models x backends x tasks grid serially and summarizes pass rate, wall time, and tool calls.
- `mcp-call.mjs` is a minimal MCP stdio client for attaching Playwright MCP
  (`--cdp-endpoint`) and chrome-devtools-mcp (`--browserUrl`) to the same pane and calling a
  few tools, to check that the other common tools attach at all.

Prerequisites: the zz app running with `browser-remote-debugging-port = 9222` (Settings >
Browser > Agents) and its window drawn at least once; `agent-browser` installed (`npm i -g
agent-browser`, then `agent-browser install` for the `own` backend); `jq`; `agy` for the task
suite.

```sh
node bench/agents/browser/site/server.mjs &
bench/agents/browser/matrix.sh own && bench/agents/browser/matrix.sh zz && bench/agents/browser/report.sh
bench/agents/browser/tasks-all.sh
```

## skill/ - does the zz-workspace skill help weak agents

`run-one.sh MODEL ARM TASK` starts a throwaway zz daemon with a fixture (an own pane, a pane
that printed a number, a background job that prints a DONE line), puts a logging `zz` and
`tmux` wrapper first on PATH, runs `agy` headless with `ZZ_PANE`, `ZZ_SESSION`, and `ZZ_SOCKET`
set, and checks daemon state or the reply. Arms: `none`, `skill` (SKILL.md under
`.agents/skills` plus `--add-dir`, which is what makes `agy` load it), `inline` (skill body in
the prompt). `run-all.sh` fans out with `xargs -P`; `summary.sh` pivots `out/results.tsv`.

Results and the traps both harnesses found live in
`knowledge/playbooks/agent-benchmarks.md`.
