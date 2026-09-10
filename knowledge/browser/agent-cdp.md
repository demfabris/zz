---
type: Playbook
title: Agent access through CDP
description: Enable a loopback CDP endpoint for browser agents and correlate targets with zz browser panes.
resource: crates/zz-browser/src/cef_runtime.rs
tags:
- browser
- cef
- cdp
- agents
timestamp: 2026-09-09T21:28:59Z
---

# Enable the endpoint

Set a bare integer in `zz/config`:

```ini
browser-remote-debugging-port = 9222
```

Leave the key unset or use `0` to disable CDP. Valid enabled ports are `1024..=65535`.
Invalid file values produce a config diagnostic and retain the preceding/default value.
Set `ZZ_BROWSER_REMOTE_DEBUGGING_PORT` in the client process environment to override the file.
An environment value of `0` disables CDP even when the file enables it; invalid values prevent
CEF initialization with a browser configuration error. The TUI uses the environment override.

Open a browser pane to initialize CEF and start listening. The GUI reads the file value before
that first initialization. After CEF starts, restart the client to change or disable the port.
The Settings route's Browser page carries an **Agents** switch that writes `9222` or removes the
key; any other port is set in the file.

CDP is off by default. Chromium listens on loopback (`127.0.0.1`); zz adds only the
`remote-debugging-port` switch and no `remote-debugging-address` switch. Any local process that
can reach this port can drive your logged-in browser, read pages, and run JavaScript with your
session's access. Enable it only while you intend to grant that access.

# Attach an agent

Replace `PORT` with the configured number:

```sh
agent-browser --cdp PORT snapshot -i
npx @playwright/mcp@latest --cdp-endpoint http://127.0.0.1:PORT
npx chrome-devtools-mcp@latest --browserUrl http://127.0.0.1:PORT
```

These recipes use the attach flags documented by [agent-browser](https://agent-browser.dev/cdp-mode),
[Playwright MCP](https://github.com/microsoft/playwright-mcp#configuration), and
[Chrome DevTools MCP](https://github.com/ChromeDevTools/chrome-devtools-mcp#connecting-to-a-running-chrome-instance).

Install agent-browser rather than running it through `npx` per call. Measured on 2026-09-10 against
a zz pane with its daemon warm: the native binary answers `snapshot -i`, `get title`, and
`wait --load load` in 40-55 ms each, so the snapshot, click, wait, snapshot loop costs about
230 ms; `npx agent-browser@0.37.1` adds 330 ms of resolver time to every one of those calls.
`wait --load networkidle` costs 0.7-1.7 s because the idle window is real network time; prefer
`--load load` or `--text` unless the page keeps loading after the load event. Raw CDP on the same
pane answers `Runtime.evaluate` and `Accessibility.getFullAXTree` in under a millisecond on small
pages and in about 30 ms on a Wikipedia article, so the tool's process startup is the whole cost.

Attach to the pane's existing target. Never create pages: `Target.createTarget`, Playwright's
`newPage`, and any tool that opens a fresh tab on connect make CEF open a native Chromium window
beside zz, not a pane. Six `chrome://newtab/` windows appeared this way during the measurement
run. List them with `/json/list` and close each with `curl http://127.0.0.1:PORT/json/close/ID`.

# Match a pane to a CDP target

```sh
zz list-panes -a -F '#{pane_id} #{pane_kind} #{browser_url}'
zz display-message -p -t %0 '#{browser_url}'
curl http://127.0.0.1:PORT/json/list
```

Compare `browser_url` with each CDP target's `url`; the target list also provides titles.
`browser_url` contains the browser pane's active tab URL and an empty string for other pane kinds.
If several tabs share a URL, the URL alone cannot identify one target. CDP may also list background
tabs while the pane format reports only the active tab.

You can use the same variable in status formats and `refresh-client -B` subscriptions.
`MuxEngine::format_facts` in `crates/zz-mux/src/command.rs` reads `BrowserDescriptor::url()`;
`DaemonFormatHooks::variable` in `crates/zz-daemon/src/status.rs` exposes the value.

# Off-screen screenshots

For `Page.captureScreenshot`, set `clip.scale` to `1.0` when supplying a clip. CEF off-screen
rendering does not support custom capture scale; see [CEF issue 3103](https://github.com/chromiumembedded/cef/issues/3103)
and the [CefSharp off-screen capture implementation](https://github.com/cefsharp/CefSharp/blob/master/CefSharp.OffScreen/ChromiumWebBrowser.cs).
If an attached tool supplies another scale, adjust its screenshot request.

# Related

- [Application configuration](/configuration/app-config.md)
- [CEF runtime](/browser/cef-runtime.md)
- [Off-screen rendering](/browser/osr-rendering.md)
