---
type: Design Plan
title: Agent backbone v3 - browser over CDP, typed agent state, headless completeness
description: Handoff plan from the 2026-09-09 headless audit of what an AI agent can drive through the zz CLI - the verified working set, the verified gaps, the locked decisions (zz is the browser process and the agent brings its own CLI, no MCP server, no native snapshot verbs before measurement), and three lanes of work with file pointers, acceptance checks, and traps.
status: Proposed 2026-09-09; nothing built; lane A step 2 belongs to the TUI rework session
tags:
- agent
- browser
- cdp
- tmux-compat
- daemon
- tui
- roadmap
timestamp: 2026-09-09T17:00:00-03:00
last_updated: 2026-09-09
---

# Read this first

You are picking this up on another machine with no memory of the session that wrote it. Everything
you need is in this file plus the source it points at. Line numbers are from `main` at `4f069977`
on 2026-09-09; treat them as "near here", and reverify before editing.

The question fabrico asked: how far can an AI agent go using zz as its whole backbone (terminal
orchestration, browser use, agent panes), and what is missing. The answer, after probing a headless
daemon with the installed 0.6.1 CLI: the terminal side is complete, the ACP agent pane side works
headless with two rough edges, and the browser side has no agent surface at all.

# Ground truth from the audit

## Reproduce in five minutes

```sh
export ZZ_SOCKET=/tmp/zz-ai.sock     # short path: sun_path caps at ~104 bytes
unset TMUX                           # the CLI refuses TMUX-without-ZZ_SOCKET
zz new-session -d -s ai -x 200 -y 50 -c /tmp
zz tools                             # the catalog an agent actually runs
zz list-commands | wc -l             # 105 on 0.6.1
zz kill-server                       # when done
```

## Verified working with no GUI attached

| Capability | Command that proved it |
| --- | --- |
| spawn argv, no shell, print pane id | `zz split-window -d -P -F '#{pane_id}' -e FOO=bar -c /tmp cat` |
| verified paste plus Enter into a TUI | `zz send-text -t %N "hello"`; against `stty -echo` it exits 1 with "text not echoed within 500 ms; nothing submitted" |
| read back | `capture-pane -p -J`, `pipe-pane -o 'cat >> log'`, `load-buffer` + `paste-buffer -p -r -d` |
| OSC 133 command read-back | `zz show-last-output -t %N` once the shell emits marks |
| sticky push signal on state writes | `zz wait-for '@agent_state@%N'` returns when anyone runs `zz set-option -p -t %N @agent_state idle` |
| hook on state writes | `set-hook -g @option-changed 'run-shell "echo #{hook_target} #{hook_option}"'` fires with `%N @agent_state` |
| timers | `zz run-shell -b -d 1 'touch marker'` |
| control-mode push | `zz -C attach -t ai` then `refresh-client -B st:%0:#{@agent_state}` emits `%subscription-changed st $0 @0 1 %0 : idle` on each write; `%output` streams pane bytes |
| hooks | `alert-bell`, `pane-exited` run with `#{hook_pane}` |
| tmux wrapper | a symlink `tmux -> zz` answers `tmux -V` with `tmux 3.8-zz` and works when both `TMUX` and `ZZ_SOCKET` are set, which every zz pane already has (`crates/zz-daemon/src/daemon.rs:7575`, `:7586`) |
| ACP agent pane end to end | `zz set-option -g experimental-agent-pane on`, `zz split-picker -h -d`, `zz select-pane-kind -t %N agent`, `zz set-agent-provider -t %N claude`, `zz agent-send -t %N --wait "Reply pong"`. The adapter spawned and the turn ran; it failed with a real API error (see B4) that came back as exit 1 plus the message plus the projection text |

## Verified gaps

1. **Browser has no read, act, or wait verbs.** The CLI offers `set-browser-url`, `set-browser-tabs`,
   `set-browser-profile`, `capture-browser`, `split-browser`, `new-browser`. Nothing returns page text
   or an accessibility tree, nothing clicks or types, nothing evaluates JS, nothing reads console or
   network, nothing waits for a condition.
2. **Browser needs a hosting client.** Headless `capture-browser` answers "pane is not attached". A TUI
   attach (`env -u TMUX zz attach -t ai` from a pane in another session) starts CEF, five helper
   processes appear, and `capture-browser` then answers "browser screenshots require the zz app"
   from `crates/zz-tui/src/app.rs:1575`. `#{pane_title}` on a browser pane stayed at the first URL
   after `set-browser-url`.
3. **No typed agent state on the CLI.** The daemon publishes `EventPayload::AgentState` carrying
   `AgentPaneWire` whose `phase` is `AgentConnectionPhase { Starting, Ready, Running,
   AwaitingPermission, Failed }` (`crates/zz-protocol/src/message.rs:2046`, `:2081`, `:3056`). No
   format variable exposes it, no wait channel fires on it, control mode never mentions it.
4. **No permission answer from the CLI.** `ProtocolMessage::AgentRespondPermission` is the only
   route (`daemon.rs:25230`) and only the GUI sends it. With `agent-auto-approve` at `off` or
   `reads` an unattended agent pane blocks until a human opens the GUI.
5. **Pane id discovery for non-terminal splits.** `split-picker` declares `-P` unsupported
   (`catalog.rs:1578`) and `split-browser` omits it (`catalog.rs:1626`); scripts diff `list-panes`.
   Creating an agent pane takes three verbs.
6. **The agent-pane gate starts off in CLI-spawned daemons.** `experimental_agent_pane` defaults to
   false in the engine (`command.rs:2237`); the GUI forwards the config key as a `set-option`
   (`crates/zz/src/config/mod.rs:1387`), a daemon nobody attached a GUI to never receives it.
7. **Bundled shell integration emits no OSC 133.** bash, zsh, and PowerShell assets emit OSC 2 and
   OSC 7 only, so `show-last-output` and `send-last-output` fail on a stock shell.
8. **`zz tools` undersells the surface.** `WORKSPACE_TOOLS` (`daemon.rs:37212`) omits `wait-for`,
   `set-hook`, `run-shell -b -d`, `pipe-pane`, `show-options -p`, and `list-panes -F '#{pane_kind}'`.
   The skill at `.claude/skills/zz-workspace/SKILL.md` covers them and drifts from the catalog.
9. **Adapter pin.** `DEFAULT_AGENT_COMMAND` pins `claude-agent-acp@0.68.0`
   (`message.rs:129`), which bundles Claude Code 2.1.232; current models answer
   `400 ... version 2.1.251 or newer is required`.
10. **No session restore on main.** A local branch `feat/session-resurrect` built one; fabrico did
    not find it useful and it stays out of this plan.

## What agent browser tools converged on in 2026

Playwright MCP, Chrome DevTools MCP, Vercel agent-browser, Anthropic's browser toolset, browser-use,
and Stagehand all run the same loop: accessibility snapshot with stable refs (interactive filter),
act by ref (click, type, fill, press), navigate, `wait_for` text or URL or network idle, page text,
find, tabs, console, network, eval, screenshot as fallback. A snapshot costs 200 to 400 tokens; a
screenshot costs 1,000 to 1,800. agent-browser attaches to a running browser with
`agent-browser --cdp 9222 snapshot` and speaks CDP with no Playwright. Playwright MCP takes
`--cdp-endpoint`, Chrome DevTools MCP takes `--browserUrl`. zz needs to be the browser they attach to.

# Decisions

1. **zz is the browser process; the agent brings its own CLI.** Expose CDP and let agent-browser,
   Playwright MCP, and Chrome DevTools MCP do snapshot, click, and wait. Do not port agent-browser
   into zz. Do not add an MCP server. The zz CLI stays the tool surface for what only zz knows:
   panes, tabs, profiles, capture.
2. **No native snapshot, click, or wait verbs until measured.** After a week of real use over CDP,
   decide from token and latency numbers whether the hot loop deserves thin verbs over the
   in-process `execute_dev_tools_method` calls (`cef_runtime.rs:1522`, `:3301`). Not before.
3. **The TUI rework session owns the TUI request handler (A2).** Do not patch
   `crates/zz-tui/src/app.rs` from this plan; that file is moving in another session.
4. **`#{agent_state}` is a separate variable from `@agent_state`.** Typed state for ACP panes, the
   user option for foreign agents in terminal panes. Scripts that want both write
   `#{?agent_state,#{agent_state},#{@agent_state}}`.
5. **Permission answering is a verb, plus early return from `--wait`.** The orchestrator loop
   becomes wait, blocked, respond, wait.
6. **Flip `experimental-agent-pane` to on by default.** It has shipped in every build since
   2026-08-14; the off default only bites CLI-spawned daemons.
7. **Anti-goals stay.** No payload pub/sub bus, no socket auth (isolation is the machine boundary),
   no screen-scraping state detection for terminal panes.

# Lane 0 - agent teams through the tmux wrapper (do first, 30 minutes)

Claude Code agent teams: enable with `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`; split panes come
from `teammateMode: "tmux"` in `~/.claude/settings.json` or `claude --teammate-mode tmux`; the
session must be interactive (`-p` never spawns teammates); it checks `which tmux`; the team config
at `~/.claude/teams/session-*/config.json` persists tmux pane ids; the docs' orphan cleanup is
`tmux ls` and `tmux kill-session -t NAME`.

```sh
S=$PWD/scratch; mkdir -p $S/bin
cat > $S/bin/tmux <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >> "${TMUX_WRAPPER_LOG:-/tmp/tmux-calls.log}"
exec zz "$@"
EOF
chmod +x $S/bin/tmux
export ZZ_SOCKET=/tmp/zz-ai.sock; unset TMUX
zz new-session -d -s ai -x 220 -y 60 -c ~/dev/zz
L=$(zz split-window -d -P -F '#{pane_id}' -e PATH=$S/bin:$PATH -e CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1 -e TMUX_WRAPPER_LOG=$S/calls.log -c ~/dev/zz claude --teammate-mode tmux)
sleep 8; zz capture-pane -p -t $L | tail -20        # past the trust / onboarding screens?
zz send-text -t $L "Spawn two teammates named alpha and beta. Each should run 'echo hi from <name>' and report back. Then shut them both down."
watch -n 2 "zz list-panes -F '#{pane_id} #{pane_current_command} #{pane_title}'"   # new panes appear?
cat $S/calls.log                                     # which verbs and flags Claude Code called
cat ~/.claude/teams/session-*/config.json            # pane ids recorded as %N?
```

Record: the verb and flag list from `calls.log`, whether teammates landed in zz panes, whether
`send-keys` into them worked, whether shutdown removed the panes, and any verb zz rejected. Any
rejection becomes a compat item in `compat/tmux-gaps.json` before lane B starts. Known variables:
Claude Code may parse `tmux -V` (`tmux 3.8-zz`) and may use `-t` targets in `session:window.pane`
form; both are covered by the compat corpus, so a failure here is news.

**Result 2026-09-09: passed with no setup.** Claude Code 2.1.267 in a zz pane, `--teammate-mode tmux`,
asked for teammates alpha and beta: both appeared as new zz panes (`%2`, `%3`) within 15 s, ran their
commands, reported back, approved the shutdown request, and their panes were gone by 40 s. The team
config recorded `tmuxPaneId` values. `calls.log` stayed empty because the daemon already prepends its
own `tmux` shim directory to every pane's PATH (`TmuxShimGuard` in `crates/zz-daemon/src/daemon.rs`,
`tmux_shim_environment` in `crates/zz-daemon/src/lib.rs`), so Claude Code drove zz through that shim,
ahead of the wrapper on PATH. No compat items came out of it.

# Lane A - browser

## A1. CDP endpoint (about one day)

**What.** Inject `--remote-debugging-port=N` into the CEF command line when configured. The cef
crate at 151.2.0 has no `remote_debugging_port` field on `Settings`, so the switch is the only
route. Bind stays loopback (Chromium default). Off by default.

**Where.**
- Switch injection: `fn on_before_command_line_processing` at
  `crates/zz-browser/src/cef_runtime.rs:3609`, existing `append_switch_with_value` calls at `:3658`.
- Config key: add `browser-remote-debugging-port` next to `ConfigKey::BrowserElementSelectorHotkey`
  (`crates/zz-config/src/lib.rs:135`, `as_str` `:180`, `from_str` `:225`, validation `:886`) and a
  field on `BrowserConfig` (`:309`). The GUI installs `BrowserConfig` as a global at
  `crates/zz/src/config/mod.rs:275`. Thread the value into `BrowserRuntime::start`
  (`cef_runtime.rs:583`, `Settings` literal at `:588`).
- Env override for TUI and headless hosts, same shape as `ZZ_BROWSER_SHARED_TEXTURE`
  (`cef_runtime.rs:1935`): `ZZ_BROWSER_REMOTE_DEBUGGING_PORT`.
- Format variable `#{browser_url}` answered from the pane's `BrowserDescriptor` active tab in
  `DaemonFormatHooks::variable` (`crates/zz-daemon/src/status.rs:1351`) via `FormatFacts`
  (`crates/zz-mux/src/command.rs:1606`, built at `:15252`). This is how an agent correlates a zz
  pane with a CDP target from `/json/list`, which only carries URL and title.
- Docs: a "Drive the browser from an agent" section in the zz-workspace skill and
  `knowledge/browser/` with the three attach recipes.

**Acceptance.**
```sh
# in the GUI (browser lives in the client), config has browser-remote-debugging-port = 9222
curl -s localhost:9222/json/list | jq '.[].url'
zz list-panes -F '#{pane_id} #{pane_kind} #{browser_url}'
agent-browser --cdp 9222 snapshot -i          # refs come back
agent-browser --cdp 9222 click @e1            # OSR accepts Input.dispatchMouseEvent
agent-browser --cdp 9222 screenshot out.png   # no tiled/duplicated image (clip.scale must stay 1.0 under OSR; CEF #3103)
npx @playwright/mcp@latest --cdp-endpoint http://localhost:9222   # connects
```
With the port unset, `curl localhost:9222` refuses. Day-one risks to settle before anything else:
CDP input under OSR, and screenshot scale.

**Security note for the docs.** Any local process can drive the user's logged-in browser through
this port. Same posture as Chrome's own flag. Off by default, loopback only, and the docs say so.

## A2. TUI answers browser requests (belongs to the TUI rework session)

**What.** Replace the refusal at `crates/zz-tui/src/app.rs:1575` (inside `fn handle_core_event`,
arm `CoreEvent::BrowserCommand { command: BrowserCommand::Screenshot { .. } }`) with the same work
the GUI does in `BrowserView::screenshot` (`crates/zz/src/browser/view.rs:946`) and
`drain_gui_requests` (`crates/zz/src/workspace/view.rs:1648`): encode the latest frame to PNG at
the requested absolute path and answer through `send_gui_response`
(`crates/zz-daemon/src/client.rs:924`). Keep the encoder in `crates/zz/src/browser/screenshot.rs`
reachable from the TUI's provider path rather than copying it.

**Acceptance.** From a pane in a second session, `env -u TMUX zz attach -t ai`; then
`zz capture-browser -t %B -o /tmp/shot.png` writes a PNG of the page. Design inputs for that
session: keep CEF hosting in the TUI process behind the provider path, and leave room for a
TTY-less host mode (`zz attach --headless`: attach, host CEF, answer browser requests, render
nothing). The headless mode does not ship in this plan.

## A3. Native hot-loop verbs (decision gate, not scheduled)

Only if measurements after A1 say the agent-browser round trips cost too much: `browser-snapshot`,
`browser-click`, `browser-wait`, each a client-answered request riding the `capture-browser`
GuiRequest path and calling `execute_dev_tools_method` in process. Write the numbers down before
opening this.

# Lane B - agent panes as an orchestration target

## B1. `#{agent_state}` plus a wait channel (half a day)

**What.** Map `AgentConnectionPhase` to a string: Starting -> `starting`, Ready -> `idle`, Running
-> `working`, AwaitingPermission -> `blocked`, Failed -> `failed`; an empty string for non-agent
panes. Answer it in `DaemonFormatHooks::variable` (`status.rs:1351`). On every phase transition,
call `signal_wait_channel` (`daemon.rs:9991`) with `agent_state@%N`, from the same place
`publish_agent_state` runs (`daemon.rs:25555`). Control-mode subscriptions expand formats through
`refresh_control_subscriptions` (`daemon.rs:4735`, expansion at `:4770`), so the new variable
reaches `%subscription-changed` with no extra wiring.

**Acceptance.**
```sh
zz list-panes -F '#{pane_id} #{pane_kind} #{agent_state}'         # idle / working / blocked
( zz wait-for 'agent_state@%3' && echo woke ) & zz agent-send -t %3 --submit "say hi"   # woke prints on Running
printf 'refresh-client -B a:%%3:#{agent_state}\n'; sleep 30 | zz -C attach -t ai     # %subscription-changed on each edge
```
No new verb. `until [ "$(zz display-message -p -t %3 '#{agent_state}')" = idle ]; do zz wait-for agent_state@%3; done`
is the `--until` idiom; put it in the skill.

## B2. `agent-respond` and early return from `--wait` (one day)

**What.** Daemon-native verb `agent-respond -t %N (--allow | --deny | --option ID) [REQUEST_ID]`
that builds `HostCommand::RespondPermission` (`crates/zz-daemon/src/agent/host.rs:50`, handled at
`:719`) for the oldest entry in `pending_permissions` (`host.rs:181`) when no id is given. A read
twin `show-agent-permission -t %N` prints the pending request JSON (`AgentPaneWire.pending_permission`).
`agent-send --wait` gains `--on-block wait|fail` (default `wait`, today's behavior): with `fail`, when the turn enters
AwaitingPermission, settle the waiter (`settle_active_turn`, `host.rs:1093`) with a new
`AgentTurnFailure::Blocked { payload }`, print the payload JSON on stdout, exit with a distinct
code (suggest 3), and leave the turn running so `agent-respond` can continue it.

**Acceptance.**
```sh
zz set-option -g agent-auto-approve off
zz agent-send -t %3 --wait "create a file named probe.txt"; echo $?        # prints permission JSON, exits 3
zz show-agent-permission -t %3 | jq .options
zz agent-respond -t %3 --allow
zz agent-send -t %3 --wait "what did you just do?"                          # completes
```
Follow the native verb checklist below for both verbs.

## B3. `split-agent`, `-P` everywhere, flag default (half a day)

**What.** `split-agent [-h|-v] [-d] [-p provider] [-c dir] [-P [-F fmt]] [-t target]` composes
`split_picker` (`command.rs:5848`) + `select_pane_kind` (`:5904`) + provider in one engine call and
emits `MuxEffect::PaneFormatOutput` (`command.rs:919`, rendered at `daemon.rs:8939`) the way
`split_window_with_options` does at `command.rs:6495`. Add `-P`/`-F` to the `split-browser` spec
(`catalog.rs:1626`) and drop the `unsupported_flag("-P")` from `split-picker` (`catalog.rs:1578`).
Flip `experimental_agent_pane` at `command.rs:2237` to true and keep the option so a user can turn
it off; update the GUI forwarding at `crates/zz/src/config/mod.rs:1387` and the settings copy.

**Acceptance.** In a fresh CLI-spawned daemon with no `set-option`:
```sh
P=$(zz split-agent -h -d -p claude -P -F '#{pane_id}'); zz list-panes -F '#{pane_id} #{pane_kind}' | grep "$P agent"
B=$(zz split-browser -h -d -P -F '#{pane_id}' https://example.com)
```

## B4. Adapter pins (an hour)

Bump `claude-agent-acp` at `crates/zz-protocol/src/message.rs:129` to a release whose bundled
Claude Code is at or above 2.1.251 (check the package's dependency on `@anthropic-ai/claude-agent-sdk`
before choosing), and `codex-acp` at `:127` to current. Document the `agent-command` and
`agent-claude-code-command` overrides (`command.rs:11306`) in the skill so a user can outrun the pin.

**Acceptance.** Fresh install, `zz agent-send -t %3 --wait "Reply with exactly the word pong"`
prints `pong`.

# Lane C - hygiene (hand to codex)

## C1. OSC 133 in the bundled shell integration

Add A (prompt start), B (prompt end), C (command start), D;exit (command end) marks beside the
existing OSC 2 and OSC 7 writers: bash `crates/zz-terminal/assets/shell-integration/bash/zz-integration.bash:44`
and `:53`, zsh `zsh/zz-integration.zsh:8` and `:18` (precmd/preexec), PowerShell
`powershell/zz-integration.ps1:25` and `:34`. Ghostty's shell integration is the reference
implementation for all three. Bash needs a DEBUG trap or bash-preexec for C; guard against firing
inside PROMPT_COMMAND.

**Acceptance.** In a fresh zz bash pane: `echo hi`, then `zz show-last-output -t %N` prints the
fenced block; `zz send-last-output -t %N` lands in an agent pane. Same for zsh.

## C2. One text for `zz tools` and the skill

Make `WORKSPACE_TOOLS` (`daemon.rs:37212`) the single source: it prints the markdown the skill
ships, and a `just` recipe regenerates `.claude/skills/zz-workspace/SKILL.md` from
`zz tools --skill` (frontmatter kept in the recipe). Add the missing verbs: `wait-for` channels,
`set-hook @option-changed`, `run-shell -b -d`, `pipe-pane`, `show-options -p -v`,
`list-panes -F '#{pane_kind} #{agent_state}'`, and after B1/B2 the state and permission idioms.
Extend `TOOL_VERBS` in `tools_catalog_matches_dispatchable_verbs` (`daemon.rs:63144`) so a verb
named in the text without a dispatch entry fails CI.

**Acceptance.** A test asserts the skill body equals `zz tools --skill`; `cargo test -p zz-daemon tools_catalog` passes.

# Daemon-native verb checklist

Every new verb in lanes A and B touches all of these, or a test fails:

1. `CommandSpec` in `DAEMON_COMMAND_SPECS` (`crates/zz-protocol/src/catalog.rs:823`).
2. Name in `DAEMON_COMMAND_NAMES` (`:549`) and `NATIVE_COMMAND_NAMES` (`:798`); the consistency test
   at `:3863` checks both.
3. `DaemonCommandDispatch` variant plus a row in `DAEMON_COMMAND_DISPATCHES`
   (`crates/zz-daemon/src/daemon.rs:2862`, `:2887`; `show-last-output` at `:2896` is the model, its
   executor at `:12588`).
4. A `native-command:<verb>` item in `compat/tmux-gaps.json` (example at line 209).
5. The verb in `TOOL_VERBS` (`daemon.rs:63144`) and in the `WORKSPACE_TOOLS` text (`:37212`).
6. Counts in `knowledge/tmux/commands.md:286` and `:516`.
7. Daemon specs are not found by `zz_mux::command_spec`; look them up through the protocol catalog.

# Traps

- **Shared checkout.** Other agent sessions edit this tree concurrently (the TUI rework session has
  `AGENTS.md`, `Justfile`, `knowledge/designs/index.md`, `knowledge/designs/tui-client.md`,
  `knowledge/log.md` dirty as of 2026-09-09 17:00 -03). Never `git stash`, reset, or clean. Stage
  your own hunks only (`git diff -U0 FILE | git apply --cached` after filtering), and run rustfmt
  on files you own, never `cargo fmt --all` while others have unformatted work in flight.
- **Sockets.** Unix socket paths cap near 104 bytes on macOS; put test sockets directly under `/tmp`.
- **HOME.** A hermetic daemon needs `HOME` overridden as well as `XDG_*`; the config writers prefer
  the first existing candidate and will write into the real `~/.config/zz/mux.conf`. Never
  `cargo run` with a fake HOME (rustup downloads a toolchain); build normally, then invoke the binary.
- **Stale binary.** `cargo build ... | tail` hides the exit code; `set -o pipefail` and check the
  binary mtime before a probe run.
- **OSR and CDP.** `Page.captureScreenshot` needs `clip.scale` 1.0 under OSR (CEF #3103) or you get
  tiled duplicates. Keep the BeginFrame pump running during capture.
- **Agent pane gate.** In a CLI-spawned daemon run `zz set-option -g experimental-agent-pane on`
  before `select-pane-kind agent` until B3 lands.
- **Flaky daemon tests.** A few `zz-daemon` tests fail only under full-workspace parallel load; rerun
  the failing test alone before diagnosing. On headless machines
  `concurrent_default_interactive_attaches_atomically_share_session_zero` fails with "not a
  terminal" by design.
- **Failed turns and OSC 133.** A turn that fails leaves `show-last-output` on the agent pane saying
  "has not completed a command yet"; the projection closes the D mark only on PromptFinished.
  Worth a one-line fix while in `PaneLane::project` if you are nearby.

# Order and parallelism

```
day 0   Lane 0 wrapper trial (30 min)  ──►  any rejection → compat item first
day 1   A1 CDP endpoint         ‖  B1 agent_state + wait channel   ‖  codex: C1, C2
day 2   A1 recipes + docs       ‖  B2 agent-respond + --on-block
day 3   measure A1 in use       ‖  B3 split-agent, -P, flag default, B4 pins
week 2  A3 decision from numbers; A2 lands with the TUI rework whenever that merges
```

Lanes A and B never touch the same files. Lane C touches `daemon.rs` (C2) near B2's dispatch
table; sequence C2 after B2 or accept a small merge.

# Out of scope

Native snapshot verbs before measurement (A3 gate), a TTY-less browser host, an MCP server,
screen-scraping agent detection for terminal panes, socket authentication, a plugin system, and
session restore (the `feat/session-resurrect` branch stays unmerged; fabrico's call on 2026-09-09).
