---
type: Playbook
title: Agent benchmarks
description: Measure zz as an agent target: browser-pane capability parity over CDP against standalone browser tools, and whether the zz-workspace skill helps weak agents; harness layout, prerequisites, results, and traps.
resource: bench/agents/browser/matrix.sh
tags: [agents, browser, cdp, benchmark, skill, agent-browser, playwright-mcp, chrome-devtools-mcp]
timestamp: 2026-09-10T00:00:00Z
---

# What the benchmarks answer

Two questions, two harnesses under `bench/agents/`:

1. **Browser parity.** An agent that drives a page inside a zz browser pane through the loopback
   CDP endpoint should be able to do everything it can do in the standalone browser its tool
   ships with. `bench/agents/browser/` measures that with one primitive per row (matrix) and
   with whole tasks driven by an LLM (task suite), on the same deterministic fixture site.
2. **Skill effectiveness.** `zz tools` doubles as the `zz-workspace` skill. `bench/agents/skill/`
   runs weak models against headless zz tasks with the skill absent, discoverable, or inlined.

Both use the Antigravity CLI (`agy`) as the agent runner because it exposes Gemini Flash, GPT-OSS,
and Claude models headlessly with one flag set. Any CLI that runs a prompt non-interactively with
shell access can replace it.

# Browser parity

## Backends

- `zz`: agent-browser attached with `--cdp 9222` to the pane whose URL starts at the fixture
  site. The harness creates that pane as window `bench` when missing and selects the window, then
  switches the agent-browser session to the pane's CDP target id so no new page is created.
- `own`: agent-browser's own managed Chromium (installed once with `agent-browser install`).

The same binary, the same commands, the same site. Any difference is the pane.

## Common browser tools, surveyed 2026-09-09

The primitives below are the union of what agent-browser 0.37, Playwright MCP 0.0.80,
chrome-devtools-mcp 1.9, and Claude in Chrome expose. All but the last attach to an existing
Chromium over CDP; Claude in Chrome drives the user's own Chrome through an extension and has no
attach mode, so it sets the capability bar rather than joining the matrix. Notable per-tool
holes: Playwright MCP has no forward, reload, selector, URL, or download wait; chrome-devtools-mcp
has no scroll, checkbox, CSS locator, cookie, storage, PDF, or download tool and requires a
`pageId` on every call; Claude in Chrome has no dialog, cookie, or emulation tool and blocks on
JavaScript dialogs. agent-browser accepts alerts automatically but leaves confirm and prompt to
`dialog accept`, and its `--pin-tab` binds a session to one CDP target so it never adopts another
pane's page. Documented CEF off-screen-rendering hazards that the matrix probes on purpose:
`Page.captureScreenshot` scale and blank-image bugs, `Target.createTarget` and `window.open`
producing windows rather than panes, and JavaScript dialogs passing through the embedder's
dialog handler.

## Results (2026-09-10, installed app 1494d00a plus the observer sync fix, agent-browser 0.37.1)

| backend | primitives | pass | failing rows |
|---|---|---|---|
| agent-browser on its own Chromium (`own`) | 51 | 50 | `shadow-dom-click-css` |
| agent-browser on a zz pane over CDP (`zz`) | 52 | 50 | `shadow-dom-click-css`, `download-click` |
| Playwright MCP attached to the pane | 5 | 5 | |
| chrome-devtools-mcp attached to the pane | 5 | 5 | |

The pane matches the standalone browser on every primitive except one:

- **Downloads are refused in a zz pane.** `download` and `wait --download` time out. Raw CDP
  confirms it is the pane, not the tool: `Browser.setDownloadBehavior` is accepted, a click on the
  attachment link produces no `downloadWillBegin`, no navigation, and no file. This is zz policy,
  not an omission: `DeniedDownloadHandler` in `crates/zz-browser/src/cef_runtime.rs` answers
  `can_download` with 0 for every request, beside the handler that denies media permissions.
  Parity needs a decision first (allow per pane, per agent, or behind a setting), then a download
  directory and progress surfacing; until then agents fetch the URL with `curl`, carrying the
  pane's cookies from `agent-browser cookies get` when the resource needs them.
- `shadow-dom-click-css` fails on both backends: agent-browser's CSS selectors do not pierce open
  shadow roots. Refs from `snapshot` do (`shadow-dom-click-ref` passes on both), and so do
  Playwright selectors.

Everything else passes on the pane with latencies within noise of the standalone browser (most
rows 40-500 ms on both; the waits are the page's own delays): observe (`snapshot`, text, attr,
count, box, `read`), locate (ref, CSS, `find role`, `find text`), act (click, double-click, fill,
select, check, radio, type, press, keyboard, hover-reveal, scroll, scrollintoview, mouse wheel),
navigate (back, forward, reload, redirect, pushState, pagination), wait (`--text`, `--fn`,
`--url`, `--load networkidle`), iframe by ref and by `frame`, shadow DOM by ref, `eval`, cookies
and storage read and write, console and network logs, network mocking, viewport and media
emulation, upload through `DOM.setFileInputFiles`, viewport and full-page screenshots, PDF,
alert, confirm and prompt dialogs, `tab new`, a `target=_blank` link, and `window.open`.

Two behaviors are parity with a caveat. Tabs and popups work but each one is a native Chromium
window beside zz while it lives, and their latency swung from 0.3 s to 31 s between runs; the
harness closes every target it created. Dialogs pass through CDP's `Page.handleJavaScriptDialog`;
zz registers no JavaScript dialog handler of its own, so what a page's `confirm()` does when no
CDP client is attached was not measured here.

`bench/agents/browser/report.sh` prints the full row-by-row table from `out/results.tsv`.

## Task suite (2026-09-10, 40 runs, ten tasks, agent-browser on PATH, same prompt on both backends)

| model | backend | pass | mean wall | mean tool calls | mean failed calls |
|---|---|---|---|---|---|
| gemini-3.8-flash-low | own | 10/10 | 16.9 s | 7.4 | 0.1 |
| gemini-3.8-flash-low | zz | 10/10 | 18.0 s | 8.1 | 0.1 |
| gpt-oss-120b-medium | own | 6/10 | 29.5 s | 8.3 | 0.6 |
| gpt-oss-120b-medium | zz | 6/10 | 44.2 s | 8.6 | 0.7 |

Pass rates are identical per model on both backends, and the tasks each model fails are the same
on both (gpt-oss invents a confirmation code on the form task, reports the page title instead of
the paragraph after the hover, and reads the button label instead of the shadow widget's text).
The pane costs a Gemini agent about a second more per task; the gpt-oss mean on zz is pulled up
by its long failing runs (hover task 62 s against 57 s, table task 32 s), not by a systematic
per-call cost. The download task passes on zz even though the pane refuses downloads: both
models read the CSV by other means (fetching the URL from the page context or with `curl`),
which is the workaround the catalog should keep recommending until zz has a download policy.

## Running it

```sh
node bench/agents/browser/site/server.mjs &                 # 127.0.0.1:4173
bench/agents/browser/matrix.sh own
bench/agents/browser/matrix.sh zz                            # needs the app up with CDP on 9222
bench/agents/browser/report.sh                               # markdown matrix, both columns
bench/agents/browser/tasks-all.sh                            # LLM tasks, both backends, serial
node bench/agents/browser/mcp-call.mjs "npx -y @playwright/mcp@latest --cdp-endpoint http://127.0.0.1:9222" list
```

`matrix.sh` closes every CDP page target the run created that was not there before, so popups
and new tabs do not leave native Chromium windows behind. The task runner does the same per run.

# Skill effectiveness

`bench/agents/skill/run-one.sh MODEL ARM TASK` starts a throwaway daemon on `/tmp/zze-*.sock`
with a fixture (the agent's own pane, a pane that printed a number, a `jobs` window whose pane
prints a `DONE-<token>` line after 12 s), puts a logging `zz` and `tmux` wrapper first on PATH,
runs `agy -p` with `ZZ_PANE`, `ZZ_SESSION`, and `ZZ_SOCKET` set, and checks daemon state or the
reply. `run-all.sh` fans out; `summary.sh` pivots. Arms: `none`, `skill` (SKILL.md under
`.agents/skills/zz-workspace/` plus `--add-dir`), `inline` (the skill body appended to the prompt).

Results (2026-09-10, 81 runs, gemini-3.6-flash-low, gemini-3.8-flash-low, gpt-oss-120b-medium,
nine tasks):

| arm | pass | mean zz calls per task | mean failed calls |
|---|---|---|---|
| inline | 27/27 | 3.0 | 0.1 |
| skill (discoverable) | 24/27 | 3.7 | 0.8 |
| none | 24/27 | 7.7 | 5.3 |

Both Gemini models pass every task in every arm, so the tasks sit inside tmux vocabulary and
only gpt-oss discriminates: without the skill it made 16 calls and 15 failures per task (128
retries of `capture-pane -t work:%2`), with the skill discoverable 4.7 calls and 2 failures, with
the skill inlined 3.3 calls and 9/9. The text works when it is in context; discovery halves the
flailing for a weak model but does not fix its outcomes. The next round needs zz-native tasks
(CDP browser loop, permission answer, routing output between agents, `agent_state` waits, a
second agent addressed by `@name`) to separate the Gemini models. The failure modes this round
exposed went into `zz tools`: session-prefixed pane targets, hunting for help in the tmux usage
banner, running the bare binary, `set-option` without `-p`, `agent-send` without `-t`.

# Traps

- `agy` print mode does not read `<cwd>/.agents/skills`, `.agent/skills`, or `~/.gemini/skills`;
  it reads a workspace passed with `--add-dir` and `~/.gemini/config/skills/<name>/`. Without
  that flag the "skill" arm silently equals "none".
- The user's daemon may carry `base-index 1`; fixtures must target panes by id, never `work:0`.
- `agent-browser eval` prints JSON, so string results arrive quoted; strip before comparing.
- `agent-browser` CSS selectors do not pierce shadow roots; refs from `snapshot` do, and so do
  Playwright selectors. Iframes need either the inlined ref or `frame <sel>`.
- A form submit that navigates is not awaited by `click`; wait on text from the next page.
- `#!/usr/bin/env bash` resolves to bash 3.2 on macOS, which has no `EPOCHREALTIME`; the scripts
  fall back to `perl -MTime::HiRes`.
- Only one CEF instance can host browser panes per cache directory: run the matrix against the
  installed app, never against a `dist/zz-dev` instance while the app is up.
